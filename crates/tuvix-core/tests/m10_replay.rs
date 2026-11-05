//! The M10 replay gates: end-to-end over the real terminal pipeline.
//!
//! - a session recorded through `Terminal` + `SessionRecorder` replays
//!   byte-identically through the same app logic (the golden loop);
//! - a divergent app names the exact frame that changed;
//! - bounded rings replay their retained window;
//! - the M9 JSON export replays after a round trip.

use tuvix_core::backend::TestBackend;
use tuvix_core::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use tuvix_core::geometry::Size;
use tuvix_core::replay::{ReplayStep, SessionReplay};
use tuvix_core::session::SessionRecorder;
use tuvix_core::style::Style;
use tuvix_core::terminal::Terminal;

fn key(character: char) -> Event {
    Event::Key(KeyEvent::new(
        KeyCode::Char(character),
        KeyModifiers::empty(),
    ))
}

/// The app under test: typed chars accumulate, Backspace pops, and every
/// draw repaints the full line. Returns the handler so the recorder and
/// the replay share one implementation.
fn handle(state: &mut String, event: &Event) {
    if let Event::Key(key_event) = event {
        match key_event.code {
            KeyCode::Char(character) => state.push(character),
            KeyCode::Backspace => {
                state.pop();
            }
            _ => {}
        }
    }
}

fn paint(state: &str, step: &mut ReplayStep<'_>) {
    step.buffer.set_string(0, 0, state, Style::new());
}

#[test]
fn a_recorded_session_replays_golden_through_the_pipeline() {
    let recorder = SessionRecorder::unbounded();
    let mut terminal = Terminal::new(TestBackend::new(Size::new(40, 8))).unwrap();
    terminal.set_preview(recorder.clone());

    let mut state = String::new();
    let script = ['h', 'i', '!', '\u{6f22}']; // typing, then a wide glyph
    for character in script {
        let event = key(character);
        recorder.record_event(event.clone());
        handle(&mut state, &event);
        terminal
            .draw(|frame| {
                frame.buffer.set_string(0, 0, state.as_str(), Style::new());
            })
            .unwrap();
    }
    // a repaint with no input (e.g. a tick) — a quiet step
    terminal.draw(|_| {}).unwrap();
    // then a deletion
    let event = Event::Key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::empty()));
    recorder.record_event(event.clone());
    handle(&mut state, &event);
    terminal
        .draw(|frame| {
            frame.buffer.set_string(0, 0, state.as_str(), Style::new());
        })
        .unwrap();

    assert_eq!(recorder.frame_count(), 6);
    assert_eq!(recorder.event_count(), 5);

    let replay = SessionReplay::from_recorder(&recorder);
    assert_eq!(replay.frame_count(), 6);
    assert_eq!(replay.event_count(), 5);

    let mut replayed = String::new();
    let report = replay
        .run(|mut step| {
            for event in step.events {
                handle(&mut replayed, event);
            }
            paint(&replayed, &mut step);
        })
        .expect("recorded snapshots are canonical");
    assert!(
        report.is_golden(),
        "mismatches: {:?}",
        report
            .mismatches()
            .first()
            .map(std::string::ToString::to_string)
    );
    assert_eq!(report.steps(), 6);
    assert_eq!(report.matched(), 6);
    assert_eq!(report.unverified_events(), 0);
    // the live app and the replayed app agree — including the Backspace
    assert_eq!(replayed, "hi!");
}

