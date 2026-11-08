//! M18 integration gates: the floating-surface stack as the routing
//! layer of the M16/M17 composite.
//!
//! The seams this pins (docs/ARCHITECTURE.md, the M18 decisions):
//!
//! - **one stack drives the frame** — the base paints, then the
//!   surfaces paint bottom-to-top in [`SurfaceStack::order`]; the same
//!   sequence registers the `HitMap`, so the screen and the mouse agree
//!   on z-order by construction instead of by parallel hand-rolled
//!   bookkeeping;
//! - **`close_top` is the Esc precedence** — the top surface dismisses
//!   first (a modal over an open dropdown), the seam the hand-rolled
//!   example got wrong (Esc closing the dropdown underneath an open
//!   modal);
//! - **raising flips the composite** — pushing an open surface reorders
//!   the paint, so "bring the dropdown to front" is one call;
//! - the M17 visibility pin survives the stack: the highlighted option
//!   is always on screen through the managed composite.
//!
//! B21 — the stacked composite budget: dashboard + scrolled dropdown +
//! tooltip + modal routed through the stack in one 200x60 frame inside
//! **1 ms** p99 — the B20 line drawn for three surfaces and the routing.

use std::hint::black_box;
use std::time::{Duration, Instant};

use tuvix_core::buffer::Buffer;
use tuvix_core::geometry::{Position, Rect, Size};
use tuvix_core::hit::HitMap;
use tuvix_core::layer::{Placement, SurfaceStack, dropdown_rows, place};
use tuvix_core::style::{Color, Style};
use tuvix_core::text::Line;
use tuvix_core::widget::{RenderWidget, Widget};
use tuvix_widgets::{Block, Overlay, Select, SelectState};

const REGIONS: [&str; 6] = [
    "eu-west", "eu-north", "us-east", "us-west", "ap-south", "sa-east",
];

/// The app names its floating surfaces with its own enum — the stack
/// never learns the widget types, exactly like the `FocusRing` pane
/// enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Panel {
    Dropdown,
    Tooltip,
    Modal,
}

use Panel::{Dropdown, Modal, Tooltip};

/// The dashboard base: fourteen text rows plus a `Select` trigger on
/// row 8. Returns the trigger area — the dropdown's anchor.
fn paint_dashboard(buffer: &mut Buffer, trigger: &mut SelectState) -> Rect {
    for row in 0..14_u16 {
        let label = format!("base-{row}");
        buffer.set_string(0, row, label, Style::new());
    }
    let trigger_area = Rect::new(2, 8, 14, 1);
    Select::new(REGIONS)
        .label("region")
        .placeholder("all")
        .render(trigger_area, buffer, trigger);
    trigger_area
}

/// The dropdown surface: size, place, erase, paint (the M17 recipe).
fn float_dropdown(
    buffer: &mut Buffer,
    host: Rect,
    trigger_area: Rect,
    state: &SelectState,
) -> Rect {
    let rows = dropdown_rows(host, trigger_area, REGIONS.len());
    let surface = place(
        host,
        trigger_area,
        Size::new(16, rows.saturating_add(2)),
        Placement::Below,
    );
    Overlay::new(Block::bordered().title("region")).render(surface, buffer);
    Select::new(REGIONS).render_options(Block::bordered().inner(surface), buffer, state);
    surface
}

/// The modal surface: centered, erased, painted. 24 columns wide so it
/// genuinely overlaps the dropdown's option text (the overlap is the
/// assertion).
fn float_modal(buffer: &mut Buffer, host: Rect, trigger_area: Rect) -> Rect {
    let surface = place(host, trigger_area, Size::new(24, 5), Placement::Centered);
    Overlay::new(Block::bordered().title("confirm")).render(surface, buffer);
    let inner = Block::bordered().inner(surface);
    buffer.set_string(inner.x, inner.y, "filter applied.", Style::new());
    buffer.set_string(
        inner.x,
        inner.y + 1,
        "press any key",
        Style::new().fg(Color::DarkGray),
    );
    surface
}

/// The tooltip surface: clamped at the pointer position.
fn float_tooltip(buffer: &mut Buffer, host: Rect, pointer: Position) -> Rect {
    let surface = place(
        host,
        Rect::new(pointer.x, pointer.y, 1, 1),
        Size::new(12, 1),
        Placement::At(pointer),
    );
    Overlay::new(Block::bordered().title("tip")).render(surface, buffer);
    let inner = Block::bordered().inner(surface);
    buffer.set_string(inner.x, inner.y, "floating", Style::new());
    surface
}

/// The M18 composite: the base paints, then the stack routes each open
/// surface bottom-to-top. Returns the resolved rects in stack order —
/// the sequence the app feeds the `HitMap`.
fn compose(
    buffer: &mut Buffer,
    host: Rect,
    trigger_area: Rect,
    stack: &SurfaceStack<Panel>,
    state: &SelectState,
) -> Vec<(Panel, Rect)> {
    paint_dashboard(buffer, &mut trigger_state());
    let mut rects = Vec::new();
    for key in stack.order() {
        let rect = match *key {
            Dropdown => float_dropdown(buffer, host, trigger_area, state),
            Modal => float_modal(buffer, host, trigger_area),
            Tooltip => float_tooltip(buffer, host, Position::new(30, 11)),
        };
        rects.push((*key, rect));
    }
    rects
}

