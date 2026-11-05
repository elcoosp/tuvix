//! M1 benchmark gates: the architecture budget, measured.
//!
//! Targets (docs/BENCHMARKS.md): B2 steady frame <= 2 ms, B5 incremental
//! layout <= 0.5 ms, B8 no-op frame decision <= 50 us. The deterministic
//! byte-ratio gate (B9) lives in `tuvix-backend-crossterm/tests/gates.rs`
//! where the ANSI encoder is available; timing gates as `#[ignore]` tests
//! live in `tests/bench_gates.rs` and are recorded in BENCHMARKS.md.

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use tuvix_core::backend::TestBackend;
use tuvix_core::buffer::Buffer;
use tuvix_core::geometry::{Rect, Size};
use tuvix_core::layout::{Constraint, Layout};
use tuvix_core::style::Style;
use tuvix_core::terminal::Terminal;

/// The architecture budget viewport: 200x60.
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

/// Scatters `count` single-cell changes across the buffer.
fn scatter(buffer: &mut Buffer, count: usize) {
    for index in 0..count {
        let x = ((index * 37 + 11) % usize::from(WIDTH)) as u16;
        let y = ((index * 53 + 7) % usize::from(HEIGHT)) as u16;
        buffer.set_string(x, y, "X", Style::new().fg(tuvix_core::style::Color::Red));
    }
}

fn bench_gates(c: &mut Criterion) {
    let mut group = c.benchmark_group("gates");
    group.throughput(Throughput::Elements(u64::from(WIDTH) * u64::from(HEIGHT)));

    // B8 — no-op frame decision: two identical frames, the diff must walk
    // the viewport and emit nothing.
    let steady = text_buffer();
    group.bench_function("b8_noop_frame_decision_200x60", |b| {
        b.iter(|| black_box(&steady).diff_runs(black_box(&steady)).count());
    });

    // B2 — steady-state frame: alternate between two 1k-cell-apart states
    // through the whole stack (paint -> diff -> TestBackend apply).
    let base = text_buffer();
    let mut changed = text_buffer();
    scatter(&mut changed, 1_000);
    let mut show_changed = true;
    let mut terminal = Terminal::new(TestBackend::new(Size::new(WIDTH, HEIGHT))).unwrap();
    terminal
        .draw(|frame| frame.buffer.content.clone_from(&base.content))
        .unwrap();
    group.bench_function("b2_steady_frame_1k_cells_200x60", |b| {
        b.iter(|| {
            let snapshot = if show_changed {
                changed.content.clone()
            } else {
                base.content.clone()
            };
            show_changed = !show_changed;
            terminal
                .draw(|frame| frame.buffer.content.clone_from(&snapshot))
                .unwrap();
        });
    });

    // B5 — incremental layout: a hot 20-constraint split served from the
    // split cache.
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
    let _ = layout.split(area); // warm the cache so the bench measures hits
    group.bench_function("b5_layout_cached_20_200x60", |b| {
        b.iter(|| layout.split(black_box(area)));
    });

    group.finish();
}

criterion_group!(benches, bench_gates);
criterion_main!(benches);
