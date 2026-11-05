//! The M4 IPC preview: watch this app from another terminal.
//!
//! ```text
//! cargo run -p tuvix --example preview
//! # in another shell:
//! telnet 127.0.0.1 4223
//! ```
//!
//! The app runs the persistent-DOM loop: the header repaints once per
//! second, the rest of the DOM carries across frames untouched, and every
//! flushed frame is serialized to a full-frame ANSI snapshot and broadcast
//! to any client connected to the preview tap.

use std::time::{Duration, Instant};

use tuvix::prelude::*;
use tuvix::preview::PreviewTap;
use tuvix_backend_crossterm::CrosstermEvents;

fn main() -> std::io::Result<()> {
    // The tap listens on loopback; any TCP client receives every frame.
    let mut tap = PreviewTap::tcp(4223)?;
    let port = tap.local_addr().map_or(4223, |addr| addr.port());
    println!("tuvix preview listening — connect with: telnet 127.0.0.1 {port}");

    let mut runner = tuvix::try_init()?;
    runner.push_input_mode(InputMode::BracketedPaste)?;

    let (_reader, mut events) = CrosstermEvents::spawn()?;

    // Stream every flushed frame to the tap.
    runner.terminal_mut().set_preview(tap.sink());

    let theme = Theme::dark();
    let mut ticks = 0_u64;
    let mut last_tick = Instant::now();

    loop {
        // Persistent DOM: the header band repaints once per second; the
        // rest of the screen keeps its content across frames.
        if last_tick.elapsed() >= Duration::from_secs(1) {
            last_tick = Instant::now();
            ticks += 1;
            runner
                .terminal_mut()
                .draw_persistent(|frame| {
                    frame.buffer.clear(Rect::new(0, 0, 60, 1));
                    frame.buffer.set_string(
                        0,
                        0,
                        format!("tuvix preview — tick {ticks} (press q to quit)"),
                        theme.title,
                    );
                })
                .unwrap();
        }

        if let Some(Event::Key(key)) = events.try_next()
            && key.kind == KeyEventKind::Press
            && key.code == KeyCode::Char('q')
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    tap.stop();
    runner.restore()?;
    Ok(())
}
