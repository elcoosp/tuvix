//! M19 integration gates: the pointer half of the floating dropdown —
//! the M14 pointer layer converged with the M15-M18 surfaces.
//!
//! M18 made `order()` the one sequence for the paint and the hit-map
//! registration, but the second half stayed a promise: the `layers`
//! example ships no hit map, and nothing maps a click on the surface
//! back to an option — every app re-derives the windowed row math that
//! `visible_window` already owns, and the pointer and the keyboard
//! would eventually disagree about the same dropdown. [`SelectState::
//! option_at`] is that inverse as a pure query next to
//! [`SelectState::visible_window`].
//!
//! The contract pinned here:
//!
//! - **the inverse of the paint** — the window is derived exactly as
//!   `render_options` derives it, so a click on an option row answers
//!   that option's index wherever the window scrolled (absolute
//!   indexes, never window-relative);
//! - **chrome is not an option** — positions outside the painted area,
//!   the borders and the blank rows below the painted window answer
//!   `None`; a total query, `None` at every corner;
//! - **the highlighted row, clicked, answers the highlight** — the
//!   pointer twin of the M17 invariant (navigation never points at an
//!   option that renders nowhere): walking the highlight through every
//!   option and clicking its row commits that option, every time;
//! - **B22 — the click decision budget**: hit-test through the
//!   registered surface map plus the inverse query on the 100-option
//!   windowed surface stays under 1 us, the B17 event ceiling.

use std::hint::black_box;
use std::time::{Duration, Instant};

use tuvix_core::buffer::Buffer;
use tuvix_core::geometry::{Position, Rect, Size};
use tuvix_core::hit::HitMap;
use tuvix_core::layer::{Placement, SurfaceStack, dropdown_rows, place};
use tuvix_core::style::Style;
use tuvix_core::text::Line;
use tuvix_core::widget::Widget;
use tuvix_widgets::{Block, Overlay, Select, SelectState};

// ---------------------------------------------------------------------
// A. The inverse query: the paint, read backwards.
// ---------------------------------------------------------------------

#[test]
fn the_top_of_the_window_answers_the_first_option() {
    let state = SelectState::new();
    let area = Rect::new(2, 5, 12, 3);
    assert_eq!(state.option_at(6, area, Position::new(2, 5)), Some(0));
    assert_eq!(state.option_at(6, area, Position::new(7, 6)), Some(1));
    assert_eq!(state.option_at(6, area, Position::new(13, 7)), Some(2));
}

#[test]
fn every_row_of_the_area_answers_its_option_or_none() {
    // The row is the click target, not the glyphs: a click anywhere on
    // an option's row — over the label or over the blank tail — answers
    // that option, and the rows below the painted window answer none.
    let mut state = SelectState::new();
    state.set_highlight(1);
    let area = Rect::new(0, 0, 10, 4);
    let window = state.visible_window(3, area.height);
    assert_eq!(window, 0..3);
    for y in area.y..area.bottom() {
        let expected = if usize::from(y) < window.len() {
            Some(usize::from(y))
        } else {
            None
        };
        for x in area.x..area.right() {
            assert_eq!(
                state.option_at(3, area, Position::new(x, y)),
                expected,
                "row {y}, column {x}"
            );
        }
    }
}

#[test]
fn a_click_outside_the_surface_answers_none() {
    // A non-origin area, so every side has an outside neighbor cell.
    let state = SelectState::new();
    let area = Rect::new(5, 7, 10, 3);
    let outside = [
        Position::new(4, 7),
        Position::new(15, 7),
        Position::new(5, 6),
        Position::new(5, 10),
        Position::new(4, 6),
        Position::new(15, 10),
    ];
    for position in outside {
        assert_eq!(
            state.option_at(6, area, position),
            None,
            "{position} is not on the surface"
        );
    }
}

#[test]
fn an_empty_area_answers_none() {
    let state = SelectState::new();
    assert_eq!(
        state.option_at(6, Rect::new(0, 0, 0, 3), Position::ZERO),
        None
    );
    assert_eq!(
        state.option_at(6, Rect::new(0, 0, 10, 0), Position::ZERO),
        None
    );
}

