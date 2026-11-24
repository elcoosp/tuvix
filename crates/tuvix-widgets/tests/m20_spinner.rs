//! M20 — the animated indicator (`Spinner` + `SpinnerState`).
//!
//! Contract tests for the M13×M3 convergence: the frame index is a pure
//! function of elapsed time (the M13 "the clock owns the value" discipline
//! applied to a widget), the paused clock freezes in place, and the paint
//! follows the M15 one-row family disciplines (theme slots, block wrap,
//! clipping, first-row only).
//!
//! The first test pins the M20 roadmap acceptance criterion
//! (docs/ARCHITECTURE.md §11, row M20): "the frame index is a pure
//! function of elapsed time — a 60 fps loop and a 5 fps one land on the
//! same glyph".

use std::time::Duration;

use tuvix_core::buffer::Buffer;
use tuvix_core::geometry::Rect;
use tuvix_core::style::{Color, Modifier, Style};
use tuvix_core::stylize::Stylize as _;
use tuvix_core::widget::RenderWidget;
use tuvix_widgets::{Block, Spinner, SpinnerState};

fn buf(width: u16, height: u16) -> Buffer {
    Buffer::empty(Rect::new(0, 0, width, height))
}

/// The acceptance criterion: a 60 fps loop, a 5 fps loop and a single
/// one-second hop advance the same spinner to the same frame — the value
/// depends on elapsed time, never on the ticks that produced it.
#[test]
fn frame_index_is_a_pure_function_of_elapsed_time() {
    let spinner = Spinner::dots();

    // One second in 60 fps steps (16.666... ms per frame).
    let mut sixty_fps = SpinnerState::new();
    for _ in 0..60 {
        sixty_fps.advance(Duration::from_nanos(16_666_666));
    }
    // One second in 5 fps steps.
    let mut five_fps = SpinnerState::new();
    for _ in 0..5 {
        five_fps.advance(Duration::from_millis(200));
    }
    // One second in a single hop.
    let mut one_hop = SpinnerState::new();
    one_hop.advance(Duration::from_secs(1));

    assert_eq!(
        spinner.frame(&one_hop),
        2,
        "1 s / 80 ms = 12 whole periods, mod 10 glyphs = glyph index 2"
    );
    let frames = [
        spinner.frame(&sixty_fps),
        spinner.frame(&five_fps),
        spinner.frame(&one_hop),
    ];
    assert_eq!(
        frames[0], frames[1],
        "60 fps vs 5 fps: same elapsed, same frame"
    );
    assert_eq!(
        frames[1], frames[2],
        "5 fps vs one hop: same elapsed, same frame"
    );
}

/// A fresh spinner runs from the rest glyph (frame 0) without any advance.
#[test]
fn fresh_state_renders_the_rest_glyph() {
    let spinner = Spinner::dots();
    let mut state = SpinnerState::new();
    let mut buffer = buf(3, 1);
    spinner.render(Rect::new(0, 0, 3, 1), &mut buffer, &mut state);
    assert_eq!(buffer.to_string(), "⠋  ");
    assert!(state.is_running(), "a fresh spinner is running");
}

/// The paused clock freezes the frame: advance is a no-op while stopped.
#[test]
fn stop_freezes_the_frame() {
    let spinner = Spinner::dots();
    let mut state = SpinnerState::new();
    state.advance(Duration::from_millis(120));
    let frozen = spinner.frame(&state);
    state.stop();
    assert!(!state.is_running());
    state.advance(Duration::from_secs(10));
    assert_eq!(
        spinner.frame(&state),
        frozen,
        "a stopped spinner must not move"
    );
}

/// `reset` zeroes the clock; a running spinner restarts from the rest glyph.
#[test]
fn reset_returns_to_the_rest_glyph() {
    let spinner = Spinner::dots();
    let mut state = SpinnerState::new();
    state.advance(Duration::from_secs(5));
    state.reset();
    assert_eq!(spinner.frame(&state), 0);
    assert!(state.is_running(), "reset keeps the run state");
}

/// The frame wraps at the glyph-set length.
#[test]
fn frames_wrap_at_the_set_length() {
    let spinner = Spinner::dots();
    let mut state = SpinnerState::new();
    state.advance(Duration::from_millis(800)); // exactly 10 periods
    assert_eq!(spinner.frame(&state), 0, "10 periods over 10 glyphs wraps");
}

/// A custom period changes which frame one second lands on.
#[test]
fn custom_period_rescales_the_frames() {
    let spinner = Spinner::dots().period(Duration::from_millis(500));
    let mut state = SpinnerState::new();
    state.advance(Duration::from_secs(1));
    assert_eq!(spinner.frame(&state), 2);
}

/// A custom glyph set renders verbatim and indexes in order.
#[test]
fn custom_glyph_set() {
    let spinner = Spinner::custom(["n", "e", "c", "k"]);
    let mut state = SpinnerState::new();
    let mut buffer = buf(2, 1);
    spinner
        .clone()
        .render(Rect::new(0, 0, 2, 1), &mut buffer, &mut state);
    assert_eq!(buffer.to_string(), "n ");
    state.advance(Duration::from_millis(400)); // five default periods
    let mut buffer = buf(2, 1);
    spinner.render(Rect::new(0, 0, 2, 1), &mut buffer, &mut state);
    assert_eq!(buffer.to_string(), "e ");
}

