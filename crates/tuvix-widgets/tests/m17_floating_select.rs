//! M17 integration gates: the floating dropdown as a first-class
//! composite — the M15 `Select` wired through the M16 primitives.
//!
//! Three seams the tests pin (docs/ARCHITECTURE.md, the M17 decisions):
//!
//! - **the visibility window** — `SelectState::visible_window` is the
//!   pure, List-parity scroll discipline: the highlight is always a
//!   member of the window the renderer paints, so keyboard navigation
//!   can never point at an option that renders nowhere;
//! - **the surface half** — `Select::render_options` paints the visible
//!   option window into a floating rect the app resolved with `place`
//!   and erased with `Overlay`; the trigger half stays the closed
//!   render. No duplicate `List`, no re-rolled option painter;
//! - **the sizing decision** — `dropdown_rows` caps the surface at the
//!   roomiest side of the host, so `place`'s flip ladder always opens.
//!
//! B20 — the scrolling composite budget: a 100-option dropdown whose
//! window scrolls per frame composites over the 200x60 dashboard inside
//! **1 ms** p99 — the same half-of-B2 line B19 draws for the 16-option
//! static dropdown.

use std::hint::black_box;
use std::ops::Range;
use std::time::{Duration, Instant};

use tuvix_core::backend::TestBackend;
use tuvix_core::buffer::Buffer;
use tuvix_core::geometry::{Position, Rect, Size};
use tuvix_core::hit::HitMap;
use tuvix_core::layer::{Placement, dropdown_rows, place};
use tuvix_core::style::{Color, Style};
use tuvix_core::terminal::Terminal;
use tuvix_core::text::Line;
use tuvix_core::widget::{RenderWidget, Widget};
use tuvix_widgets::{Block, Overlay, Select, SelectState};

const REGIONS: [&str; 6] = [
    "eu-west", "eu-north", "us-east", "us-west", "ap-south", "sa-east",
];

fn window_of(state: &SelectState, count: usize, visible: u16) -> Range<usize> {
    state.visible_window(count, visible)
}

// ---------------------------------------------------------------------
// A. The visibility window.
// ---------------------------------------------------------------------

#[test]
fn the_window_hugs_the_top_while_the_highlight_fits() {
    let mut state = SelectState::new();
    for highlight in 0..3_usize {
        state.set_highlight(highlight);
        assert_eq!(window_of(&state, 6, 3), 0..3, "highlight {highlight}");
    }
}

#[test]
fn the_window_scrolls_minimally_at_the_bottom_edge() {
    let mut state = SelectState::new();
    state.set_highlight(3);
    assert_eq!(window_of(&state, 6, 3), 1..4);
    state.set_highlight(4);
    assert_eq!(window_of(&state, 6, 3), 2..5);
    state.set_highlight(5);
    assert_eq!(window_of(&state, 6, 3), 3..6, "clamped at the last option");
}

#[test]
fn the_window_is_a_pure_function_of_the_highlight() {
    // One state navigated down and back, one set directly: the same
    // highlight answers with the same window — the renderer never
    // writes the state, the window never hides a scroll history.
    let mut walked = SelectState::new();
    for _ in 0..4 {
        walked.select_next(6);
    }
    for _ in 0..2 {
        walked.select_previous();
    }
    let mut direct = SelectState::new();
    direct.set_highlight(2);
    assert_eq!(window_of(&walked, 6, 3), window_of(&direct, 6, 3));
    assert_eq!(walked.highlight(), direct.highlight());
}

#[test]
fn a_window_taller_than_the_options_clamps_to_the_count() {
    let mut state = SelectState::new();
    state.set_highlight(2);
    assert_eq!(window_of(&state, 3, 10), 0..3);
}

#[test]
fn a_zero_row_window_is_empty() {
    let mut state = SelectState::new();
    state.set_highlight(1);
    assert_eq!(window_of(&state, 6, 0), 0..0);
}

#[test]
fn zero_options_produce_an_empty_window() {
    let state = SelectState::new();
    assert_eq!(window_of(&state, 0, 5), 0..0);
}

#[test]
fn the_window_always_contains_the_highlight() {
    // The honesty invariant, swept: for every option count, every
    // window height and every in-range highlight, the highlight is a
    // member of the window the renderer would paint.
    for count in [1_usize, 2, 5, 9] {
        for visible in [1_u16, 2, 3, 7] {
            for highlight in 0..count {
                let mut state = SelectState::new();
                state.set_highlight(highlight);
                let window = window_of(&state, count, visible);
                assert!(
                    window.contains(&highlight),
                    "count {count}, visible {visible}, highlight {highlight}: window {window:?}"
                );
                assert!(window.end <= count, "window escapes the options");
            }
        }
    }
}

