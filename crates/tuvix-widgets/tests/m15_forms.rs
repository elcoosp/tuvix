//! M15 integration gates: the form family through the whole render
//! stack (Terminal + `TestBackend`), the choice flows end to end, the
//! theme mapping across the four widgets, wide-glyph honesty — and the
//! B18 form-panel paint budget, measured, not asserted.
//!
//! B18 — a 24-row settings form (checkboxes, a radio group, switches,
//! a select trigger and its open option list) repaints into a fresh
//! 200x60 buffer inside **1 ms** p99 — the same half-of-B2 line B14
//! draws for a scroll storm. A settings screen re-renders on every
//! keystroke and every toggle, so the whole family must stay a
//! rounding error against the 2 ms B2 steady-frame budget.

use std::hint::black_box;
use std::time::{Duration, Instant};

use tuvix_core::backend::TestBackend;
use tuvix_core::buffer::Buffer;
use tuvix_core::geometry::{Rect, Size};
use tuvix_core::layout::{Constraint, Layout};
use tuvix_core::style::Style;
use tuvix_core::stylize::Stylize as _;
use tuvix_core::terminal::Terminal;
use tuvix_core::theme::Theme;
use tuvix_core::widget::{RenderWidget, Widget};
use tuvix_widgets::{Block, Checkbox, RadioGroup, Select, SelectState, Switch};

const WIDTH: u16 = 200;
const HEIGHT: u16 = 60;
const FRAMES: usize = 300;

/// The B18 form panel: a bordered settings screen over the four
/// controls, laid out exactly the way an app would.
fn paint_form_panel(buffer: &mut Buffer, select_state: &mut SelectState) {
    let area = Rect::new(0, 0, WIDTH, 24);
    let block = Block::bordered().title("settings");
    block.clone().render(area, buffer);
    let inner = block.inner(area);

    let [
        auto_save,
        telemetry,
        protocol,
        mode,
        dark_mode,
        region,
        _rest,
    ] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(5),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(&inner);

    Widget::render(
        Checkbox::new("auto-save every edit").checked(true),
        auto_save,
        buffer,
    );
    Widget::render(
        Checkbox::new("share anonymous telemetry"),
        telemetry,
        buffer,
    );
    Widget::render(
        RadioGroup::new(["rsa", "ed25519", "hybrid"])
            .select(Some(1))
            .block(Block::bordered().title("protocol")),
        protocol,
        buffer,
    );
    Widget::render(Switch::new("dark mode").on(true), mode, buffer);
    Widget::render(
        Switch::new("spell check")
            .on(false)
            .style(Style::new().dim()),
        dark_mode,
        buffer,
    );
    let select = Select::new(["eu-west", "eu-north", "us-east", "ap-south"])
        .label("region")
        .placeholder("pick one");
    select.render(region, buffer, select_state);
}

#[test]
fn form_panel_renders_every_control() {
    let mut terminal = Terminal::new(TestBackend::new(Size::new(WIDTH, HEIGHT))).unwrap();
    let mut select_state = SelectState::new();
    terminal
        .draw(|frame| {
            paint_form_panel(frame.buffer, &mut select_state);
        })
        .unwrap();

    let rendered = terminal.backend().buffer().to_string();
    assert!(rendered.contains("settings"), "panel title missing");
    assert!(
        rendered.contains("☑ auto-save every edit"),
        "checked box missing"
    );
    assert!(
        rendered.contains("☐ share anonymous telemetry"),
        "unchecked box missing"
    );
    assert!(rendered.contains("protocol"), "radio block title missing");
    assert!(rendered.contains("◉ ed25519"), "selected radio missing");
    assert!(rendered.contains("○ hybrid"), "unselected radio missing");
    assert!(rendered.contains("dark mode [● ]"), "on switch missing");
    assert!(rendered.contains("spell check [ ●]"), "off switch missing");
    assert!(
        rendered.contains("region: pick one ▾"),
        "select trigger missing"
    );
}

