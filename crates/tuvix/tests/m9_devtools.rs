//! The M9 differential gates: end-to-end DevTools behavior over the
//! real terminal pipeline.
//!
//! - recorded frames reconstruct byte-identically (screen round trip);
//! - the session timeline interleaves input events with the frames they
//!   produced, and bounded rings stay replay-safe;
//! - the scheduler + session recorder compose (the DevTools loop);
//! - golden sessions: export -> import -> diff is the identity.

use std::time::Duration;

use tuvix_core::backend::TestBackend;
use tuvix_core::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind,
};
use tuvix_core::geometry::{Rect, Size};
use tuvix_core::preview::{FrameSink, HistorySink, serialize_ansi};
use tuvix_core::scheduler::{Scheduler, TickOutcome};
use tuvix_core::screen::Screen;
use tuvix_core::session::{SessionDiff, SessionRecorder};
use tuvix_core::style::{Color, Modifier, Style};
use tuvix_core::terminal::Terminal;

fn key(code: KeyCode) -> tuvix_core::event::Event {
    tuvix_core::event::Event::Key(KeyEvent::new(code, KeyModifiers::empty()))
}

#[test]
fn recorded_frames_reconstruct_byte_identically() {
    let mut terminal = Terminal::new(TestBackend::new(Size::new(40, 8))).unwrap();
    let history = HistorySink::unbounded();
    terminal.set_preview(history.clone());

    // A rich frame through the real pipeline: styled text, hyperlinks,
    // wide glyphs — the M5 surface — painted by an ordinary draw.
    terminal
        .draw(|frame| {
            frame.buffer.set_string(
                0,
                0,
                "dashboard",
                Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            );
            frame
                .buffer
                .set_string(0, 1, "汉字!", Style::new().fg(Color::Indexed(214)));
            frame.buffer.set_string_with_hyperlink(
                6,
                1,
                "docs",
                Style::new()
                    .fg(Color::Rgb {
                        r: 10,
                        g: 200,
                        b: 30,
                    })
                    .add_modifier(Modifier::UNDERLINED),
                tuvix_core::style::Hyperlink::new("https://tuvix.dev/docs"),
            );
            frame.buffer.set_string(
                0,
                3,
                "warn",
                Style::new()
                    .fg(Color::LightYellow)
                    .add_modifier(Modifier::REVERSED),
            );
        })
        .unwrap();

    let captured = history.latest().unwrap();
    let screen = Screen::from_ansi(&captured).expect("the pipeline emits canonical snapshots");

    // Cell-level identity against the live terminal grid.
    let live = terminal.backend().buffer().clone();
    assert_eq!(screen.to_buffer(Rect::new(0, 0, 40, 8)), live);

    // Byte-level identity: reconstructing and re-serializing at the
    // original viewport reproduces the captured snapshot exactly.
    assert_eq!(
        serialize_ansi(&screen.to_buffer(Rect::new(0, 0, 40, 8))),
        captured
    );
}

#[test]
fn the_session_timeline_interleaves_input_with_frames() {
    let mut terminal = Terminal::new(TestBackend::new(Size::new(10, 1))).unwrap();
    let session = SessionRecorder::unbounded();
    terminal.set_preview(session.clone());

    terminal
        .draw(|f| {
            f.buffer.set_string(0, 0, "a", Style::new());
        })
        .unwrap();
    session.record_event(key(KeyCode::Enter));
    session.record_event(key(KeyCode::Left));
    terminal
        .draw(|f| {
            f.buffer.set_string(0, 0, "b", Style::new());
        })
        .unwrap();

    let entries = session.entries();
    assert_eq!(entries.len(), 4);
    assert!(entries[0].is_frame());
    assert!(entries[1].is_input());
    assert!(entries[2].is_input());
    assert!(entries[3].is_frame());
    // The events keep their order and content.
    assert_eq!(
        session.events()[0].event,
        tuvix_core::event::Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()))
    );
    // Monotone timestamps across the interleaving.
    assert!(entries[0].at() <= entries[1].at() && entries[1].at() <= entries[3].at());
}

