//! M5 benchmark gates for the dependency-free ANSI output path.
//!
//! The B6 gate — paint + diff + ANSI serialization for 5 damaged rows on
//! the 200x60 budget viewport, target <= 0.5 ms — must hold for every
//! backend. This bench proves the simple backend keeps the budget without
//! a terminal library in between, and reports full-frame throughput for
//! the `docs/BENCHMARKS.md` scorecard.

use std::hint::black_box;
use std::io::Write;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use tuvix_backend_ansi::AnsiBackend;
use tuvix_core::backend::Backend;
use tuvix_core::buffer::Buffer;
use tuvix_core::geometry::{Rect, Size};
use tuvix_core::style::{Color, Style};

/// The architecture budget viewport: 200x60.
const WIDTH: u16 = 200;
const HEIGHT: u16 = 60;
/// Rows dirtied per B6 frame.
const DIRTY_ROWS: u16 = 5;

/// A `Write` sink that only counts bytes: serialization cost without I/O.
#[derive(Default)]
struct CountingSink {
    bytes: u64,
}

impl Write for CountingSink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.bytes += buf.len() as u64;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn text_buffer() -> Buffer {
    let mut buffer = Buffer::empty(Rect::new(0, 0, WIDTH, HEIGHT));
    for row in 0..HEIGHT {
        let line =
            format!("row {row:02}: the quick brown fox jumps over the lazy dog — 汉字 spacing");
        buffer.set_string(0, row, line, Style::new());
    }
    buffer
}

fn bench_gates(c: &mut Criterion) {
    let mut group = c.benchmark_group("gates");
    group.throughput(Throughput::Elements(u64::from(WIDTH) * u64::from(HEIGHT)));

    let previous = text_buffer();
    let mut next = text_buffer();
    for index in 0..DIRTY_ROWS {
        let row = 5 + index * 12;
        let line = format!("row {row:02}: UPDATED — the quick brown fox 汉字 {index} style");
        next.set_string(0, row, line, Style::new().fg(Color::Cyan));
    }

    group.bench_function("m5_ansi_backend_b6_incremental_frame", |b| {
        b.iter(|| {
            let mut backend = AnsiBackend::new(CountingSink::default(), Size::new(WIDTH, HEIGHT));
            let runs: Vec<_> = black_box(&previous).diff_runs(black_box(&next)).collect();
            backend.draw(runs).unwrap();
            backend.flush().unwrap();
        });
    });

    group.bench_function("m5_ansi_backend_full_frame", |b| {
        b.iter(|| {
            let mut backend = AnsiBackend::new(CountingSink::default(), Size::new(WIDTH, HEIGHT));
            let blank = Buffer::empty(Rect::new(0, 0, WIDTH, HEIGHT));
            let runs: Vec<_> = blank.diff_runs(black_box(&next)).collect();
            backend.draw(runs).unwrap();
            backend.flush().unwrap();
        });
    });

    group.finish();
}

criterion_group!(benches, bench_gates);
criterion_main!(benches);
