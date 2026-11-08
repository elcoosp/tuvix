//! The session recorder: a time-travel timeline of frames and input
//! events (M9, architecture §5.8).
//!
//! M8's [`HistorySink`](crate::preview::HistorySink) captured *what the
//! screen showed*; this module captures *what happened*: every flushed
//! frame **and** every input event the app handled, on one timestamped
//! timeline. The timeline is the raw material of the `DevTools` story —
//! scrub to any frame, see which keystrokes produced it, export the
//! whole session as JSON ([`SessionRecorder::to_json`]) and replay or
//! golden-test it later.
//!
//! The recorder is a [`FrameSink`], so it
//! attaches through the same M4 preview hook:
//! [`Terminal::set_preview`](crate::terminal::Terminal::set_preview).
//! Input recording is explicit — the app calls
//! [`record_event`](SessionRecorder::record_event) when it handles an
//! event — because only the app knows what it acted on.
//!
//! # Ring semantics
//!
//! A bounded recorder retains the newest `max_frames` frames. When the
//! ring is full the oldest frame is evicted *together with the input
//! events adjacent to it* — the events recorded before it (the app state
//! they acted on is gone) and the events recorded after it, up to the
//! next retained frame (the frame they produced is gone). The retained
//! timeline therefore always starts with a frame, so a replay from any
//! retained entry is well-defined. Events between two retained frames
//! are never touched. Evictions are counted in
//! [`dropped_frames`](SessionRecorder::dropped_frames) and
//! [`dropped_events`](SessionRecorder::dropped_events); the absolute
//! frame number of local index `i` is `dropped_frames() + i`.
//!
//! # Examples
//!
//! Recording a session and scrubbing it:
//!
//! ```
//! use tuvix_core::backend::TestBackend;
//! use tuvix_core::event::Event;
//! use tuvix_core::geometry::Size;
//! use tuvix_core::preview::FrameSink;
//! use tuvix_core::session::SessionRecorder;
//! use tuvix_core::terminal::Terminal;
//!
//! let mut terminal = Terminal::new(TestBackend::new(Size::new(10, 1))).unwrap();
//! let session = SessionRecorder::unbounded();
//! terminal.set_preview(session.clone());
//!
//! terminal
//!     .draw(|f| {
//!         f.buffer.set_string(0, 0, "a", Default::default());
//!     })
//!     .unwrap();
//! session.record_event(Event::FocusGained);
//! terminal
//!     .draw(|f| {
//!         f.buffer.set_string(0, 0, "b", Default::default());
//!     })
//!     .unwrap();
//!
//! let entries = session.entries();
//! assert_eq!(entries.len(), 3);
//! assert!(entries[0].is_frame());
//! assert!(entries[1].is_input());
//! assert!(entries[2].is_frame());
//! assert_eq!(session.frame_count(), 2);
//! assert_eq!(session.latest_frame().unwrap().ansi, session.frame(1).unwrap().ansi);
//! ```

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MediaKeyCode,
    ModifierKeyCode, MouseButton, MouseEvent, MouseEventKind,
};
use crate::preview::{FrameSink, json_escape};

/// One captured frame on the session timeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameRecord {
    /// Absolute frame number: 0 for the first frame ever captured by this
    /// recorder, counting up across evictions.
    pub number: u64,
    /// Time since the recorder was created.
    pub at: Duration,
    /// The exact ANSI snapshot the screen showed (`serialize_ansi` bytes).
    pub ansi: String,
}

/// One recorded input event on the session timeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputRecord {
    /// Time since the recorder was created.
    pub at: Duration,
    /// The event as the app received it.
    pub event: Event,
}

/// One timeline entry: a frame or an input event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    /// A flushed frame.
    Frame(FrameRecord),
    /// A handled input event.
    Input(InputRecord),
}

impl Entry {
    /// Whether this entry is a frame.
    #[must_use]
    pub fn is_frame(&self) -> bool {
        matches!(self, Self::Frame(_))
    }

    /// Whether this entry is an input event.
    #[must_use]
    pub fn is_input(&self) -> bool {
        matches!(self, Self::Input(_))
    }

    /// The entry's time since the recorder was created.
    #[must_use]
    pub fn at(&self) -> Duration {
        match self {
            Self::Frame(frame) => frame.at,
            Self::Input(input) => input.at,
        }
    }
}

/// The shared state behind a [`SessionRecorder`] handle.
#[derive(Debug)]
struct SessionState {
    /// Frame retention limit (`usize::MAX` when unbounded).
    capacity: usize,
    /// When the recording started; all timestamps are relative to it.
    started: Instant,
    /// The append-only timeline, in capture order.
    entries: std::collections::VecDeque<Entry>,
    /// The absolute number the *next* captured frame gets.
    next_frame: u64,
    /// Frames evicted from the front of the ring.
    dropped_frames: u64,
    /// Input events evicted together with an older frame.
    dropped_events: u64,
}

/// A time-travel session recorder: every flushed frame and every recorded
/// input event, on one timestamped timeline.
///
/// See the [module docs](self) for the model; the recorder is a cheap
/// `Clone` handle onto one timeline — clone it before handing it to the
/// terminal via
/// [`Terminal::set_preview`](crate::terminal::Terminal::set_preview) and
/// keep calling [`record_event`](Self::record_event) on the original (or
/// the clone) while the terminal records.
#[derive(Debug, Clone)]
pub struct SessionRecorder {
    state: Arc<Mutex<SessionState>>,
}

impl SessionRecorder {
    /// Creates a recorder that retains the newest `max_frames` frames
    /// (plus their following input events), evicting older entries when
    /// full. A capacity of 0 is clamped to 1.
    #[must_use]
    pub fn bounded(max_frames: usize) -> Self {
        Self {
            state: Arc::new(Mutex::new(SessionState {
                capacity: max_frames.max(1),
                started: Instant::now(),
                entries: std::collections::VecDeque::new(),
                next_frame: 0,
                dropped_frames: 0,
                dropped_events: 0,
            })),
        }
    }

    /// Creates a recorder that keeps every frame and every event.
    #[must_use]
    pub fn unbounded() -> Self {
        Self::bounded(usize::MAX)
    }

    /// The frame retention limit (`usize::MAX` when unbounded).
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.lock().capacity
    }

    /// Records an input event the app just handled. Call this from the
    /// event loop, next to the state update the event caused: the
    /// timeline then interleaves keystrokes with the frames they
    /// produced.
    pub fn record_event(&self, event: Event) {
        let mut state = self.lock();
        let at = state.started.elapsed();
        state
            .entries
            .push_back(Entry::Input(InputRecord { at, event }));
    }

    /// Time elapsed since the recording started.
    #[must_use]
    pub fn duration(&self) -> Duration {
        self.lock().started.elapsed()
    }

    /// Timeline entries currently retained (frames and input events, in
    /// capture order).
    #[must_use]
    pub fn len(&self) -> usize {
        self.lock().entries.len()
    }

    /// Whether nothing is retained.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Retained frames.
    #[must_use]
    pub fn frame_count(&self) -> usize {
        self.lock()
            .entries
            .iter()
            .filter(|entry| entry.is_frame())
            .count()
    }

    /// Retained input events.
    #[must_use]
    pub fn event_count(&self) -> usize {
        self.lock()
            .entries
            .iter()
            .filter(|entry| entry.is_input())
            .count()
    }

    /// Frames ever captured (retained + evicted).
    #[must_use]
    pub fn total_frames(&self) -> u64 {
        self.lock().next_frame
    }

    /// Frames evicted from the front of the ring so far.
    #[must_use]
    pub fn dropped_frames(&self) -> u64 {
        self.lock().dropped_frames
    }

    /// Input events ever recorded (retained + evicted).
    #[must_use]
    pub fn total_events(&self) -> u64 {
        let state = self.lock();
        state.dropped_events
            + state
                .entries
                .iter()
                .filter(|entry| entry.is_input())
                .count() as u64
    }

    /// Input events evicted together with an older frame so far.
    #[must_use]
    pub fn dropped_events(&self) -> u64 {
        self.lock().dropped_events
    }

    /// The retained frame at local `index` (0 = oldest retained frame).
    #[must_use]
    pub fn frame(&self, index: usize) -> Option<FrameRecord> {
        self.frames().into_iter().nth(index)
    }

    /// The most recent retained frame, if any.
    #[must_use]
    pub fn latest_frame(&self) -> Option<FrameRecord> {
        self.frames().pop()
    }

    /// Every retained frame, oldest first.
    #[must_use]
    pub fn frames(&self) -> Vec<FrameRecord> {
        self.lock()
            .entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::Frame(frame) => Some(frame.clone()),
                Entry::Input(_) => None,
            })
            .collect()
    }

    /// Every retained input event, oldest first.
    #[must_use]
    pub fn events(&self) -> Vec<InputRecord> {
        self.lock()
            .entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::Input(input) => Some(input.clone()),
                Entry::Frame(_) => None,
            })
            .collect()
    }

    /// The merged timeline, oldest first.
    #[must_use]
    pub fn entries(&self) -> Vec<Entry> {
        self.lock().entries.iter().cloned().collect()
    }

    /// Retained frames captured in the half-open time window
    /// `[from, to)`, oldest first.
    #[must_use]
    pub fn frames_between(&self, from: Duration, to: Duration) -> Vec<FrameRecord> {
        self.frames()
            .into_iter()
            .filter(|frame| frame.at >= from && frame.at < to)
            .collect()
    }

    /// Retained input events recorded in the half-open time window
    /// `[from, to)`, oldest first.
    #[must_use]
    pub fn events_between(&self, from: Duration, to: Duration) -> Vec<InputRecord> {
        self.events()
            .into_iter()
            .filter(|input| input.at >= from && input.at < to)
            .collect()
    }

    /// Empties the timeline and resets the counters: the start of a new
    /// recording.
    pub fn clear(&mut self) {
        let mut state = self.lock();
        state.entries.clear();
        state.next_frame = 0;
        state.dropped_frames = 0;
        state.dropped_events = 0;
        state.started = Instant::now();
    }

    /// Locks the state, recovering from a poisoned mutex (a panicking
    /// reader must not take the recording down).
    fn lock(&self) -> MutexGuard<'_, SessionState> {
        match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    /// The ring eviction: drop the oldest frame plus every input entry
    /// adjacent to the front — before it (no app state survives them)
    /// and after it, up to the next retained frame (the frame they
    /// produced is gone). The retained timeline always starts with a
    /// frame, so a replay is well-defined.
    fn evict_oldest_frame(state: &mut SessionState) {
        let mut frame_seen = false;
        loop {
            match state.entries.front() {
                None => return,
                Some(Entry::Frame(_)) if frame_seen => return,
                Some(Entry::Frame(_)) => {
                    state.entries.pop_front();
                    state.dropped_frames += 1;
                    frame_seen = true;
                }
                Some(Entry::Input(_)) => {
                    state.entries.pop_front();
                    state.dropped_events += 1;
                }
            }
        }
    }
}

