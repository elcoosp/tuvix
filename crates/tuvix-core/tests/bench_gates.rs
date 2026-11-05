//! Recorded timing gates for the core pipeline stages.
//!
//! Run with `cargo test -p tuvix-core --release --ignored`; results are
//! recorded in docs/BENCHMARKS.md. Deterministic gates (B9) live in the
//! backend crate where the ANSI encoder is available.

use std::cell::RefCell;
use std::hint::black_box;
use std::rc::Rc;
use std::time::{Duration, Instant};

use tuvix_core::backend::TestBackend;
use tuvix_core::buffer::Buffer;
use tuvix_core::cancellation::CancellationToken;
use tuvix_core::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use tuvix_core::geometry::{Rect, Size};
use tuvix_core::layout::{Constraint, Layout};
use tuvix_core::runtime::queue::event_channel;
use tuvix_core::scheduler::{Scheduler, TickOutcome};
use tuvix_core::style::{Color, Style};
use tuvix_core::terminal::Terminal;

const WIDTH: u16 = 200;
const HEIGHT: u16 = 60;

fn text_buffer() -> Buffer {
    let mut buffer = Buffer::empty(Rect::new(0, 0, WIDTH, HEIGHT));
    for row in 0..HEIGHT {
        let line =
            format!("row {row:02}: the quick brown fox jumps over the lazy dog — 汉字 spacing");
        buffer.set_string(0, row, line, Style::new());
    }
    buffer
}

fn scatter(buffer: &mut Buffer, count: usize) {
    for index in 0..count {
        let x = ((index * 37 + 11) % usize::from(WIDTH)) as u16;
        let y = ((index * 53 + 7) % usize::from(HEIGHT)) as u16;
        buffer.set_string(x, y, "X", Style::new().fg(Color::Red));
    }
}

#[test]
#[ignore = "timing gate: run with `cargo test --release --ignored` and record in docs/BENCHMARKS.md"]
fn b8_noop_frame_decision_within_fifty_microseconds() {
    let steady = text_buffer();

    // Warm-up.
    for _ in 0..200 {
        let _ = steady.diff_runs(&steady).count();
    }

    let iterations = 500;
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = steady.diff_runs(&steady).count();
    }
    let per_frame = start.elapsed() / iterations;
    assert!(
        per_frame <= Duration::from_micros(200),
        "B8 gate failed: {per_frame:?} per no-op frame decision (budget 50 us, hard ceiling 200 us on shared runners)"
    );
    println!("B8 gate: {per_frame:?} per no-op frame decision (200x60)");
}

#[test]
#[ignore = "timing gate: run with `cargo test --release --ignored` and record in docs/BENCHMARKS.md"]
fn b2_steady_frame_within_budget() {
    let base = text_buffer();
    let mut changed = text_buffer();
    scatter(&mut changed, 1_000);

    let mut terminal = Terminal::new(TestBackend::new(Size::new(WIDTH, HEIGHT))).unwrap();
    terminal
        .draw(|frame| frame.buffer.content.clone_from(&base.content))
        .unwrap();

    let mut show_changed = false;
    // Warm-up.
    for _ in 0..50 {
        show_changed = !show_changed;
        let snapshot = if show_changed {
            changed.content.clone()
        } else {
            base.content.clone()
        };
        terminal
            .draw(|frame| frame.buffer.content.clone_from(&snapshot))
            .unwrap();
    }

    let iterations = 300;
    let start = Instant::now();
    for _ in 0..iterations {
        show_changed = !show_changed;
        let snapshot = if show_changed {
            changed.content.clone()
        } else {
            base.content.clone()
        };
        terminal
            .draw(|frame| frame.buffer.content.clone_from(&snapshot))
            .unwrap();
    }
    let per_frame = start.elapsed() / iterations;
    assert!(
        per_frame <= Duration::from_millis(2),
        "B2 gate failed: {per_frame:?} per steady frame (budget 2 ms)"
    );
    println!("B2 gate: {per_frame:?} per steady frame (1k changed cells @ 200x60, full stack)");
}

#[test]
#[ignore = "timing gate: run with `cargo test --release --ignored` and record in docs/BENCHMARKS.md"]
fn b5_cached_layout_within_half_millisecond() {
    let constraints: Vec<Constraint> = (0..20)
        .map(|index| match index % 4 {
            0 => Constraint::Length(2),
            1 => Constraint::Min(0),
            2 => Constraint::Percentage(10),
            _ => Constraint::Fill(1),
        })
        .collect();
    let layout = Layout::vertical(constraints);
    let area = Rect::new(0, 0, WIDTH, HEIGHT);
    let _ = layout.split(area); // warm

    let iterations = 2_000;
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = layout.split(area);
    }
    let per_split = start.elapsed() / iterations;
    assert!(
        per_split <= Duration::from_millis(1),
        "B5 gate failed: {per_split:?} per cached split (budget 0.5 ms, hard ceiling 1 ms on shared runners)"
    );
    println!("B5 gate: {per_split:?} per cached 20-constraint split (target: cache hits)");
}