#[test]
fn bounded_rings_stay_replay_safe() {
    let session = SessionRecorder::bounded(2);
    session.clone().on_frame("f0");
    session.record_event(key(KeyCode::Enter));
    session.clone().on_frame("f1");
    session.clone().on_frame("f2");
    session.record_event(key(KeyCode::Left));
    // The retained timeline: f1, f2, and the trailing event. The evicted
    // frame took its interleaved input with it; the trailing input stays
    // until its producing frame evicts.
    assert_eq!(session.dropped_frames(), 1);
    assert_eq!(session.dropped_events(), 1);
    let frames = session.frames();
    assert_eq!(
        frames.iter().map(|f| f.ansi.as_str()).collect::<Vec<_>>(),
        ["f1", "f2"]
    );
    assert_eq!(session.events().len(), 1);
}

#[test]
fn the_scheduler_and_the_session_recorder_compose() {
    // The DevTools loop shape: a scheduler drives the app, the session
    // recorder watches every committed frame, and cancelled passes
    // never reach the timeline.
    let mut scheduler = Scheduler::new(Terminal::new(TestBackend::new(Size::new(10, 1))).unwrap());
    let session = SessionRecorder::unbounded();
    scheduler.terminal_mut().set_preview(session.clone());

    for tick in 0..3 {
        scheduler.request_render();
        let label = format!("tick {tick}");
        let outcome = scheduler
            .tick(|frame| {
                frame.buffer.set_string(0, 0, label.as_str(), Style::new());
            })
            .unwrap();
        assert_eq!(outcome, TickOutcome::Rendered);
    }
    // Three renders, three timeline entries (no input events yet).
    assert_eq!(session.frame_count(), 3);
    assert_eq!(session.len(), 3);

    // An abandoned render leaves no trace in the history.
    let token = tuvix_core::cancellation::CancellationToken::new();
    let pass = token.child();
    token.cancel();
    scheduler.request_render();
    let outcome = scheduler
        .tick_cancellable(&pass, |frame| {
            frame.buffer.set_string(0, 0, "abandoned", Style::new());
        })
        .unwrap();
    assert_eq!(outcome, TickOutcome::Cancelled);
    assert_eq!(session.frame_count(), 3);
    assert_eq!(session.total_frames(), 3);
}

#[test]
fn golden_sessions_export_import_and_diff_clean() {
    let mut terminal = Terminal::new(TestBackend::new(Size::new(20, 2))).unwrap();
    let session = SessionRecorder::unbounded();
    terminal.set_preview(session.clone());

    // Record an interaction: two states with a mouse event between.
    terminal
        .draw(|f| {
            f.buffer.set_string(0, 0, "menu", Style::new());
        })
        .unwrap();
    session.record_event(tuvix_core::event::Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(tuvix_core::event::MouseButton::Left),
        column: 3,
        row: 0,
        modifiers: KeyModifiers::empty(),
    }));
    terminal
        .draw(|f| {
            f.buffer
                .set_string(0, 0, "menu [open]", Style::new().fg(Color::Blue));
        })
        .unwrap();

    // Export, import, and check the closed round trip.
    let golden = session.to_json();
    let imported = SessionRecorder::from_json(&golden).unwrap();
    assert_eq!(imported.to_json(), golden);
    assert_eq!(imported.frame_count(), session.frame_count());
    assert_eq!(imported.events().len(), session.events().len());

    // The frames are byte-identical and the diff is empty.
    for (a, b) in session.frames().iter().zip(imported.frames()) {
        assert_eq!(a.ansi, b.ansi);
        assert_eq!(a.number, b.number);
    }
    assert_eq!(session.diff(&imported), SessionDiff::default());
}

