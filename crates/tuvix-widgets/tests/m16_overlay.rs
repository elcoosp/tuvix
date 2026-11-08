//! M16 integration gates: the [`Overlay`] compositor and the B19 budget.
//!
//! `Overlay` is the M15 popup discipline as a type: it erases its area
//! (the buffer's pair-repairing clear, so no orphaned wide-glyph halves
//! survive) and paints one widget on top — the app resolves the area
//! with `place`, the overlay owns the erase-then-paint sequence.
//!
//! B19 — the floating dropdown composite: a 200x60 dashboard (bordered
//! panel, a 24-row list, a select trigger) with an open dropdown placed
//! over it composites inside **1 ms** p99 — the same half-of-B2 line
//! B14 draws for a scroll storm and B18 for a form panel. A dropdown
//! opens on every keystroke of a filter, so the erase-then-paint step
//! must stay a rounding error against the 2 ms B2 steady-frame budget.

use std::hint::black_box;
use std::time::{Duration, Instant};

use tuvix_core::backend::TestBackend;
use tuvix_core::buffer::Buffer;
use tuvix_core::geometry::{Rect, Size};
use tuvix_core::layer::{Placement, place};
use tuvix_core::layout::{Constraint, Layout};
use tuvix_core::style::Style;
use tuvix_core::terminal::Terminal;
use tuvix_core::widget::{RenderWidget, Widget};
use tuvix_widgets::{Block, List, ListItem, ListState, Overlay, Select, SelectState};

const WIDTH: u16 = 200;
const HEIGHT: u16 = 60;
const FRAMES: usize = 300;

/// A minimal paint probe: writes its text at the area's origin.
struct Label(&'static str);

impl Widget for Label {
    fn render(self, area: Rect, buf: &mut Buffer) {
        buf.set_string(area.x, area.y, self.0, Style::new());
    }
}

/// A stateful paint probe: counts its renders through the contract.
struct Counting;

impl RenderWidget for Counting {
    type State = usize;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut usize) {
        *state += 1;
        buf.set_string(area.x, area.y, "#", Style::new());
    }
}

// ---------------------------------------------------------------------
// The composite primitive.
// ---------------------------------------------------------------------

#[test]
fn overlay_erases_the_base_then_paints() {
    let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 1));
    buffer.set_string(0, 0, "aaaaaaaaaa", Style::new());
    Overlay::new(Label("hi")).render(Rect::new(4, 0, 4, 1), &mut buffer);
    assert_eq!(buffer.to_string(), "aaaahi  aa");
}

#[test]
fn overlay_passes_state_through_the_contract() {
    let mut buffer = Buffer::empty(Rect::new(0, 0, 3, 1));
    let mut renders = 0_usize;
    Overlay::new(Counting).render(Rect::new(0, 0, 3, 1), &mut buffer, &mut renders);
    Overlay::new(Counting).render(Rect::new(0, 0, 3, 1), &mut buffer, &mut renders);
    assert_eq!(renders, 2, "the wrapped widget sees the same state");
    assert_eq!(buffer.to_string(), "#  ");
}

#[test]
fn the_last_overlay_wins_the_overlap() {
    let mut buffer = Buffer::empty(Rect::new(0, 0, 4, 1));
    Overlay::new(Label("aa")).render(Rect::new(0, 0, 4, 1), &mut buffer);
    Overlay::new(Label("bb")).render(Rect::new(0, 0, 4, 1), &mut buffer);
    assert_eq!(buffer.to_string(), "bb  ", "z-order is paint order");
}

#[test]
fn overlay_clear_repairs_wide_pairs_under_the_surface() {
    let mut buffer = Buffer::empty(Rect::new(0, 0, 8, 1));
    buffer.set_string(0, 0, "x日本語x", Style::new());
    // The surface covers the trailing half of 日 and the leading half
    // of 語 — every straddled pair must heal, none may orphan.
    Overlay::new(Label("#")).render(Rect::new(2, 0, 5, 1), &mut buffer);
    assert_eq!(buffer.to_string(), "x #    x");
}

#[test]
fn overlay_outside_the_buffer_is_a_no_op() {
    let mut buffer = Buffer::empty(Rect::new(0, 0, 4, 1));
    buffer.set_string(0, 0, "aaaa", Style::new());
    Overlay::new(Label("hi")).render(Rect::new(40, 40, 4, 1), &mut buffer);
    assert_eq!(buffer.to_string(), "aaaa");
}

// ---------------------------------------------------------------------
// The floating dropdown, end to end through the render stack.
// ---------------------------------------------------------------------

