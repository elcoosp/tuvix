//! Benchmarks for the M16 layer module: the floating composite at the
//! architecture budget viewport — the widget-side counterpart of the
//! B19 gate (see `tests/m16_overlay.rs`).

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use tuvix_core::buffer::Buffer;
use tuvix_core::geometry::{Rect, Size};
use tuvix_core::layer::{Placement, place};
use tuvix_core::layout::{Constraint, Layout};
use tuvix_core::widget::{RenderWidget, Widget};
use tuvix_widgets::{Block, List, ListItem, ListState, Overlay, Select, SelectState};

/// The architecture budget viewport: 200x60.
const WIDTH: u16 = 200;
const HEIGHT: u16 = 60;

/// The B19 base: a bordered dashboard with a 24-row list and a select
/// trigger row (row 21).
fn paint_dashboard_base(buffer: &mut Buffer) {
    let area = Rect::new(0, 0, WIDTH, HEIGHT);
    let block = Block::bordered().title("dashboard");
    block.clone().render(area, buffer);
    let inner = block.inner(area);

    let [list_area, trigger_area, _rest] = Layout::vertical([
        Constraint::Min(20),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(&inner);

    let rows: Vec<ListItem> = (0..24)
        .map(|row| ListItem::new(format!("row {row}")))
        .collect();
    List::new(rows).render(list_area, buffer, &mut ListState::default());
    let mut trigger = SelectState::new().with_selected(Some(1));
    Select::new(["eu-west", "eu-north", "us-east"])
        .label("region")
        .placeholder("pick one")
        .render(trigger_area, buffer, &mut trigger);
}

fn bench_layers(c: &mut Criterion) {
    let mut group = c.benchmark_group("gates");
    group.throughput(Throughput::Elements(u64::from(WIDTH) * u64::from(HEIGHT)));

    // M16 gate bench: the base dashboard plus the open dropdown —
    // `place` resolves the area below the trigger, `Overlay` erases it
    // (wide-glyph pairs repaired) and paints the bordered body, the
    // option list paints into the clean surface. One frame, the way a
    // filter-as-you-type dropdown repaints.
    group.bench_function("m16_floating_dropdown_composite_200x60", |b| {
        b.iter(|| {
            let area = Rect::new(0, 0, WIDTH, HEIGHT);
            let mut buffer = Buffer::empty(area);
            paint_dashboard_base(&mut buffer);

            let body_area = place(
                area,
                Rect::new(1, 21, WIDTH - 2, 1),
                Size::new(30, 18),
                Placement::Below,
            );
            Overlay::new(Block::bordered().title("region")).render(body_area, &mut buffer);
            let rows: Vec<ListItem> = (0..16)
                .map(|row| ListItem::new(format!("eu-region-{row:02}")))
                .collect();
            List::new(rows).render(
                Block::bordered().inner(body_area),
                &mut buffer,
                &mut ListState::default(),
            );
            black_box(buffer)
        });
    });

    group.finish();
}

criterion_group!(benches, bench_layers);
criterion_main!(benches);