#[test]
fn a_regression_points_at_the_frame() {
    // Golden: the recorded truth.
    let mut terminal = Terminal::new(TestBackend::new(Size::new(20, 2))).unwrap();
    let golden_session = SessionRecorder::unbounded();
    terminal.set_preview(golden_session.clone());
    terminal
        .draw(|f| {
            f.buffer.set_string(0, 0, "v1.0", Style::new());
        })
        .unwrap();
    golden_session.record_event(key(KeyCode::Right));
    terminal
        .draw(|f| {
            f.buffer.set_string(0, 0, "v1.0>", Style::new());
        })
        .unwrap();
    let golden = golden_session.to_json();
    let golden = SessionRecorder::from_json(&golden).unwrap();

    // A later run renders differently on the second frame.
    let mut terminal = Terminal::new(TestBackend::new(Size::new(20, 2))).unwrap();
    let session = SessionRecorder::unbounded();
    terminal.set_preview(session.clone());
    terminal
        .draw(|f| {
            f.buffer.set_string(0, 0, "v1.0", Style::new());
        })
        .unwrap();
    session.record_event(key(KeyCode::Right));
    terminal
        .draw(|f| {
            f.buffer
                .set_string(0, 0, "v2.0>", Style::new().add_modifier(Modifier::BOLD));
        })
        .unwrap();

    let diff = session.diff(&golden);
    assert_eq!(diff.frames_changed(), [1]);
    assert_eq!(diff.events_changed(), []);
    assert!(!diff.is_empty());
}

#[test]
fn scrubbing_the_history_feeds_the_inspector() {
    // The DevTools scrub loop: record a session, reconstruct a past
    // frame, hand it to FrameInspector, and verify the overlay shows
    // that moment (styled) with the timeline footer.
    let mut terminal = Terminal::new(TestBackend::new(Size::new(12, 1))).unwrap();
    let session = SessionRecorder::unbounded();
    terminal.set_preview(session.clone());
    for tick in 0..3 {
        let label = format!("tick {tick}");
        terminal
            .draw(|f| {
                f.buffer.set_string(0, 0, label.as_str(), Style::new());
            })
            .unwrap();
    }

    let record = session.frame(1).unwrap();
    let screen = Screen::from_ansi(&record.ansi).unwrap();
    let mut overlay = tuvix_core::buffer::Buffer::empty(Rect::new(0, 0, 24, 4));
    tuvix_core::widget::Widget::render(
        tuvix_widgets::FrameInspector::new(screen)
            .timeline(1, 3)
            .at(Duration::from_millis(42)),
        Rect::new(0, 0, 24, 4),
        &mut overlay,
    );
    let rendered = overlay.to_string();
    assert!(rendered.contains("tick 1"), "{rendered}");
    assert!(rendered.contains("●●○ frame 2/3"), "{rendered}");
    assert!(rendered.contains("42 ms"), "{rendered}");
}

#[test]
fn every_event_variant_survives_the_session_document() {
    // The corpus: one of each event shape the terminal can report.
    let corpus = vec![
        tuvix_core::event::Event::FocusGained,
        tuvix_core::event::Event::FocusLost,
        key(KeyCode::Char('x')),
        key(KeyCode::F(5)),
        key(KeyCode::Esc),
        tuvix_core::event::Event::Key(KeyEvent {
            code: KeyCode::Char('c'),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Release,
            state: tuvix_core::event::KeyEventState::CAPS_LOCK,
        }),
        tuvix_core::event::Event::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 7,
            row: 9,
            modifiers: KeyModifiers::ALT,
        }),
        tuvix_core::event::Event::Paste("multi\nline \t paste".to_string()),
        tuvix_core::event::Event::Resize(120, 40),
    ];
    let session = SessionRecorder::unbounded();
    for event in &corpus {
        session.record_event(event.clone());
    }
    let imported = SessionRecorder::from_json(&session.to_json()).unwrap();
    let replayed: Vec<_> = imported.events().into_iter().map(|e| e.event).collect();
    assert_eq!(replayed, corpus);
    // And a bold + reversed styled frame rides through too.
    let session = SessionRecorder::unbounded();
    session.clone().on_frame(&serialize_ansi(&{
        let mut b = tuvix_core::buffer::Buffer::empty(Rect::new(0, 0, 4, 1));
        b.set_string(
            0,
            0,
            "bold",
            Style::new()
                .add_modifier(Modifier::BOLD | Modifier::CROSSED_OUT)
                .underline_color(Color::Gray),
        );
        b
    }));
    let imported = SessionRecorder::from_json(&session.to_json()).unwrap();
    assert_eq!(
        imported.frame(0).unwrap().ansi,
        session.frame(0).unwrap().ansi
    );
}
