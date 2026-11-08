//! `RadioGroup`: an exclusive-choice list, one row per option.
//!
//! The items and the selected index are the app's data; the
//! [`RadioGroup`] widget is the paint configuration (styles, marker
//! glyphs, block). The selected row carries
//! [`RadioGroup::selected_style`] patched over the base style — the same
//! one-highlight discipline as `List` — and an out-of-range selection
//! highlights nothing. One row per item, top row first; rows beyond the
//! area's height are not drawn.

use tuvix_core::buffer::Buffer;
use tuvix_core::geometry::Rect;
use tuvix_core::style::Style;
use tuvix_core::text::{Line, Span};
use tuvix_core::widget::Widget;

use crate::block::Block;

/// An exclusive-choice list: one marker-plus-label row per option.
///
/// # Examples
///
/// ```
/// use tuvix_core::buffer::Buffer;
/// use tuvix_core::geometry::Rect;
/// use tuvix_core::style::{Color, Style};
/// use tuvix_widgets::RadioGroup;
///
/// let mut buffer = Buffer::empty(Rect::new(0, 0, 14, 2));
/// RadioGroup::new(["rsa", "ed25519"])
///     .select(Some(1))
///     .selected_style(Style::new().fg(Color::Cyan))
///     .render(Rect::new(0, 0, 14, 2), &mut buffer);
/// assert_eq!(buffer.to_string(), "○ rsa         \n◉ ed25519     ");
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct RadioGroup {
    items: Vec<Line>,
    select: Option<usize>,
    style: Style,
    selected_style: Style,
    selected_symbol: Span,
    unselected_symbol: Span,
    block: Option<Block>,
}

impl Default for RadioGroup {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            select: None,
            style: Style::new(),
            selected_style: Style::new(),
            selected_symbol: Span::raw("◉"),
            unselected_symbol: Span::raw("○"),
            block: None,
        }
    }
}

impl RadioGroup {
    /// A group from anything that converts into [`Line`]s, nothing selected.
    pub fn new<T, C>(items: C) -> Self
    where
        T: Into<Line>,
        C: IntoIterator<Item = T>,
    {
        Self {
            items: items.into_iter().map(Into::into).collect(),
            ..Self::default()
        }
    }

    /// The selected option index; `None` selects nothing.
    #[must_use]
    pub fn select(mut self, index: Option<usize>) -> Self {
        self.select = index;
        self
    }

    /// The base style of every unselected row.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// The style of the selected row's marker and label.
    #[must_use]
    pub fn selected_style(mut self, style: Style) -> Self {
        self.selected_style = style;
        self
    }

    /// The marker rendered on the selected row (default `◉`).
    #[must_use]
    pub fn selected_symbol<T: Into<Span>>(mut self, symbol: T) -> Self {
        self.selected_symbol = symbol.into();
        self
    }

    /// The marker rendered on every unselected row (default `○`).
    #[must_use]
    pub fn unselected_symbol<T: Into<Span>>(mut self, symbol: T) -> Self {
        self.unselected_symbol = symbol.into();
        self
    }

    /// Wraps the group in a block; rows fill the block's inner area.
    #[must_use]
    pub fn block(mut self, block: Block) -> Self {
        self.block = Some(block);
        self
    }

    /// Maps the theme: the `text` slot styles the rows, the `highlight`
    /// slot the selected one.
    #[must_use]
    pub fn theme(mut self, theme: &Theme) -> Self {
        self.style = theme.text;
        self.selected_style = theme.highlight;
        self
    }

    /// Paints the group into `buffer`, clipped to `area`.
    ///
    /// Option `i` renders on the inner area's row `i` as
    /// `marker + ' ' + label`; the selected row takes
    /// [`RadioGroup::selected_style`] patched over the base style, and
    /// items past the area's height are not drawn.
    pub fn render(&self, area: Rect, buffer: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        if let Some(block) = &self.block {
            block.render(area, buffer);
        }
        let list_area = self.block.as_ref().map_or(area, |block| block.inner(area));
        if list_area.is_empty() {
            return;
        }
        let right = list_area.right();
        for (index, item) in self.items.iter().enumerate() {
            let row = list_area.y + u16::try_from(index).unwrap_or(u16::MAX);
            if row >= list_area.bottom() {
                break;
            }
            let selected = self.select == Some(index);
            let style = if selected {
                self.style.patch(self.selected_style)
            } else {
                self.style
            };
            let marker = if selected {
                &self.selected_symbol
            } else {
                &self.unselected_symbol
            };
            let mut x = list_area.x;
            x = buffer
                .set_stringn(
                    x,
                    row,
                    marker.content.as_ref(),
                    right - x,
                    style.patch(marker.style),
                )
                .0;
            if x >= right {
                continue;
            }
            x = buffer.set_stringn(x, row, " ", right - x, style).0;
            for span in &item.spans {
                if x >= right {
                    break;
                }
                x = buffer
                    .set_stringn(
                        x,
                        row,
                        span.content.as_ref(),
                        right - x,
                        style.patch(span.style),
                    )
                    .0;
            }
        }
    }
}

impl Widget for RadioGroup {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        RadioGroup::render(&self, area, buffer);
    }
}

