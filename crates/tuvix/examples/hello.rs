//! Minimal tuvix application using the M2 runtime: a styled greeting that
//! quits on 'q', with bracketed paste and mouse capture pushed onto the
//! input stack and events flowing through the async event stream.
//!
//! Run with: cargo run -p tuvix --example hello

use std::io::Result;
use std::time::Duration;

use tuvix::prelude::*;
use tuvix::{CrosstermEvents, try_init};

fn main() -> Result<()> {
    // One call: panic hook, raw mode, alternate screen. The runner restores
    // the terminal on drop, on error and on panic.
    let mut runner = try_init()?;
    runner.push_input_mode(InputMode::BracketedPaste)?;
    runner.push_input_mode(InputMode::MouseCapture)?;
    runner.push_input_mode(InputMode::FocusChange)?;

    let (mut reader, mut events) = CrosstermEvents::spawn()?;
    let mut clicks = 0_u32;
    let mut quit = false;

    while !quit {
        runner.draw(|frame| {
            let [header, body, footer] = Layout::vertical([
                Constraint::Length(1),
                Constraint::Min(0),
                Constraint::Length(1),
            ])
            .areas(&frame.area());

            frame
                .buffer
                .set_string(2, header.y, "tuvix", Style::new().fg(Color::Cyan).bold());
            frame.buffer.set_string(
                7,
                header.y,
                "- minimal hello example",
                Style::new().fg(Color::DarkGray),
            );

            let inner = Rect::new(2, body.y + 1, 30, 1);
            frame.buffer.set_string(
                inner.x,
                inner.y,
                format!("events handled: {clicks}"),
                Style::new().fg(Color::Green),
            );

            frame
                .buffer
                .set_string(2, footer.y, "press q to quit", Style::new().dim());
        })?;

        // Wait briefly for input, then drain everything that accumulated.
        // The 200 ms deadline keeps the loop responsive without busy-spinning.
        wait_for(&mut events, Duration::from_millis(200));
        while let Some(event) = events.try_next() {
            clicks += 1;
            if let Event::Key(key) = event {
                quit = key.kind == KeyEventKind::Press
                    && key.code == KeyCode::Char('q')
                    && key.modifiers == KeyModifiers::empty();
            }
        }
    }

    reader.stop();
    runner.restore()?;
    Ok(())
}

/// Waits until an event arrives or the deadline passes.
fn wait_for(events: &mut EventStream, deadline: Duration) {
    let started = std::time::Instant::now();
    while started.elapsed() < deadline {
        if !events.is_empty() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
