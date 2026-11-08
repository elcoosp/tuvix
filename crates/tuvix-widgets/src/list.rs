//! Lists: selectable items with scroll-to-selection state.
//!
//! A [`List`] is a [`RenderWidget`] with `State = ListState`: render it
//! with `RenderWidget::render(list, area, buf, &mut state)` or the
//! method-call form `list.render(area, buf, &mut state)`.

use std::borrow::Cow;

use unicode_width::UnicodeWidthStr as _;

use tuvix_core::buffer::Buffer;
use tuvix_core::geometry::Rect;
use tuvix_core::style::Style;
use tuvix_core::text::{Line, Text};
use tuvix_core::widget::RenderWidget;

use crate::block::Block;

/// One row-group of a [`List`]: a [`Text`] plus an item style.
///
/// # Examples
///
/// ```
/// use tuvix_core::style::{Color, Style};
/// use tuvix_core::text::Line;
/// use tuvix_widgets::ListItem;
///
/// let item = ListItem::new("plain");
/// let styled = ListItem::new(Line::styled("hi", Style::new().fg(Color::Red)));
/// assert_eq!(item.height(), 1);
/// assert_eq!(styled.height(), 1);
/// ```
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ListItem {
    /// The text of the item (multi-line items span several rows).
    pub text: Text,
    /// Style layered under the item's lines and spans.
    pub style: Style,
}

impl ListItem {
    /// Creates an item from anything [`Text`]-convertible.
    pub fn new<T: Into<Text>>(text: T) -> Self {
        Self {
            text: text.into(),
            style: Style::new(),
        }
    }

    /// Sets the style layered under the item's lines and spans.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Number of display rows this item occupies.
    #[must_use]
    pub fn height(&self) -> usize {
        self.text.height()
    }
}

impl From<&'static str> for ListItem {
    fn from(text: &'static str) -> Self {
        Self::new(text)
    }
}

impl From<String> for ListItem {
    fn from(text: String) -> Self {
        Self::new(text)
    }
}

impl From<Line> for ListItem {
    fn from(line: Line) -> Self {
        Self::new(line)
    }
}

impl From<Text> for ListItem {
    fn from(text: Text) -> Self {
        Self::new(text)
    }
}

/// The scroll offset and selection of a [`List`].
///
/// `offset` counts display rows (multi-line items occupy several). The
/// renderer scrolls `offset` minimally so the selected item stays
/// visible; navigation helpers move `selected` without knowing the item
/// count — the app clamps against its own data.
///
/// # Examples
///
/// ```
/// use tuvix_widgets::ListState;
///
/// let mut state = ListState::default().with_selected(Some(3));
/// state.select_next();
/// assert_eq!(state.selected(), Some(4));
/// state.select_previous();
/// state.select_previous();
/// assert_eq!(state.selected(), Some(2));
/// state.select_first();
/// assert_eq!(state.selected(), Some(0));
/// ```
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ListState {
    /// Display rows scrolled off the top.
    pub offset: usize,
    /// The selected item index, if any.
    selected: Option<usize>,
}

impl ListState {
    /// Creates a state with the given initial selection.
    #[must_use]
    pub fn with_selected(mut self, selected: Option<usize>) -> Self {
        self.selected = selected;
        self
    }

    /// The selected item index, if any.
    #[must_use]
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// Sets the selected item index (raw; out-of-range values render
    /// without a highlight).
    pub fn select(&mut self, index: Option<usize>) {
        self.selected = index;
    }

    /// Selects the next item; no selection selects the first.
    pub fn select_next(&mut self) {
        self.selected = Some(self.selected.map_or(0, |index| index + 1));
    }

    /// Selects the previous item (saturating at the first); no selection
    /// selects the first.
    pub fn select_previous(&mut self) {
        self.selected = Some(self.selected.map_or(0, |index| index.saturating_sub(1)));
    }

    /// Selects the first item.
    pub fn select_first(&mut self) {
        self.selected = Some(0);
    }

