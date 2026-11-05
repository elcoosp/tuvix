//! M8 benchmarks: the responsiveness surfaces.
//!
//! Targets are recorded in docs/BENCHMARKS.md. The scheduler's idle tick
//! is the static-content budget as a type (O(1), no probe call); the
//! preempted tick is the cancellation contract's cost; the B10 gate
//! (input-to-paint under continuous input) lives in
//! `tests/bench_gates.rs` with the other recorded gates.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use tuvix_core::backend::TestBackend;
use tuvix_core::cancellation::CancellationToken;
use tuvix_core::geometry::Size;
use tuvix_core::scheduler::{Scheduler, TickOutcome};
use tuvix_core::style::Style;
use tuvix_core::terminal::Terminal;

const W: u16 = 200;
const H: u16 = 60;

/// A small but real dashboard paint: a header, a status row and a data
/// band across the budget viewport.
fn paint_dashboard(frame: &mut tuvix_core::terminal::Frame<'_>, tick: u64) {
    let header = format!("SCHEDULER BENCH tick {tick}");
    frame.buffer.set_string(0, 0, header.as_str(), Style::new());
    for row in 1..H.min(20) {
        let band = format!("row {row:02}: the quick brown fox jumps over the lazy dog {tick}");
        frame.buffer.set_string(0, row, band.as_str(), Style::new());
    }
}

fn bench_scheduler(c: &mut Criterion) {
    let mut group = c.benchmark_group("scheduler");

    // S1 — the idle tick: no request pending. The O(1) static-content
    // decision; the probe must not even be consulted.
    group.bench_function("s1_idle_tick_200x60", |b| {
        let terminal = Terminal::new(TestBackend::new(Size::new(W, H))).unwrap();
        let mut scheduler = Scheduler::new(terminal);
        scheduler.set_probe(|| false);
        b.iter(|| {
            let outcome = black_box(&mut scheduler)
                .tick(|_| {})
                .expect("TestBackend cannot fail");
            debug_assert_eq!(outcome, TickOutcome::Idle);
        });
    });

    // S2 — a committed tick with coalescing: 10 requests collapse into
    // one full dashboard frame over a steady screen.
    group.bench_function("s2_coalesced_10_requests_200x60", |b| {
        let terminal = Terminal::new(TestBackend::new(Size::new(W, H))).unwrap();
        let mut scheduler = Scheduler::new(terminal);
        let mut tick = 0_u64;
        // Steady state: paint once so the diff walk has something to do.
        scheduler.request_render();
        scheduler.tick(|frame| paint_dashboard(frame, 0)).unwrap();
        b.iter(|| {
            for _ in 0..10 {
                black_box(&mut scheduler).request_render();
            }
            tick += 1;
            let local = tick;
            let outcome = black_box(&mut scheduler)
                .tick(|frame| paint_dashboard(frame, local))
                .expect("TestBackend cannot fail");
            debug_assert_eq!(outcome, TickOutcome::Rendered);
        });
    });

    // S3 — the preempted tick: a pending render yields to input. The cost
    // of the priority decision itself (probe call + yield bookkeeping).
    group.bench_function("s3_yielded_tick_probe_true", |b| {
        let terminal = Terminal::new(TestBackend::new(Size::new(W, H))).unwrap();
        let mut scheduler = Scheduler::new(terminal);
        scheduler.set_probe(|| true);
        b.iter(|| {
            black_box(&mut scheduler).request_render();
            let outcome = black_box(&mut scheduler)
                .tick(|_| {})
                .expect("TestBackend cannot fail");
            debug_assert_eq!(outcome, TickOutcome::Yielded);
        });
    });

    // S4 — the cancelled tick: a pending render abandoned by an
    // already-cancelled token (zero terminal I/O).
    group.bench_function("s4_cancelled_tick_pre_token", |b| {
        let terminal = Terminal::new(TestBackend::new(Size::new(W, H))).unwrap();
        let mut scheduler = Scheduler::new(terminal);
        let token = CancellationToken::new();
        token.cancel();
        b.iter(|| {
            black_box(&mut scheduler).request_render();
            let outcome = black_box(&mut scheduler)
                .tick_cancellable(&token, |_| {})
                .expect("TestBackend cannot fail");
            debug_assert_eq!(outcome, TickOutcome::Cancelled);
        });
    });

    // S5 — the token check itself: the stage-boundary cost every
    // cancellable frame pays at its checkpoints.
    group.bench_function("s5_cancellation_checkpoint", |b| {
        let token = CancellationToken::new();
        let pass = token.child();
        b.iter(|| {
            for _ in 0..10 {
                black_box(pass.is_cancelled());
            }
        });
    });

    group.finish();
}

criterion_group!(benches, bench_scheduler);
criterion_main!(benches);
