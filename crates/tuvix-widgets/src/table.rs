//! Tables: headers, columns solved through [`Layout`], row selection.
//!
//! A [`Table`] is a [`RenderWidget`] with `State = TableState`: render it
//! with `RenderWidget::render(table, area, buf, &mut state)` or the
//! method-call form `table.render(area, buf, &mut state)`.

use unicode_width::UnicodeWidthStr as _;

use tuvix_core::buffer::Buffer;
use tuvix_core::geometry::Rect;
use tuvix_core::layout::{Constraint, Layout};
use tuvix_core::style::Style;
use tuvix_core::text::{Alignment, Line, Text};
use tuvix_core::widget::RenderWidget;

use crate::block::Block;

/// One cell of a [`TableRow`]: a [`Text`] plus a cell style.
///
/// Named `TableCell` (not `Cell`) so the facade prelude stays
/// unambiguous next to `tuvix_core::buffer::Cell`.
///
/// # Examples
///
/// ```
/// use tuvix_core::buffer::Buffer;
/// use tuvix_core::geometry::Rect;
/// use tuvix_widgets::TableCell;
///
/// let mut buffer = Buffer::empty(Rect::new(0, 0, 3, 1));
/// TableCell::new("ab").render(Rect::new(0, 0, 3, 1), &mut buffer);
/// assert_eq!(buffer.to_string(), "ab ");
/// ```
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TableCell {
    /// The text of the cell (multi-line cells grow the row).
    pub text: Text,
    /// Style layered under the cell's lines and spans.
    pub style: Style,
}

impl TableCell {
    /// Creates a cell from anything [`Text`]-convertible.
    pub fn new<T: Into<Text>>(text: T) -> Self {
        Self {
            text: text.into(),
            style: Style::new(),
        }
    }

    /// Sets the style layered under the cell's lines and spans.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Number of display rows the cell occupies.
    #[must_use]
    pub fn height(&self) -> usize {
        self.text.height()
    }

    /// Paints the cell into `buffer`, clipped to `area` (line
    /// alignments within the cell are honored).
    pub fn render(&self, area: Rect, buffer: &mut Buffer) {
        self.render_with_base(area, buffer, Style::new());
    }

    /// Paints the cell with `base` layered under the cell style and the
    /// cell's lines and spans.
    pub fn render_with_base(&self, area: Rect, buffer: &mut Buffer, base: Style) {
        for (index, line) in self.text.lines.iter().enumerate() {
            let y = area
                .y
                .saturating_add(u16::try_from(index).unwrap_or(u16::MAX));
            if y >= area.bottom() {
                break;
            }
            let line_width = line.width().min(area.width);
            let x = match line.alignment.unwrap_or_default() {
                Alignment::Left => area.x,
                Alignment::Center => area.x + (area.width - line_width) / 2,
                Alignment::Right => area.x + area.width.saturating_sub(line_width),
            };
            let line_style = base.patch(self.style).patch(line.style);
            let mut cursor = x;
            let right_limit = area.right();
            for span in &line.spans {
                if cursor >= right_limit {
                    break;
                }
                let span_style = line_style.patch(span.style);
                let (end_x, _) = buffer.set_stringn(
                    cursor,
                    y,
                    span.content.as_ref(),
                    right_limit - cursor,
                    span_style,
                );
                cursor = end_x;
            }
        }
    }
}

impl From<&'static str> for TableCell {
    fn from(text: &'static str) -> Self {
        Self::new(text)
    }
}

impl From<String> for TableCell {
    fn from(text: String) -> Self {
        Self::new(text)
    }
}

impl From<Line> for TableCell {
    fn from(line: Line) -> Self {
        Self::new(line)
    }
}

impl From<Text> for TableCell {
    fn from(text: Text) -> Self {
        Self::new(text)
    }
}

/// A table row: cells sharing a row style.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TableRow {
    /// The cells, left to right.
    pub cells: Vec<TableCell>,
    /// Style layered under the row's cells.
    pub style: Style,
}