#[test]
fn zero_options_answer_none() {
    let state = SelectState::new();
    let area = Rect::new(0, 0, 10, 3);
    assert_eq!(state.option_at(0, area, Position::new(0, 0)), None);
}

#[test]
fn rows_below_the_last_option_answer_none() {
    // A surface taller than its content: the sizing can hand `place` a
    // roomy rect and the list paints three rows — the blank tail is
    // chrome, not an invisible option.
    let state = SelectState::new();
    let area = Rect::new(0, 0, 10, 5);
    assert_eq!(state.option_at(3, area, Position::new(0, 2)), Some(2));
    assert_eq!(state.option_at(3, area, Position::new(0, 3)), None);
    assert_eq!(state.option_at(3, area, Position::new(0, 4)), None);
}

#[test]
fn the_scrolled_window_answers_absolute_indexes() {
    // The inverse discipline, pinned at the scroll: the click answers
    // the option the renderer painted there, never a window-relative
    // row — the pointer and the keyboard agree through one window.
    let mut state = SelectState::new();
    state.set_highlight(50);
    let area = Rect::new(0, 0, 20, 10);
    assert_eq!(state.visible_window(100, area.height), 41..51);
    assert_eq!(state.option_at(100, area, Position::new(0, 0)), Some(41));
    assert_eq!(state.option_at(100, area, Position::new(9, 9)), Some(50));
}

#[test]
fn an_out_of_range_highlight_answers_inside_the_options() {
    // The M15 honesty, pointer edition: a verbatim out-of-range
    // highlight paints no row, and the query still answers real
    // options — never the out-of-range index.
    let mut state = SelectState::new();
    state.set_highlight(99);
    let area = Rect::new(0, 0, 10, 3);
    assert_eq!(state.visible_window(6, area.height), 3..6);
    assert_eq!(state.option_at(6, area, Position::new(0, 0)), Some(3));
    assert_eq!(state.option_at(6, area, Position::new(0, 2)), Some(5));
}

#[test]
fn a_click_on_the_chrome_answers_none() {
    // The app hands the query the inner rect it painted into; the
    // border row of the surface is outside it and answers none.
    let host = Rect::new(0, 0, 40, 12);
    let trigger = Rect::new(2, 7, 14, 1);
    let rows = dropdown_rows(host, trigger, 6);
    let surface = place(host, trigger, Size::new(16, rows + 2), Placement::Below);
    let inner = Block::bordered().inner(surface);
    let state = SelectState::new();

    assert_eq!(
        state.option_at(6, inner, Position::new(surface.x, surface.y)),
        None
    );
    assert_eq!(
        state.option_at(6, inner, Position::new(surface.right() - 1, surface.y)),
        None,
        "the right border is chrome"
    );
    assert_eq!(
        state.option_at(6, inner, Position::new(inner.x, inner.y)),
        Some(0),
        "the first inner cell is the first option"
    );
}

#[test]
fn the_query_is_a_pure_function_of_the_highlight() {
    // Walked away and back, or set directly: the same highlight answers
    // the same click map — the query reads, it never writes.
    let mut walked = SelectState::new();
    for _ in 0..7 {
        walked.select_next(8);
    }
    for _ in 0..3 {
        walked.select_previous();
    }
    let mut direct = SelectState::new();
    direct.set_highlight(4);

    let area = Rect::new(3, 3, 12, 4);
    for y in 0..area.height {
        let position = Position::new(area.x + 1, area.y + y);
        assert_eq!(
            walked.option_at(8, area, position),
            direct.option_at(8, area, position),
            "row {y}"
        );
    }
}

// ---------------------------------------------------------------------
// B. The pointer commit: the clicked option becomes the selection.
// ---------------------------------------------------------------------

#[test]
fn the_clicked_option_commits_and_closes() {
    let mut state = SelectState::new();
    state.open();
    let area = Rect::new(0, 2, 12, 4);
    state.set_highlight(1);

    let clicked = state.option_at(6, area, Position::new(4, 4));
    assert_eq!(clicked, Some(2));
    if let Some(index) = clicked {
        state.set_highlight(index);
    }
    assert_eq!(state.choose(), Some(2), "the click commits");
    assert!(!state.is_open(), "a committed dropdown is closed");
    assert_eq!(state.selected(), Some(2));
}

