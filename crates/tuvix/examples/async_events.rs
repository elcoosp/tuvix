//! The async event stream, with no async runtime: a ~30-line `block_on`
//! parks the main thread and lets the stream's Waker unpark it, proving the
//! M2 event stream is executor-agnostic.
//!
//! Prints every event as it arrives (raw mode makes the terminal echo it as
//! escape sequences; Ctrl+C quits via the kitty disambiguated report).
//!
//! Run with: cargo run -p tuvix --example `async_events`

use std::future::Future;
use std::io::Result;
use std::task::{Context, Poll, Wake, Waker};
use std::thread::{self, Thread};
use std::time::Duration;

use tuvix::prelude::*;
use tuvix::runtime::KittyEnhancementFlags;
use tuvix::{CrosstermEvents, try_init};

fn main() -> Result<()> {
    let mut runner = try_init()?;

    // Push the kitty keyboard protocol so Esc, Ctrl+C and friends arrive as
    // unambiguous CSI-u reports with release kinds on supporting terminals.
    runner.push_input_mode(InputMode::KittyKeyboard(
        KittyEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
            | KittyEnhancementFlags::REPORT_EVENT_TYPES,
    ))?;

    let (_reader, mut events) = CrosstermEvents::spawn()?;

    runner.draw(|frame| {
        frame.buffer.set_string(
            0,
            0,
            "async_events: type keys, press Ctrl+C to quit (executor-free)",
            Style::new().fg(Color::Cyan),
        );
    })?;

    while let Some(event) = block_on(events.next_event()) {
        let Event::Key(key) = event else {
            continue;
        };

        let is_quit = matches!(
            (key.code, key.modifiers),
            (KeyCode::Char('c'), _) if key.modifiers.contains(KeyModifiers::CONTROL)
        ) || key.code == KeyCode::Esc;
        if is_quit && key.kind == KeyEventKind::Press {
            break;
        }

        let line = format!("key {key:?}");
        runner.draw(|frame| {
            frame
                .buffer
                .set_string(0, 2, &line, Style::new().fg(Color::Green));
        })?;
    }

    runner.restore()?;
    Ok(())
}

/// Parks the current thread until `future` resolves; the event stream wakes
/// it through the standard waker protocol.
fn block_on<T>(future: impl Future<Output = T>) -> T {
    struct ParkWake(Thread);

    impl Wake for ParkWake {
        fn wake(self: std::sync::Arc<Self>) {
            self.0.unpark();
        }

        fn wake_by_ref(self: &std::sync::Arc<Self>) {
            self.0.unpark();
        }
    }

    let waker = Waker::from(std::sync::Arc::new(ParkWake(thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(value) => return value,
            Poll::Pending => thread::park_timeout(Duration::from_millis(100)),
        }
    }
}