impl TableRow {
    /// Creates a row from an iterator of [`TableCell`]-convertibles.
    pub fn new<T, I>(cells: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<TableCell>,
    {
        Self {
            cells: cells.into_iter().map(Into::into).collect(),
            style: Style::new(),
        }
    }

    /// Sets the style layered under the row's cells.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Number of display rows the row occupies (its tallest cell).
    pub fn height(&self) -> usize {
        self.cells.iter().map(TableCell::height).max().unwrap_or(1)
    }
}

impl From<Vec<TableCell>> for TableRow {
    fn from(cells: Vec<TableCell>) -> Self {
        Self::new(cells)
    }
}

/// The scroll offset and selection of a [`Table`].
///
/// Same shape and navigation semantics as [`ListState`](crate::ListState)
/// (offset counts display rows, selection scrolls minimally into view).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TableState {
    /// Display rows scrolled off the top of the rows area.
    pub offset: usize,
    /// The selected row index, if any.
    selected: Option<usize>,
}

impl TableState {
    /// Creates a state with the given initial selection.
    #[must_use]
    pub fn with_selected(mut self, selected: Option<usize>) -> Self {
        self.selected = selected;
        self
    }

    /// The selected row index, if any.
    #[must_use]
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// Sets the selected row index (raw; out-of-range values render
    /// without a highlight).
    pub fn select(&mut self, index: Option<usize>) {
        self.selected = index;
    }

    /// Selects the next row; no selection selects the first.
    pub fn select_next(&mut self) {
        self.selected = Some(self.selected.map_or(0, |index| index + 1));
    }

    /// Selects the previous row (saturating at the first); no selection
    /// selects the first.
    pub fn select_previous(&mut self) {
        self.selected = Some(self.selected.map_or(0, |index| index.saturating_sub(1)));
    }

    /// Selects the first row.
    pub fn select_first(&mut self) {
        self.selected = Some(0);
    }

    /// Selects the last row; the caller supplies the count so no
    /// sentinel value is involved.
    pub fn select_last(&mut self, row_count: usize) {
        self.selected = row_count.checked_sub(1);
    }

    /// Scrolls the view down by `rows` display rows.
    pub fn scroll_down_by(&mut self, rows: usize) {
        self.offset += rows;
    }

    /// Scrolls the view up by `rows` display rows (saturating at 0).
    pub fn scroll_up_by(&mut self, rows: usize) {
        self.offset = self.offset.saturating_sub(rows);
    }
}

/// A header-and-columns table with selectable rows.
///
/// Column widths are solved by the core [`Layout`] solver (its
/// documented pass semantics apply: `Min` claims, desired claims, `Fill`
/// weights), with [`Table::column_spacing`] between segments.
///
/// # Examples
///
/// ```
/// use tuvix_core::buffer::Buffer;
/// use tuvix_core::geometry::Rect;
/// use tuvix_core::layout::Constraint;
/// use tuvix_core::widget::RenderWidget;
/// use tuvix_widgets::{Table, TableRow, TableState};
///
/// let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 3));
/// let table = Table::new(
///     [TableRow::new(["a", "b"]), TableRow::new(["c", "d"])],
///     [Constraint::Length(4), Constraint::Length(4)],
/// )
/// .header(TableRow::new(["id", "val"]))
/// .column_spacing(1);
/// let mut state = TableState::default().with_selected(Some(0));
/// table.render(Rect::new(0, 0, 10, 3), &mut buffer, &mut state);
/// // Columns solved by Layout: [0..4) and [5..9).
/// assert_eq!(buffer.to_string(), "id   val  \na    b    \nc    d    ");
/// ```
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Table {
    header: Option<TableRow>,
    rows: Vec<TableRow>,
    widths: Vec<Constraint>,
    column_spacing: u16,
    block: Option<Block>,
    style: Style,
    header_style: Style,
    row_style: Style,
    highlight_style: Style,
    highlight_symbol: Option<String>,
}

impl Table {
    /// Creates a table from rows and column width constraints.
    pub fn new<T, I, W, J>(rows: I, widths: J) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<TableRow>,
        J: IntoIterator<Item = W>,
        W: Into<Constraint>,
    {
        Self {
            header: None,
            rows: rows.into_iter().map(Into::into).collect(),
            widths: widths.into_iter().map(Into::into).collect(),
            column_spacing: 1,
            block: None,
            style: Style::new(),
            header_style: Style::new(),
            row_style: Style::new(),
            highlight_style: Style::new(),
            highlight_symbol: None,
        }
    }

    /// Sets the header row (always visible above the scrolling rows).
    #[must_use]
    pub fn header(mut self, header: TableRow) -> Self {
        self.header = Some(header);
        self
    }