#[test]
fn a_divergent_app_names_the_culprit_frame() {
    let recorder = SessionRecorder::unbounded();
    let mut terminal = Terminal::new(TestBackend::new(Size::new(20, 3))).unwrap();
    terminal.set_preview(recorder.clone());

    let mut state = String::new();
    for character in ['a', 'b', 'c'] {
        let event = key(character);
        recorder.record_event(event.clone());
        handle(&mut state, &event);
        terminal
            .draw(|frame| {
                frame.buffer.set_string(0, 0, state.as_str(), Style::new());
            })
            .unwrap();
    }

    let replay = SessionReplay::from_recorder(&recorder);
    // the regression: the app drops uppercase-ish glyphs — 'b' vanishes
    let mut replayed = String::new();
    let report = replay
        .run(|mut step| {
            for event in step.events {
                if let Event::Key(key_event) = event
                    && key_event.code != KeyCode::Char('b')
                {
                    handle(&mut replayed, event);
                }
            }
            paint(&replayed, &mut step);
        })
        .unwrap();
    assert!(!report.is_golden());
    let first = report.first_mismatch().expect("the 'b' frame diverges");
    assert_eq!(first.frame, 1, "the second frame is where 'b' landed");
    assert_eq!(report.matched(), 1, "only the initial frame survived");
}

#[test]
fn bounded_recordings_replay_the_retained_window() {
    let recorder = SessionRecorder::bounded(2);
    let mut terminal = Terminal::new(TestBackend::new(Size::new(20, 3))).unwrap();
    terminal.set_preview(recorder.clone());

    let mut state = String::new();
    for character in ['a', 'b', 'c', 'd'] {
        let event = key(character);
        recorder.record_event(event.clone());
        handle(&mut state, &event);
        terminal
            .draw(|frame| {
                frame.buffer.set_string(0, 0, state.as_str(), Style::new());
            })
            .unwrap();
    }
    // 4 frames recorded, 2 retained: the timeline starts at frame 2
    assert_eq!(recorder.total_frames(), 4);
    assert_eq!(recorder.frame_count(), 2);
    assert_eq!(recorder.dropped_frames(), 2);

    let replay = SessionReplay::from_recorder(&recorder);
    assert_eq!(replay.frame_count(), 2);
    let first = replay.frames().first().expect("retained");
    assert_eq!(first.number, 2, "the retained window starts mid-session");

    // The seed step is the checkpoint: its events were evicted with the
    // frames they produced, so the app restores its state from the seed
    // buffer (the FrameInspector "resume from here" flow). Every later
    // step replays from the events the ring retained with it.
    let seed = replay.seed_buffer().expect("a retained frame");
    // the app restores its own state model from the checkpoint — here the
    // first content line of the parsed grid ("abc")
    let mut state = seed
        .to_string()
        .lines()
        .next()
        .expect("at least one row")
        .trim_end()
        .to_owned();
    let mut seeded = false;
    let report = replay
        .run(|mut step| {
            if step.seed {
                *step.buffer = seed.clone();
                seeded = true;
                return;
            }
            for event in step.events {
                handle(&mut state, event);
            }
            paint(&state, &mut step);
        })
        .unwrap();
    assert!(seeded, "the first step is the checkpoint");
    assert!(report.is_golden(), "the window resumes from its checkpoint");
    assert_eq!(state, "abcd", "the retained events advanced the state");
}

#[test]
fn the_m9_json_export_replays_after_a_round_trip() {
    let recorder = SessionRecorder::unbounded();
    let mut terminal = Terminal::new(TestBackend::new(Size::new(30, 4))).unwrap();
    terminal.set_preview(recorder.clone());

    let mut state = String::new();
    for character in ['o', 'k'] {
        let event = key(character);
        recorder.record_event(event.clone());
        handle(&mut state, &event);
        terminal
            .draw(|frame| {
                frame.buffer.set_string(0, 0, state.as_str(), Style::new());
            })
            .unwrap();
    }

    let document = recorder.to_json();
    let imported = SessionRecorder::from_json(&document).expect("the export is valid");
    let replay = SessionReplay::from_recorder(&imported);

    let mut replayed = String::new();
    let report = replay
        .run(|mut step| {
            for event in step.events {
                handle(&mut replayed, event);
            }
            paint(&replayed, &mut step);
        })
        .unwrap();
    assert!(report.is_golden());
    assert_eq!(replayed, "ok");
}
