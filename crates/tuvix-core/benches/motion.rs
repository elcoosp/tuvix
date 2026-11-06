//! M13 motion benches: the animation layer under measurement.
//!
//! Targets (docs/BENCHMARKS.md, the M13 scorecard): one eased evaluation
//! (the per-bar per-frame cost), the B16 shape — 1,000 concurrent tweens
//! advanced in one 16 ms tick — the per-frame value read, and the RGB
//! color blend. The B16 gate itself lives in
//! `tests/m13_motion.rs` with the other `#[ignore]`d timing gates.

use std::hint::black_box;
use std::time::Duration;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use tuvix_core::anim::{Animator, Easing, Progress, Repeat, Tween};

/// The 60 fps tick the scheduler coalesces to.
const TICK: Duration = Duration::from_millis(16);
/// The concurrent-fleet size the B16 gate budgets.
const FLEET: u32 = 1_000;

fn bench_motion(c: &mut Criterion) {
    let mut group = c.benchmark_group("motion");

    // A1 — one eased evaluation: the priciest curves in the catalog.
    // Every animated bar pays this once per frame.
    group.bench_function("a1_ease_out_elastic", |b| {
        b.iter(|| {
            let p = Progress::new(black_box(0.37));
            black_box(Easing::OutElastic.ease(p))
        });
    });
    group.bench_function("a2_ease_out_bounce", |b| {
        b.iter(|| {
            let p = Progress::new(black_box(0.73));
            black_box(Easing::OutBounce.ease(p))
        });
    });

    // A3 — the B16 shape: 1,000 infinite alternating tweens advanced one
    // frame. Elements throughput reports the per-tween cost.
    group.throughput(Throughput::Elements(u64::from(FLEET)));
    group.bench_function("a3_animator_advance_1000_tweens_16ms", |b| {
        let mut animator: Animator<u32, f64> = Animator::new();
        for index in 0..FLEET {
            let from = f64::from(index % 7);
            animator.start(
                index,
                Tween::new(from, from + 10.0, Duration::from_millis(10_000))
                    .easing(Easing::OutCubic)
                    .repeat(Repeat::Infinite)
                    .alternate(),
            );
        }
        b.iter(|| {
            black_box(&mut animator).advance(TICK);
        });
    });
    group.throughput(Throughput::Bytes(1));

    // A4 — one value read on a warm tween: what every animated widget
    // pays to style itself.
    group.bench_function("a4_tween_value_read", |b| {
        let mut tween = Tween::new(0.0_f64, 100.0, Duration::from_secs(1)).easing(Easing::OutCubic);
        tween.advance(Duration::from_millis(250));
        b.iter(|| black_box(&tween).value());
    });

    group.finish();
}

criterion_group!(benches, bench_motion);
criterion_main!(benches);
