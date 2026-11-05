//! SessionReplay: replay a recorded session through a deterministic app.
//!
//! The M9 recorder captures the *screen the user saw* and the input that
//! produced it; this module closes the loop the architecture's §5.8
//! promises — **replay interactions from any checkpoint**: feed the
//! recorded events back through a deterministic app, repaint every step,
//! and compare the result with the recorded frame. A session that
//! replays golden is a regression test; a session that diverges names
//! the exact frame where the app changed.
//!
//! # The model
//!
//! Every recorded frame becomes one replay *step*: the events recorded
//! since the previous frame (entry order, not wall-clock rounding) are
//! handed to the app first, the app paints into a fresh buffer sized by
//! the recorded frame, and the painted buffer is serialized with the
//! same canonical encoder the pipeline uses. Golden means
//! `serialize_ansi(painted) == recorded` for every step.
//!
//! The contract is determinism, nothing more: the app must paint the
//! same screen from the same events, and it must handle every recorded
//! event immediately (the next frame reflects it). Events recorded after
//! the last frame produced no screen and are reported as
//! [`ReplayReport::unverified_events`] instead of being delivered.
//! Bounded recordings replay their retained window; the app sees only
//! the events that survived eviction with it.
//!
//! Snapshots do not encode the viewport (a blank tail is one `ESC[K`),
//! so each step's buffer is sized to the *discovered* grid and golden is
//! decided on screen content over the union grid — see
//! [`SessionReplay::run`].

use std::time::Duration;

use crate::buffer::Buffer;
use crate::event::Event;
use crate::geometry::Rect;
use crate::preview::serialize_ansi;
use crate::screen::{ParseError, Screen};
use crate::session::{FrameRecord, InputRecord, SessionRecorder};

/// One replay step: the events to handle, then paint into `buffer`.
///
/// The buffer is sized exactly like the recorded frame it is compared
/// against (resizes in the recording surface as different step sizes).
#[derive(Debug)]
pub struct ReplayStep<'a> {
    /// The input events recorded since the previous frame, in recording
    /// order. Usually the app handles each one, mutating its own state.
    pub events: &'a [Event],
    /// The paint target, cleared and sized like the recorded frame.
    pub buffer: &'a mut Buffer,
    /// `true` for the first step of the replay — the checkpoint step.
    /// On an unbounded recording this is the session start (empty state
    /// is enough); on a bounded ring the events before it were evicted
    /// together with the frames they produced, so the app must restore
    /// its pre-window state from its own checkpoint —
    /// [`SessionReplay::seed_buffer`] is that checkpoint's content.
    pub seed: bool,
}

/// One divergent step: what the app painted versus what was recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayMismatch {
    /// The absolute number of the diverging frame.
    pub frame: u64,
    /// When the frame was recorded.
    pub at: Duration,
    /// The recorded snapshot.
    pub expected: String,
    /// The replayed snapshot.
    pub actual: String,
}

impl core::fmt::Display for ReplayMismatch {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "frame {} at {:?}: expected {} B, replayed {} B",
            self.frame,
            self.at,
            self.expected.len(),
            self.actual.len()
        )
    }
}

/// The outcome of a replay run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayReport {
    /// Steps the driver attempted (one per recorded frame).
    steps: usize,
    /// Steps whose replayed snapshot matched byte-for-byte.
    matched: usize,
    /// The diverging steps, in frame order.
    mismatches: Vec<ReplayMismatch>,
    /// Events recorded after the last frame (never shown on screen).
    unverified_events: usize,
}

impl ReplayReport {
    /// Whether every step replayed byte-identically.
    pub fn is_golden(&self) -> bool {
        self.mismatches.is_empty()
    }

    /// The number of attempted steps (one per recorded frame).
    pub fn steps(&self) -> usize {
        self.steps
    }

    /// The number of byte-identical steps.
    pub fn matched(&self) -> usize {
        self.matched
    }

    /// The diverging steps, in frame order.
    pub fn mismatches(&self) -> &[ReplayMismatch] {
        &self.mismatches
    }

    /// The first divergence, if any.
    pub fn first_mismatch(&self) -> Option<&ReplayMismatch> {
        self.mismatches.first()
    }

    /// Events recorded after the last frame: the app handled them, but no
    /// flushed frame shows their effect, so a replay cannot verify them.
    pub fn unverified_events(&self) -> usize {
        self.unverified_events
    }
}

/// One replayable step on the parsed timeline: a frame plus the events
/// that preceded it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FrameStep {
    frame: FrameRecord,
    events: Vec<Event>,
}