/// The trigger state lives app-side; the dashboard paints a fresh one.
fn trigger_state() -> SelectState {
    SelectState::new().with_selected(None)
}

// ---------------------------------------------------------------------
// A. One stack drives the frame.
// ---------------------------------------------------------------------

#[test]
fn the_stack_paints_surfaces_bottom_to_top() {
    let host = Rect::new(0, 0, 40, 14);
    let mut stack = SurfaceStack::new();
    stack.push(Dropdown);
    stack.push(Modal);
    let mut state = SelectState::new();
    state.open();

    let mut buffer = Buffer::empty(host);
    let rects = compose(&mut buffer, host, Rect::new(2, 8, 14, 1), &stack, &state);

    assert_eq!(
        rects,
        vec![
            (Dropdown, Rect::new(2, 0, 16, 8)),
            (Modal, Rect::new(8, 4, 24, 5))
        ],
        "the rects come back in stack order"
    );

    let text = buffer.to_string();
    assert!(
        text.contains("eu-west"),
        "the dropdown options show: {text}"
    );
    assert!(
        text.contains("filter applied."),
        "the modal shows above the dropdown: {text}"
    );
    assert!(
        !text.contains("us-west"),
        "the modal erased the option it covers: {text}"
    );
    assert!(
        text.contains("base-12"),
        "the uncovered base survives: {text}"
    );
}

#[test]
fn raising_a_surface_flips_the_composite() {
    let host = Rect::new(0, 0, 40, 14);
    let trigger_area = Rect::new(2, 8, 14, 1);
    let mut stack = SurfaceStack::new();
    stack.push(Dropdown);
    stack.push(Modal);
    let mut state = SelectState::new();
    state.open();

    let mut buffer = Buffer::empty(host);
    compose(&mut buffer, host, trigger_area, &stack, &state);
    assert!(
        !buffer.to_string().contains("us-west"),
        "the modal is on top: it covers the option"
    );

    assert!(
        !stack.push(Dropdown),
        "raising returns false — already open"
    );
    let mut buffer = Buffer::empty(host);
    compose(&mut buffer, host, trigger_area, &stack, &state);
    assert!(
        buffer.to_string().contains("us-west"),
        "the raised dropdown paints last and wins the overlap"
    );
    assert_eq!(stack.top(), Some(&Dropdown));
}

#[test]
fn the_managed_composite_keeps_the_highlight_visible() {
    // The M17 honesty gate, replayed through the stack: the highlight
    // walks every option and the recomposited frame always shows it.
    let host = Rect::new(0, 0, 40, 14);
    let trigger_area = Rect::new(2, 8, 14, 1);
    let mut stack = SurfaceStack::new();
    stack.push(Dropdown);
    let mut state = SelectState::new();
    state.open();

    for step in 0..REGIONS.len() {
        if step > 0 {
            state.select_next(REGIONS.len());
        }
        let mut buffer = Buffer::empty(host);
        compose(&mut buffer, host, trigger_area, &stack, &state);
        let highlighted = REGIONS[state.highlight()];
        assert!(
            buffer.to_string().contains(highlighted),
            "step {step}: the highlighted option \"{highlighted}\" renders nowhere"
        );
    }
}

// ---------------------------------------------------------------------
// B. The dismissal routing: close_top is the Esc precedence.
// ---------------------------------------------------------------------

#[test]
fn close_top_dismisses_the_modal_before_the_dropdown() {
    let host = Rect::new(0, 0, 40, 14);
    let trigger_area = Rect::new(2, 8, 14, 1);
    let mut stack = SurfaceStack::new();
    stack.push(Dropdown);
    stack.push(Modal);
    let mut state = SelectState::new();
    state.open();

    // Esc: the modal is on top, so the modal goes...
    assert_eq!(stack.close_top(), Some(Modal));
    assert!(state.is_open(), "the dropdown underneath stays open");

    let mut buffer = Buffer::empty(host);
    compose(&mut buffer, host, trigger_area, &stack, &state);
    let text = buffer.to_string();
    assert!(
        !text.contains("filter applied."),
        "the modal is gone: {text}"
    );
    assert!(text.contains("eu-west"), "the dropdown survives: {text}");

    // ...Esc again: now the dropdown dismisses and the app closes its
    // state, which commits nothing (the closed select selects nothing).
    assert_eq!(stack.close_top(), Some(Dropdown));
    state.close();
    assert!(!state.is_open());
    assert_eq!(state.choose(), None, "a closed dropdown selects nothing");
    assert_eq!(stack.close_top(), None, "the stack is empty");
}