    /// Selects the last item; the caller supplies the count so no
    /// sentinel value is involved.
    pub fn select_last(&mut self, item_count: usize) {
        self.selected = item_count.checked_sub(1);
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

/// A selectable list of items.
///
/// # Examples
///
/// ```
/// use tuvix_core::buffer::Buffer;
/// use tuvix_core::geometry::Rect;
/// use tuvix_core::widget::RenderWidget;
/// use tuvix_widgets::{List, ListState};
///
/// let mut buffer = Buffer::empty(Rect::new(0, 0, 8, 3));
/// let list = List::new(["alpha", "beta", "gamma"])
///     .highlight_symbol("> ");
/// let mut state = ListState::default().with_selected(Some(1));
/// list.render(Rect::new(0, 0, 8, 3), &mut buffer, &mut state);
/// assert_eq!(buffer.to_string(), "  alpha \n> beta  \n  gamma ");
/// ```
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct List {
    items: Vec<ListItem>,
    block: Option<Block>,
    style: Style,
    highlight_style: Style,
    highlight_symbol: Option<Cow<'static, str>>,
}

impl List {
    /// Creates a list from an iterator of [`ListItem`]-convertibles.
    pub fn new<T, I>(items: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<ListItem>,
    {
        Self {
            items: items.into_iter().map(Into::into).collect(),
            block: None,
            style: Style::new(),
            highlight_style: Style::new(),
            highlight_symbol: None,
        }
    }

    /// Wraps the list in a block; items fill the block's inner area.
    #[must_use]
    pub fn block(mut self, block: Block) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the base style of the list area.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the style patched over the selected item's rows.
    #[must_use]
    pub fn highlight_style(mut self, style: Style) -> Self {
        self.highlight_style = style;
        self
    }

    /// Sets the marker written in front of the selected item (and spaces
    /// of the same width in front of every other row, so columns align).
    #[must_use]
    pub fn highlight_symbol(mut self, symbol: impl Into<Cow<'static, str>>) -> Self {
        self.highlight_symbol = Some(symbol.into());
        self
    }

    /// Paints the list into `buffer`, clipped to `area`, updating `state`.
    ///
    /// Called through [`RenderWidget::render`]; the method-call form is
    /// `list.render(area, buffer, &mut state)`.
    fn paint(self, area: Rect, buffer: &mut Buffer, state: &mut ListState) {
        if let Some(block) = &self.block {
            block.render(area, buffer);
        }
        let list_area = self.block.as_ref().map_or(area, |block| block.inner(area));
        if list_area.is_empty() {
            return;
        }
        buffer.set_style(list_area, self.style);

        let height = usize::from(list_area.height);
        let heights: Vec<usize> = self.items.iter().map(ListItem::height).collect();
        // Cumulative display row of each item's first line.
        let mut starts = Vec::with_capacity(self.items.len());
        let mut running = 0_usize;
        for item_height in &heights {
            starts.push(running);
            running += *item_height;
        }

        // Minimal scroll so the selection stays visible.
        if let Some(selected) = state
            .selected
            .filter(|selected| *selected < self.items.len())
        {
            let selected_top = starts[selected];
            let selected_bottom = selected_top + heights[selected];
            if selected_top < state.offset {
                state.offset = selected_top;
            } else if selected_bottom > state.offset + height {
                state.offset = selected_bottom - height;
            }
        }

        let gutter_width = if state
            .selected
            .is_some_and(|selected| selected < self.items.len())
        {
            self.highlight_symbol.as_ref().map_or(0, |symbol| {
                u16::try_from(symbol.width()).unwrap_or(u16::MAX)
            })
        } else {
            0
        };

        let mut row = 0_usize;
        for (index, item) in self.items.iter().enumerate() {
            let item_top = starts[index];
            let item_bottom = item_top + heights[index];
            if item_bottom <= state.offset || row >= height {
                continue;
            }
            let is_selected = state.selected == Some(index);
            for (line_index, line) in item.text.lines.iter().enumerate() {
                let display_row = item_top + line_index;
                if display_row < state.offset {
                    continue;
                }
                if row >= height {
                    break;
                }
                let y = list_area.y + u16::try_from(row).unwrap_or(u16::MAX);
                let mut x = list_area.x;

                if gutter_width > 0 {
                    let symbol = if is_selected {
                        self.highlight_symbol.clone().unwrap_or_default()
                    } else {
                        Cow::Borrowed(" ")
                    };
                    let symbol_style = if is_selected {
                        self.style
                            .patch(item.style)
                            .patch(line.style)
                            .patch(self.highlight_style)
                    } else {
                        self.style.patch(item.style).patch(line.style)
                    };
                    let (end_x, _) =
                        buffer.set_stringn(x, y, symbol.as_ref(), gutter_width, symbol_style);
                    x = end_x.max(x + gutter_width.min(list_area.right() - x));
                }

                let base = if is_selected {
                    self.style.patch(item.style).patch(self.highlight_style)
                } else {
                    self.style.patch(item.style)
                };
                let line_style = base.patch(line.style);
                let remaining = list_area.right().saturating_sub(x);
                for span in &line.spans {
                    if remaining == 0 || x >= list_area.right() {
                        break;
                    }
                    let span_style = line_style.patch(span.style);
                    let (end_x, _) = buffer.set_stringn(
                        x,
                        y,
                        span.content.as_ref(),
                        remaining.min(list_area.right() - x),
                        span_style,
                    );
                    x = end_x;
                }
                row += 1;
            }
        }
    }
}

impl RenderWidget for List {
    type State = ListState;

    fn render(self, area: Rect, buffer: &mut Buffer, state: &mut Self::State) {
        List::paint(self, area, buffer, state);
    }
}

impl List {
    /// Maps the theme onto the list: the `highlight` slot styles the
    /// selected row.
    #[must_use]
    pub fn theme(mut self, theme: &Theme) -> Self {
        self.highlight_style = theme.highlight;
        self
    }
}

use tuvix_core::theme::Theme;
#[cfg(test)]
mod tests {
    use super::*;
    use tuvix_core::style::Color;

    fn render_at(list: List, area: Rect, buffer: &mut Buffer, state: &mut ListState) {
        RenderWidget::render(list, area, buffer, state);
    }

    #[test]
    fn renders_items_without_selection() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 8, 3));
        render_at(
            List::new(["alpha", "beta", "gamma"]),
            Rect::new(0, 0, 8, 3),
            &mut buffer,
            &mut ListState::default(),
        );
        assert_eq!(buffer.to_string(), "alpha   \nbeta    \ngamma   ");
    }