#[test]
fn a_closed_dropdown_still_answers_the_query() {
    // `option_at` is a query, not a commit path: like
    // `visible_window`, it answers by geometry whether the dropdown is
    // open or not — the app asks only while the surface is open, the
    // same way it paints the surface only while it is open.
    let mut closed = SelectState::new();
    closed.set_highlight(2);
    let mut open = SelectState::new();
    open.set_highlight(2);
    open.open();

    let area = Rect::new(0, 0, 10, 3);
    assert_eq!(closed.option_at(6, area, Position::new(0, 1)), Some(1));
    assert_eq!(open.option_at(6, area, Position::new(0, 1)), Some(1));
}

#[test]
fn clicking_the_highlighted_row_commits_the_highlight_every_time() {
    // The pointer twin of the M17 gate: walk the highlight through all
    // 100 options; the clicked row of the highlight answers the
    // highlight on every step — the pointer can never point at an
    // option the window does not paint.
    let count = 100_usize;
    let area = Rect::new(0, 0, 20, 10);
    for highlight in 0..count {
        let mut state = SelectState::new();
        state.set_highlight(highlight);
        let window = state.visible_window(count, area.height);
        let row = u16::try_from(highlight - window.start).unwrap_or(u16::MAX);
        let clicked = state.option_at(count, area, Position::new(area.x, area.y + row));
        assert_eq!(clicked, Some(highlight), "highlight {highlight}");
    }
}

// ---------------------------------------------------------------------
// C. The stack wiring: one `order()` sequence registers the hit map.
// ---------------------------------------------------------------------

/// The app's own surface and region names — the stack's keys and the
/// hit map's ids, exactly the `layers` example's vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Region {
    Base,
    Trigger,
    Dropdown,
    Modal,
}

/// One frame of the managed composite: the 200x60 dashboard base, the
/// select trigger, and the dropdown + modal surfaces routed through a
/// fresh `SurfaceStack`; returns the buffer and the hit map registered
/// in paint order — the same `order()` walk.
fn managed_frame(highlight: usize) -> (Buffer, HitMap<Region>) {
    let area = Rect::new(0, 0, 200, 60);
    let mut buffer = Buffer::empty(area);
    let mut hits: HitMap<Region> = HitMap::new();

    let block = Block::bordered().title("dashboard");
    block.clone().render(area, &mut buffer);
    let trigger_area = Rect::new(1, 21, 198, 1);
    let options: Vec<Line> = (0..100)
        .map(|row| Line::from(format!("eu-region-{row:03}")))
        .collect();

    // The base and the trigger register first, in paint order.
    hits.push(Region::Base, area);
    hits.push(Region::Trigger, trigger_area);

    let mut surfaces: SurfaceStack<Region> = SurfaceStack::new();
    surfaces.push(Region::Dropdown);
    surfaces.push(Region::Modal);
    for surface in surfaces.order() {
        match *surface {
            Region::Dropdown => {
                let rows = dropdown_rows(area, trigger_area, 100);
                let placed = place(
                    area,
                    trigger_area,
                    Size::new(30, rows.saturating_add(2)),
                    Placement::Below,
                );
                Overlay::new(Block::bordered().title("region")).render(placed, &mut buffer);
                let inner = Block::bordered().inner(placed);
                let mut state = SelectState::new();
                state.open();
                state.set_highlight(highlight);
                Select::new(options.clone())
                    .highlight_style(Style::new())
                    .render_options(inner, &mut buffer, &state);
                hits.push(Region::Dropdown, inner);
            }
            Region::Modal => {
                let modal = place(area, trigger_area, Size::new(34, 5), Placement::Centered);
                Overlay::new(Block::bordered().title("confirm")).render(modal, &mut buffer);
                hits.push(Region::Modal, Block::bordered().inner(modal));
            }
            Region::Base | Region::Trigger => {}
        }
    }
    (buffer, hits)
}