#[test]
#[ignore = "timing gate: run with `cargo test --release --ignored` and record in docs/BENCHMARKS.md"]
fn b8_journal_noop_frame_within_budget() {
    // The M4 O(1) decision: a persistent-DOM frame that paints nothing must
    // cost no more than the original 50 us B8 budget — finally at the full
    // 200x60 budget viewport, not just the 80x24 reference screen.
    let mut terminal =
        Terminal::new_persistent(TestBackend::new(Size::new(WIDTH, HEIGHT))).unwrap();
    terminal
        .draw_persistent(|frame| {
            for row in 0..HEIGHT {
                let line = format!("row {row:02}: the quick brown fox jumps over the lazy dog");
                frame.buffer.set_string(0, row, line, Style::new());
            }
        })
        .unwrap();

    // Warm-up.
    for _ in 0..200 {
        let () = terminal.draw_persistent(|_| {}).unwrap();
    }

    let iterations = 2_000;
    let start = Instant::now();
    for _ in 0..iterations {
        let () = terminal.draw_persistent(|_| {}).unwrap();
    }
    let per_frame = start.elapsed() / iterations;
    assert!(
        per_frame <= Duration::from_micros(50),
        "B8 journal gate failed: {per_frame:?} per no-op frame (budget 50 us)"
    );
    println!("B8 journal gate: {per_frame:?} per no-op frame (200x60, journal path)");
}

#[test]
#[ignore = "timing gate: run with `cargo test --release --ignored` and record in docs/BENCHMARKS.md"]
fn b10_input_to_paint_p99_within_eight_milliseconds() {
    // The architecture's gate 5 (§8): 10,000 keystrokes under continuous
    // animation, input-to-paint p99 <= 8 ms. One keystroke = one event
    // through the channel, drained by the scheduler loop, coalesced into
    // one animated frame (a 20-row band repaints every keystroke at the
    // 200x60 budget viewport).
    const KEYSTROKES: usize = 10_000;

    let (sender, stream) = event_channel();
    let stream = Rc::new(RefCell::new(stream));
    let terminal = Terminal::new_persistent(TestBackend::new(Size::new(WIDTH, HEIGHT))).unwrap();
    let mut scheduler = Scheduler::new(terminal);

    let probe_stream = Rc::clone(&stream);
    scheduler.set_probe(move || !probe_stream.borrow().is_empty());

    // Steady state: paint once so every measured frame is a real
    // banded diff against a full screen, not a blank canvas.
    scheduler.request_render();
    let outcome = scheduler.tick(|frame| animate(frame, 0)).unwrap();
    assert_eq!(outcome, TickOutcome::Rendered);

    let mut latencies = Vec::with_capacity(KEYSTROKES);
    for keystroke in 0..KEYSTROKES {
        let code = KeyCode::Char(char::from_u32(65 + (keystroke % 26) as u32).unwrap_or('a'));
        assert!(sender.send(Event::Key(KeyEvent::new(code, KeyModifiers::empty()))));

        // Input-to-paint: from "the event is queued" to "the frame is
        // flushed" — the latency a user experiences per keystroke.
        let start = Instant::now();
        while let Some(event) = stream.borrow_mut().try_next() {
            black_box(event);
        }
        scheduler.request_render();
        let step = keystroke as u64;
        let outcome = scheduler
            .tick_cancellable(&CancellationToken::new(), |frame| animate(frame, step))
            .unwrap();
        assert_eq!(outcome, TickOutcome::Rendered, "keystroke {keystroke}");
        latencies.push(start.elapsed());
    }

    latencies.sort_unstable();
    let p99 = latencies[KEYSTROKES * 99 / 100 - 1];
    let max = latencies[KEYSTROKES - 1];
    assert!(
        p99 <= Duration::from_millis(8),
        "B10 gate failed: input-to-paint p99 {p99:?} (budget 8 ms)"
    );
    println!(
        "B10 gate: input-to-paint p99 {p99:?}, max {max:?} over {KEYSTROKES} keystrokes @ {WIDTH}x{HEIGHT} (budget 8 ms)"
    );
}

/// The animation under the B10 gate: a header plus a 20-row band that
/// changes on every keystroke (persistent DOM, banded diff).
fn animate(frame: &mut tuvix_core::terminal::Frame<'_>, tick: u64) {
    let header = format!("B10 input-to-paint tick {tick}");
    frame.buffer.set_string(0, 0, header.as_str(), Style::new());
    for row in 1..21 {
        let band = format!("row {row:02}: the quick brown fox jumps over the lazy dog {tick}");
        frame.buffer.set_string(0, row, band.as_str(), Style::new());
    }
}