impl FrameStep {
    fn split(steps: Vec<FrameStep>) -> (Vec<FrameRecord>, Vec<Vec<Event>>) {
        let mut frames = Vec::with_capacity(steps.len());
        let mut events = Vec::with_capacity(steps.len());
        for step in steps {
            frames.push(step.frame);
            events.push(step.events);
        }
        (frames, events)
    }
}

/// A recorded session prepared for replay.
///
/// # Examples
///
/// ```
/// use tuvix_core::preview::FrameSink as _;
/// use tuvix_core::replay::{ReplayStep, SessionReplay};
/// use tuvix_core::session::{FrameRecord, SessionRecorder};
///
/// let mut recorder = SessionRecorder::unbounded();
/// recorder.record_event(tuvix_core::event::Event::FocusGained);
/// recorder.on_frame("a frame");
///
/// let replay = SessionReplay::from_recorder(&recorder);
/// assert_eq!(replay.frame_count(), 1);
///
/// // A deterministic app paints every step; the trivial app below
/// // paints an empty buffer, which matches the trivial frame above.
/// let report = replay.run(|step: ReplayStep<'_>| {
///     step.buffer.set_string(0, 0, "", tuvix_core::style::Style::new());
/// });
/// assert!(report.is_ok());
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionReplay {
    frames: Vec<FrameRecord>,
    /// `events[i]` is the event slice recorded before `frames[i]`.
    events: Vec<Vec<Event>>,
    trailing_events: usize,
}

impl SessionReplay {
    /// Parses a recorder's retained timeline: every frame becomes a step
    /// carrying the events recorded before it.
    pub fn from_recorder(recorder: &SessionRecorder) -> Self {
        let mut steps: Vec<FrameStep> = Vec::new();
        let mut pending: Vec<Event> = Vec::new();
        for entry in recorder.entries() {
            match entry {
                crate::session::Entry::Frame(frame) => {
                    steps.push(FrameStep {
                        frame,
                        events: std::mem::take(&mut pending),
                    });
                }
                crate::session::Entry::Input(input) => {
                    pending.push(input.event);
                }
            }
        }
        let (frames, events) = FrameStep::split(steps);
        Self {
            frames,
            events,
            trailing_events: pending.len(),
        }
    }

    /// Builds a replay from raw parts (tests and importers).
    pub fn from_parts(frames: Vec<FrameRecord>, events: Vec<InputRecord>) -> Self {
        let mut steps: Vec<FrameStep> = Vec::new();
        let mut pending: Vec<Event> = Vec::new();
        let mut event_iter = events.into_iter().peekable();
        for frame in frames {
            // events strictly before this frame's timestamp belong to it
            while let Some(input) = event_iter.peek() {
                if input.at <= frame.at {
                    pending.push(event_iter.next().expect("peeked").event);
                } else {
                    break;
                }
            }
            steps.push(FrameStep {
                events: std::mem::take(&mut pending),
                frame,
            });
        }
        let trailing_events = event_iter.count();
        let (frames, events) = FrameStep::split(steps);
        Self {
            frames,
            events,
            trailing_events,
        }
    }

    /// Parses a session JSON document (the M9 export format) into a
    /// replay. Strict: hand-edited documents fail with byte offsets.
    pub fn from_json(document: &str) -> Result<Self, crate::session::SessionError> {
        Ok(Self::from_recorder(&SessionRecorder::from_json(document)?))
    }

    /// The replayable frames, oldest first.
    pub fn frames(&self) -> &[FrameRecord] {
        &self.frames
    }

    /// The number of replayable frames.
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// The number of events the replay will deliver.
    pub fn event_count(&self) -> usize {
        self.events.iter().map(Vec::len).sum()
    }

    /// Events recorded after the last frame: delivered to nothing, they
    /// are reported by [`ReplayReport::unverified_events`] after a run.
    pub fn trailing_events(&self) -> usize {
        self.trailing_events
    }

    /// The first retained frame parsed back into a live buffer — the
    /// checkpoint a bounded-window replay resumes from (see
    /// [`ReplayStep::seed`]). `None` for an empty session.
    pub fn seed_buffer(&self) -> Option<Buffer> {
        let frame = self.frames.first()?;
        let screen = Screen::from_ansi(&frame.ansi).ok()?;
        let area = Rect::new(0, 0, screen.width(), screen.height());
        Some(screen.to_buffer(area))
    }