#[test]
fn select_choice_flow_lands_in_the_trigger() {
    let mut terminal = Terminal::new(TestBackend::new(Size::new(24, 7))).unwrap();
    let mut state = SelectState::new();
    let options = ["eu-west", "eu-north", "us-east"];

    // Closed: the placeholder shows, nothing is highlighted.
    terminal
        .draw(|frame| {
            Select::new(options)
                .label("region")
                .placeholder("pick one")
                .render(Rect::new(0, 0, 24, 1), frame.buffer, &mut state);
        })
        .unwrap();
    assert!(
        terminal
            .backend()
            .buffer()
            .to_string()
            .contains("region: pick one ▾")
    );

    // Open, navigate twice, choose: the trigger shows the committed row.
    state.open();
    state.select_next(options.len());
    state.select_next(options.len());
    state.choose();
    terminal
        .draw(|frame| {
            Select::new(options)
                .label("region")
                .placeholder("pick one")
                .render(Rect::new(0, 0, 24, 1), frame.buffer, &mut state);
        })
        .unwrap();
    let rendered = terminal.backend().buffer().to_string();
    assert!(
        rendered.contains("region: us-east ▾"),
        "committed choice missing: {rendered}"
    );
    assert_eq!(state.selected(), Some(2));
    assert!(!state.is_open());
}

/// Differential gate: navigating open -> highlight -> choose lands the
/// same rendered state as a direct `set_selected` of the same row.
#[test]
fn keyboard_choice_session_matches_direct_selection() {
    let options = ["eu-west", "eu-north", "us-east"];
    let widget = Select::new(options).label("region").placeholder("pick one");

    let mut stepped = SelectState::new();
    stepped.open();
    stepped.select_next(options.len());
    stepped.select_next(options.len());
    stepped.choose();
    let mut stepped_buffer = Buffer::empty(Rect::new(0, 0, 24, 1));
    widget
        .clone()
        .render(Rect::new(0, 0, 24, 1), &mut stepped_buffer, &mut stepped);

    let mut direct = SelectState::new().with_selected(Some(2));
    let mut direct_buffer = Buffer::empty(Rect::new(0, 0, 24, 1));
    widget.render(Rect::new(0, 0, 24, 1), &mut direct_buffer, &mut direct);

    assert_eq!(
        stepped_buffer, direct_buffer,
        "a choice session must render identically to the direct selection"
    );
}

/// A closed dropdown selects nothing: `choose` is a no-op and the state
/// stays put.
#[test]
fn choose_is_inert_while_closed() {
    let mut state = SelectState::new().with_selected(Some(1));
    state.set_highlight(0);
    assert_eq!(state.choose(), None);
    assert_eq!(state.selected(), Some(1), "the committed choice survives");
    assert_eq!(state.highlight(), 0, "the highlight survives");
    assert!(!state.is_open());
}

/// The theme maps through the family: text on the base rows, accent on
/// the checked box and the on switch, highlight on the radio row and
/// the open list's highlighted option.
#[test]
fn theme_maps_across_the_family() {
    let theme = Theme::dark();

    let mut buffer = Buffer::empty(Rect::new(0, 0, 20, 1));
    Checkbox::new("ab")
        .checked(true)
        .theme(&theme)
        .render(Rect::new(0, 0, 20, 1), &mut buffer);
    assert_eq!(buffer.get(2, 0).unwrap().style.fg, theme.accent.fg);

    let mut buffer = Buffer::empty(Rect::new(0, 0, 20, 2));
    RadioGroup::new(["a", "b"])
        .select(Some(1))
        .theme(&theme)
        .render(Rect::new(0, 0, 20, 2), &mut buffer);
    assert_eq!(buffer.get(0, 1).unwrap().style, theme.highlight);

    let mut buffer = Buffer::empty(Rect::new(0, 0, 20, 1));
    Switch::new("ab")
        .on(true)
        .theme(&theme)
        .render(Rect::new(0, 0, 20, 1), &mut buffer);
    assert_eq!(buffer.get(3, 0).unwrap().style, theme.accent);

    let mut buffer = Buffer::empty(Rect::new(0, 0, 20, 4));
    let mut state = SelectState::new();
    state.open();
    state.select_next(3);
    Select::new(["a", "b", "c"]).theme(&theme).render(
        Rect::new(0, 0, 20, 4),
        &mut buffer,
        &mut state,
    );
    assert_eq!(buffer.get(0, 2).unwrap().style, theme.highlight);
    assert_eq!(
        buffer.get(0, 0).unwrap().style.fg,
        theme.text.fg,
        "the trigger keeps the text slot"
    );
}

