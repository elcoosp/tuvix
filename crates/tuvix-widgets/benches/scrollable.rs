//! Benchmarks for the M11 `Scrollable` viewport: the paint-into-scratch +
//! blit pipeline. The gate these back is B14 (a scrolled 200x60 repaint
//! inside the steady-frame budget), measured as an `#[ignore]` timing test
//! in `tests/m11_interaction.rs`.

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use tuvix_core::buffer::Buffer;
use tuvix_core::geometry::{Rect, Size};
use tuvix_core::style::Style;
use tuvix_core::widget::RenderWidget;
use tuvix_widgets::{Scrollable, Scrollbar, ViewportState};

/// The architecture budget viewport: 200x60.
const WIDTH: u16 = 200;
const HEIGHT: u16 = 60;
/// Four hundred rows of realistic log-like content behind the viewport.
const CONTENT_ROWS: u16 = 400;

/// A log-like content widget: one line per row, rendered like a real app.
#[derive(Clone, Copy)]
struct LogLines(u16);

impl RenderWidget for LogLines {
    type State = ();

    fn render(self, area: Rect, buf: &mut Buffer, _state: &mut ()) {
        for row in 0..self.0.min(area.height) {
            let line = format!(
                "2026-11-05 12:{row:02}:00 INFO  worker-{row} handled request in {ms}ms",
                ms = row % 97
            );
            buf.set_string(area.x, area.y + row, line, Style::new());
        }
    }
}

fn bench_scrollable(c: &mut Criterion) {
    let mut group = c.benchmark_group("scrollable");
    group.throughput(Throughput::Elements(u64::from(WIDTH) * u64::from(HEIGHT)));
    let area = Rect::new(0, 0, WIDTH, HEIGHT);

    // First paint: the scratch surface starts empty and the child paints
    // the full 400-row content once.
    group.bench_function("first_paint_400_rows_200x60", |b| {
        b.iter_batched(
            || {
                (
                    Scrollable::new(LogLines(CONTENT_ROWS)).content(Size::new(WIDTH, CONTENT_ROWS)),
                    ViewportState::new(()),
                )
            },
            |(widget, mut state)| {
                widget
                    .clone()
                    .render(black_box(area), &mut Buffer::empty(area), &mut state);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    // Steady state: the scratch surface is warm; a scroll step blits the
    // new window out of the cached paint. This is the B14 cost.
    group.bench_function("scrolled_repaint_400_rows_200x60", |b| {
        let widget =
            Scrollable::new(LogLines(CONTENT_ROWS)).content(Size::new(WIDTH, CONTENT_ROWS));
        let mut state = ViewportState::new(());
        widget
            .clone()
            .render(area, &mut Buffer::empty(area), &mut state);
        b.iter(|| {
            state.scroll_down(1);
            widget
                .clone()
                .render(black_box(area), &mut Buffer::empty(area), &mut state);
        });
    });

    // After an invalidation: the child paints the full content again and
    // the window blits — the app-paid cost of a content change.
    group.bench_function("invalidated_repaint_400_rows_200x60", |b| {
        let widget =
            Scrollable::new(LogLines(CONTENT_ROWS)).content(Size::new(WIDTH, CONTENT_ROWS));
        let mut state = ViewportState::new(());
        widget
            .clone()
            .render(area, &mut Buffer::empty(area), &mut state);
        b.iter(|| {
            state.invalidate();
            widget
                .clone()
                .render(black_box(area), &mut Buffer::empty(area), &mut state);
        });
    });

    // With the attached scrollbar: the viewport shrinks by one column.
    group.bench_function("scrolled_repaint_with_scrollbar_200x60", |b| {
        let widget = Scrollable::new(LogLines(CONTENT_ROWS))
            .content(Size::new(WIDTH, CONTENT_ROWS))
            .scrollbar(Scrollbar::default());
        let mut state = ViewportState::new(());
        widget
            .clone()
            .render(area, &mut Buffer::empty(area), &mut state);
        b.iter(|| {
            state.scroll_down(1);
            widget
                .clone()
                .render(black_box(area), &mut Buffer::empty(area), &mut state);
        });
    });

    group.finish();
}

criterion_group!(scrollable, bench_scrollable);
criterion_main!(scrollable);
