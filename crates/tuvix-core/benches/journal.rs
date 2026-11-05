//! M4 benchmarks: the persistent-DOM frame journal.
//!
//! The B8 note (docs/BENCHMARKS.md) budgeted the no-op frame decision at
//! 50 us; the immediate-mode equivalent (an identical-frame diff walk) cost
//! 103-125 us at the 200x60 budget viewport. The journal's O(1) decision
//! finally meets the budget: painting nothing must cost nothing, regardless
//! of viewport size. Partial frames are measured too — a ticking clock on a
//! static dashboard repaints its row band, not the screen.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use tuvix_core::backend::TestBackend;
use tuvix_core::buffer::Buffer;
use tuvix_core::geometry::{Rect, Size};
use tuvix_core::style::Style;
use tuvix_core::terminal::Terminal;

/// The architecture budget viewport: 200x60.
const WIDTH: u16 = 200;
const HEIGHT: u16 = 60;

/// A "dashboard": static text on every row, persisted in the DOM.
fn dashboard(terminal: &mut Terminal<TestBackend>) {
    terminal
        .draw_persistent(|frame| {
            for row in 0..HEIGHT {
                let line = format!(
                    "row {row:02}: the quick brown fox jumps over the lazy dog — 汉字 spacing"
                );
                frame.buffer.set_string(0, row, line, Style::new());
            }
        })
        .unwrap();
}

fn bench_journal(c: &mut Criterion) {
    let mut group = c.benchmark_group("journal");

    // B8 (journal) — the O(1) no-op decision at the budget viewport: a
    // frame whose closure paints nothing. The immediate-mode equivalent
    // (identical-frame diff walk) measured 103-125 us here.
    let mut terminal =
        Terminal::new_persistent(TestBackend::new(Size::new(WIDTH, HEIGHT))).unwrap();
    dashboard(&mut terminal);
    group.bench_function("b8_journal_noop_frame_200x60", |b| {
        b.iter(|| terminal.draw_persistent(|_| {}).unwrap());
    });

    // Partial frame: one ticking clock region (a 20-column band on row 59)
    // repaints per frame; 59 of 60 rows are untouched DOM.
    let mut ticks = 0u64;
    group.bench_function("journal_partial_tick_band_200x60", |b| {
        b.iter(|| {
            ticks += 1;
            let text = format!("tick {ticks}");
            terminal
                .draw_persistent(|frame| {
                    frame.buffer.clear(Rect::new(0, HEIGHT - 1, 20, 1));
                    frame.buffer.set_string(0, HEIGHT - 1, text, Style::new());
                })
                .unwrap();
        });
    });

    // Full repaint through the journal path (the app resets the DOM it
    // owns and repaints everything): the journal flips full and the classic
    // full diff runs, plus the band-cell reference sync.
    group.bench_function("journal_full_repaint_200x60", |b| {
        b.iter(|| {
            terminal
                .draw_persistent(|frame| {
                    frame.buffer.reset();
                    for row in 0..HEIGHT {
                        let line = format!("row {row:02}: the quick brown fox");
                        frame.buffer.set_string(0, row, line, Style::new());
                    }
                })
                .unwrap();
        });
    });

    // No-op decision in isolation (no closure, no backend): the empty
    // journal check itself.
    let mut dom = Buffer::empty(Rect::new(0, 0, WIDTH, HEIGHT));
    dom.enable_journal();
    group.bench_function("journal_empty_check_200x60", |b| {
        b.iter(|| black_box(dom.journal_is_empty()));
    });

    group.finish();
}

criterion_group!(benches, bench_journal);
criterion_main!(benches);
