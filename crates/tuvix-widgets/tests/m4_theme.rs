//! The M4 theme gate: every widget's `.theme()` maps the named slots onto
//! its rendered output, pinned through buffer-level style assertions.

use tuvix_core::backend::TestBackend;
use tuvix_core::buffer::Buffer;
use tuvix_core::geometry::{Rect, Size};
use tuvix_core::style::{Color, Modifier, Style};
use tuvix_core::terminal::Terminal;
use tuvix_core::theme::Theme;
use tuvix_core::widget::{RenderWidget, Widget};
use tuvix_widgets::{
    Block, BorderType, Borders, Chart, Dataset, Gauge, GraphType, List, ListItem, ListState,
    Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Sparkline, Table, TableCell,
    TableRow, TableState,
};

fn cell_style(buffer: &Buffer, x: u16, y: u16) -> Style {
    buffer.get(x, y).unwrap().style
}

#[test]
fn block_theme_maps_border_and_title_slots() {
    let theme = Theme::dark();
    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .title("title")
        .theme(&theme);
    let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 3));
    block.render(Rect::new(0, 0, 10, 3), &mut buffer);

    // Corner cell carries the border style.
    assert_eq!(cell_style(&buffer, 0, 0), theme.border);
    // The title sits on the top edge with the title style.
    let title_cell = buffer
        .content
        .iter()
        .find(|cell| cell.symbol.as_str() == "t")
        .expect("title glyph");
    assert_eq!(title_cell.style, theme.title);
}

#[test]
fn gauge_theme_maps_gauge_slot() {
    let theme = Theme::dark();
    let gauge = Gauge::percent(50).theme(&theme);
    let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 1));
    gauge.render(Rect::new(0, 0, 10, 1), &mut buffer);
    assert_eq!(cell_style(&buffer, 0, 0), theme.gauge);
    assert_eq!(cell_style(&buffer, 0, 0).fg, Some(Color::Green));
}

#[test]
fn sparkline_theme_maps_sparkline_slot() {
    let theme = Theme::dark();
    let sparkline = Sparkline::new([1u64, 4, 2, 8]).theme(&theme);
    let mut buffer = Buffer::empty(Rect::new(0, 0, 8, 1));
    sparkline.render(Rect::new(0, 0, 8, 1), &mut buffer);
    let bar = buffer
        .content
        .iter()
        .find(|cell| !cell.symbol.is_empty())
        .expect("sparkline bar");
    assert_eq!(bar.style, theme.sparkline);
}

#[test]
fn list_theme_maps_highlight_slot() {
    let theme = Theme::dark();
    let items = vec![ListItem::new("alpha"), ListItem::new("beta")];
    let list = List::new(items).theme(&theme);
    let mut state = ListState::default().with_selected(Some(0));
    let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 2));
    list.render(Rect::new(0, 0, 10, 2), &mut buffer, &mut state);
    // The selected row carries the highlight style (reversed background).
    assert_eq!(cell_style(&buffer, 0, 0).bg, theme.highlight.bg);
    // The unselected row does not.
    assert_ne!(cell_style(&buffer, 0, 1).bg, theme.highlight.bg);
}

#[test]
fn table_theme_maps_title_and_highlight_slots() {
    let theme = Theme::dark();
    let rows: Vec<TableRow> = vec![TableRow::new([TableCell::from("one")])];
    let header = TableRow::new([TableCell::from("head")]);
    let table = Table::new(rows, [tuvix_core::layout::Constraint::Length(6)])
        .header(header)
        .theme(&theme);
    let mut state = TableState::default().with_selected(Some(0));
    let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 2));
    table.render(Rect::new(0, 0, 10, 2), &mut buffer, &mut state);
    // Header row: title style (bold white).
    assert_eq!(cell_style(&buffer, 0, 0).fg, theme.title.fg);
    assert_eq!(cell_style(&buffer, 0, 0).add_modifier, Modifier::BOLD);
    // Selected data row: highlight background.
    assert_eq!(cell_style(&buffer, 0, 1).bg, theme.highlight.bg);
}

#[test]
fn scrollbar_theme_maps_scrollbar_slot() {
    let theme = Theme::dark();
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight).theme(&theme);
    let mut state = ScrollbarState::new(10);
    state.position(0);
    let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 5));
    scrollbar.render(Rect::new(0, 0, 10, 5), &mut buffer, &mut state);
    // Layout for a 5-row vertical track: begin glyph, thumb (length 1 at
    // position 0), then track, then end glyph.
    let begin = cell_style(&buffer, 9, 0);
    let thumb = cell_style(&buffer, 9, 1);
    let track = cell_style(&buffer, 9, 2);
    // Begin/track carry the scrollbar slot; the thumb inverts it.
    assert_eq!(track.fg, theme.scrollbar.fg);
    assert_eq!(thumb.add_modifier, Modifier::REVERSED);
    assert_ne!(track.add_modifier, Modifier::REVERSED);
    let _ = begin;
}

#[test]
fn chart_theme_maps_axis_slot() {
    let theme = Theme::dark();
    let dataset = Dataset::new("series", [(0.0, 0.0), (1.0, 1.0)]).graph_type(GraphType::Line);
    let chart = Chart::new([dataset])
        .x_axis(
            tuvix_widgets::Axis::new()
                .bounds([0.0, 1.0])
                .labels(["0", "1"]),
        )
        .y_axis(
            tuvix_widgets::Axis::new()
                .bounds([0.0, 1.0])
                .labels(["0", "1"]),
        )
        .theme(&theme);
    let mut buffer = Buffer::empty(Rect::new(0, 0, 12, 6));
    chart.render(Rect::new(0, 0, 12, 6), &mut buffer);
    // The axis gutter column (between labels and graph) carries the axis style.
    let axis_line = buffer
        .content
        .iter()
        .find(|cell| cell.symbol.as_str() == "│")
        .expect("axis line glyph");
    assert_eq!(axis_line.style, theme.axis);
}

#[test]
fn paragraph_theme_maps_text_slot() {
    let theme = Theme::dark();
    let paragraph = Paragraph::new("hello").theme(&theme);
    let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 1));
    paragraph.render(Rect::new(0, 0, 10, 1), &mut buffer);
    assert_eq!(cell_style(&buffer, 0, 0), theme.text);
    assert_eq!(cell_style(&buffer, 0, 0).fg, Some(Color::Gray));
}

#[test]
fn theme_changes_propagate_through_the_terminal() {
    // End to end: re-theming a widget changes the painted buffer through
    // Terminal::draw.
    let mut terminal = Terminal::new(TestBackend::new(Size::new(10, 1))).unwrap();
    let theme = Theme::dark();
    terminal
        .draw(|frame| {
            Gauge::percent(100)
                .theme(&theme)
                .render(frame.buffer.area, frame.buffer);
        })
        .unwrap();
    assert_eq!(
        terminal.backend().buffer().get(0, 0).unwrap().style,
        theme.gauge
    );
}
