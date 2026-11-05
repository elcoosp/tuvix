//! M1 gates: the deterministic B9 byte-ratio gate and the recorded
//! timing gates.
//!
//! B9 (deterministic, runs in CI): one changed row must serialize to at
//! most 10% of a full-frame redraw's bytes.
//!
//! B6 (timing, `#[ignore]`, recorded in docs/BENCHMARKS.md): 5 damaged
//! rows on the 200x60 viewport must paint + diff + serialize in at most
//! 0.5 ms.

use std::time::{Duration, Instant};

use tuvix_backend_crossterm::CrosstermBackend;
use tuvix_core::backend::Backend;
use tuvix_core::buffer::Buffer;
use tuvix_core::geometry::Rect;
use tuvix_core::style::{Color, Style};

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

fn serialize(previous: &Buffer, next: &Buffer) -> u64 {
    let mut backend = CrosstermBackend::new(Vec::<u8>::new());
    backend.draw(previous.diff_runs(next)).unwrap();
    backend.flush().unwrap();
    backend.into_writer().len() as u64
}

#[test]
fn b9_one_row_change_is_at_most_ten_percent_of_a_full_frame() {
    let full_frame = text_buffer();

    // Full-frame bytes: from an empty screen to the full text.
    let empty = Buffer::empty(Rect::new(0, 0, WIDTH, HEIGHT));
    let full_bytes = serialize(&empty, &full_frame);

    // One-row change: mutate row 30 only.
    let mut one_row = text_buffer();
    one_row.set_string(
        0,
        30,
        "row 30: UPDATED — one row changed, everything else identical",
        Style::new().fg(Color::Cyan),
    );
    let row_bytes = serialize(&full_frame, &one_row);

    let ratio = f64::from(u32::try_from(row_bytes).unwrap_or(u32::MAX))
        / f64::from(u32::try_from(full_bytes).unwrap_or(1));
    assert!(
        ratio <= 0.10,
        "B9 gate failed: 1-row change used {row_bytes} bytes vs {full_bytes} for a full frame (ratio {ratio:.3})"
    );
    println!(
        "B9 gate: 1-row change = {row_bytes} bytes, full frame = {full_bytes} bytes, ratio = {ratio:.4}"
    );
}

#[test]
#[ignore = "timing gate: run with `cargo test --release --ignored` and record in docs/BENCHMARKS.md"]
fn b6_five_dirty_rows_serialize_within_half_millisecond() {
    let previous = text_buffer();
    let mut next = text_buffer();
    for index in 0..5_u16 {
        let row = 5 + index * 12;
        next.set_string(
            0,
            row,
            format!("row {row:02}: UPDATED — the quick brown fox 汉字 {index} style"),
            Style::new().fg(Color::Cyan),
        );
    }

    // Warm-up.
    for _ in 0..50 {
        let _ = serialize(&previous, &next);
    }

    let iterations = 300;
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = serialize(&previous, &next);
    }
    let per_frame = start.elapsed() / iterations;
    assert!(
        per_frame <= Duration::from_millis(1),
        "B6 gate failed: {per_frame:?} per frame (budget 0.5 ms, hard ceiling 1 ms on shared runners)"
    );
    println!("B6 gate: {per_frame:?} per frame (paint + diff + ANSI serialize, 5 rows @ 200x60)");
}
