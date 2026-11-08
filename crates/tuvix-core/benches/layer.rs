//! M16 layer benches: the floating-surface placement under measurement.
//!
//! Targets (docs/BENCHMARKS.md, the M16 scorecard): one placement
//! decision on the three paths an app cycles through — the fast path
//! (the dropdown fits below its trigger), the flip path (the edge is
//! near, the mirror side wins) and the tooltip clamp — the whole cost
//! of "where does the popup go" per frame. The B19 composite gate that
//! rides on these decisions lives in `tests/m16_overlay.rs` with the
//! other `#[ignore]`d timing gates.

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use tuvix_core::geometry::{Position, Rect, Size};
use tuvix_core::layer::{Placement, place};

fn bench_layer(c: &mut Criterion) {
    let mut group = c.benchmark_group("layer");
    let host = Rect::new(0, 0, 200, 60);
    let trigger = Rect::new(10, 20, 24, 1);

    group.throughput(Throughput::Elements(1));

    // L1 — the fast path: the dropdown fits below its trigger.
    group.bench_function("l1_place_below_fits", |b| {
        b.iter(|| {
            black_box(place(
                black_box(host),
                black_box(trigger),
                Size::new(24, 12),
                Placement::Below,
            ))
        });
    });

    // L2 — the flip path: the trigger sits near the bottom edge, the
    // surface opens above instead.
    group.bench_function("l2_place_below_flips", |b| {
        b.iter(|| {
            black_box(place(
                black_box(host),
                black_box(Rect::new(10, 50, 24, 1)),
                Size::new(24, 12),
                Placement::Below,
            ))
        });
    });

    // L3 — the tooltip: the corner clamps at the pointer position.
    group.bench_function("l3_place_at_tooltip", |b| {
        b.iter(|| {
            black_box(place(
                black_box(host),
                black_box(trigger),
                Size::new(20, 1),
                Placement::At(Position::new(190, 55)),
            ))
        });
    });

    group.finish();
}

criterion_group!(benches, bench_layer);
criterion_main!(benches);
