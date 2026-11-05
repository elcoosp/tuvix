//! M3 integration gate: the widget family through the whole render
//! stack (Terminal + TestBackend), the unified `RenderWidget` contract
//! for stateful widgets, and cross-frame selection navigation.

use tuvix_core::backend::TestBackend;
use tuvix_core::geometry::{Rect, Size};
use tuvix_core::layout::{Constraint, Layout};
use tuvix_core::style::{Color, Style};
use tuvix_core::terminal::Terminal;
use tuvix_core::widget::{RenderWidget, Widget};
use tuvix_widgets::{
    Axis, Block, Borders, Chart, Dataset, Gauge, List, ListState, Marker, Paragraph, Scrollbar,
    ScrollbarOrientation, ScrollbarState, Sparkline, Table, TableRow, TableState,
};

/// Renders a full dashboard frame through the terminal stack.
#[test]
fn dashboard_renders_through_the_terminal_stack() {
    let mut terminal = Terminal::new(TestBackend::new(Size::new(60, 14))).unwrap();
    terminal
        .draw(|frame| {
            let buffer: &mut tuvix_core::buffer::Buffer = &mut *frame.buffer;
            let rows = Layout::vertical([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(3),
                Constraint::Min(0),
            ])
            .split(Rect::new(0, 0, 60, 14));

            // Stateless widgets paint through `Widget`.
            Gauge::percent(75).render(rows[0], buffer);
            Sparkline::new([1_u64, 3, 2, 8, 5, 13, 9, 21]).render(rows[1], buffer);

            // A blocked chart with axes.
            let chart = Chart::new([Dataset::new("series", [(0.0, 0.0), (1.0, 1.0)])
                .graph_type(tuvix_widgets::GraphType::Line)
                .marker(Marker::Dot)])
            .x_axis(Axis::new().bounds([0.0, 1.0]).labels(["0", "1"]))
            .y_axis(Axis::new().bounds([0.0, 1.0]).labels(["0", "1"]))
            .block(Block::bordered().title("chart"));
            chart.render(rows[2], buffer);

            // The stateful family paints through the RenderWidget contract.
            let table = Table::new(
                [TableRow::new(["row-a", "1"]), TableRow::new(["row-b", "2"])],
                [Constraint::Percentage(50), Constraint::Percentage(50)],
            )
            .header(TableRow::new(["name", "value"]))
            .block(Block::new().borders(Borders::TOP));
            let mut table_state = TableState::default();
            table.render(rows[3], buffer, &mut table_state);
        })
        .unwrap();

    let rendered = terminal.backend().buffer().to_string();
    assert!(rendered.contains("75%"), "gauge label missing");
    assert!(rendered.contains('▁'), "sparkline bars missing");
    assert!(rendered.contains("chart"), "block title missing");
    assert!(rendered.contains("name"), "table header missing");
    assert!(rendered.contains("row-a"), "table rows missing");
}

/// The RenderWidget contract: one method drives every stateful widget.
#[test]
fn render_widget_contract_drives_the_stateful_family() {
    fn paint<W: RenderWidget>(
        widget: W,
        area: Rect,
        buffer: &mut tuvix_core::buffer::Buffer,
        state: &mut W::State,
    ) {
        RenderWidget::render(widget, area, buffer, state);
    }

    let mut buffer = tuvix_core::buffer::Buffer::empty(Rect::new(0, 0, 10, 4));
    let mut list_state = ListState::default();
    paint(
        List::new(["a", "b"]),
        Rect::new(0, 0, 10, 2),
        &mut buffer,
        &mut list_state,
    );
    let mut scrollbar_state = ScrollbarState::new(0);
    paint(
        Scrollbar::new(ScrollbarOrientation::VerticalRight),
        Rect::new(9, 0, 1, 4),
        &mut buffer,
        &mut scrollbar_state,
    );
    let rendered = buffer.to_string();
    let rows: Vec<&str> = rendered.lines().collect();
    // The list paints rows 0-1; the scrollbar bookends rows 0 and 3
    // with a full thumb in between (content length 0).
    assert_eq!(rows[0], "a        ↑");
    assert_eq!(rows[1], "b        █");
    assert_eq!(rows[3], "         ↓");
}

/// Selection navigation across frames keeps the selection visible and
/// the list grows its scroll state monotonically.
#[test]
fn list_selection_navigates_across_frames() {
    let items: Vec<String> = (0..10).map(|index| format!("item-{index}")).collect();
    let mut terminal = Terminal::new(TestBackend::new(Size::new(10, 3))).unwrap();
    let mut state = ListState::default();

    // Walk the selection to the end via select_next; every frame must
    // show the selected row.
    for _ in 0..9 {
        state.select_next();
        terminal
            .draw(|frame| {
                let list = List::new(items.clone());
                RenderWidget::render(list, Rect::new(0, 0, 10, 3), frame.buffer, &mut state);
            })
            .unwrap();
    }
    // After 9 nexts the selection is 8 (0-based); the last frame shows
    // the bottom of the list.
    let rendered = terminal.backend().buffer().to_string();
    assert!(
        rendered.contains("item-8"),
        "selection row missing: {rendered}"
    );
    assert_eq!(state.selected(), Some(8));
}

/// The stateless family still renders through the classic `Widget`
/// trait next to the M3 contract.
#[test]
fn stateless_widgets_render_through_widget_trait() {
    let mut buffer = tuvix_core::buffer::Buffer::empty(Rect::new(0, 0, 6, 1));
    Widget::render(
        Gauge::percent(100).label(""),
        Rect::new(0, 0, 6, 1),
        &mut buffer,
    );
    assert_eq!(buffer.to_string(), "██████");

    let mut buffer = tuvix_core::buffer::Buffer::empty(Rect::new(0, 0, 6, 1));
    Widget::render(
        Paragraph::new("styled").style(Style::new().fg(Color::Red)),
        Rect::new(0, 0, 6, 1),
        &mut buffer,
    );
    assert_eq!(buffer.get(0, 0).unwrap().style.fg, Some(Color::Red));
}