impl FrameSink for SessionRecorder {
    fn on_frame(&mut self, ansi: &str) {
        let mut state = self.lock();
        let number = state.next_frame;
        state.next_frame += 1;
        if state.capacity != usize::MAX
            && state
                .entries
                .iter()
                .filter(|entry| entry.is_frame())
                .count()
                == state.capacity
        {
            Self::evict_oldest_frame(&mut state);
        }
        let at = state.started.elapsed();
        state.entries.push_back(Entry::Frame(FrameRecord {
            number,
            at,
            ansi: ansi.to_string(),
        }));
    }
}

/// A session document failure: what went wrong and the byte offset it
/// was detected at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionError {
    /// Byte offset into the document.
    pub at: usize,
    /// What went wrong.
    pub kind: SessionErrorKind,
}

impl core::fmt::Display for SessionError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "session document error at byte {}: {:?}",
            self.at, self.kind
        )
    }
}

impl std::error::Error for SessionError {}

/// Why a session document failed to parse.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SessionErrorKind {
    /// The document is not valid JSON for the session schema (bad
    /// syntax, wrong shape, a missing required field).
    Malformed {
        /// What the parser expected instead.
        message: String,
    },
    /// The document's `version` is not 1.
    UnsupportedVersion {
        /// The version that was found.
        found: u64,
    },
    /// An entry `kind` outside the session vocabulary.
    UnknownEntryKind {
        /// The rejected kind.
        kind: String,
    },
    /// An event `type` outside the event vocabulary.
    UnknownEventType {
        /// The rejected type.
        kind: String,
    },
    /// A key code, media key, modifier key, mouse kind or button
    /// outside the vocabulary.
    UnknownToken {
        /// The rejected token.
        token: String,
    },
}

/// The result of comparing two sessions frame-for-frame and event-for-
/// event: the golden-session regression helper. Record a session once,
/// export it, and later recordings replay the same interaction and
/// diff against the golden document.
///
/// Frames compare by absolute frame number (ansi bytes); input events
/// compare by position. Counts and orderings that differ show up as
/// changed entries.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SessionDiff {
    /// Absolute frame numbers whose ansi differs (or that exist in only
    /// one of the two sessions).
    frames_changed: Vec<u64>,
    /// Positions of input events that differ (or that exist in only one
    /// of the two sessions).
    events_changed: Vec<usize>,
}

impl SessionDiff {
    /// Whether the two sessions agree completely.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.frames_changed.is_empty() && self.events_changed.is_empty()
    }

    /// The changed (or unmatched) frame numbers, ascending.
    #[must_use]
    pub fn frames_changed(&self) -> &[u64] {
        &self.frames_changed
    }

    /// The changed (or unmatched) input-event positions, ascending.
    #[must_use]
    pub fn events_changed(&self) -> &[usize] {
        &self.events_changed
    }
}

/// A parsed JSON value: the subset the session document needs (the
/// canonical format never emits floats).
#[derive(Debug, Clone, PartialEq)]
enum Json {
    Null,
    Bool(bool),
    Number(u64),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    /// Object field lookup.
    fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// A required u64 field.
    fn number(&self, key: &str) -> Result<u64, SessionError> {
        match self.get(key) {
            Some(Json::Number(value)) => Ok(*value),
            _ => Err(SessionError {
                at: 0,
                kind: SessionErrorKind::Malformed {
                    message: format!("expected a number in `{key}`"),
                },
            }),
        }
    }

    /// A required string field.
    fn string(&self, key: &str) -> Result<String, SessionError> {
        match self.get(key) {
            Some(Json::String(value)) => Ok(value.clone()),
            _ => Err(SessionError {
                at: 0,
                kind: SessionErrorKind::Malformed {
                    message: format!("expected a string in `{key}`"),
                },
            }),
        }
    }
}

/// A minimal strict JSON parser for session documents: objects, arrays,
/// strings (with the standard escapes and surrogate pairs), `u64`
/// numbers, booleans and null. Byte-offset errors; no floats.
struct JsonParser<'a> {
    input: &'a str,
    pos: usize,
}

