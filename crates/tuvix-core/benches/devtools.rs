//! M9 benchmarks: the time-travel `DevTools` surfaces.
//!
//! Targets are recorded in docs/BENCHMARKS.md. The scrub path is the
//! `DevTools` story's latency: reconstructing any recorded frame into a
//! renderable grid must be far below a frame budget. The B11 gate
//! (scrub latency + recording overhead) lives in `tests/bench_gates.rs`.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use tuvix_core::backend::TestBackend;
use tuvix_core::geometry::{Rect, Size};
use tuvix_core::preview::FrameSink;
use tuvix_core::screen::Screen;
use tuvix_core::session::SessionRecorder;
use tuvix_core::style::Style;
use tuvix_core::terminal::Terminal;

const W: u16 = 200;
const H: u16 = 60;

/// A full-screen dashboard paint over the budget viewport: a header and
/// a data band that changes every tick (a realistic snapshot), captured
/// through the preview hook exactly as a recording would see it.
fn paint_dashboard(terminal: &mut Terminal<TestBackend>, tick: u64) -> String {
    let history = tuvix_core::preview::HistorySink::unbounded();
    terminal.set_preview(history.clone());
    terminal
        .draw(|frame| {
            let header = format!("DEVTOOLS BENCH tick {tick} — the quick brown fox");
            frame.buffer.set_string(0, 0, header.as_str(), Style::new());
            for row in 1..H {
                let band = format!("row {row:02}: 0123456789 abcdefghijklmnopqrstuvwxyz {tick}");
                frame.buffer.set_string(0, row, band.as_str(), Style::new());
            }
        })
        .expect("TestBackend cannot fail");
    terminal.clear_preview();
    history.latest().expect("the draw flushed one frame")
}

fn bench_devtools(c: &mut Criterion) {
    let mut group = c.benchmark_group("devtools");

    // Capture a real 200x60 snapshot once for the reconstruction and
    // diff benches.
    let mut terminal = Terminal::new(TestBackend::new(Size::new(W, H))).unwrap();
    let snapshot_a = paint_dashboard(&mut terminal, 1);
    let snapshot_b = paint_dashboard(&mut terminal, 2);

    // D1 — the scrub path: parse a recorded 200x60 snapshot back into a
    // Screen. This is "jump to any moment" latency.
    group.bench_function("d1_reconstruct_screen_200x60", |b| {
        b.iter(|| {
            let screen = Screen::from_ansi(black_box(&snapshot_a)).expect("canonical");
            black_box(screen.width());
        });
    });

    // D2 — reconstruct + paste into a buffer (the FrameInspector path).
    let screen_a = Screen::from_ansi(&snapshot_a).unwrap();
    let screen_b = Screen::from_ansi(&snapshot_b).unwrap();
    group.bench_function("d2_screen_to_buffer_200x60", |b| {
        let area = Rect::new(0, 0, W, H);
        b.iter(|| black_box(&screen_a).to_buffer(area));
    });

    // D3 — frame-to-frame diff over the union grid.
    group.bench_function("d3_screen_diff_200x60", |b| {
        b.iter(|| {
            let changed = black_box(&screen_a).diff(black_box(&screen_b));
            black_box(changed.len());
        });
    });

    // D4 — the recording overhead per frame (SessionRecorder on top of
    // the serialize the preview hook already performs).
    group.bench_function("d4_record_frame_200x60", |b| {
        let session = SessionRecorder::unbounded();
        let mut sink = session.clone();
        b.iter(|| {
            sink.on_frame(black_box(&snapshot_a));
        });
    });

    // D5 — the golden-session export: to_json over a 100-frame session
    // (amortized per frame).
    let session = SessionRecorder::unbounded();
    {
        let mut sink = session.clone();
        for _ in 0..100 {
            sink.on_frame(&snapshot_a);
        }
    }
    group.bench_function("d5_session_json_export_100_frames", |b| {
        b.iter(|| {
            let bytes = black_box(&session).to_json().len();
            black_box(bytes);
        });
    });

    group.finish();
}

criterion_group!(benches, bench_devtools);
criterion_main!(benches);