#[test]
fn the_stack_routes_a_click_outside_the_top_surface() {
    let host = Rect::new(0, 0, 40, 14);
    let mut stack = SurfaceStack::new();
    stack.push(Dropdown);
    stack.push(Modal);
    let mut state = SelectState::new();
    state.open();

    let mut buffer = Buffer::empty(host);
    let rects = compose(&mut buffer, host, Rect::new(2, 8, 14, 1), &stack, &state);

    // Registration in stack order: base first, then the surfaces
    // bottom-to-top — one loop, no parallel bookkeeping.
    let mut hits: HitMap<&'static str> = HitMap::new();
    hits.push("base", host);
    for (key, rect) in &rects {
        let id = match key {
            Dropdown => "dropdown",
            Modal => "modal",
            Tooltip => "tooltip",
        };
        hits.push(id, *rect);
    }

    // The modal interior lands on the modal...
    let (_, modal_rect) = rects.iter().find(|(key, _)| *key == Modal).unwrap();
    let hit = hits
        .hit_test(Position::new(modal_rect.x + 2, modal_rect.y + 2))
        .unwrap();
    assert_eq!(hit.id, "modal");

    // ...a click in the base's corner is a miss for every surface, and
    // the app dismisses only the top surface.
    let hit = hits.hit_test(Position::new(38, 13)).unwrap();
    assert_eq!(hit.id, "base");
    assert_eq!(stack.close_top(), Some(Modal), "the top surface dismisses");
    assert!(stack.is_open(&Dropdown), "the dropdown stays open");
}

// ---------------------------------------------------------------------
// B21 — the stacked composite budget (measured, not asserted in the
// normal suite; the perf workflow runs it with --ignored).
// ---------------------------------------------------------------------

/// One hundred options, the honest long list B20 measured.
const SCROLL_OPTIONS: usize = 100;

/// One frame of the B21 composite: the 200x60 dashboard base plus the
/// dropdown (100 options, window scrolled), a tooltip and a modal, all
/// routed through a fresh `SurfaceStack` — the pushes, the paint walk
/// and the erase-then-paint per surface in one frame.
fn stack_composite_frame(highlight_step: usize) -> Buffer {
    let area = Rect::new(0, 0, 200, 60);
    let trigger_area = Rect::new(1, 21, 198, 1);

    let mut stack = SurfaceStack::new();
    stack.push(Dropdown);
    stack.push(Tooltip);
    stack.push(Modal);

    let mut buffer = Buffer::empty(area);
    paint_dashboard(&mut buffer, &mut trigger_state());

    for key in stack.order() {
        match *key {
            Dropdown => {
                let rows = dropdown_rows(area, trigger_area, SCROLL_OPTIONS);
                let surface = place(area, trigger_area, Size::new(30, rows), Placement::Below);
                Overlay::new(Block::bordered().title("region")).render(surface, &mut buffer);
                let options: Vec<Line> = (0..SCROLL_OPTIONS)
                    .map(|row| Line::from(format!("eu-region-{row:03}")))
                    .collect();
                let mut state = SelectState::new();
                state.open();
                for _ in 0..highlight_step {
                    state.select_next(SCROLL_OPTIONS);
                }
                Select::new(options)
                    .highlight_style(Style::new().fg(Color::Cyan))
                    .render_options(Block::bordered().inner(surface), &mut buffer, &state);
            }
            Tooltip => {
                float_tooltip(&mut buffer, area, Position::new(160, 40));
            }
            Modal => {
                float_modal(&mut buffer, area, trigger_area);
            }
        }
    }
    buffer
}

#[test]
#[ignore = "timing gate: run with --ignored (perf workflow)"]
fn b21_stack_composite_within_budget() {
    // Warm-up (allocator + caches; not part of the gate).
    for step in 0..16 {
        black_box(stack_composite_frame(step));
    }

    let frames = 300_usize;
    let mut latencies = Vec::with_capacity(frames);
    for step in 0..frames {
        let start = Instant::now();
        let buffer = black_box(stack_composite_frame(step.min(SCROLL_OPTIONS - 1)));
        let elapsed = start.elapsed();
        // Sanity: the scrolled dropdown really painted under the stack.
        let highlighted = format!("eu-region-{:03}", step.min(SCROLL_OPTIONS - 1));
        assert!(
            buffer.to_string().contains(&highlighted),
            "the highlighted option {highlighted} renders nowhere"
        );
        latencies.push(elapsed);
    }
    latencies.sort();
    let median = latencies[frames / 2];
    let p99 = latencies[frames * 99 / 100 - 1];
    let max = latencies[frames - 1];

    assert!(
        p99 <= Duration::from_millis(1),
        "B21 gate failed: stacked composite p99 {p99:?} (budget 1 ms)"
    );
    println!(
        "B21 gate: stacked composite (dropdown + tooltip + modal via SurfaceStack) median {median:?}, p99 {p99:?}, max {max:?} over {frames} frames @ 200x60 (budget 1 ms)"
    );
}