impl JsonParser<'_> {
    fn parse(mut self) -> Result<Json, SessionError> {
        self.skip_ws();
        let value = self.value()?;
        self.skip_ws();
        if self.pos != self.input.len() {
            return Err(self.error("trailing content after the document"));
        }
        Ok(value)
    }

    fn error(&self, message: &str) -> SessionError {
        SessionError {
            at: self.pos,
            kind: SessionErrorKind::Malformed {
                message: message.to_string(),
            },
        }
    }

    fn skip_ws(&mut self) {
        while self
            .input
            .get(self.pos..)
            .is_some_and(|rest| rest.starts_with([' ', '\t', '\n', '\r']))
        {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    fn expect(&mut self, expected: char) -> Result<(), SessionError> {
        if self.peek() == Some(expected) {
            self.pos += expected.len_utf8();
            Ok(())
        } else {
            Err(self.error(&format!("expected `{expected}`")))
        }
    }

    fn value(&mut self) -> Result<Json, SessionError> {
        match self.peek() {
            Some('{') => self.object(),
            Some('[') => self.array(),
            Some('"') => Ok(Json::String(self.string()?)),
            Some('t') => self.literal("true", Json::Bool(true)),
            Some('f') => self.literal("false", Json::Bool(false)),
            Some('n') => self.literal("null", Json::Null),
            Some(digit) if digit.is_ascii_digit() => self.number(),
            _ => Err(self.error("expected a JSON value")),
        }
    }

    fn literal(&mut self, text: &str, value: Json) -> Result<Json, SessionError> {
        if self.input[self.pos..].starts_with(text) {
            self.pos += text.len();
            Ok(value)
        } else {
            Err(self.error("invalid literal"))
        }
    }

    fn number(&mut self) -> Result<Json, SessionError> {
        let start = self.pos;
        while self
            .peek()
            .is_some_and(|character| character.is_ascii_digit())
        {
            self.pos += 1;
        }
        self.input[start..self.pos]
            .parse::<u64>()
            .map(Json::Number)
            .map_err(|_| self.error("number out of range"))
    }

    fn string(&mut self) -> Result<String, SessionError> {
        self.expect('"')?;
        let mut out = String::new();
        loop {
            let Some(character) = self.peek() else {
                return Err(self.error("unterminated string"));
            };
            match character {
                '"' => {
                    self.pos += 1;
                    return Ok(out);
                }
                '\\' => {
                    self.pos += 1;
                    self.escape(&mut out)?;
                }
                character if (character as u32) < 0x20 => {
                    return Err(self.error("raw control character in string"));
                }
                character => {
                    out.push(character);
                    self.pos += character.len_utf8();
                }
            }
        }
    }

    fn escape(&mut self, out: &mut String) -> Result<(), SessionError> {
        let Some(character) = self.peek() else {
            return Err(self.error("unterminated escape"));
        };
        self.pos += character.len_utf8();
        match character {
            '"' => out.push('"'),
            '\\' => out.push('\\'),
            '/' => out.push('/'),
            'b' => out.push('\u{8}'),
            'f' => out.push('\u{c}'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            'u' => {
                let high = self.hex4()?;
                let scalar = if (0xD800..0xDC00).contains(&high) {
                    // A surrogate pair: expect \uDC00-\uDFFF next.
                    if self.input[self.pos..].starts_with("\\u") {
                        self.pos += 2;
                        let low = self.hex4()?;
                        let combined = 0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00);
                        char::from_u32(combined)
                    } else {
                        None
                    }
                } else {
                    char::from_u32(high)
                };
                let Some(scalar) = scalar else {
                    return Err(self.error("invalid unicode escape"));
                };
                out.push(scalar);
            }
            _ => return Err(self.error("unknown escape")),
        }
        Ok(())
    }

    fn hex4(&mut self) -> Result<u32, SessionError> {
        let end = (self.pos + 4).min(self.input.len());
        if end - self.pos < 4 {
            return Err(self.error("truncated unicode escape"));
        }
        let digits = &self.input[self.pos..end];
        let value = if digits.chars().all(|c| c.is_ascii_hexdigit()) {
            u32::from_str_radix(digits, 16).ok()
        } else {
            None
        };
        self.pos = end;
        value.ok_or_else(|| self.error("invalid unicode escape"))
    }

    fn array(&mut self) -> Result<Json, SessionError> {
        self.expect('[')?;
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(']') {
            self.pos += 1;
            return Ok(Json::Array(items));
        }
        loop {
            self.skip_ws();
            items.push(self.value()?);
            self.skip_ws();
            match self.peek() {
                Some(',') => self.pos += 1,
                Some(']') => {
                    self.pos += 1;
                    return Ok(Json::Array(items));
                }
                _ => return Err(self.error("expected `,` or `]`")),
            }
        }
    }

    fn object(&mut self) -> Result<Json, SessionError> {
        self.expect('{')?;
        let mut fields = Vec::new();
        self.skip_ws();
        if self.peek() == Some('}') {
            self.pos += 1;
            return Ok(Json::Object(fields));
        }
        loop {
            self.skip_ws();
            let key = self.string()?;
            self.skip_ws();
            self.expect(':')?;
            self.skip_ws();
            let value = self.value()?;
            fields.push((key, value));
            self.skip_ws();
            match self.peek() {
                Some(',') => self.pos += 1,
                Some('}') => {
                    self.pos += 1;
                    return Ok(Json::Object(fields));
                }
                _ => return Err(self.error("expected `,` or `}`")),
            }
        }
    }
}

/// The JSON name of a key code: named keys are plain strings, `Char`
/// becomes `{"char":...}`, `F(n)` becomes `{"f":n}`, media and modifier
/// keys nest one level deeper.
fn key_code_json(code: KeyCode) -> String {
    fn media_json(media: MediaKeyCode) -> &'static str {
        match media {
            MediaKeyCode::Play => "play",
            MediaKeyCode::Pause => "pause",
            MediaKeyCode::PlayPause => "play_pause",
            MediaKeyCode::Reverse => "reverse",
            MediaKeyCode::Stop => "stop",
            MediaKeyCode::FastForward => "fast_forward",
            MediaKeyCode::Rewind => "rewind",
            MediaKeyCode::TrackNext => "track_next",
            MediaKeyCode::TrackPrevious => "track_previous",
            MediaKeyCode::Record => "record",
            MediaKeyCode::LowerVolume => "lower_volume",
            MediaKeyCode::RaiseVolume => "raise_volume",
            MediaKeyCode::Mute => "mute",
        }
    }
    fn modifier_json(modifier: ModifierKeyCode) -> &'static str {
        match modifier {
            ModifierKeyCode::LeftShift => "left_shift",
            ModifierKeyCode::LeftControl => "left_control",
            ModifierKeyCode::LeftAlt => "left_alt",
            ModifierKeyCode::LeftSuper => "left_super",
            ModifierKeyCode::LeftHyper => "left_hyper",
            ModifierKeyCode::LeftMeta => "left_meta",
            ModifierKeyCode::RightShift => "right_shift",
            ModifierKeyCode::RightControl => "right_control",
            ModifierKeyCode::RightAlt => "right_alt",
            ModifierKeyCode::RightSuper => "right_super",
            ModifierKeyCode::RightHyper => "right_hyper",
            ModifierKeyCode::RightMeta => "right_meta",
            ModifierKeyCode::IsoLevel3Shift => "iso_level3_shift",
            ModifierKeyCode::IsoLevel5Shift => "iso_level5_shift",
        }
    }
    match code {
        KeyCode::Char(character) => {
            format!("{{\"char\":\"{}\"}}", json_escape(&character.to_string()))
        }
        KeyCode::F(number) => format!("{{\"f\":{number}}}"),
        KeyCode::Media(media) => format!("{{\"media\":\"{}\"}}", media_json(media)),
        KeyCode::Modifier(modifier) => {
            format!("{{\"modifier\":\"{}\"}}", modifier_json(modifier))
        }
        KeyCode::Null => "\"null\"".to_string(),
        KeyCode::Backspace => "\"backspace\"".to_string(),
        KeyCode::Enter => "\"enter\"".to_string(),
        KeyCode::Left => "\"left\"".to_string(),
        KeyCode::Right => "\"right\"".to_string(),
        KeyCode::Up => "\"up\"".to_string(),
        KeyCode::Down => "\"down\"".to_string(),
        KeyCode::Home => "\"home\"".to_string(),
        KeyCode::End => "\"end\"".to_string(),
        KeyCode::PageUp => "\"page_up\"".to_string(),
        KeyCode::PageDown => "\"page_down\"".to_string(),
        KeyCode::Tab => "\"tab\"".to_string(),
        KeyCode::BackTab => "\"back_tab\"".to_string(),
        KeyCode::Delete => "\"delete\"".to_string(),
        KeyCode::Insert => "\"insert\"".to_string(),
        KeyCode::Esc => "\"esc\"".to_string(),
        KeyCode::CapsLock => "\"caps_lock\"".to_string(),
        KeyCode::ScrollLock => "\"scroll_lock\"".to_string(),
        KeyCode::NumLock => "\"num_lock\"".to_string(),
        KeyCode::PrintScreen => "\"print_screen\"".to_string(),
        KeyCode::Pause => "\"pause\"".to_string(),
        KeyCode::Menu => "\"menu\"".to_string(),
        KeyCode::KeypadBegin => "\"keypad_begin\"".to_string(),
    }
}

/// The named-key inverse table: the plain-string key codes.
fn named_key_from_json(name: &str) -> Option<KeyCode> {
    Some(match name {
        "null" => KeyCode::Null,
        "backspace" => KeyCode::Backspace,
        "enter" => KeyCode::Enter,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "page_up" => KeyCode::PageUp,
        "page_down" => KeyCode::PageDown,
        "tab" => KeyCode::Tab,
        "back_tab" => KeyCode::BackTab,
        "delete" => KeyCode::Delete,
        "insert" => KeyCode::Insert,
        "esc" => KeyCode::Esc,
        "caps_lock" => KeyCode::CapsLock,
        "scroll_lock" => KeyCode::ScrollLock,
        "num_lock" => KeyCode::NumLock,
        "print_screen" => KeyCode::PrintScreen,
        "pause" => KeyCode::Pause,
        "menu" => KeyCode::Menu,
        "keypad_begin" => KeyCode::KeypadBegin,
        _ => return None,
    })
}