#[test]
fn an_out_of_range_highlight_leaves_the_window_inside_the_options() {
    // The M15 honesty: a verbatim out-of-range highlight paints no row.
    // The window it derives still stays inside the real options.
    let mut state = SelectState::new();
    state.set_highlight(99);
    let window = window_of(&state, 3, 2);
    assert_eq!(window, 1..3);
    assert!(!window.contains(&99));
}

// ---------------------------------------------------------------------
// B. The surface half: render_options.
// ---------------------------------------------------------------------

#[test]
fn render_options_paints_the_visible_window() {
    let mut buffer = Buffer::empty(Rect::new(0, 0, 12, 2));
    let mut state = SelectState::new();
    state.open();
    Select::new(REGIONS).render_options(Rect::new(0, 0, 12, 2), &mut buffer, &state);
    assert_eq!(buffer.to_string(), "eu-west     \neu-north    ");

    // The same surface after the highlight walks to the fifth option:
    // the window scrolled, the rows followed.
    for _ in 0..4 {
        state.select_next(REGIONS.len());
    }
    let mut buffer = Buffer::empty(Rect::new(0, 0, 12, 2));
    Select::new(REGIONS).render_options(Rect::new(0, 0, 12, 2), &mut buffer, &state);
    assert_eq!(buffer.to_string(), "us-west     \nap-south    ");
}

#[test]
fn render_options_highlights_within_the_window() {
    let mut buffer = Buffer::empty(Rect::new(0, 0, 12, 2));
    let mut state = SelectState::new();
    state.open();
    state.select_next(REGIONS.len());
    Select::new(REGIONS)
        .highlight_style(Style::new().fg(Color::Cyan))
        .render_options(Rect::new(0, 0, 12, 2), &mut buffer, &state);
    assert_eq!(buffer.get(0, 0).unwrap().style.fg, None);
    assert_eq!(buffer.get(0, 1).unwrap().style.fg, Some(Color::Cyan));
}

#[test]
fn render_options_clips_at_the_area() {
    let mut buffer = Buffer::empty(Rect::new(0, 0, 12, 1));
    let mut state = SelectState::new();
    state.open();
    Select::new(REGIONS).render_options(Rect::new(0, 0, 12, 1), &mut buffer, &state);
    assert_eq!(buffer.to_string(), "eu-west     ");
}

#[test]
fn render_options_with_zero_options_paints_nothing() {
    let mut buffer = Buffer::empty(Rect::new(0, 0, 12, 2));
    let mut state = SelectState::new();
    state.open();
    let select: Select = Select::new(Vec::<Line>::new());
    select.render_options(Rect::new(0, 0, 12, 2), &mut buffer, &state);
    assert!(
        buffer
            .content
            .iter()
            .all(tuvix_core::buffer::Cell::is_empty)
    );
}

// ---------------------------------------------------------------------
// C. The floating composite: size, place, erase, paint.
// ---------------------------------------------------------------------

/// The 24x14 dashboard base: fourteen text rows a dropdown must be
/// able to erase, plus a `Select` trigger on row 8. Returns the
/// trigger area — the anchor the dropdown opens from.
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

