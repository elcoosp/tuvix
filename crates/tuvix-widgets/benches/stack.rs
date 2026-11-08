//! Benchmarks for the M18 floating-surface stack: the routing layer at
//! the architecture budget viewport — the widget-side counterpart of
//! the B21 gate (see `tests/m18_surface_stack.rs`).
//!
//! The M17 bench measured one floating dropdown; this one measures the
//! managed composite: a dropdown (100 options, window scrolled), a
//! tooltip and a modal routed through a fresh [`SurfaceStack`] per
//! frame — the pushes, the `order()` walk, the erase-then-paint per
//! surface and the windowed option paint in one frame. This is what a
//! dashboard with several open popovers pays per repaint.

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use tuvix_core::buffer::Buffer;
use tuvix_core::geometry::{Position, Rect, Size};
use tuvix_core::layer::{Placement, SurfaceStack, dropdown_rows, place};
use tuvix_core::style::{Color, Style};
use tuvix_core::text::Line;
use tuvix_core::widget::{RenderWidget, Widget};
use tuvix_widgets::{Block, Overlay, Select, SelectState};

/// The architecture budget viewport: 200x60.
const WIDTH: u16 = 200;
const HEIGHT: u16 = 60;

/// One hundred options: the long list the window scrolls through.
const OPTIONS: usize = 100;

/// The app's own surface names — the stack's keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Surface {
    Dropdown,
    Tooltip,
    Modal,
}

/// The B21 base: a bordered dashboard with a filled body and a select
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

/// One frame of the stacked composite: the base, then the dropdown
/// (scrolled to `highlight_step`), the tooltip and the modal routed by
/// the stack.
fn stack_composite(highlight_step: usize) -> Buffer {
    let area = Rect::new(0, 0, WIDTH, HEIGHT);
    let trigger_area = Rect::new(1, 21, WIDTH - 2, 1);

    // The routing: three surfaces open in one stack — the paint order
    // and the hit-map registration order in one sequence.
    let mut stack = SurfaceStack::new();
    stack.push(Surface::Dropdown);
    stack.push(Surface::Tooltip);
    stack.push(Surface::Modal);

    let mut buffer = Buffer::empty(area);
    paint_dashboard_base(&mut buffer);

    for surface in stack.order() {
        match *surface {
            Surface::Dropdown => {
                let rows = dropdown_rows(area, trigger_area, OPTIONS);
                let surface_rect = place(area, trigger_area, Size::new(30, rows), Placement::Below);
                Overlay::new(Block::bordered().title("region")).render(surface_rect, &mut buffer);
                let options: Vec<Line> = (0..OPTIONS)
                    .map(|row| Line::from(format!("eu-region-{row:03}")))
                    .collect();
                let mut state = SelectState::new();
                state.open();
                for _ in 0..highlight_step {
                    state.select_next(OPTIONS);
                }
                Select::new(options)
                    .highlight_style(Style::new().fg(Color::Cyan))
                    .render_options(Block::bordered().inner(surface_rect), &mut buffer, &state);
            }
            Surface::Tooltip => {
                let pointer = Position::new(160, 40);
                let tooltip = place(
                    area,
                    Rect::new(pointer.x, pointer.y, 1, 1),
                    Size::new(14, 3),
                    Placement::At(pointer),
                );
                Overlay::new(Block::bordered().title("tip")).render(tooltip, &mut buffer);
                let inner = Block::bordered().inner(tooltip);
                buffer.set_string(inner.x, inner.y, "floating", Style::new());
            }
            Surface::Modal => {
                let modal = place(area, trigger_area, Size::new(34, 5), Placement::Centered);
                Overlay::new(Block::bordered().title("confirm")).render(modal, &mut buffer);
                let inner = Block::bordered().inner(modal);
                buffer.set_string(inner.x, inner.y, "filter applied.", Style::new());
            }
        }
    }
    buffer
}

fn bench_stack(c: &mut Criterion) {
    let mut group = c.benchmark_group("gates");
    group.throughput(Throughput::Elements(u64::from(WIDTH) * u64::from(HEIGHT)));

    // M18 gate bench: the base dashboard plus the dropdown, tooltip and
    // modal routed through the stack in one frame — the routing, the
    // order walk and three erase-then-paint composites, the way a
    // busy dashboard repaints with several popovers open.
    group.bench_function("m18_stack_composite_200x60", |b| {
        b.iter(|| black_box(stack_composite(OPTIONS / 2)));
    });

    group.finish();
}

criterion_group!(benches, bench_stack);
criterion_main!(benches);