/// Inverse of [`key_code_json`]; strict against the vocabulary.
fn key_code_from_json(value: &Json, at: usize) -> Result<KeyCode, SessionError> {
    fn media_from(name: &str) -> Option<MediaKeyCode> {
        Some(match name {
            "play" => MediaKeyCode::Play,
            "pause" => MediaKeyCode::Pause,
            "play_pause" => MediaKeyCode::PlayPause,
            "reverse" => MediaKeyCode::Reverse,
            "stop" => MediaKeyCode::Stop,
            "fast_forward" => MediaKeyCode::FastForward,
            "rewind" => MediaKeyCode::Rewind,
            "track_next" => MediaKeyCode::TrackNext,
            "track_previous" => MediaKeyCode::TrackPrevious,
            "record" => MediaKeyCode::Record,
            "lower_volume" => MediaKeyCode::LowerVolume,
            "raise_volume" => MediaKeyCode::RaiseVolume,
            "mute" => MediaKeyCode::Mute,
            _ => return None,
        })
    }
    fn modifier_from(name: &str) -> Option<ModifierKeyCode> {
        Some(match name {
            "left_shift" => ModifierKeyCode::LeftShift,
            "left_control" => ModifierKeyCode::LeftControl,
            "left_alt" => ModifierKeyCode::LeftAlt,
            "left_super" => ModifierKeyCode::LeftSuper,
            "left_hyper" => ModifierKeyCode::LeftHyper,
            "left_meta" => ModifierKeyCode::LeftMeta,
            "right_shift" => ModifierKeyCode::RightShift,
            "right_control" => ModifierKeyCode::RightControl,
            "right_alt" => ModifierKeyCode::RightAlt,
            "right_super" => ModifierKeyCode::RightSuper,
            "right_hyper" => ModifierKeyCode::RightHyper,
            "right_meta" => ModifierKeyCode::RightMeta,
            "iso_level3_shift" => ModifierKeyCode::IsoLevel3Shift,
            "iso_level5_shift" => ModifierKeyCode::IsoLevel5Shift,
            _ => return None,
        })
    }
    let unknown = |token: &str| SessionError {
        at,
        kind: SessionErrorKind::UnknownToken {
            token: token.to_string(),
        },
    };
    match value {
        Json::String(name) => named_key_from_json(name).ok_or_else(|| unknown(name)),
        Json::Object(fields) if fields.len() == 1 => {
            let (key, inner) = &fields[0];
            match (key.as_str(), inner) {
                ("char", Json::String(text)) => {
                    let mut chars = text.chars();
                    match (chars.next(), chars.next()) {
                        (Some(character), None) => Ok(KeyCode::Char(character)),
                        _ => Err(SessionError {
                            at,
                            kind: SessionErrorKind::Malformed {
                                message: "`char` must hold exactly one character".to_string(),
                            },
                        }),
                    }
                }
                ("f", Json::Number(number)) if *number <= 255 => {
                    Ok(KeyCode::F(u8::try_from(*number).unwrap_or(u8::MAX)))
                }
                ("media", Json::String(name)) => media_from(name)
                    .map(KeyCode::Media)
                    .ok_or_else(|| unknown(name)),
                ("modifier", Json::String(name)) => modifier_from(name)
                    .map(KeyCode::Modifier)
                    .ok_or_else(|| unknown(name)),
                _ => Err(SessionError {
                    at,
                    kind: SessionErrorKind::Malformed {
                        message: format!("unexpected key-code field `{key}`"),
                    },
                }),
            }
        }
        _ => Err(SessionError {
            at,
            kind: SessionErrorKind::Malformed {
                message: "expected a key code".to_string(),
            },
        }),
    }
}

/// The JSON array for a bitflags set, using `names` in bit order.
fn flags_json(flags: impl IntoIterator<Item = &'static str>) -> String {
    let names: Vec<&str> = flags.into_iter().collect();
    let quoted: Vec<String> = names.iter().map(|name| format!("\"{name}\"")).collect();
    format!("[{}]", quoted.join(","))
}

/// Modifier-bit names in bit order.
const MODIFIER_NAMES: &[(KeyModifiers, &str)] = &[
    (KeyModifiers::SHIFT, "shift"),
    (KeyModifiers::CONTROL, "control"),
    (KeyModifiers::ALT, "alt"),
    (KeyModifiers::SUPER, "super"),
    (KeyModifiers::HYPER, "hyper"),
    (KeyModifiers::META, "meta"),
];

/// Event-state names in bit order.
const STATE_NAMES: &[(KeyEventState, &str)] = &[
    (KeyEventState::KEYPAD, "keypad"),
    (KeyEventState::CAPS_LOCK, "caps_lock"),
    (KeyEventState::NUM_LOCK, "num_lock"),
];

/// The JSON document for one event.
fn event_json(event: &Event) -> String {
    match event {
        Event::FocusGained => "{\"type\":\"focus_gained\"}".to_string(),
        Event::FocusLost => "{\"type\":\"focus_lost\"}".to_string(),
        Event::Key(key) => {
            let modifiers = MODIFIER_NAMES
                .iter()
                .filter(|(bit, _)| key.modifiers.contains(*bit))
                .map(|(_, name)| *name);
            let state = STATE_NAMES
                .iter()
                .filter(|(bit, _)| key.state.contains(*bit))
                .map(|(_, name)| *name);
            let kind = match key.kind {
                KeyEventKind::Press => "press",
                KeyEventKind::Repeat => "repeat",
                KeyEventKind::Release => "release",
            };
            format!(
                "{{\"type\":\"key\",\"code\":{},\"modifiers\":{},\"kind\":\"{kind}\",\"state\":{}}}",
                key_code_json(key.code),
                flags_json(modifiers),
                flags_json(state)
            )
        }
        Event::Mouse(mouse) => {
            let modifiers = MODIFIER_NAMES
                .iter()
                .filter(|(bit, _)| mouse.modifiers.contains(*bit))
                .map(|(_, name)| *name);
            let (kind, button): (String, Option<&str>) = match mouse.kind {
                MouseEventKind::Down(button) => ("down".to_string(), Some(button_json(button))),
                MouseEventKind::Up(button) => ("up".to_string(), Some(button_json(button))),
                MouseEventKind::Drag(button) => ("drag".to_string(), Some(button_json(button))),
                MouseEventKind::Moved => ("moved".to_string(), None),
                MouseEventKind::ScrollDown => ("scroll_down".to_string(), None),
                MouseEventKind::ScrollUp => ("scroll_up".to_string(), None),
                MouseEventKind::ScrollLeft => ("scroll_left".to_string(), None),
                MouseEventKind::ScrollRight => ("scroll_right".to_string(), None),
            };
            let button = button
                .map(|button| format!("\"button\":\"{button}\","))
                .unwrap_or_default();
            format!(
                "{{\"type\":\"mouse\",\"kind\":\"{kind}\",{button}\"column\":{},\"row\":{},\"modifiers\":{}}}",
                mouse.column,
                mouse.row,
                flags_json(modifiers)
            )
        }
        Event::Paste(text) => {
            format!("{{\"type\":\"paste\",\"text\":\"{}\"}}", json_escape(text))
        }
        Event::Resize(width, height) => {
            format!("{{\"type\":\"resize\",\"width\":{width},\"height\":{height}}}")
        }
    }
}

/// The JSON name of a mouse button.
fn button_json(button: MouseButton) -> &'static str {
    match button {
        MouseButton::Left => "left",
        MouseButton::Right => "right",
        MouseButton::Middle => "middle",
    }
}

/// The `key` event branch of [`event_from_json`].
fn key_from_json(value: &Json, at: usize) -> Result<Event, SessionError> {
    let malformed = |message: &str| SessionError {
        at,
        kind: SessionErrorKind::Malformed {
            message: message.to_string(),
        },
    };
    let code = key_code_from_json(
        value
            .get("code")
            .ok_or_else(|| malformed("`key` needs a `code`"))?,
        at,
    )?;
    let modifiers = modifiers_from_json(value.get("modifiers"), at)?;
    let kind = match value
        .string("kind")
        .map_err(|_| malformed("`key` needs a `kind`"))?
        .as_str()
    {
        "press" => KeyEventKind::Press,
        "repeat" => KeyEventKind::Repeat,
        "release" => KeyEventKind::Release,
        other => {
            return Err(SessionError {
                at,
                kind: SessionErrorKind::UnknownToken {
                    token: other.to_string(),
                },
            });
        }
    };
    let state = state_from_json(value.get("state"), at)?;
    Ok(Event::Key(KeyEvent {
        code,
        modifiers,
        kind,
        state,
    }))
}