    /// Wraps the table in a block; content fills the block's inner area.
    #[must_use]
    pub fn block(mut self, block: Block) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the base style of the table area.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the style patched over the header row.
    #[must_use]
    pub fn header_style(mut self, style: Style) -> Self {
        self.header_style = style;
        self
    }

    /// Sets the style layered under every data row.
    #[must_use]
    pub fn row_style(mut self, style: Style) -> Self {
        self.row_style = style;
        self
    }

    /// Sets the style patched over the selected row.
    #[must_use]
    pub fn highlight_style(mut self, style: Style) -> Self {
        self.highlight_style = style;
        self
    }

    /// Sets the marker written in front of the selected row (and spaces
    /// of the same width in front of every other row, so columns align).
    #[must_use]
    pub fn highlight_symbol(mut self, symbol: impl Into<String>) -> Self {
        self.highlight_symbol = Some(symbol.into());
        self
    }

    /// Sets the gap between columns (default 1).
    #[must_use]
    pub fn column_spacing(mut self, spacing: u16) -> Self {
        self.column_spacing = spacing;
        self
    }

    /// Paints the table into `buffer`, clipped to `area`, updating `state`.
    ///
    /// The header stays at the top of the table area; data rows scroll
    /// underneath it (`state.offset` counts rows *below the header*).
    /// Called through [`RenderWidget::render`]; the method-call form is
    /// `table.render(area, buffer, &mut state)`.
    fn paint(self, area: Rect, buffer: &mut Buffer, state: &mut TableState) {
        if let Some(block) = &self.block {
            block.render(area, buffer);
        }
        let table_area = self.block.as_ref().map_or(area, |block| block.inner(area));
        if table_area.is_empty() || self.widths.is_empty() {
            return;
        }
        buffer.set_style(table_area, self.style);

        // The selection gutter: when a row is selected, every row (header
        // included) renders into columns shifted right by the symbol
        // width, so the grid stays aligned.
        let gutter_width = if state
            .selected
            .is_some_and(|selected| selected < self.rows.len())
        {
            self.highlight_symbol.as_ref().map_or(0, |symbol| {
                u16::try_from(symbol.width()).unwrap_or(u16::MAX)
            })
        } else {
            0
        };

        // Solve the column layout across the (gutter-shrunk) table width.
        let solve_area = Rect::new(
            table_area.x + gutter_width,
            table_area.y,
            table_area.width.saturating_sub(gutter_width),
            1,
        );
        let columns = Layout::horizontal(self.widths.iter().copied())
            .spacing(self.column_spacing)
            .split(solve_area);

        let mut y = table_area.y;
        if let Some(header) = &self.header {
            let header_style = self.style.patch(self.header_style).patch(header.style);
            if gutter_width > 0 {
                let blank = " ".repeat(usize::from(gutter_width));
                buffer.set_stringn(table_area.x, y, blank.as_str(), gutter_width, header_style);
            }
            for (cell, column) in header.cells.iter().zip(&columns) {
                cell.render_with_base(*column, buffer, header_style);
            }
            y += 1;
        }

        let rows_height = table_area.bottom().saturating_sub(y);
        let rows_area = Rect::new(table_area.x, y, table_area.width, rows_height);
        if rows_area.is_empty() {
            return;
        }
        let height = usize::from(rows_area.height);
        let heights: Vec<usize> = self.rows.iter().map(TableRow::height).collect();
        let mut starts = Vec::with_capacity(self.rows.len());
        let mut running = 0_usize;
        for row_height in &heights {
            starts.push(running);
            running += *row_height;
        }

        // Minimal scroll so the selection stays visible.
        if let Some(selected) = state
            .selected
            .filter(|selected| *selected < self.rows.len())
        {
            let selected_top = starts[selected];
            let selected_bottom = selected_top + heights[selected];
            if selected_top < state.offset {
                state.offset = selected_top;
            } else if selected_bottom > state.offset + height {
                state.offset = selected_bottom - height;
            }
        }

        let mut row = 0_usize;
        for (index, table_row) in self.rows.iter().enumerate() {
            let row_top = starts[index];
            let row_bottom = row_top + heights[index];
            if row_bottom <= state.offset || row >= height {
                continue;
            }
            let is_selected = state.selected == Some(index);
            let base = if is_selected {
                self.style
                    .patch(self.row_style)
                    .patch(table_row.style)
                    .patch(self.highlight_style)
            } else {
                self.style.patch(self.row_style).patch(table_row.style)
            };
            if gutter_width > 0 {
                let symbol: String = if is_selected {
                    self.highlight_symbol.clone().unwrap_or_default()
                } else {
                    " ".repeat(usize::from(gutter_width))
                };
                buffer.set_stringn(table_area.x, y, symbol.as_str(), gutter_width, base);
            }
            for (cell, column) in table_row.cells.iter().zip(&columns) {
                let cell_area = Rect::new(
                    column.x,
                    y,
                    column.width,
                    u16::try_from(heights[index]).unwrap_or(u16::MAX),
                );
                cell.render_with_base(cell_area, buffer, base);
            }
            y += u16::try_from(heights[index]).unwrap_or(u16::MAX);
            row += heights[index];
        }
    }
}