    #[test]
    fn highlight_symbol_gutter_aligns_columns() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 8, 3));
        let mut state = ListState::default().with_selected(Some(1));
        render_at(
            List::new(["alpha", "beta", "gamma"]).highlight_symbol("> "),
            Rect::new(0, 0, 8, 3),
            &mut buffer,
            &mut state,
        );
        assert_eq!(buffer.to_string(), "  alpha \n> beta  \n  gamma ");
    }

    #[test]
    fn no_selection_means_no_gutter() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 8, 2));
        render_at(
            List::new(["alpha", "beta"]).highlight_symbol("> "),
            Rect::new(0, 0, 8, 2),
            &mut buffer,
            &mut ListState::default(),
        );
        assert_eq!(buffer.to_string(), "alpha   \nbeta    ");
    }

    #[test]
    fn selection_scrolls_down_minimally() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 4, 2));
        let mut state = ListState::default().with_selected(Some(3));
        render_at(
            List::new(["a", "b", "c", "d"]),
            Rect::new(0, 0, 4, 2),
            &mut buffer,
            &mut state,
        );
        // Selection 3 with a 2-row viewport scrolls to offset 2.
        assert_eq!(state.offset, 2);
        assert_eq!(buffer.to_string(), "c   \nd   ");
    }

    #[test]
    fn selection_scrolls_up_minimally() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 4, 2));
        let mut state = ListState::default().with_selected(Some(3));
        state.offset = 2;
        render_at(
            List::new(["a", "b", "c", "d"]),
            Rect::new(0, 0, 4, 2),
            &mut buffer,
            &mut state,
        );
        assert_eq!(state.offset, 2);
        let mut state2 = ListState::default().with_selected(Some(0));
        state2.offset = 2;
        let mut buffer2 = Buffer::empty(Rect::new(0, 0, 4, 2));
        render_at(
            List::new(["a", "b", "c", "d"]),
            Rect::new(0, 0, 4, 2),
            &mut buffer2,
            &mut state2,
        );
        assert_eq!(state2.offset, 0);
        assert_eq!(buffer2.to_string(), "a   \nb   ");
    }

    #[test]
    fn offset_renders_middle_of_the_list() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 4, 2));
        let mut state = ListState {
            offset: 1,
            ..ListState::default()
        };
        render_at(
            List::new(["a", "b", "c", "d"]),
            Rect::new(0, 0, 4, 2),
            &mut buffer,
            &mut state,
        );
        assert_eq!(buffer.to_string(), "b   \nc   ");
    }

    #[test]
    fn out_of_range_selection_renders_without_highlight() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 4, 2));
        let mut state = ListState::default().with_selected(Some(9));
        render_at(
            List::new(["a", "b"]).highlight_symbol("> "),
            Rect::new(0, 0, 4, 2),
            &mut buffer,
            &mut state,
        );
        // No gutter: the selection is not rendered.
        assert_eq!(buffer.to_string(), "a   \nb   ");
    }

    #[test]
    fn highlight_style_wins_over_item_style() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 4, 2));
        let mut state = ListState::default().with_selected(Some(0));
        render_at(
            List::new([
                ListItem::new("a").style(Style::new().fg(Color::Red)),
                ListItem::new("b"),
            ])
            .highlight_style(Style::new().fg(Color::Green)),
            Rect::new(0, 0, 4, 2),
            &mut buffer,
            &mut state,
        );
        assert_eq!(buffer.get(0, 0).unwrap().style.fg, Some(Color::Green));
        // "b" carries no item style; the base list style is default.
        assert_eq!(buffer.get(0, 1).unwrap().style.fg, None);
    }

    #[test]
    fn multiline_items_occupy_several_rows() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 6, 4));
        let mut state = ListState::default().with_selected(Some(0));
        render_at(
            List::new([ListItem::new("one\ntwo"), ListItem::new("three")]).highlight_symbol("* "),
            Rect::new(0, 0, 6, 4),
            &mut buffer,
            &mut state,
        );
        // The highlight symbol marks every row of the selected item;
        // "three" is clipped to the 4 columns left after the gutter.
        assert_eq!(buffer.to_string(), "* one \n* two \n  thre\n      ");
    }

    #[test]
    fn navigation_moves_selection() {
        let mut state = ListState::default();
        state.select_next();
        assert_eq!(state.selected(), Some(0));
        state.select_next();
        state.select_next();
        assert_eq!(state.selected(), Some(2));
        state.select_previous();
        assert_eq!(state.selected(), Some(1));
        state.select_previous();
        state.select_previous();
        assert_eq!(state.selected(), Some(0));
        state.select_previous();
        assert_eq!(state.selected(), Some(0));
        state.select_last(4);
        assert_eq!(state.selected(), Some(3));
        state.select_last(0);
        assert_eq!(state.selected(), None);
        state.select(None);
        assert_eq!(state.selected(), None);
        state.select_next();
        assert_eq!(state.selected(), Some(0));
    }

    #[test]
    fn scroll_by_moves_offset() {
        let mut state = ListState::default();
        state.scroll_down_by(3);
        assert_eq!(state.offset, 3);
        state.scroll_up_by(5);
        assert_eq!(state.offset, 0);
    }

    #[test]
    fn block_wraps_and_items_fill_inner() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 8, 3));
        render_at(
            List::new(["alpha"]).block(Block::bordered()),
            Rect::new(0, 0, 8, 3),
            &mut buffer,
            &mut ListState::default(),
        );
        assert_eq!(buffer.to_string(), "┌──────┐\n│alpha │\n└──────┘");
    }

    #[test]
    fn list_renders_through_the_render_widget_contract() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 4, 2));
        let mut state = ListState::default().with_selected(Some(1));
        // The unified contract: List is a RenderWidget with ListState.
        RenderWidget::render(
            List::new(["a", "b"]),
            Rect::new(0, 0, 4, 2),
            &mut buffer,
            &mut state,
        );
        assert_eq!(buffer.to_string(), "a   \nb   ");
    }
}