/// The `mouse` event branch of [`event_from_json`].
fn mouse_from_json(value: &Json, at: usize) -> Result<Event, SessionError> {
    let malformed = |message: &str| SessionError {
        at,
        kind: SessionErrorKind::Malformed {
            message: message.to_string(),
        },
    };
    let kind_name = value
        .string("kind")
        .map_err(|_| malformed("`mouse` needs a `kind`"))?;
    let kind = match kind_name.as_str() {
        "down" | "up" | "drag" => {
            let button_name = value
                .string("button")
                .map_err(|_| malformed("button events need a `button`"))?;
            let button = match button_name.as_str() {
                "left" => MouseButton::Left,
                "right" => MouseButton::Right,
                "middle" => MouseButton::Middle,
                other => {
                    return Err(SessionError {
                        at,
                        kind: SessionErrorKind::UnknownToken {
                            token: other.to_string(),
                        },
                    });
                }
            };
            match kind_name.as_str() {
                "down" => MouseEventKind::Down(button),
                "up" => MouseEventKind::Up(button),
                _ => MouseEventKind::Drag(button),
            }
        }
        "moved" => MouseEventKind::Moved,
        "scroll_up" => MouseEventKind::ScrollUp,
        "scroll_down" => MouseEventKind::ScrollDown,
        "scroll_left" => MouseEventKind::ScrollLeft,
        "scroll_right" => MouseEventKind::ScrollRight,
        other => {
            return Err(SessionError {
                at,
                kind: SessionErrorKind::UnknownToken {
                    token: other.to_string(),
                },
            });
        }
    };
    let column = u16::try_from(
        value
            .number("column")
            .map_err(|_| malformed("`mouse` needs a numeric `column`"))?,
    )
    .map_err(|_| malformed("`column` out of range"))?;
    let row = u16::try_from(
        value
            .number("row")
            .map_err(|_| malformed("`mouse` needs a numeric `row`"))?,
    )
    .map_err(|_| malformed("`row` out of range"))?;
    let modifiers = modifiers_from_json(value.get("modifiers"), at)?;
    Ok(Event::Mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers,
    }))
}

/// Inverse of [`event_json`]; strict against the vocabulary.
fn event_from_json(value: &Json, at: usize) -> Result<Event, SessionError> {
    let malformed = |message: &str| SessionError {
        at,
        kind: SessionErrorKind::Malformed {
            message: message.to_string(),
        },
    };
    let event_type = value
        .string("type")
        .map_err(|_| malformed("every event needs a string `type`"))?;
    let unknown = || SessionError {
        at,
        kind: SessionErrorKind::UnknownEventType {
            kind: event_type.clone(),
        },
    };
    match event_type.as_str() {
        "focus_gained" => Ok(Event::FocusGained),
        "focus_lost" => Ok(Event::FocusLost),
        "paste" => Ok(Event::Paste(
            value
                .string("text")
                .map_err(|_| malformed("`paste` needs a string `text`"))?,
        )),
        "resize" => {
            let width = value
                .number("width")
                .map_err(|_| malformed("`resize` needs a numeric `width`"))?;
            let height = value
                .number("height")
                .map_err(|_| malformed("`resize` needs a numeric `height`"))?;
            let width = u16::try_from(width).map_err(|_| malformed("`width` out of range"))?;
            let height = u16::try_from(height).map_err(|_| malformed("`height` out of range"))?;
            Ok(Event::Resize(width, height))
        }
        "key" => key_from_json(value, at),
        "mouse" => mouse_from_json(value, at),
        _ => Err(unknown()),
    }
}

/// A modifiers array; missing field means no modifiers.
fn modifiers_from_json(value: Option<&Json>, at: usize) -> Result<KeyModifiers, SessionError> {
    let Some(Json::Array(names)) = value else {
        return Ok(KeyModifiers::empty());
    };
    let mut modifiers = KeyModifiers::empty();
    for name in names {
        let Json::String(name) = name else {
            return Err(SessionError {
                at,
                kind: SessionErrorKind::Malformed {
                    message: "modifier names must be strings".to_string(),
                },
            });
        };
        let Some((bit, _)) = MODIFIER_NAMES.iter().find(|(_, known)| known == name) else {
            return Err(SessionError {
                at,
                kind: SessionErrorKind::UnknownToken {
                    token: name.clone(),
                },
            });
        };
        modifiers |= *bit;
    }
    Ok(modifiers)
}

/// A key-event state array; missing field means empty.
fn state_from_json(value: Option<&Json>, at: usize) -> Result<KeyEventState, SessionError> {
    let Some(Json::Array(names)) = value else {
        return Ok(KeyEventState::empty());
    };
    let mut state = KeyEventState::empty();
    for name in names {
        let Json::String(name) = name else {
            return Err(SessionError {
                at,
                kind: SessionErrorKind::Malformed {
                    message: "state names must be strings".to_string(),
                },
            });
        };
        let Some((bit, _)) = STATE_NAMES.iter().find(|(_, known)| known == name) else {
            return Err(SessionError {
                at,
                kind: SessionErrorKind::UnknownToken {
                    token: name.clone(),
                },
            });
        };
        state |= *bit;
    }
    Ok(state)
}

/// Parses the `entries` array of a session document.
fn entries_from_json(
    entries_json: &[Json],
) -> Result<std::collections::VecDeque<Entry>, SessionError> {
    let mut entries = std::collections::VecDeque::with_capacity(entries_json.len());
    let timestamp = |entry: &Json| {
        entry
            .number("t_ms")
            .map(Duration::from_millis)
            .map_err(|_| SessionError {
                at: 0,
                kind: SessionErrorKind::Malformed {
                    message: "every entry needs a numeric `t_ms`".to_string(),
                },
            })
    };
    for entry in entries_json {
        let kind = entry.string("kind").map_err(|_| SessionError {
            at: 0,
            kind: SessionErrorKind::Malformed {
                message: "every entry needs a string `kind`".to_string(),
            },
        })?;
        match kind.as_str() {
            "frame" => {
                let at = timestamp(entry)?;
                let number = entry.number("number").map_err(|_| SessionError {
                    at: 0,
                    kind: SessionErrorKind::Malformed {
                        message: "frame entries need a numeric `number`".to_string(),
                    },
                })?;
                let ansi = entry.string("ansi").map_err(|_| SessionError {
                    at: 0,
                    kind: SessionErrorKind::Malformed {
                        message: "frame entries need a string `ansi`".to_string(),
                    },
                })?;
                entries.push_back(Entry::Frame(FrameRecord { number, at, ansi }));
            }
            "input" => {
                let at = timestamp(entry)?;
                let event_json = entry
                    .get("event")
                    .ok_or(SessionError {
                        at: 0,
                        kind: SessionErrorKind::Malformed {
                            message: "input entries need an `event`".to_string(),
                        },
                    })?
                    .clone();
                let event = event_from_json(&event_json, 0)?;
                entries.push_back(Entry::Input(InputRecord { at, event }));
            }
            other => {
                return Err(SessionError {
                    at: 0,
                    kind: SessionErrorKind::UnknownEntryKind {
                        kind: other.to_string(),
                    },
                });
            }
        }
    }
    Ok(entries)
}