use tuvix_core::theme::Theme;
#[cfg(test)]
mod tests {
    use super::*;
    use tuvix_core::style::Color;

    fn buffer_14x2() -> Buffer {
        Buffer::empty(Rect::new(0, 0, 14, 2))
    }

    #[test]
    fn renders_marker_space_label_per_row() {
        let mut buffer = buffer_14x2();
        RadioGroup::new(["rsa", "ed25519"]).render(Rect::new(0, 0, 14, 2), &mut buffer);
        assert_eq!(buffer.to_string(), "○ rsa         \n○ ed25519     ");
    }

    #[test]
    fn selection_highlights_marker_and_label() {
        let mut buffer = buffer_14x2();
        RadioGroup::new(["rsa", "ed25519"])
            .select(Some(1))
            .selected_style(Style::new().fg(Color::Cyan))
            .render(Rect::new(0, 0, 14, 2), &mut buffer);
        assert_eq!(buffer.get(0, 0).unwrap().style.fg, None);
        assert_eq!(buffer.get(0, 1).unwrap().style.fg, Some(Color::Cyan));
        assert_eq!(buffer.get(2, 1).unwrap().style.fg, Some(Color::Cyan));
    }

    #[test]
    fn none_selection_highlights_nothing() {
        let mut buffer = buffer_14x2();
        RadioGroup::new(["a", "b"])
            .select(None)
            .selected_style(Style::new().fg(Color::Cyan))
            .render(Rect::new(0, 0, 14, 2), &mut buffer);
        assert!(buffer.content.iter().all(|cell| cell.style.fg.is_none()));
    }

    #[test]
    fn out_of_range_selection_highlights_nothing() {
        let mut buffer = buffer_14x2();
        RadioGroup::new(["a", "b"])
            .select(Some(9))
            .selected_style(Style::new().fg(Color::Cyan))
            .render(Rect::new(0, 0, 14, 2), &mut buffer);
        assert!(buffer.content.iter().all(|cell| cell.style.fg.is_none()));
    }

    #[test]
    fn custom_symbols_render_verbatim() {
        let mut buffer = buffer_14x2();
        RadioGroup::new(["a", "b"])
            .select(Some(0))
            .selected_symbol("(x)")
            .unselected_symbol("( )")
            .render(Rect::new(0, 0, 14, 2), &mut buffer);
        assert_eq!(buffer.to_string(), "(x) a         \n( ) b         ");
    }

    #[test]
    fn base_style_applies_to_unselected_rows() {
        let mut buffer = buffer_14x2();
        RadioGroup::new(["a", "b"])
            .select(Some(1))
            .style(Style::new().fg(Color::Gray))
            .selected_style(Style::new().fg(Color::Cyan))
            .render(Rect::new(0, 0, 14, 2), &mut buffer);
        assert_eq!(buffer.get(0, 0).unwrap().style.fg, Some(Color::Gray));
        assert_eq!(buffer.get(0, 1).unwrap().style.fg, Some(Color::Cyan));
    }

    #[test]
    fn rows_past_the_area_height_are_not_drawn() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 3));
        RadioGroup::new(["a", "b", "c", "d"]).render(Rect::new(0, 0, 10, 3), &mut buffer);
        assert_eq!(buffer.to_string(), "○ a       \n○ b       \n○ c       ");
    }

    #[test]
    fn labels_clip_at_the_right_edge() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 5, 1));
        RadioGroup::new(["ed25519"]).render(Rect::new(0, 0, 5, 1), &mut buffer);
        assert_eq!(buffer.to_string(), "○ ed2");
    }

    #[test]
    fn block_wraps_and_fills_inner() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 4));
        RadioGroup::new(["a", "b"])
            .block(Block::bordered())
            .render(Rect::new(0, 0, 10, 4), &mut buffer);
        assert_eq!(
            buffer.to_string(),
            "┌────────┐\n│○ a     │\n│○ b     │\n└────────┘"
        );
    }

    #[test]
    fn theme_maps_text_and_highlight_slots() {
        let theme = Theme::dark();
        let mut buffer = buffer_14x2();
        RadioGroup::new(["a", "b"])
            .theme(&theme)
            .render(Rect::new(0, 0, 14, 2), &mut buffer);
        assert_eq!(buffer.get(0, 0).unwrap().style.fg, theme.text.fg);
        let mut buffer = buffer_14x2();
        RadioGroup::new(["a", "b"])
            .select(Some(1))
            .theme(&theme)
            .render(Rect::new(0, 0, 14, 2), &mut buffer);
        assert_eq!(buffer.get(0, 1).unwrap().style, theme.highlight);
    }

    #[test]
    fn renders_through_the_widget_trait() {
        let mut buffer = buffer_14x2();
        Widget::render(RadioGroup::new(["a"]), Rect::new(0, 0, 14, 2), &mut buffer);
        assert_eq!(buffer.to_string(), "○ a           \n              ");
    }

    #[test]
    fn empty_area_is_a_noop() {
        let mut buffer = buffer_14x2();
        RadioGroup::new(["a"]).render(Rect::ZERO, &mut buffer);
        assert!(
            buffer
                .content
                .iter()
                .all(tuvix_core::buffer::Cell::is_empty)
        );
    }
}
