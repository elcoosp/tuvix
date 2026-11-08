//! Minimal tuvix application on the termion backend (unix only): a styled
//! greeting that quits on 'q', with the M2 runtime driven by the M5
//! termion event reader.
//!
//! Run with: cargo run -p tuvix --example `termion_hello`

#![cfg(unix)]

use std::io::Result;
use std::time::Duration;

use tuvix::prelude::*;
use tuvix::{TermionEvents, try_init_termion};

fn main() -> Result<()> {
    // One call: panic hook (sequence half), termion raw mode, alternate
    // screen. Dropping the runner restores everything.
    let mut runner = try_init_termion()?;

    let (mut reader, mut events) = TermionEvents::spawn()?;
    let mut ticks = 0_u32;
    let mut quit = false;

    while !quit {
        runner.draw(|frame| {
            let [header, body, footer] = Layout::vertical([
                Constraint::Length(1),
                Constraint::Weight(1),
                Constraint::Length(1),
            ])
            .areas(&frame.area());

            frame
                .buffer
                .set_string(2, header.y, "tuvix", Style::new().fg(Color::Cyan).bold());
            frame.buffer.set_string(
                8,
                header.y,
                "- termion backend example",
                Style::new().fg(Color::DarkGray),
            );
            frame.buffer.set_string(
                2,
                body.y,
                format!("events handled: {ticks}"),
                Style::new().fg(Color::Green),
            );
            frame
                .buffer
                .set_string(2, footer.y, "press q to quit", Style::new().dim());
        })?;

        wait_for(&mut events, Duration::from_millis(200));
        while let Some(event) = events.try_next() {
            ticks += 1;
            if let Event::Key(key) = event {
                quit = key.code == KeyCode::Char('q') && key.modifiers == KeyModifiers::empty();
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