impl SessionRecorder {
    /// The session as a JSON document: a versioned, self-contained
    /// replay log (architecture §5.8 — "record input events alongside
    /// mutations for replayable test cases"). The format is pinned by
    /// tests:
    ///
    /// ```json
    /// {"version":1,"capacity":null,"dropped_frames":0,"dropped_events":0,
    ///  "entries":[{"kind":"frame","number":0,"t_ms":0,"ansi":"..."},
    ///             {"kind":"input","t_ms":3,"event":{"type":"focus_gained"}}]}
    /// ```
    ///
    /// Only the *retained* entries are exported (a bounded ring's
    /// evictions are summarized by the dropped counters). The document
    /// round-trips exactly through [`SessionRecorder::from_json`].
    #[must_use]
    pub fn to_json(&self) -> String {
        let state = self.lock();
        let capacity = if state.capacity == usize::MAX {
            "null".to_string()
        } else {
            state.capacity.to_string()
        };
        let mut out = String::with_capacity(state.entries.len() * 96 + 96);
        out.push_str("{\"version\":1,\"capacity\":");
        out.push_str(&capacity);
        out.push_str(",\"dropped_frames\":");
        out.push_str(&state.dropped_frames.to_string());
        out.push_str(",\"dropped_events\":");
        out.push_str(&state.dropped_events.to_string());
        out.push_str(",\"entries\":[");
        for (index, entry) in state.entries.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            match entry {
                Entry::Frame(frame) => {
                    out.push_str("{\"kind\":\"frame\",\"number\":");
                    out.push_str(&frame.number.to_string());
                    out.push_str(",\"t_ms\":");
                    out.push_str(&frame.at.as_millis().to_string());
                    out.push_str(",\"ansi\":\"");
                    out.push_str(&json_escape(&frame.ansi));
                    out.push_str("\"}");
                }
                Entry::Input(input) => {
                    out.push_str("{\"kind\":\"input\",\"t_ms\":");
                    out.push_str(&input.at.as_millis().to_string());
                    out.push_str(",\"event\":");
                    out.push_str(&event_json(&input.event));
                    out.push('}');
                }
            }
        }
        out.push_str("]}");
        out
    }

    /// Rebuilds a recorder from a [`to_json`](Self::to_json) document.
    /// The imported recording keeps its retained entries, timestamps,
    /// ring capacity and eviction counters; its clock restarts now (a
    /// replay can keep appending after the imported tail).
    ///
    /// # Errors
    ///
    /// A [`SessionError`] when the document is not a valid session:
    /// bad JSON, an unsupported version, or an entry outside the
    /// vocabulary. Strictness pins the format — hand-edited documents
    /// fail loudly instead of replaying silently wrong.
    pub fn from_json(document: &str) -> Result<Self, SessionError> {
        let parsed = JsonParser {
            input: document,
            pos: 0,
        }
        .parse()?;
        let Json::Object(_) = &parsed else {
            return Err(SessionError {
                at: 0,
                kind: SessionErrorKind::Malformed {
                    message: "expected a JSON object".to_string(),
                },
            });
        };
        let version = parsed
            .get("version")
            .and_then(|value| match value {
                Json::Number(number) => Some(*number),
                _ => None,
            })
            .ok_or(SessionError {
                at: 0,
                kind: SessionErrorKind::Malformed {
                    message: "expected a numeric `version`".to_string(),
                },
            })?;
        if version != 1 {
            return Err(SessionError {
                at: 0,
                kind: SessionErrorKind::UnsupportedVersion { found: version },
            });
        }
        let capacity = match parsed.get("capacity") {
            None | Some(Json::Null) => usize::MAX,
            Some(Json::Number(number)) => usize::try_from(*number).unwrap_or(usize::MAX),
            Some(_) => {
                return Err(SessionError {
                    at: 0,
                    kind: SessionErrorKind::Malformed {
                        message: "`capacity` must be a number or null".to_string(),
                    },
                });
            }
        };
        let dropped_frames = parsed.get("dropped_frames").map_or(0, |value| match value {
            Json::Number(number) => *number,
            _ => 0,
        });
        let dropped_events = parsed.get("dropped_events").map_or(0, |value| match value {
            Json::Number(number) => *number,
            _ => 0,
        });
        let entries_json = match parsed.get("entries") {
            Some(Json::Array(items)) => items.clone(),
            _ => {
                return Err(SessionError {
                    at: 0,
                    kind: SessionErrorKind::Malformed {
                        message: "expected an `entries` array".to_string(),
                    },
                });
            }
        };
        let entries = entries_from_json(&entries_json)?;
        let next_frame = entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::Frame(frame) => Some(frame.number + 1),
                Entry::Input(_) => None,
            })
            .max()
            .unwrap_or(0);
        Ok(Self {
            state: Arc::new(Mutex::new(SessionState {
                capacity,
                started: Instant::now(),
                entries,
                next_frame,
                dropped_frames,
                dropped_events,
            })),
        })
    }

    /// Compares this session against `other`: the golden-session
    /// regression check. Frames match by absolute number (ansi bytes),
    /// input events match by position; anything missing, extra or
    /// differing is reported. An empty diff means the two recordings
    /// replayed the same screen sequence.
    #[must_use]
    pub fn diff(&self, other: &SessionRecorder) -> SessionDiff {
        if Arc::ptr_eq(&self.state, &other.state) {
            return SessionDiff::default();
        }
        let (mine, theirs) = (self.lock(), other.lock());
        let mut diff = SessionDiff::default();
        // Frames by absolute number.
        let my_frames: std::collections::BTreeMap<u64, &str> = mine
            .entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::Frame(frame) => Some((frame.number, frame.ansi.as_str())),
                Entry::Input(_) => None,
            })
            .collect();
        let their_frames: std::collections::BTreeMap<u64, &str> = theirs
            .entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::Frame(frame) => Some((frame.number, frame.ansi.as_str())),
                Entry::Input(_) => None,
            })
            .collect();
        diff.frames_changed = my_frames
            .iter()
            .filter(|(number, ansi)| their_frames.get(number) != Some(*ansi))
            .map(|(number, _)| *number)
            .collect();
        diff.frames_changed.extend(
            their_frames
                .keys()
                .filter(|number| !my_frames.contains_key(*number))
                .copied(),
        );
        diff.frames_changed.sort_unstable();
        diff.frames_changed.dedup();
        // Events by position.
        let my_events: Vec<&Event> = mine
            .entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::Input(input) => Some(&input.event),
                Entry::Frame(_) => None,
            })
            .collect();
        let their_events: Vec<&Event> = theirs
            .entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::Input(input) => Some(&input.event),
                Entry::Frame(_) => None,
            })
            .collect();
        let shared = my_events.len().min(their_events.len());
        diff.events_changed = (0..shared)
            .filter(|index| my_events[*index] != their_events[*index])
            .collect();
        diff.events_changed.extend(shared..my_events.len());
        diff.events_changed.extend(shared..their_events.len());
        diff
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::empty()))
    }

    #[test]
    fn frames_capture_a_numbered_timeline() {
        let recorder = SessionRecorder::unbounded();
        recorder.clone().on_frame("one");
        recorder.clone().on_frame("two");
        recorder.clone().on_frame("three");
        let frames = recorder.frames();
        assert_eq!(frames.len(), 3);
        let numbers: Vec<u64> = frames.iter().map(|frame| frame.number).collect();
        assert_eq!(numbers, [0, 1, 2]);
        assert!(frames[0].at <= frames[1].at && frames[1].at <= frames[2].at);
        assert_eq!(frames[2].ansi, "three");
        assert_eq!(recorder.len(), 3);
        assert!(!recorder.is_empty());
    }

    #[test]
    fn input_interleaves_on_the_timeline() {
        let recorder = SessionRecorder::unbounded();
        recorder.clone().on_frame("a");
        recorder.record_event(key(KeyCode::Enter));
        recorder.clone().on_frame("b");
        let kinds: Vec<(bool, bool)> = recorder
            .entries()
            .iter()
            .map(|entry| (entry.is_frame(), entry.is_input()))
            .collect();
        assert_eq!(kinds, [(true, false), (false, true), (true, false)]);
        let Some(Entry::Input(input)) = recorder.entries().into_iter().nth(1) else {
            panic!("expected the middle entry to be input");
        };
        assert_eq!(input.event, key(KeyCode::Enter));
    }

    #[test]
    fn entry_at_reports_a_monotone_timeline() {
        let recorder = SessionRecorder::unbounded();
        recorder.clone().on_frame("a");
        recorder.record_event(Event::FocusGained);
        recorder.clone().on_frame("b");
        let entries = recorder.entries();
        assert!(entries[0].at() <= entries[1].at());
        assert!(entries[1].at() <= entries[2].at());
        assert!(recorder.duration() >= entries[2].at());
    }

    #[test]
    fn bounded_ring_evicts_the_oldest_frame_and_stale_input() {
        let recorder = SessionRecorder::bounded(2);
        recorder.clone().on_frame("f0");
        recorder.record_event(Event::FocusGained);
        recorder.clone().on_frame("f1");
        recorder.clone().on_frame("f2");
        // f0 and the input recorded between f0 and f1 are gone together.
        assert_eq!(recorder.dropped_frames(), 1);
        assert_eq!(recorder.dropped_events(), 1);
        let frames = recorder.frames();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].ansi, "f1");
        assert_eq!(frames[1].ansi, "f2");
        assert_eq!(recorder.event_count(), 0);
        assert_eq!(recorder.len(), 2);
        // The retained timeline starts with a frame.
        assert!(recorder.entries().first().is_some_and(Entry::is_frame));
    }

    #[test]
    fn eviction_drops_events_up_to_the_next_retained_frame() {
        let recorder = SessionRecorder::bounded(1);
        recorder.clone().on_frame("f0");
        recorder.record_event(key(KeyCode::Left));
        recorder.record_event(key(KeyCode::Right));
        recorder.clone().on_frame("f1");
        // Evicting f0 also drops the two events between f0 and f1: their
        // producing frame is gone, so a replay from them would start
        // from unknown state.
        assert_eq!(recorder.dropped_frames(), 1);
        assert_eq!(recorder.dropped_events(), 2);
        assert_eq!(recorder.len(), 1);
        assert_eq!(recorder.frame(0).unwrap().ansi, "f1");
    }

    #[test]
    fn events_between_retained_frames_survive_eviction() {
        let recorder = SessionRecorder::bounded(2);
        recorder.clone().on_frame("f0");
        recorder.clone().on_frame("f1");
        recorder.record_event(key(KeyCode::Left));
        recorder.record_event(key(KeyCode::Right));
        recorder.clone().on_frame("f2");
        // f0 is evicted; the events between the retained f1 and f2 stay.
        assert_eq!(recorder.dropped_frames(), 1);
        assert_eq!(recorder.dropped_events(), 0);
        assert_eq!(recorder.len(), 4);
        assert_eq!(recorder.frame_count(), 2);
        assert_eq!(recorder.event_count(), 2);
    }

    #[test]
    fn events_survive_while_no_frame_is_evicted() {
        let recorder = SessionRecorder::bounded(1);
        recorder.clone().on_frame("f0");
        recorder.record_event(key(KeyCode::Left));
        recorder.record_event(key(KeyCode::Right));
        // Recording never evicts: the events trail the last frame.
        assert_eq!(recorder.dropped_frames(), 0);
        assert_eq!(recorder.dropped_events(), 0);
        assert_eq!(recorder.len(), 3);
        assert_eq!(recorder.event_count(), 2);
    }

    #[test]
    fn absolute_numbers_count_evictions() {
        let recorder = SessionRecorder::bounded(2);
        for tick in 0..5 {
            recorder.clone().on_frame("f");
            let _ = tick;
        }
        assert_eq!(recorder.dropped_frames(), 3);
        assert_eq!(recorder.total_frames(), 5);
        // dropped_frames + local index == absolute number.
        let retained: Vec<u64> = recorder.frames().iter().map(|frame| frame.number).collect();
        assert_eq!(retained, [3, 4]);
    }

    #[test]
    fn time_windows_are_half_open() {
        let recorder = SessionRecorder::unbounded();
        recorder.clone().on_frame("early");
        std::thread::sleep(Duration::from_millis(3));
        recorder.record_event(key(KeyCode::Enter));
        std::thread::sleep(Duration::from_millis(3));
        recorder.clone().on_frame("late");
        let split = recorder.entries()[1].at();
        let early = recorder.frames_between(Duration::ZERO, split);
        let late = recorder.frames_between(split, recorder.duration());
        assert_eq!(early.len(), 1);
        assert_eq!(early[0].ansi, "early");
        assert_eq!(late.len(), 1);
        assert_eq!(late[0].ansi, "late");
        // Half-open: the boundary itself belongs to the second window.
        let events = recorder.events_between(split, recorder.duration());
        assert_eq!(events.len(), 1);
        assert_eq!(recorder.frames_between(split, split), []);
    }

    #[test]
    fn recorder_handle_records_and_reads() {
        let recorder = SessionRecorder::unbounded();
        recorder.clone().on_frame("f0");
        recorder.record_event(Event::FocusGained);
        recorder.clone().on_frame("f1");
        assert_eq!(recorder.len(), 3);
        assert_eq!(recorder.frame_count(), 2);
        assert_eq!(recorder.event_count(), 1);
        assert_eq!(recorder.total_frames(), 2);
        assert_eq!(recorder.total_events(), 1);
        assert_eq!(recorder.dropped_frames(), 0);
        assert_eq!(recorder.dropped_events(), 0);
        let frame = recorder.frame(1).unwrap();
        assert_eq!(frame.number, 1);
        assert_eq!(frame.ansi, "f1");
        assert_eq!(recorder.latest_frame().unwrap().number, 1);
        assert_eq!(recorder.frame(2), None);
        assert_eq!(recorder.capacity(), usize::MAX);
    }

    #[test]
    fn bounded_recorder_counts_evictions() {
        let recorder = SessionRecorder::bounded(2);
        recorder.clone().on_frame("f0");
        recorder.record_event(Event::FocusLost);
        recorder.clone().on_frame("f1");
        recorder.clone().on_frame("f2");
        assert_eq!(recorder.dropped_frames(), 1);
        assert_eq!(recorder.dropped_events(), 1);
        assert_eq!(recorder.frame_count(), 2);
        assert_eq!(recorder.frame(0).unwrap().ansi, "f1");
        assert_eq!(recorder.total_frames(), 3);
        assert_eq!(recorder.total_events(), 1);
        assert_eq!(recorder.capacity(), 2);
    }

    #[test]
    fn zero_capacity_is_clamped_to_one() {
        let recorder = SessionRecorder::bounded(0);
        assert_eq!(recorder.capacity(), 1);
        recorder.clone().on_frame("only");
        recorder.clone().on_frame("bumps");
        assert_eq!(recorder.frame_count(), 1);
        assert_eq!(recorder.frame(0).unwrap().ansi, "bumps");
    }

    #[test]
    fn cloned_handles_share_one_timeline() {
        let recorder = SessionRecorder::unbounded();
        let reader = recorder.clone();
        recorder.clone().on_frame("shared");
        recorder.record_event(key(KeyCode::Tab));
        assert_eq!(reader.len(), 2);
        assert_eq!(reader.event_count(), 1);
    }

    #[test]
    fn clear_resets_the_handle() {
        let mut recorder = SessionRecorder::unbounded();
        recorder.clone().on_frame("old");
        recorder.record_event(Event::FocusGained);
        recorder.clear();
        assert!(recorder.is_empty());
        assert_eq!(recorder.total_frames(), 0);
        assert_eq!(recorder.total_events(), 0);
        assert_eq!(recorder.dropped_frames(), 0);
        recorder.clone().on_frame("new");
        assert_eq!(recorder.frame(0).unwrap().number, 0);
    }

    #[test]
    fn recorder_is_send_and_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}
        assert_send::<SessionRecorder>();
        assert_sync::<SessionRecorder>();
    }

    #[test]
    fn concurrent_recording_never_loses_entries() {
        let recorder = SessionRecorder::unbounded();
        let handles: Vec<_> = (0..4)
            .map(|thread| {
                let recorder = recorder.clone();
                std::thread::spawn(move || {
                    for tick in 0..25 {
                        if thread % 2 == 0 {
                            recorder.clone().on_frame("x");
                        } else {
                            recorder.record_event(key(KeyCode::Char(
                                char::from_u32(97 + tick % 26).unwrap_or('x'),
                            )));
                        }
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
        assert_eq!(recorder.frame_count(), 50);
        assert_eq!(recorder.event_count(), 50);
        assert_eq!(recorder.len(), 100);
        assert_eq!(recorder.total_frames(), 50);
        assert_eq!(recorder.total_events(), 50);
    }

    #[test]
    fn to_json_produces_the_documented_shape() {
        let recorder = SessionRecorder::unbounded();
        recorder.clone().on_frame("ANSI");
        recorder.record_event(Event::FocusGained);
        let json = recorder.to_json();
        assert!(
            json.starts_with("{\"version\":1,\"capacity\":null,\"dropped_frames\":0,\"dropped_events\":0,\"entries\":["),
            "{json}"
        );
        assert!(
            json.contains("{\"kind\":\"frame\",\"number\":0,\"t_ms\":0,\"ansi\":\"ANSI\"}"),
            "{json}"
        );
        assert!(
            json.contains("{\"kind\":\"input\",\"t_ms\":0,\"event\":{\"type\":\"focus_gained\"}}"),
            "{json}"
        );
        assert!(json.ends_with("]}"));
    }

    #[test]
    fn json_round_trips_all_entry_kinds() {
        let recorder = SessionRecorder::bounded(3);
        recorder.clone().on_frame("f0");
        recorder.record_event(key(KeyCode::Enter));
        recorder.clone().on_frame("f1");
        recorder.clone().on_frame("f2");
        let json = recorder.to_json();
        let imported = SessionRecorder::from_json(&json).unwrap();
        assert_eq!(imported.capacity(), 3);
        assert_eq!(imported.dropped_frames(), recorder.dropped_frames());
        assert_eq!(imported.dropped_events(), recorder.dropped_events());
        assert_eq!(imported.total_frames(), 3);
        // Timestamps truncate to milliseconds on export, so the closed
        // form of the round trip is re-serialization identity plus
        // frame/event content equality.
        assert_eq!(imported.to_json(), json);
        let (frames, events): (Vec<_>, Vec<_>) = (
            imported.frames().into_iter().map(|f| f.ansi).collect(),
            imported.events().into_iter().map(|i| i.event).collect(),
        );
        assert_eq!(frames, ["f0", "f1", "f2"]);
        assert_eq!(events, [key(KeyCode::Enter)]);
    }

    #[test]
    fn event_encoding_round_trips_a_corpus() {
        use crate::event::{KeyEventKind, KeyModifiers};
        let corpus = vec![
            Event::FocusGained,
            Event::FocusLost,
            Event::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::empty())),
            Event::Key(KeyEvent::new(
                KeyCode::Char('\u{7f}'),
                KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT,
            )),
            Event::Key(KeyEvent::new(KeyCode::F(12), KeyModifiers::SUPER)),
            Event::Key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::empty())),
            Event::Key(KeyEvent::new(
                KeyCode::Media(MediaKeyCode::PlayPause),
                KeyModifiers::empty(),
            )),
            Event::Key(KeyEvent::new(
                KeyCode::Modifier(ModifierKeyCode::RightMeta),
                KeyModifiers::empty(),
            )),
            Event::Key(KeyEvent {
                code: KeyCode::KeypadBegin,
                modifiers: KeyModifiers::META,
                kind: KeyEventKind::Repeat,
                state: KeyEventState::KEYPAD | KeyEventState::NUM_LOCK,
            }),
            Event::Key(KeyEvent {
                code: KeyCode::Delete,
                modifiers: KeyModifiers::empty(),
                kind: KeyEventKind::Release,
                state: KeyEventState::empty(),
            }),
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 3,
                row: 4,
                modifiers: KeyModifiers::CONTROL,
            }),
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Drag(MouseButton::Middle),
                column: 0,
                row: 0,
                modifiers: KeyModifiers::empty(),
            }),
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::ScrollUp,
                column: 10,
                row: 20,
                modifiers: KeyModifiers::empty(),
            }),
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Moved,
                column: 1,
                row: 2,
                modifiers: KeyModifiers::empty(),
            }),
            Event::Paste("two\nlines \"quoted\" \u{1b} 汉 🙂".to_string()),
            Event::Resize(200, 60),
        ];
        let recorder = SessionRecorder::unbounded();
        for event in &corpus {
            recorder.record_event(event.clone());
        }
        let imported = SessionRecorder::from_json(&recorder.to_json()).unwrap();
        let replayed: Vec<Event> = imported
            .events()
            .into_iter()
            .map(|input| input.event)
            .collect();
        assert_eq!(replayed, corpus);
    }

    #[test]
    fn unsupported_versions_are_rejected() {
        let recorder = SessionRecorder::unbounded();
        recorder.clone().on_frame("f");
        let json = recorder.to_json().replace("\"version\":1", "\"version\":2");
        let error = SessionRecorder::from_json(&json).unwrap_err();
        assert_eq!(
            error.kind,
            SessionErrorKind::UnsupportedVersion { found: 2 }
        );
    }

    #[test]
    fn malformed_documents_report_errors() {
        assert!(SessionRecorder::from_json("not json").is_err());
        assert!(SessionRecorder::from_json("[]").is_err());
        assert!(SessionRecorder::from_json("{}").is_err());
        assert!(SessionRecorder::from_json("{\"version\":1}").is_err());
        let error = SessionRecorder::from_json("{\"version\":1,").unwrap_err();
        assert!(matches!(error.kind, SessionErrorKind::Malformed { .. }));
    }

    #[test]
    fn unknown_entry_and_event_kinds_are_rejected() {
        let error =
            SessionRecorder::from_json("{\"version\":1,\"entries\":[{\"kind\":\"snapshot\"}]}")
                .unwrap_err();
        assert_eq!(
            error.kind,
            SessionErrorKind::UnknownEntryKind {
                kind: "snapshot".to_string()
            }
        );
        let error = SessionRecorder::from_json(
            "{\"version\":1,\"entries\":[{\"kind\":\"input\",\"t_ms\":0,\"event\":{\"type\":\"sunspot\"}}]}",
        )
        .unwrap_err();
        assert_eq!(
            error.kind,
            SessionErrorKind::UnknownEventType {
                kind: "sunspot".to_string()
            }
        );
        // An unknown key code inside an otherwise fine event.
        let error = SessionRecorder::from_json(
            "{\"version\":1,\"entries\":[{\"kind\":\"input\",\"t_ms\":0,\"event\":{\"type\":\"key\",\"code\":\"warp\",\"modifiers\":[],\"kind\":\"press\",\"state\":[]}}]}",
        )
        .unwrap_err();
        assert_eq!(
            error.kind,
            SessionErrorKind::UnknownToken {
                token: "warp".to_string()
            }
        );
    }

    #[test]
    fn diff_is_empty_for_identical_sessions() {
        let golden = SessionRecorder::unbounded();
        golden.clone().on_frame("f0");
        golden.record_event(key(KeyCode::Enter));
        golden.clone().on_frame("f1");
        let replay = SessionRecorder::unbounded();
        replay.clone().on_frame("f0");
        replay.record_event(key(KeyCode::Enter));
        replay.clone().on_frame("f1");
        assert!(golden.diff(&replay).is_empty());
        assert!(golden.diff(&golden).is_empty());
    }

    #[test]
    fn diff_reports_changed_frames_and_events() {
        let golden = SessionRecorder::unbounded();
        golden.clone().on_frame("f0");
        golden.record_event(key(KeyCode::Enter));
        golden.clone().on_frame("f1");
        let replay = SessionRecorder::unbounded();
        replay.clone().on_frame("f0");
        replay.record_event(key(KeyCode::Char('x')));
        replay.clone().on_frame("REGRESSED");
        let diff = golden.diff(&replay);
        assert_eq!(diff.frames_changed(), [1]);
        assert_eq!(diff.events_changed(), [0]);
        assert!(!diff.is_empty());
    }

    #[test]
    fn diff_reports_missing_and_extra_frames() {
        let golden = SessionRecorder::unbounded();
        golden.clone().on_frame("f0");
        golden.clone().on_frame("f1");
        golden.clone().on_frame("f2");
        let short = SessionRecorder::unbounded();
        short.clone().on_frame("f0");
        short.clone().on_frame("f1");
        let diff = golden.diff(&short);
        assert_eq!(diff.frames_changed(), [2]);
        let long = SessionRecorder::unbounded();
        long.clone().on_frame("f0");
        long.clone().on_frame("f1");
        long.clone().on_frame("f2");
        long.clone().on_frame("f3");
        let diff = golden.diff(&long);
        assert_eq!(diff.frames_changed(), [3]);
    }

    #[test]
    fn the_golden_pattern_catches_a_regression() {
        // Record once, export, and treat the document as the golden log.
        let recorded = SessionRecorder::unbounded();
        recorded.clone().on_frame("menu");
        recorded.record_event(key(KeyCode::Down));
        recorded.clone().on_frame("menu^");
        let golden = recorded.to_json();

        // A later run that behaves the same passes...
        let good = SessionRecorder::unbounded();
        good.clone().on_frame("menu");
        good.record_event(key(KeyCode::Down));
        good.clone().on_frame("menu^");
        assert!(
            good.diff(&SessionRecorder::from_json(&golden).unwrap())
                .is_empty()
        );

        // ...and one that renders differently fails, pointing at the frame.
        let bad = SessionRecorder::unbounded();
        bad.clone().on_frame("menu");
        bad.record_event(key(KeyCode::Down));
        bad.clone().on_frame("menu^ v2");
        let diff = bad.diff(&SessionRecorder::from_json(&golden).unwrap());
        assert_eq!(diff.frames_changed(), [1]);
    }

    #[test]
    fn imported_recordings_keep_accepting_new_frames() {
        let recorder = SessionRecorder::unbounded();
        recorder.clone().on_frame("f0");
        let imported = SessionRecorder::from_json(&recorder.to_json()).unwrap();
        imported.clone().on_frame("f1");
        assert_eq!(imported.frame(1).unwrap().number, 1);
        assert_eq!(imported.total_frames(), 2);
    }

    #[test]
    fn surrogate_pairs_in_paste_text_round_trip() {
        // The writer emits astral characters as raw UTF-8...
        let recorder = SessionRecorder::unbounded();
        recorder.record_event(Event::Paste("\u{1F642}".to_string()));
        let json = recorder.to_json();
        assert!(json.contains('\u{1F642}'), "{json}");
        let imported = SessionRecorder::from_json(&json).unwrap();
        assert_eq!(
            imported.events()[0].event,
            Event::Paste("\u{1F642}".to_string())
        );
        // ...while the parser still accepts hand-written surrogate pairs.
        let hand_written = "{\"version\":1,\"entries\":[{\"kind\":\"input\",\"t_ms\":0,\
            \"event\":{\"type\":\"paste\",\"text\":\"\\ud83d\\ude42\"}}]}";
        let imported = SessionRecorder::from_json(hand_written).unwrap();
        assert_eq!(
            imported.events()[0].event,
            Event::Paste("\u{1F642}".to_string())
        );
    }
}