/// Wide-glyph honesty: a CJK label never splits — the buffer's pair
/// discipline carries the form family for free.
#[test]
fn wide_glyph_labels_stay_atomic() {
    let mut buffer = Buffer::empty(Rect::new(0, 0, 12, 1));
    Checkbox::new("日本語チェック")
        .checked(true)
        .render(Rect::new(0, 0, 12, 1), &mut buffer);
    let rendered = buffer.to_string();
    // The covered half of every wide pair prints as the skip marker, so
    // an intact pair reads "日·" — a split pair would print differently.
    assert!(rendered.contains('☑'), "marker missing: {rendered:?}");
    assert!(
        rendered.starts_with("☑ 日·"),
        "wide label split: {rendered:?}"
    );

    let mut buffer = Buffer::empty(Rect::new(0, 0, 9, 1));
    Switch::new("設定")
        .on(true)
        .render(Rect::new(0, 0, 9, 1), &mut buffer);
    let rendered = buffer.to_string();
    assert!(
        rendered.starts_with("設·定· "),
        "wide label split: {rendered:?}"
    );
    assert!(rendered.contains("[● ]"), "track missing: {rendered:?}");

    // One column narrower, the track clips exactly like any buffer
    // write — the knob stays visible, the closing bracket does not.
    let mut buffer = Buffer::empty(Rect::new(0, 0, 8, 1));
    Switch::new("設定")
        .on(true)
        .render(Rect::new(0, 0, 8, 1), &mut buffer);
    assert_eq!(buffer.to_string(), "設·定· [● ");
}

/// One frame of the B18 panel: a fresh buffer every time, the way an
/// immediate-mode settings screen pays for a toggle.
fn form_panel_frame(select_state: &mut SelectState) -> Buffer {
    let area = Rect::new(0, 0, WIDTH, HEIGHT);
    let mut buffer = Buffer::empty(area);
    paint_form_panel(&mut buffer, select_state);
    buffer
}

#[test]
#[ignore = "timing gate: run with --ignored (perf workflow)"]
fn b18_form_panel_paint_within_budget() {
    let mut select_state = SelectState::new();
    select_state.set_selected(Some(2));

    // Warm-up (allocator + caches; not part of the gate).
    for _ in 0..16 {
        black_box(form_panel_frame(&mut select_state));
    }

    let mut latencies = Vec::with_capacity(FRAMES);
    for _ in 0..FRAMES {
        let start = Instant::now();
        let buffer = black_box(form_panel_frame(black_box(&mut select_state)));
        let elapsed = start.elapsed();
        // Sanity: the panel really painted.
        assert!(buffer.to_string().contains("settings"));
        latencies.push(elapsed);
    }
    latencies.sort();
    let median = latencies[FRAMES / 2];
    let p99 = latencies[FRAMES * 99 / 100 - 1];
    let max = latencies[FRAMES - 1];

    assert!(
        p99 <= Duration::from_millis(1),
        "B18 gate failed: form-panel paint p99 {p99:?} (budget 1 ms)"
    );
    println!(
        "B18 gate: form-panel paint median {median:?}, p99 {p99:?}, max {max:?} over {FRAMES} frames @ {WIDTH}x{HEIGHT}, 24-row settings form (budget 1 ms)"
    );
}
