//! Benchmarks for the diff engine: the hot path between frames.

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use tuvix_core::buffer::Buffer;
use tuvix_core::geometry::Rect;
use tuvix_core::style::Style;

const WIDTH: u16 = 80;
const HEIGHT: u16 = 24;

fn text_buffer() -> Buffer {
    let mut buffer = Buffer::empty(Rect::new(0, 0, WIDTH, HEIGHT));
    for row in 0..HEIGHT {
        let line = format!("row {row:02}: the quick brown fox jumps over the lazy dog");
        buffer.set_string(0, row, line, Style::new());
    }
    buffer
}

/// Mutates `count` scattered cells across the buffer.
fn mutate(buffer: &mut Buffer, count: usize) {
    for index in 0..count {
        let x = ((index * 7 + 3) % WIDTH as usize) as u16;
        let y = ((index * 5 + 1) % HEIGHT as usize) as u16;
        buffer.set_string(x, y, "X", Style::new());
    }
}

fn bench_diff(c: &mut Criterion) {
    let mut group = c.benchmark_group("diff");
    group.throughput(Throughput::Elements(u64::from(WIDTH) * u64::from(HEIGHT)));

    // Typical UI frame: a handful of scattered cell changes.
    let previous = text_buffer();
    let mut next = text_buffer();
    mutate(&mut next, 10);
    group.bench_function("80x24_10_cells_changed", |b| {
        b.iter(|| black_box(&previous).diff_runs(black_box(&next)).count());
    });

    // Heavy frame: half the cells changed.
    let mut heavy = text_buffer();
    mutate(&mut heavy, 480);
    group.bench_function("80x24_half_cells_changed", |b| {
        b.iter(|| black_box(&previous).diff_runs(black_box(&heavy)).count());
    });

    // Identical frames: the engine must walk and find nothing.
    let identical = text_buffer();
    group.bench_function("80x24_no_changes", |b| {
        b.iter(|| {
            black_box(&previous)
                .diff_runs(black_box(&identical))
                .count()
        });
    });

    // Full repaint from an empty buffer (first frame after clear).
    let empty = Buffer::empty(Rect::new(0, 0, WIDTH, HEIGHT));
    group.bench_function("80x24_full_repaint", |b| {
        b.iter(|| black_box(&empty).diff_runs(black_box(&previous)).count());
    });

    group.finish();
}

criterion_group!(benches, bench_diff);
criterion_main!(benches);
