//! Replay benchmarks: the M10 replay loop over recorded sessions.
//!
//! Measures the two replay costs the B12 gate budgets: driving 100
//! recorded 200x60 frames through a deterministic app (two parses, one
//! serialize and one paint per step), and reconstructing the seed
//! checkpoint of a bounded window.

use std::hint::black_box;
use std::time::Duration;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};

use tuvix_core::backend::TestBackend;
use tuvix_core::buffer::Buffer;
use tuvix_core::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use tuvix_core::geometry::Size;
use tuvix_core::replay::{ReplayStep, SessionReplay};
use tuvix_core::session::SessionRecorder;
use tuvix_core::style::Style;
use tuvix_core::terminal::Terminal;

const WIDTH: u16 = 200;
const HEIGHT: u16 = 60;
const FRAMES: usize = 100;

fn paint_dashboard(buffer: &mut Buffer, mutation: usize) {
    let header = format!("replay bench mutation {mutation} — the quick brown fox");
    buffer.set_string(0, 0, header.as_str(), Style::new());
    for row in 1..HEIGHT {
        let band = format!("row {row:02}: 0123456789 abcdefghijklmnopqrstuvwxyz {row}");
        buffer.set_string(0, row, band.as_str(), Style::new());
    }
}

/// Records FRAMES dashboard frames, every other one event-driven.
fn recorded_session() -> SessionReplay {
    let recorder = SessionRecorder::unbounded();
    let mut terminal = Terminal::new(TestBackend::new(Size::new(WIDTH, HEIGHT))).unwrap();
    terminal.set_preview(recorder.clone());
    let mut mutations = 0usize;
    for frame in 0..FRAMES {
        if frame % 2 == 0 {
            recorder.record_event(Event::Key(KeyEvent::new(
                KeyCode::Char('x'),
                KeyModifiers::empty(),
            )));
            mutations += 1;
        }
        let mutation = mutations;
        terminal
            .draw(|frame| paint_dashboard(frame.buffer, mutation))
            .unwrap();
    }
    drop(terminal);
    SessionReplay::from_recorder(&recorder)
}

fn bench_replay(c: &mut Criterion) {
    let mut group = c.benchmark_group("replay");
    group.throughput(Throughput::Elements(u64::try_from(FRAMES).unwrap_or(1)));

    let replay = recorded_session();

    // The full golden loop: 100 steps of parse + paint + serialize +
    // parse, the regression-test use case.
    group.bench_function("r1_replay_100_frames_200x60", |b| {
        b.iter(|| {
            let mut mutations = 0usize;
            let report = replay
                .run(|step: ReplayStep<'_>| {
                    for event in step.events {
                        if let Event::Key(key_event) = event
                            && key_event.code == KeyCode::Char('x')
                        {
                            mutations += 1;
                        }
                    }
                    paint_dashboard(step.buffer, mutations);
                })
                .expect("recorded snapshots are canonical");
            black_box(report.matched())
        });
    });

    // The bounded-window checkpoint: parse the first retained frame back
    // into a live buffer (the "resume from here" flow).
    group.bench_function("r2_seed_reconstruct_200x60", |b| {
        b.iter(|| {
            let seed = replay.seed_buffer().expect("retained frames");
            black_box(seed.area)
        });
    });

    group.finish();
}

const _: Option<Duration> = None;

criterion_group!(benches, bench_replay);
criterion_main!(benches);
