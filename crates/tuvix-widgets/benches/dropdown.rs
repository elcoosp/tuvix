//! Benchmarks for the M17 floating dropdown: the scrolling composite
//! at the architecture budget viewport — the widget-side counterpart
//! of the B20 gate (see `tests/m17_floating_select.rs`).
//!
//! The M16 benches measured the static dropdown (16 options, the
//! window fixed); this one measures the honest long list: 100 options
//! whose visible window scrolls with the highlight, so the numbers
//! cover the sizing decision (`dropdown_rows`), the placement, the
//! erase-then-paint and the windowed option paint in one frame —
//! what a filter-as-you-type dropdown over a long list pays.

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use tuvix_core::buffer::Buffer;
use tuvix_core::geometry::{Rect, Size};
use tuvix_core::layer::{Placement, dropdown_rows, place};
use tuvix_core::style::Style;
use tuvix_core::text::Line;
use tuvix_core::widget::{RenderWidget, Widget};
use tuvix_widgets::{Block, Overlay, Select, SelectState};

/// The architecture budget viewport: 200x60.
const WIDTH: u16 = 200;
const HEIGHT: u16 = 60;

/// One hundred options: the long list the window scrolls through.
const OPTIONS: usize = 100;

/// The B20 base: a bordered dashboard with a filled body and a select
/// trigger row (row 21).
fn paint_dashboard_base(buffer: &mut Buffer) {
    let area = Rect::new(0, 0, WIDTH, HEIGHT);
    let block = Block::bordered().title("dashboard");
    block.clone().render(area, buffer);
    buffer.set_string(1, 1, "base row — the quick brown fox", Style::new());
    for row in 2..21_u16 {
        let label = format!("base-{row}");
        buffer.set_string(1, row, label, Style::new());
    }
    let trigger_area = Rect::new(1, 21, WIDTH - 2, 1);
    let mut trigger = SelectState::new().with_selected(Some(1));
    Select::new(["eu-west", "eu-north", "us-east"])
        .label("region")
        .placeholder("pick one")
        .render(trigger_area, buffer, &mut trigger);
}

/// One composite frame with the window scrolled to `highlight_step`.
fn scroll_composite(highlight_step: usize) -> Buffer {
    let area = Rect::new(0, 0, WIDTH, HEIGHT);
    let mut buffer = Buffer::empty(area);
    paint_dashboard_base(&mut buffer);

    let trigger_area = Rect::new(1, 21, WIDTH - 2, 1);
    let rows = dropdown_rows(area, trigger_area, OPTIONS);
    let surface = place(
        area,
        trigger_area,
        Size::new(30, rows.saturating_add(2)),
        Placement::Below,
    );
    Overlay::new(Block::bordered().title("region")).render(surface, &mut buffer);

    let options: Vec<Line> = (0..OPTIONS)
        .map(|row| Line::from(format!("eu-region-{row:03}")))
        .collect();
    let mut state = SelectState::new();
    state.open();
    for _ in 0..highlight_step {
        state.select_next(OPTIONS);
    }
    Select::new(options).render_options(Block::bordered().inner(surface), &mut buffer, &state);
    buffer
}

fn bench_dropdown(c: &mut Criterion) {
    let mut group = c.benchmark_group("gates");
    group.throughput(Throughput::Elements(u64::from(WIDTH) * u64::from(HEIGHT)));

    // M17 gate bench: the base dashboard plus the open 100-option
    // dropdown with the window scrolled to the middle of the list —
    // sizing, placement, erase, and the visible-window paint in one
    // frame, the way keyboard navigation through a long dropdown
    // repaints.
    group.bench_function("m17_floating_select_scroll_composite_200x60", |b| {
        b.iter(|| black_box(scroll_composite(OPTIONS / 2)));
    });

    group.finish();
}

criterion_group!(benches, bench_dropdown);
criterion_main!(benches);
