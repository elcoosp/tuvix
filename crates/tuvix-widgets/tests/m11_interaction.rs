//! M11 integration gates: the B14 viewport-scroll budget, measured, not
//! asserted.
//!
//! B14 — a scrolled 200x60 viewport frame over 400 rows of content (the
//! window moves, the cached child paint blits) stays inside **1 ms**, half
//! of the B2 steady-frame budget, so a scroll storm can never push a frame
//! over the 2 ms line by itself. A content change additionally pays the
//! child's own paint when the app invalidates — that cost is the child's,
//! not the viewport's (measured in the `scrollable` bench group).

use std::hint::black_box;
use std::time::{Duration, Instant};

use tuvix_core::buffer::Buffer;
use tuvix_core::geometry::{Rect, Size};
use tuvix_core::style::Style;
use tuvix_core::widget::RenderWidget;
use tuvix_widgets::{Scrollable, ViewportState};

const WIDTH: u16 = 200;
const HEIGHT: u16 = 60;
const CONTENT_ROWS: u16 = 400;
const FRAMES: usize = 200;

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

/// One scroll step: blit the window out of the cached child paint —
/// exactly what a scroll-storm frame costs.
fn scroll_frame(
    widget: &Scrollable<LogLines>,
    state: &mut ViewportState<()>,
    area: Rect,
) -> Buffer {
    state.scroll_down(1);
    let mut buffer = Buffer::empty(area);
    widget.clone().render(area, &mut buffer, state);
    buffer
}

#[test]
#[ignore = "timing gate: run with --ignored (perf workflow)"]
fn b14_viewport_scroll_repaint_within_one_millisecond() {
    let area = Rect::new(0, 0, WIDTH, HEIGHT);
    let widget = Scrollable::new(LogLines(CONTENT_ROWS)).content(Size::new(WIDTH, CONTENT_ROWS));
    let mut state = ViewportState::new(());

    // Warm the scratch surface (the first paint is not the gate).
    widget
        .clone()
        .render(area, &mut Buffer::empty(area), &mut state);

    let mut latencies = Vec::with_capacity(FRAMES);
    for frame in 0..FRAMES {
        let start = Instant::now();
        let buffer = black_box(scroll_frame(&widget, black_box(&mut state), area));
        let elapsed = start.elapsed();
        // Sanity: the window really scrolled (content moved by the frame
        // count).
        if frame < usize::from(CONTENT_ROWS - HEIGHT - 1) {
            assert!(
                buffer.to_string().contains(&format!("12:{:02}", frame + 1)),
                "the scrolled window must show fresh rows"
            );
        }
        latencies.push(elapsed);
    }
    latencies.sort();
    let median = latencies[FRAMES / 2];
    let p99 = latencies[FRAMES * 99 / 100 - 1];
    let max = latencies[FRAMES - 1];

    assert!(
        p99 <= Duration::from_millis(1),
        "B14 gate failed: viewport scroll repaint p99 {p99:?} (budget 1 ms)"
    );
    println!(
        "B14 gate: viewport scroll repaint median {median:?}, p99 {p99:?}, max {max:?} over {FRAMES} frames @ {WIDTH}x{HEIGHT}, {CONTENT_ROWS} content rows (budget 1 ms)"
    );
}

/// Differential gate: scrolling one step at a time renders byte-identically
/// to a fresh state that jumped straight to the same offset — a scroll
/// session can never drift from the truth.
#[test]
fn scroll_session_matches_fresh_jumps() {
    let area = Rect::new(0, 0, 40, 6);
    let widget = Scrollable::new(LogLines(CONTENT_ROWS)).content(Size::new(40, CONTENT_ROWS));

    let mut stepped = ViewportState::new(());
    let mut stepped_buffer = Buffer::empty(area);
    widget
        .clone()
        .render(area, &mut stepped_buffer, &mut stepped);
    for _ in 0..12 {
        stepped.scroll_down(1);
        widget
            .clone()
            .render(area, &mut stepped_buffer, &mut stepped);
    }

    let mut jumped = ViewportState::new(());
    jumped.set_content(Size::new(40, CONTENT_ROWS));
    jumped.scroll_down(12);
    let mut jumped_buffer = Buffer::empty(area);
    widget.clone().render(area, &mut jumped_buffer, &mut jumped);

    assert_eq!(
        stepped_buffer, jumped_buffer,
        "a scroll session must render identically to a direct jump"
    );
    assert_eq!(stepped.offset_y(), 12);
}