#[test]
fn the_hit_map_answers_the_painter_s_order() {
    let (_buffer, hits) = managed_frame(10);

    // The modal paints last: a click under both surfaces lands on the
    // modal, and a click under the dropdown alone lands on the dropdown.
    let modal_area = Rect::new(83, 28, 34, 5);
    assert_eq!(
        hits.hit_test(Position::new(90, 29)).map(|hit| hit.id),
        Some(Region::Modal)
    );
    assert_eq!(
        hits.hit_test(Position::new(5, 22)).map(|hit| hit.id),
        Some(Region::Dropdown),
        "the dropdown covers the trigger row"
    );
    assert_eq!(
        hits.hit_test(Position::new(199, 59)).map(|hit| hit.id),
        Some(Region::Base),
        "the base answers where no surface painted"
    );
    assert!(
        modal_area.contains(Position::new(90, 29)),
        "the probe really sits on the modal"
    );
}

#[test]
fn a_click_through_the_stack_commits_the_option() {
    // The full pointer round trip the example wires: hit-test, region
    // match, inverse query, commit. The probe is the surface's top
    // inner row — the row the window's first option paints into, over
    // the trigger the dropdown covers (the painter's order).
    let highlight = 10_usize;
    let (_buffer, hits) = managed_frame(highlight);

    let hit = hits
        .hit_test(Position::new(5, 21))
        .expect("the dropdown answers");
    assert_eq!(hit.id, Region::Dropdown);
    let mut state = SelectState::new();
    state.open();
    state.set_highlight(highlight);
    let window = state.visible_window(100, hit.area.height);
    assert_eq!(window, 0..38, "highlight 10 fits the 38-row surface");

    let clicked = state.option_at(100, hit.area, Position::new(5, 21));
    assert_eq!(
        clicked,
        Some(window.start),
        "the top option row answers the window start"
    );
    if let Some(index) = clicked {
        state.set_highlight(index);
    }
    assert_eq!(state.choose(), Some(window.start));
}

// ---------------------------------------------------------------------
// B22 — the click decision budget (measured, not asserted in the
// normal suite; the perf workflow runs it with --ignored).
// ---------------------------------------------------------------------

/// One pointer decision on the managed composite: hit-test through the
/// registered surface map, the region match, and the inverse query on
/// the 100-option windowed surface.
fn click_decision(hits: &HitMap<Region>, area: Rect, state: &SelectState) -> Option<usize> {
    let position = Position::new(5, 22);
    let hit = hits.hit_test(position)?;
    if hit.id != Region::Dropdown {
        return None;
    }
    state.option_at(100, area, position)
}

#[test]
#[ignore = "timing gate: run with --ignored (perf workflow)"]
fn b22_floating_dropdown_click_decision_within_budget() {
    let (_buffer, hits) = managed_frame(50);
    let mut state = SelectState::new();
    state.open();
    state.set_highlight(50);
    let area = Block::bordered().inner(place(
        Rect::new(0, 0, 200, 60),
        Rect::new(1, 21, 198, 1),
        Size::new(
            30,
            dropdown_rows(Rect::new(0, 0, 200, 60), Rect::new(1, 21, 198, 1), 100) + 2,
        ),
        Placement::Below,
    ));

    // The decision is honest: the probed cell belongs to the dropdown.
    assert_eq!(
        hits.hit_test(Position::new(5, 22)).map(|hit| hit.id),
        Some(Region::Dropdown)
    );

    // Warm-up (allocator + caches; not part of the gate).
    for _ in 0..2_000 {
        black_box(click_decision(&hits, area, &state));
    }

    let iterations = 20_000_usize;
    let start = Instant::now();
    for _ in 0..iterations {
        black_box(click_decision(&hits, area, &state));
    }
    let per_decision = start.elapsed() / u32::try_from(iterations).unwrap_or(u32::MAX);

    assert!(
        per_decision <= Duration::from_micros(1),
        "B22 gate failed: {per_decision:?} per click decision (budget 1 us, the B17 event ceiling)"
    );
    println!(
        "B22 gate: {per_decision:?} per floating-dropdown click decision (hit-test + region match + option_at over the 100-option windowed surface)"
    );
}