impl RenderWidget for Table {
    type State = TableState;

    fn render(self, area: Rect, buffer: &mut Buffer, state: &mut Self::State) {
        Table::paint(self, area, buffer, state);
    }
}

impl Table {
    /// Maps the theme onto the table: the `title` slot styles the header,
    /// the `highlight` slot styles the selected row.
    #[must_use]
    pub fn theme(mut self, theme: &Theme) -> Self {
        self.header_style = theme.title;
        self.highlight_style = theme.highlight;
        self
    }
}

use tuvix_core::theme::Theme;
#[cfg(test)]
mod tests {
    use super::*;
    use tuvix_core::style::Color;

    fn render_at(table: Table, area: Rect, buffer: &mut Buffer, state: &mut TableState) {
        RenderWidget::render(table, area, buffer, state);
    }

    #[test]
    fn header_and_rows_render_into_solved_columns() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 3));
        let table = Table::new(
            [TableRow::new(["a", "b"]), TableRow::new(["c", "d"])],
            [Constraint::Length(4), Constraint::Length(4)],
        )
        .header(TableRow::new(["id", "val"]))
        .column_spacing(1);
        render_at(
            table,
            Rect::new(0, 0, 10, 3),
            &mut buffer,
            &mut TableState::default(),
        );
        // Columns: [0..4), [5..9); the trailing 10th column is a gap.
        assert_eq!(buffer.to_string(), "id   val  \na    b    \nc    d    ");
    }

    #[test]
    fn header_stays_visible_while_rows_scroll() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 6, 3));
        let table = Table::new(
            (0..5).map(|index| TableRow::new([format!("r{index}")])),
            [Constraint::Length(6)],
        )
        .header(TableRow::new(["head"]));
        let mut state = TableState::default().with_selected(Some(4));
        render_at(table, Rect::new(0, 0, 6, 3), &mut buffer, &mut state);
        // The header eats one row; the 2-row rows area scrolls to 3.
        assert_eq!(state.offset, 3);
        assert_eq!(buffer.to_string(), "head  \nr3    \nr4    ");
    }

    #[test]
    fn selection_scrolls_minimally_both_ways() {
        let rows: Vec<TableRow> = (0..4)
            .map(|index| TableRow::new([format!("r{index}")]))
            .collect();
        let mut buffer = Buffer::empty(Rect::new(0, 0, 6, 2));
        let mut state = TableState::default().with_selected(Some(0));
        state.offset = 2;
        render_at(
            Table::new(rows.clone(), [Constraint::Length(6)]),
            Rect::new(0, 0, 6, 2),
            &mut buffer,
            &mut state,
        );
        assert_eq!(state.offset, 0);
        assert_eq!(buffer.to_string(), "r0    \nr1    ");
    }

    #[test]
    fn highlight_symbol_gutter_aligns_columns() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 8, 2));
        let table = Table::new(
            [TableRow::new(["a", "b"]), TableRow::new(["c", "d"])],
            [Constraint::Length(3), Constraint::Length(3)],
        )
        .highlight_symbol("> ")
        .column_spacing(0);
        let mut state = TableState::default().with_selected(Some(1));
        render_at(table, Rect::new(0, 0, 8, 2), &mut buffer, &mut state);
        // Gutter of 2 shifts the whole column grid right; the symbol
        // marks only the selected row.
        assert_eq!(buffer.to_string(), "  a  b  \n> c  d  ");
    }

    #[test]
    fn highlight_style_wins_over_row_style() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 3, 2));
        let table = Table::new(
            [
                TableRow::new(["a"]).style(Style::new().fg(Color::Red)),
                TableRow::new(["b"]),
            ],
            [Constraint::Length(3)],
        )
        .highlight_style(Style::new().fg(Color::Green));
        let mut state = TableState::default();
        state.select(Some(0));
        render_at(table, Rect::new(0, 0, 3, 2), &mut buffer, &mut state);
        assert_eq!(buffer.get(0, 0).unwrap().style.fg, Some(Color::Green));
        // "b" carries no row style; the base table style is default.
        assert_eq!(buffer.get(0, 1).unwrap().style.fg, None);
    }

    #[test]
    fn header_style_layers_over_base() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 3, 2));
        let table = Table::new([TableRow::new(["a"])], [Constraint::Length(3)])
            .header(TableRow::new(["h"]))
            .header_style(Style::new().fg(Color::Blue));
        render_at(
            table,
            Rect::new(0, 0, 3, 2),
            &mut buffer,
            &mut TableState::default(),
        );
        assert_eq!(buffer.get(0, 0).unwrap().style.fg, Some(Color::Blue));
    }

    #[test]
    fn cells_support_alignment_within_columns() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 8, 2));
        let table = Table::new(
            [
                TableRow::new([
                    TableCell::new(Line::raw("l").alignment(Alignment::Left)),
                    TableCell::new(Line::raw("c").alignment(Alignment::Center)),
                ]),
                TableRow::new(["", ""]),
            ],
            [Constraint::Length(4), Constraint::Length(4)],
        )
        .column_spacing(0);
        render_at(
            table,
            Rect::new(0, 0, 8, 2),
            &mut buffer,
            &mut TableState::default(),
        );
        // Column 2 is [4..8): "c" centers at x = 4 + (4-1)/2 = 5.
        assert_eq!(buffer.to_string(), "l    c  \n        ");
    }

    #[test]
    fn multiline_cells_grow_the_row() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 6, 3));
        let table = Table::new(
            [TableRow::new([
                TableCell::new("one\ntwo"),
                TableCell::new("x"),
            ])],
            [Constraint::Length(3), Constraint::Length(3)],
        )
        .column_spacing(0);
        render_at(
            table,
            Rect::new(0, 0, 6, 3),
            &mut buffer,
            &mut TableState::default(),
        );
        assert_eq!(buffer.to_string(), "onex  \ntwo   \n      ");
    }

    #[test]
    fn out_of_range_selection_renders_without_highlight() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 3, 1));
        let table =
            Table::new([TableRow::new(["a"])], [Constraint::Length(3)]).highlight_symbol("> ");
        let mut state = TableState::default().with_selected(Some(7));
        render_at(table, Rect::new(0, 0, 3, 1), &mut buffer, &mut state);
        assert_eq!(buffer.to_string(), "a  ");
    }

    #[test]
    fn navigation_moves_selection() {
        let mut state = TableState::default();
        state.select_next();
        assert_eq!(state.selected(), Some(0));
        state.select_next();
        state.select_next();
        assert_eq!(state.selected(), Some(2));
        state.select_previous();
        assert_eq!(state.selected(), Some(1));
        state.select_first();
        assert_eq!(state.selected(), Some(0));
        state.select_last(4);
        assert_eq!(state.selected(), Some(3));
        state.select_last(0);
        assert_eq!(state.selected(), None);
        state.scroll_down_by(2);
        assert_eq!(state.offset, 2);
        state.scroll_up_by(9);
        assert_eq!(state.offset, 0);
    }

    #[test]
    fn block_wraps_and_content_fills_inner() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 8, 3));
        let table =
            Table::new([TableRow::new(["ab"])], [Constraint::Length(6)]).block(Block::bordered());
        render_at(
            table,
            Rect::new(0, 0, 8, 3),
            &mut buffer,
            &mut TableState::default(),
        );
        assert_eq!(buffer.to_string(), "┌──────┐\n│ab    │\n└──────┘");
    }

    #[test]
    fn table_renders_through_render_widget_contract() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 3, 1));
        let mut state = TableState::default();
        RenderWidget::render(
            Table::new([TableRow::new(["a"])], [Constraint::Length(3)]),
            Rect::new(0, 0, 3, 1),
            &mut buffer,
            &mut state,
        );
        assert_eq!(buffer.to_string(), "a  ");
    }
}