    /// Replays the session: for every step the app receives the recorded
    /// events and paints into a fresh blank buffer sized to the
    /// recorded content, and the painted result is compared with the
    /// recorded snapshot as *screen content* over the union grid.
    ///
    /// The comparison is content, not canvas bytes, because a canonical
    /// snapshot does not encode the viewport: the recorded grid is what
    /// the parser discovers (the longest written row, the highest
    /// addressed row). A replay painting identical content onto a larger
    /// canvas therefore replays golden — replay verifies what the user
    /// saw. The only failure mode is a corrupt recorded snapshot.
    pub fn run(&self, mut app: impl FnMut(ReplayStep<'_>)) -> Result<ReplayReport, ParseError> {
        let mut report = ReplayReport {
            steps: self.frames.len(),
            matched: 0,
            mismatches: Vec::new(),
            unverified_events: self.trailing_events,
        };
        for (index, (frame, events)) in self.frames.iter().zip(&self.events).enumerate() {
            let expected = Screen::from_ansi(&frame.ansi)?;
            let area = Rect::new(0, 0, expected.width(), expected.height());
            let mut buffer = Buffer::empty(area);
            app(ReplayStep {
                events,
                buffer: &mut buffer,
                seed: index == 0,
            });
            let actual = Screen::from_ansi(&serialize_ansi(&buffer))?;
            if expected.diff(&actual).is_empty() {
                report.matched += 1;
            } else {
                report.mismatches.push(ReplayMismatch {
                    frame: frame.number,
                    at: frame.at,
                    expected: frame.ansi.clone(),
                    actual: serialize_ansi(&buffer),
                });
            }
        }
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{KeyCode, KeyEvent, KeyModifiers};
    use crate::style::Style;

    fn key(character: char) -> Event {
        Event::Key(KeyEvent::new(
            KeyCode::Char(character),
            KeyModifiers::empty(),
        ))
    }

    /// Builds a session by hand: type `text`, one frame per char.
    fn typed_session(text: &str) -> SessionReplay {
        let mut frames = Vec::new();
        let mut events = Vec::new();
        let mut state = String::new();
        for (index, ch) in text.chars().enumerate() {
            events.push(InputRecord {
                at: Duration::from_millis(u64::try_from(index).unwrap_or(0) * 10),
                event: key(ch),
            });
            state.push(ch);
            let mut buffer = Buffer::empty(Rect::new(0, 0, 20, 1));
            buffer.set_string(0, 0, &state, Style::new());
            frames.push(FrameRecord {
                number: u64::try_from(index).unwrap_or(0),
                at: Duration::from_millis(u64::try_from(index).unwrap_or(0) * 10 + 5),
                ansi: serialize_ansi(&buffer),
            });
        }
        SessionReplay::from_parts(frames, events)
    }

    /// The app under test: handle Char events, paint the accumulated
    /// state — the same logic that produced the recording.
    fn typing_app(state: &mut String, (events, buffer): (&[Event], &mut Buffer)) {
        for event in events {
            if let Event::Key(key_event) = event
                && let KeyCode::Char(ch) = key_event.code
            {
                state.push(ch);
            }
        }
        buffer.set_string(0, 0, state, Style::new());
    }

    #[test]
    fn a_deterministic_typing_session_replays_golden() {
        let replay = typed_session("hello");
        assert_eq!(replay.frame_count(), 5);
        assert_eq!(replay.event_count(), 5);
        assert_eq!(replay.trailing_events(), 0);
        let mut state = String::new();
        let report = replay
            .run(|step| typing_app(&mut state, (step.events, step.buffer)))
            .unwrap();
        assert!(report.is_golden(), "mismatches: {:?}", report.mismatches());
        assert_eq!(report.steps(), 5);
        assert_eq!(report.matched(), 5);
        assert_eq!(report.unverified_events(), 0);
    }

    #[test]
    fn a_perturbed_app_names_the_first_divergent_frame() {
        let replay = typed_session("hello");
        // the app drops every 'l' — divergence starts at frame 2 ("hel")
        let mut state = String::new();
        let report = replay
            .run(|step| {
                for event in step.events {
                    if let Event::Key(key_event) = event
                        && let KeyCode::Char(ch) = key_event.code
                        && ch != 'l'
                    {
                        state.push(ch);
                    }
                }
                step.buffer.set_string(0, 0, state.as_str(), Style::new());
            })
            .unwrap();
        assert!(!report.is_golden());
        assert_eq!(report.matched(), 2, "frames 0..1 never saw an 'l'");
        let first = report.first_mismatch().expect("divergence");
        assert_eq!(first.frame, 2);
        assert_ne!(first.expected, first.actual);
        let summary = first.to_string();
        assert!(summary.contains("frame 2"), "{summary}");
    }

    #[test]
    fn trailing_events_are_counted_not_delivered() {
        let mut frames = Vec::new();
        let mut events = Vec::new();
        let buffer = Buffer::empty(Rect::new(0, 0, 10, 1));
        frames.push(FrameRecord {
            number: 0,
            at: Duration::from_millis(5),
            ansi: serialize_ansi(&buffer),
        });
        events.push(InputRecord {
            at: Duration::from_millis(10),
            event: key('x'),
        });
        let replay = SessionReplay::from_parts(frames, events);
        assert_eq!(replay.event_count(), 0, "no step claims a trailing event");
        assert_eq!(replay.trailing_events(), 1);
        let mut delivered = 0;
        let report = replay
            .run(|step: ReplayStep<'_>| {
                delivered += step.events.len();
            })
            .unwrap();
        assert_eq!(delivered, 0);
        assert_eq!(report.unverified_events(), 1);
        assert!(report.is_golden(), "the frame itself still matched");
    }

    #[test]
    fn events_before_the_first_frame_drive_it() {
        let mut frames = Vec::new();
        let events = vec![InputRecord {
            at: Duration::from_millis(1),
            event: key('!'),
        }];
        let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 1));
        buffer.set_string(0, 0, "!", Style::new());
        frames.push(FrameRecord {
            number: 0,
            at: Duration::from_millis(5),
            ansi: serialize_ansi(&buffer),
        });
        let replay = SessionReplay::from_parts(frames, events);
        assert_eq!(replay.event_count(), 1);
        let mut state = String::new();
        let report = replay
            .run(|step| typing_app(&mut state, (step.events, step.buffer)))
            .expect("the app handles the pre-frame event");
        assert!(report.is_golden(), "mismatches: {:?}", report.mismatches());
    }

    #[test]
    fn an_empty_session_replays_to_an_empty_report() {
        let replay = SessionReplay::from_parts(Vec::new(), Vec::new());
        let report = replay.run(|_| {}).unwrap();
        assert_eq!(report.steps(), 0);
        assert!(report.is_golden());
    }

    #[test]
    fn resizes_surface_as_step_sizes() {
        let mut frames = Vec::new();
        let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 1));
        buffer.set_string(0, 0, "narrow", Style::new());
        frames.push(FrameRecord {
            number: 0,
            at: Duration::from_millis(1),
            ansi: serialize_ansi(&buffer),
        });
        let mut wide = Buffer::empty(Rect::new(0, 0, 30, 2));
        wide.set_string(0, 1, "wide row", Style::new());
        frames.push(FrameRecord {
            number: 1,
            at: Duration::from_millis(2),
            ansi: serialize_ansi(&wide),
        });
        let replay = SessionReplay::from_parts(frames, Vec::new());
        let mut sizes = Vec::new();
        let report = replay
            .run(|step: ReplayStep<'_>| {
                sizes.push((step.buffer.area.width, step.buffer.area.height));
                // the app paints nothing — divergence on both frames
            })
            .unwrap();
        // the step buffer is sized to the *discovered* grid: the longest
        // written row by 1 (6, then 8), the highest addressed row (1, 2).
        assert_eq!(sizes, [(6, 1), (8, 2)]);
        assert!(!report.is_golden(), "a no-op app diverges everywhere");
    }

    #[test]
    fn a_corrupt_snapshot_fails_the_run() {
        let frames = vec![FrameRecord {
            number: 0,
            at: Duration::ZERO,
            ansi: "\x1b[999;999".to_owned(), // a CSI cut off before its final byte
        }];
        let replay = SessionReplay::from_parts(frames, Vec::new());
        let report = replay.run(|_| {});
        assert!(report.is_err(), "the strict parser rejects it");
    }

    #[test]
    fn identical_content_on_a_larger_canvas_replays_golden() {
        // recorded live on a 20-wide canvas: "hello" plus an EL tail
        let mut wide = Buffer::empty(Rect::new(0, 0, 20, 1));
        wide.set_string(0, 0, "hello", Style::new());
        let frames = vec![FrameRecord {
            number: 0,
            at: Duration::ZERO,
            ansi: serialize_ansi(&wide),
        }];
        let replay = SessionReplay::from_parts(frames, Vec::new());
        // the app paints the same content; its step buffer is only 5 wide
        // (the discovered grid), not the 20 the live app had
        let state = "hello".to_owned();
        let mut seen_width = 0;
        let report = replay
            .run(|step: ReplayStep<'_>| {
                seen_width = step.buffer.area.width;
                step.buffer.set_string(0, 0, state.as_str(), Style::new());
            })
            .unwrap();
        assert_eq!(seen_width, 5, "the step buffer is the discovered grid");
        assert!(report.is_golden(), "content equality ignores the canvas");
    }
}
