//! The M9 time-travel DevTools: record a session, scrub it, inspect a
//! historical frame and export it as a replayable golden log.
//!
//! This demo drives a headless `TestBackend` app (no terminal modes are
//! touched): three ticks of a counter app with recorded keystrokes
//! between them. It then reconstructs any past moment from the timeline,
//! prints the FrameInspector overlay, and writes the session document to
//! stdout — the artifact a bug report would attach and CI would diff.
//!
//! Run with: cargo run -p tuvix --example devtools

use std::time::Duration;

use tuvix::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut terminal = Terminal::new(TestBackend::new(Size::new(24, 4)))?;
    let session = SessionRecorder::unbounded();
    terminal.set_preview(session.clone());

    // The app: a counter that keystrokes drive. Every handled event is
    // recorded next to the frames it produced.
    let mut count = 0_u32;
    let keystrokes = [
        Event::Key(KeyEvent::new(KeyCode::Char('+'), KeyModifiers::empty())),
        Event::Key(KeyEvent::new(KeyCode::Char('+'), KeyModifiers::empty())),
    ];
    for keystroke in keystrokes {
        session.record_event(keystroke);
        count += 1;
        let label = format!("count: {count}");
        terminal.draw(|frame| {
            frame
                .buffer
                .set_string(0, 0, label.as_str(), Style::new().bold());
        })?;
    }
    terminal.draw(|frame| {
        frame
            .buffer
            .set_string(0, 0, "count: 2 (final)", Style::new().fg(Color::Green));
    })?;

    // Scrub the timeline: entries interleave frames and input events.
    println!("timeline ({} entries):", session.len());
    for (index, entry) in session.entries().iter().enumerate() {
        match entry {
            Entry::Frame(frame) => {
                println!("  [{index}] frame {} at {:?}", frame.number, frame.at);
            }
            Entry::Input(input) => {
                println!("  [{index}] input  at {:?}", input.at);
            }
        }
    }

    // Reconstruct any past moment and inspect it as an overlay.
    let frame_index = 0;
    let record = session.frame(frame_index).expect("recorded frame");
    let screen = Screen::from_ansi(&record.ansi).expect("canonical snapshot");
    let mut overlay = Buffer::empty(Rect::new(0, 0, 24, 6));
    FrameInspector::new(screen)
        .title("time travel")
        .timeline(frame_index, session.frame_count())
        .at(record.at)
        .render(Rect::new(0, 0, 24, 6), &mut overlay);
    println!("inspector overlay of frame {frame_index}:");
    for row in overlay.to_string().split('\n') {
        println!("  |{row}|");
    }

    // Diff two moments of the same session: what changed between them.
    let first = Screen::from_ansi(&session.frame(0).unwrap().ansi).unwrap();
    let last = Screen::from_ansi(&session.latest_frame().unwrap().ansi).unwrap();
    println!(
        "cells changed frame 0 -> latest: {}",
        first.diff(&last).len()
    );

    // Export the session: the replayable golden log (attach to bug
    // reports; diff it in CI with SessionRecorder::diff).
    let golden = session.to_json();
    println!("session document ({} bytes):", golden.len());
    println!("{golden}");

    // Round trip: import the log and verify it replays identically.
    let imported = SessionRecorder::from_json(&golden)?;
    assert!(session.diff(&imported).is_empty());
    println!("round trip verified: imported session diffs empty");

    std::thread::sleep(Duration::from_millis(1));
    Ok(())
}