/// The M17 recipe, one call: size the surface (the block's two border
/// rows ride on top of the content rows), place it, erase it, paint
/// the visible option window into the clean rect.
fn float_select(buffer: &mut Buffer, host: Rect, trigger_area: Rect, state: &SelectState) -> Rect {
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

#[test]
fn the_floating_composite_flips_above_a_mid_screen_trigger() {
    // Host 14 rows, trigger at y=8: five rows free below, eight above,
    // six options plus the block chrome — the surface does not fit
    // below, so `place` flips it above, ending at the trigger.
    let host = Rect::new(0, 0, 24, 14);
    let mut buffer = Buffer::empty(host);
    let mut state = SelectState::new();
    state.open();
    let trigger_area = paint_dashboard(&mut buffer, &mut state);
    let surface = float_select(&mut buffer, host, trigger_area, &state);

    assert_eq!(surface, Rect::new(2, 0, 16, 8));
    let text = buffer.to_string();
    // Six rows inside the bordered surface: the whole list is visible.
    assert!(text.contains("eu-west"), "first option missing: {text}");
    assert!(text.contains("sa-east"), "last option missing: {text}");
    // The base rows under the surface are erased; the rest survive.
    assert!(!text.contains("base-3"), "covered base survived: {text}");
    assert!(!text.contains("base-7"), "covered base survived: {text}");
    assert!(text.contains("base-12"), "uncovered base lost: {text}");
    // The open trigger keeps its own row, below the surface.
    assert!(text.contains("region: all ▴"), "trigger lost: {text}");
}

#[test]
fn the_composite_opens_below_when_the_room_allows_it() {
    // Trigger at y=1 in a 24-row host: plenty of room below, six
    // options open downward at full content height (plus the border).
    let host = Rect::new(0, 0, 24, 24);
    let mut buffer = Buffer::empty(host);
    buffer.set_string(0, 20, "base-20", Style::new());
    let mut state = SelectState::new();
    state.open();
    let trigger_area = Rect::new(2, 1, 14, 1);
    Select::new(REGIONS)
        .label("region")
        .placeholder("all")
        .render(trigger_area, &mut buffer, &mut state);
    let surface = float_select(&mut buffer, host, trigger_area, &state);

    assert_eq!(surface, Rect::new(2, 2, 16, 8), "opens under the trigger");
    let text = buffer.to_string();
    assert!(text.contains("sa-east"), "all six options fit: {text}");
    assert!(text.contains("base-20"), "uncovered base lost: {text}");
}

#[test]
fn the_composite_flips_above_when_the_edge_is_near() {
    let host = Rect::new(0, 0, 24, 10);
    let mut buffer = Buffer::empty(host);
    let mut state = SelectState::new();
    state.open();
    let trigger_area = Rect::new(2, 8, 14, 1);
    Select::new(REGIONS)
        .label("region")
        .render(trigger_area, &mut buffer, &mut state);
    let surface = float_select(&mut buffer, host, trigger_area, &state);

    // One row free below, eight above: the surface opens above and
    // ends exactly at the trigger.
    assert_eq!(surface, Rect::new(2, 0, 16, 8));
    assert_eq!(surface.bottom(), trigger_area.y);
    let text = buffer.to_string();
    assert!(text.contains("eu-west"), "options missing: {text}");
    assert!(
        text.contains("region:  ▴"),
        "the trigger row survives its own surface: {text}"
    );
}

#[test]
fn navigation_never_highlights_an_invisible_option() {
    // The honesty gate: walk the highlight through every option and
    // recomposite per step — the highlighted option is always one of
    // the rows on screen.
    let host = Rect::new(0, 0, 24, 10);
    let trigger_area = Rect::new(2, 7, 14, 1);
    let mut state = SelectState::new();
    state.open();

    for step in 0..REGIONS.len() {
        if step > 0 {
            state.select_next(REGIONS.len());
        }
        let mut buffer = Buffer::empty(host);
        paint_dashboard(&mut buffer, &mut state);
        float_select(&mut buffer, host, trigger_area, &state);
        let text = buffer.to_string();
        let highlighted = REGIONS[state.highlight()];
        assert!(
            text.contains(highlighted),
            "step {step}: the highlighted option \"{highlighted}\" renders nowhere: {text}"
        );
    }
}

#[test]
fn a_click_outside_the_surface_dismisses_through_the_hit_map() {
    let host = Rect::new(0, 0, 24, 10);
    let mut buffer = Buffer::empty(host);
    let mut state = SelectState::new();
    state.open();
    let trigger_area = paint_dashboard(&mut buffer, &mut state);
    let surface = float_select(&mut buffer, host, trigger_area, &state);

    // The app registers regions in paint order: base first, the
    // floating surface last — the painter's algorithm.
    let mut hits: HitMap<&'static str> = HitMap::new();
    hits.push("base", host);
    hits.push("dropdown", surface);

    // A click on an option row lands on the dropdown...
    let hit = hits
        .hit_test(Position::new(surface.x + 2, surface.y + 1))
        .unwrap();
    assert_eq!(hit.id, "dropdown");

    // ...a click in the host's far corner lands on the base, and the
    // app dismisses: the closed dropdown commits nothing.
    let hit = hits.hit_test(Position::new(23, 0)).unwrap();
    assert_eq!(hit.id, "base");
    state.close();
    assert!(!state.is_open());
    assert_eq!(state.choose(), None, "a closed dropdown selects nothing");
}

#[test]
fn committing_from_the_floating_surface_selects_the_clicked_row() {
    let host = Rect::new(0, 0, 24, 10);
    let mut buffer = Buffer::empty(host);
    let mut state = SelectState::new();
    state.open();
    let trigger_area = paint_dashboard(&mut buffer, &mut state);
    let surface = float_select(&mut buffer, host, trigger_area, &state);

    // A click on the third option row: the row's offset inside the
    // window is the new highlight, and the commit returns it.
    let clicked_row = surface.y + 3;
    let inner_offset = clicked_row - (surface.y + 1);
    let window = window_of(&state, REGIONS.len(), surface.height.saturating_sub(2));
    state.set_highlight(window.start + usize::from(inner_offset));
    assert_eq!(state.choose(), Some(2));
    assert!(!state.is_open());
}

// ---------------------------------------------------------------------
// D. Through the render stack.
// ---------------------------------------------------------------------

#[test]
fn the_floating_select_renders_through_the_terminal() {
    let mut terminal = Terminal::new(TestBackend::new(Size::new(24, 14))).unwrap();
    let mut state = SelectState::new();
    state.open();

    terminal
        .draw(|frame| {
            let trigger_area = paint_dashboard(frame.buffer, &mut state);
            float_select(frame.buffer, frame.area(), trigger_area, &state);
        })
        .unwrap();

    let screen = terminal.backend().buffer().to_string();
    assert!(screen.contains("eu-west"), "options missing: {screen}");
    assert!(
        !screen.contains("base-3"),
        "covered base survived: {screen}"
    );
    assert!(screen.contains("base-12"), "uncovered base lost: {screen}");
    assert!(
        screen.contains("region: all ▴"),
        "trigger missing: {screen}"
    );
}

// ---------------------------------------------------------------------
// E. The inline upgrade: the window keeps the M15 pins while fixing
//    the invisible highlight.
// ---------------------------------------------------------------------

#[test]
fn the_inline_list_still_paints_from_the_top_while_it_fits() {
    // M15 pinned: three options in a four-row buffer paint from the
    // top no matter where the highlight walks — the window hugs the
    // top while the highlight fits inside it.
    let options = ["one", "two", "three"];
    let expected = format!(
        "{:<12}\n{:<12}\n{:<12}\n{:<12}",
        " ▴", "one", "two", "three"
    );
    for step in 0..3_usize {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 12, 4));
        let mut state = SelectState::new();
        state.open();
        for _ in 0..step {
            state.select_next(options.len());
        }
        Select::new(options).render(Rect::new(0, 0, 12, 4), &mut buffer, &mut state);
        assert_eq!(buffer.to_string(), expected, "step {step}");
    }
}