/// An empty custom set is honest: no glyph, no panic, label only.
#[test]
fn empty_glyph_set_renders_the_label_only() {
    let spinner = Spinner::custom(Vec::<&str>::new()).label("idle");
    let mut state = SpinnerState::new();
    let mut buffer = buf(6, 1);
    spinner.render(Rect::new(0, 0, 6, 1), &mut buffer, &mut state);
    assert_eq!(buffer.to_string(), "idle  ");
}

/// A zero period pins frame 0 (the `Progress::of_elapsed` zero-duration rule).
#[test]
fn zero_period_pins_frame_zero() {
    let spinner = Spinner::dots().period(Duration::ZERO);
    let mut state = SpinnerState::new();
    state.advance(Duration::from_secs(9));
    assert_eq!(spinner.frame(&state), 0);
}

/// Saturating elapsed: advancing by huge durations never panics.
#[test]
fn saturating_elapsed_never_panics() {
    let spinner = Spinner::dots();
    let mut state = SpinnerState::new();
    state.advance(Duration::MAX);
    state.advance(Duration::MAX);
    let _ = spinner.frame(&state);
}

/// The label renders after the glyph and clips at the right edge.
#[test]
fn label_follows_the_glyph_and_clips() {
    let spinner = Spinner::dots().label("loading");
    let mut state = SpinnerState::new();
    let mut buffer = buf(6, 1);
    spinner.render(Rect::new(0, 0, 6, 1), &mut buffer, &mut state);
    assert_eq!(buffer.to_string(), "⠋ load");
}

/// A stopped spinner takes the stopped style; a running one the base style.
#[test]
fn stopped_style_patches_only_when_stopped() {
    let spinner = Spinner::dots()
        .style(Style::new().fg(Color::Cyan))
        .stopped_style(Style::new().fg(Color::DarkGray).dim());
    let mut state = SpinnerState::new();
    let mut buffer = buf(2, 1);
    spinner
        .clone()
        .render(Rect::new(0, 0, 2, 1), &mut buffer, &mut state);
    assert_eq!(buffer.get(0, 0).unwrap().style.fg, Some(Color::Cyan));

    state.stop();
    let mut buffer = buf(2, 1);
    spinner.render(Rect::new(0, 0, 2, 1), &mut buffer, &mut state);
    let cell = buffer.get(0, 0).unwrap();
    assert_eq!(cell.style.fg, Some(Color::DarkGray));
    assert_eq!(cell.style.add_modifier, Modifier::DIM);
}

/// `.theme()` maps the existing slots: running = `accent`, stopped = `muted`.
#[test]
fn theme_maps_accent_and_muted_slots() {
    let theme = tuvix_core::theme::Theme::dark();
    let spinner = Spinner::dots().theme(&theme);
    let mut state = SpinnerState::new();
    let mut buffer = buf(2, 1);
    spinner
        .clone()
        .render(Rect::new(0, 0, 2, 1), &mut buffer, &mut state);
    assert_eq!(buffer.get(0, 0).unwrap().style.fg, theme.accent.fg);

    state.stop();
    let mut buffer = buf(2, 1);
    spinner.render(Rect::new(0, 0, 2, 1), &mut buffer, &mut state);
    assert_eq!(buffer.get(0, 0).unwrap().style.fg, theme.muted.fg);
}

/// The block wraps the widget; the row fills the block's inner area.
#[test]
fn block_wraps_and_fills_inner() {
    let spinner = Spinner::dots().label("go").block(Block::bordered());
    let mut state = SpinnerState::new();
    let mut buffer = buf(10, 3);
    spinner.render(Rect::new(0, 0, 10, 3), &mut buffer, &mut state);
    assert_eq!(buffer.to_string(), "┌────────┐\n│⠋ go    │\n└────────┘");
}

/// A tall area uses only its first row (the M6 one-row discipline).
#[test]
fn tall_area_uses_only_the_first_row() {
    let spinner = Spinner::dots();
    let mut state = SpinnerState::new();
    let mut buffer = buf(8, 3);
    spinner.render(Rect::new(0, 0, 8, 3), &mut buffer, &mut state);
    assert_eq!(buffer.to_string(), "⠋       \n        \n        ");
}

/// Empty areas are no-ops.
#[test]
fn empty_area_is_a_noop() {
    let spinner = Spinner::dots();
    let mut state = SpinnerState::new();
    let mut buffer = buf(4, 1);
    spinner.render(Rect::ZERO, &mut buffer, &mut state);
    assert!(
        buffer
            .content
            .iter()
            .all(tuvix_core::buffer::Cell::is_empty)
    );
}

/// The named sets carry distinct glyphs at distinct speeds of traversal.
#[test]
fn named_sets_render_their_first_glyph() {
    let mut state = SpinnerState::new();
    for (spinner, first) in [
        (Spinner::dots(), "⠋"),
        (Spinner::line(), "|"),
        (Spinner::arc(), "◜"),
        (Spinner::toggle(), "◐"),
        (Spinner::bounce(), "⠁"),
    ] {
        let mut buffer = buf(2, 1);
        spinner.render(Rect::new(0, 0, 2, 1), &mut buffer, &mut state);
        let rendered = buffer.to_string();
        assert!(
            rendered.starts_with(first),
            "{rendered} should start with {first}"
        );
    }
}

/// The state lives outside the widget: two widgets over one state agree.
#[test]
fn state_is_shared_not_owned_by_the_widget() {
    let dots = Spinner::dots();
    let same = dots.clone().label("shared");
    let mut state = SpinnerState::new();
    state.advance(Duration::from_millis(240)); // three default periods
    assert_eq!(
        dots.frame(&state),
        same.frame(&state),
        "state stays app-owned"
    );
}