/// The 24x10 dashboard: a bordered panel, a list and a select trigger
/// row — painted the way any app paints its base frame. Returns the
/// trigger's area: that is the anchor the dropdown opens from.
fn paint_dashboard(buffer: &mut Buffer, trigger: &mut SelectState) -> Rect {
    let area = Rect::new(0, 0, 24, 10);
    let block = Block::bordered().title("dash");
    block.clone().render(area, buffer);
    let inner = block.inner(area);

    let [list_area, trigger_area, _rest] = Layout::vertical([
        Constraint::Min(5),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(&inner);

    let rows: Vec<ListItem> = (0..6)
        .map(|row| ListItem::new(format!("row {row}")))
        .collect();
    List::new(rows).render(list_area, buffer, &mut ListState::default());
    Select::new(["eu-west", "eu-north", "us-east"])
        .label("region")
        .placeholder("pick one")
        .render(trigger_area, buffer, trigger);
    trigger_area
}

#[test]
fn a_floating_dropdown_covers_only_its_own_area() {
    let mut terminal = Terminal::new(TestBackend::new(Size::new(24, 10))).unwrap();
    let mut trigger = SelectState::new();

    // Base frame: the trigger shows the placeholder, the list shows through.
    terminal
        .draw(|frame| {
            paint_dashboard(frame.buffer, &mut trigger);
        })
        .unwrap();
    let base = terminal.backend().buffer().to_string();
    assert!(
        base.contains("region: pick one ▾"),
        "trigger missing: {base}"
    );
    assert!(base.contains("row 3"), "base list missing: {base}");

    // Open: the dropdown body is placed above the trigger (it does not
    // fit below in a 10-row app) and overlays the rows beneath it — and
    // only those rows. The recipe is place, overlay, then paint into
    // the clean surface.
    trigger.open();
    terminal
        .draw(|frame| {
            let trigger_area = paint_dashboard(frame.buffer, &mut trigger);
            let dropdown_area = place(
                frame.area(),
                trigger_area,
                Size::new(16, 5),
                Placement::Above,
            );
            Overlay::new(Block::bordered().title("region")).render(dropdown_area, frame.buffer);
            let body = Block::bordered().inner(dropdown_area);
            List::new([
                ListItem::new("eu-region-00"),
                ListItem::new("eu-region-01"),
                ListItem::new("eu-region-02"),
            ])
            .render(body, frame.buffer, &mut ListState::default());
        })
        .unwrap();
    let floated = terminal.backend().buffer().to_string();
    assert!(
        floated.contains("eu-region-00"),
        "options missing: {floated}"
    );
    assert!(floated.contains("row 0"), "uncovered base lost: {floated}");
    assert!(
        !floated.contains("row 3"),
        "the dropdown must erase the base rows it covers: {floated}"
    );
    assert!(
        floated.contains("region: pick one ▴"),
        "the open trigger survives its own dropdown: {floated}"
    );
}

// ---------------------------------------------------------------------
// B19 — the floating dropdown composite budget (measured, not asserted
// in the normal suite; the perf workflow runs it with --ignored).
// ---------------------------------------------------------------------

/// One frame of the B19 composite: the base dashboard plus the open
/// dropdown body placed below the trigger, the way a filter-as-you-type
/// dropdown repaints. The trigger row is 21 (a bordered 200x60 panel
/// with a Min(20) list above a one-row trigger).
fn dropdown_composite_frame() -> Buffer {
    let area = Rect::new(0, 0, WIDTH, HEIGHT);
    let mut buffer = Buffer::empty(area);
    let mut trigger = SelectState::new().with_selected(Some(1));
    paint_dashboard_200x60(&mut buffer, &mut trigger);

    let body_area = place(
        area,
        Rect::new(1, 21, WIDTH - 2, 1),
        Size::new(30, 18),
        Placement::Below,
    );
    let rows: Vec<ListItem> = (0..16)
        .map(|row| ListItem::new(format!("eu-region-{row:02}")))
        .collect();
    Overlay::new(Block::bordered().title("region")).render(body_area, &mut buffer);
    List::new(rows).render(
        Block::bordered().inner(body_area),
        &mut buffer,
        &mut ListState::default(),
    );
    buffer
}

/// The 200x60 variant of the dashboard used by the B19 gate.
fn paint_dashboard_200x60(buffer: &mut Buffer, trigger: &mut SelectState) {
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
    Select::new(["eu-west", "eu-north", "us-east"])
        .label("region")
        .placeholder("pick one")
        .render(trigger_area, buffer, trigger);
}

#[test]
#[ignore = "timing gate: run with --ignored (perf workflow)"]
fn b19_floating_dropdown_composite_within_budget() {
    // Warm-up (allocator + caches; not part of the gate).
    for _ in 0..16 {
        black_box(dropdown_composite_frame());
    }

    let mut latencies = Vec::with_capacity(FRAMES);
    for _ in 0..FRAMES {
        let start = Instant::now();
        let buffer = black_box(dropdown_composite_frame());
        let elapsed = start.elapsed();
        // Sanity: the dropdown really composited over the dashboard.
        assert!(buffer.to_string().contains("eu-region-15"));
        latencies.push(elapsed);
    }
    latencies.sort();
    let median = latencies[FRAMES / 2];
    let p99 = latencies[FRAMES * 99 / 100 - 1];
    let max = latencies[FRAMES - 1];

    assert!(
        p99 <= Duration::from_millis(1),
        "B19 gate failed: floating-dropdown composite p99 {p99:?} (budget 1 ms)"
    );
    println!(
        "B19 gate: floating-dropdown composite median {median:?}, p99 {p99:?}, max {max:?} over {FRAMES} frames @ {WIDTH}x{HEIGHT} (budget 1 ms)"
    );
}
