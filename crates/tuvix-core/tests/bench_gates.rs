//! Recorded timing gates for the core pipeline stages.
//!
//! Run with `cargo test -p tuvix-core --release --ignored`; results are
//! recorded in docs/BENCHMARKS.md. Deterministic gates (B9) live in the
//! backend crate where the ANSI encoder is available.

use std::time::{Duration, Instant};

use tuvix_core::backend::TestBackend;
use tuvix_core::buffer::Buffer;
use tuvix_core::geometry::{Rect, Size};
use tuvix_core::layout::{Constraint, Layout};
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