#[test]
fn the_inline_list_scrolls_to_keep_the_highlight_visible() {
    // The M15 gap, closed: four options in a three-row buffer — the
    // highlight can walk to the last option and the window follows it
    // instead of leaving it clipped into nowhere.
    let options = ["one", "two", "three", "four"];
    let mut buffer = Buffer::empty(Rect::new(0, 0, 12, 3));
    let mut state = SelectState::new();
    state.open();
    for _ in 0..3 {
        state.select_next(options.len());
    }
    assert_eq!(state.highlight(), 3);
    Select::new(options).render(Rect::new(0, 0, 12, 3), &mut buffer, &mut state);
    let expected = format!("{:<12}\n{:<12}\n{:<12}", " ▴", "three", "four");
    assert_eq!(buffer.to_string(), expected);
}

// ---------------------------------------------------------------------
// B20 — the scrolling composite budget (measured, not asserted in the
// normal suite; the perf workflow runs it with --ignored).
// ---------------------------------------------------------------------

/// One hundred regions, the way a long filter list opens.
const SCROLL_OPTIONS: usize = 100;

/// One frame of the B20 composite: the 200x60 dashboard base plus an
/// open 100-option dropdown whose window has scrolled to the highlight.
fn scroll_composite_frame(highlight_step: usize) -> Buffer {
    let area = Rect::new(0, 0, 200, 60);
    let mut buffer = Buffer::empty(area);

    // The base: a bordered panel, a filled body, a one-row trigger.
    let block = Block::bordered().title("dashboard");
    block.clone().render(area, &mut buffer);
    buffer.set_string(1, 1, "base row — the quick brown fox", Style::new());
    for row in 2..21_u16 {
        let label = format!("base-{row}");
        buffer.set_string(1, row, label, Style::new());
    }
    let trigger_area = Rect::new(1, 21, 198, 1);
    let mut trigger = SelectState::new().with_selected(Some(1));
    Select::new(REGIONS)
        .label("region")
        .placeholder("all")
        .render(trigger_area, &mut buffer, &mut trigger);

    // The dropdown: sized from the host's room, placed, erased, painted
    // with the visible window of the 100-option list.
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
    buffer
}

#[test]
#[ignore = "timing gate: run with --ignored (perf workflow)"]
fn b20_floating_select_scroll_composite_within_budget() {
    // Warm-up (allocator + caches; not part of the gate).
    for step in 0..16 {
        black_box(scroll_composite_frame(step));
    }

    let frames = 300_usize;
    let mut latencies = Vec::with_capacity(frames);
    for step in 0..frames {
        let start = Instant::now();
        let buffer = black_box(scroll_composite_frame(step));
        let elapsed = start.elapsed();
        // Sanity: the window really scrolled with the highlight (the
        // navigation clamps at the last option, so the walk saturates
        // there).
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
        "B20 gate failed: scrolling floating-select composite p99 {p99:?} (budget 1 ms)"
    );
    println!(
        "B20 gate: scrolling floating-select composite median {median:?}, p99 {p99:?}, max {max:?} over {frames} frames @ 200x60, {SCROLL_OPTIONS} options (budget 1 ms)"
    );
}
