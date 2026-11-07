//! Checkbox: a single-row boolean control with a state-bearing marker.
//!
//! The label and the checked flag are the app's data; the [`Checkbox`]
//! widget is the paint configuration (styles, marker glyphs, block). The
//! marker glyph carries the state (`☑` checked, `☐` unchecked by
//! default), and a checked row takes [`Checkbox::checked_style`] patched
//! over the base style. Like `Tabs` and `LineGauge` this is a one-row
//! widget: a tall area only uses its first row.

use tuvix_core::buffer::Buffer;
use tuvix_core::geometry::Rect;
use tuvix_core::style::Style;
use tuvix_core::text::{Line, Span};
use tuvix_core::widget::Widget;

use crate::block::Block;

/// A single-row boolean control: marker glyph plus label.
///
/// # Examples
///
/// ```
/// use tuvix_core::buffer::Buffer;
/// use tuvix_core::geometry::Rect;
/// use tuvix_core::style::{Color, Modifier, Style};
/// use tuvix_widgets::Checkbox;
///
/// let mut buffer = Buffer::empty(Rect::new(0, 0, 12, 1));
/// Checkbox::new("notifications")
///     .checked(true)
///     .checked_style(Style::new().fg(Color::Cyan))
///     .render(Rect::new(0, 0, 12, 1), &mut buffer);
/// assert_eq!(buffer.to_string(), "☑ notificati");
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Checkbox {
    label: Line,
    checked: bool,
    style: Style,
    checked_style: Style,
    checked_symbol: Span,
    unchecked_symbol: Span,
    block: Option<Block>,
}

impl Default for Checkbox {
    fn default() -> Self {
        Self {
            label: Line::default(),
            checked: false,
            style: Style::new(),
            checked_style: Style::new(),
            checked_symbol: Span::raw("☑"),
            unchecked_symbol: Span::raw("☐"),
            block: None,
        }
    }
}

impl Checkbox {
    /// A checkbox with the given label, initially unchecked.
    pub fn new<T: Into<Line>>(label: T) -> Self {
        Self {
            label: label.into(),
            ..Self::default()
        }
    }

    /// The checked flag the app owns; `true` renders the checked marker.
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    /// The base style of the marker and the label.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Patched over the base style while the box is checked.
    pub fn checked_style(mut self, style: Style) -> Self {
        self.checked_style = style;
        self
    }

    /// The marker rendered while checked (default `☑`).
    pub fn checked_symbol<T: Into<Span>>(mut self, symbol: T) -> Self {
        self.checked_symbol = symbol.into();
        self
    }

    /// The marker rendered while unchecked (default `☐`).
    pub fn unchecked_symbol<T: Into<Span>>(mut self, symbol: T) -> Self {
        self.unchecked_symbol = symbol.into();
        self
    }

    /// Wraps the checkbox in a block; the row fills the block's inner area.
    pub fn block(mut self, block: Block) -> Self {
        self.block = Some(block);
        self
    }

    /// Maps the theme: the `text` slot styles the row, the `accent` slot
    /// patches it while checked.
    pub fn theme(mut self, theme: &Theme) -> Self {
        self.style = theme.text;
        self.checked_style = theme.accent;
        self
    }

    /// Paints the checkbox into `buffer`, clipped to `area`.
    ///
    /// The row is `marker + ' ' + label` on the area's first row; while
    /// checked, [`Checkbox::checked_style`] patches every cell of the
    /// row. The label clips at the area's right edge.
    pub fn render(&self, area: Rect, buffer: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        if let Some(block) = &self.block {
            block.render(area, buffer);
        }
        let row_area = self.block.as_ref().map_or(area, |block| block.inner(area));
        if row_area.is_empty() {
            return;
        }
        let row = row_area.y;
        let right = row_area.right();
        let style = if self.checked {
            self.style.patch(self.checked_style)
        } else {
            self.style
        };

        let marker = Span {
            content: self.marker_symbol().content.clone(),
            style: style.patch(self.marker_symbol().style),
        };
        let mut x = row_area.x;
        x = buffer
            .set_stringn(x, row, marker.content.as_ref(), right - x, marker.style)
            .0;
        if x >= right {
            return;
        }
        x = buffer.set_stringn(x, row, " ", right - x, style).0;
        for span in &self.label.spans {
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

impl Checkbox {
    /// The marker glyph for the current state.
    fn marker_symbol(&self) -> &Span {
        if self.checked {
            &self.checked_symbol
        } else {
            &self.unchecked_symbol
        }
    }
}

impl Widget for Checkbox {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        Checkbox::render(&self, area, buffer);
    }
}

use tuvix_core::theme::Theme;
#[cfg(test)]
mod tests {
    use super::*;
    use tuvix_core::style::{Color, Modifier};
    use tuvix_core::stylize::Stylize as _;

    fn buffer_12x1() -> Buffer {
        Buffer::empty(Rect::new(0, 0, 12, 1))
    }

    #[test]
    fn renders_marker_space_label_unchecked() {
        let mut buffer = buffer_12x1();
        Checkbox::new("sail").render(Rect::new(0, 0, 12, 1), &mut buffer);
        assert_eq!(buffer.to_string(), "☐ sail      ");
    }

    #[test]
    fn checked_swaps_the_marker() {
        let mut buffer = buffer_12x1();
        Checkbox::new("sail")
            .checked(true)
            .render(Rect::new(0, 0, 12, 1), &mut buffer);
        assert_eq!(buffer.to_string(), "☑ sail      ");
    }

    #[test]
    fn checked_style_patches_marker_and_label() {
        let mut buffer = buffer_12x1();
        Checkbox::new("ab")
            .checked(true)
            .style(Style::new().fg(Color::Gray))
            .checked_style(Style::new().fg(Color::Cyan).bold())
            .render(Rect::new(0, 0, 12, 1), &mut buffer);
        for x in [0_u16, 2, 3] {
            assert_eq!(
                buffer.get(x, 0).unwrap().style.fg,
                Some(Color::Cyan),
                "checked cells take the checked style at x={x}"
            );
        }
        assert_eq!(buffer.get(0, 0).unwrap().style.add_modifier, Modifier::BOLD);
    }

    #[test]
    fn unchecked_keeps_the_base_style() {
        let mut buffer = buffer_12x1();
        Checkbox::new("ab")
            .style(Style::new().fg(Color::Gray))
            .checked_style(Style::new().fg(Color::Cyan))
            .render(Rect::new(0, 0, 12, 1), &mut buffer);
        for x in [0_u16, 2] {
            assert_eq!(buffer.get(x, 0).unwrap().style.fg, Some(Color::Gray));
        }
    }

    #[test]
    fn custom_symbols_render_verbatim() {
        let mut buffer = buffer_12x1();
        Checkbox::new("x")
            .checked_symbol("[x]")
            .unchecked_symbol("[ ]")
            .render(Rect::new(0, 0, 12, 1), &mut buffer);
        assert_eq!(buffer.to_string(), "[ ] x       ");
        let mut buffer = buffer_12x1();
        Checkbox::new("x")
            .checked(true)
            .checked_symbol("[x]")
            .unchecked_symbol("[ ]")
            .render(Rect::new(0, 0, 12, 1), &mut buffer);
        assert_eq!(buffer.to_string(), "[x] x       ");
    }

    #[test]
    fn label_clips_at_the_right_edge() {
        let mut buffer = buffer_12x1();
        Checkbox::new("notifications").render(Rect::new(0, 0, 12, 1), &mut buffer);
        assert_eq!(buffer.to_string(), "☐ notificati");
    }

    #[test]
    fn block_wraps_and_fills_inner() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 3));
        Checkbox::new("on")
            .checked(true)
            .block(Block::bordered())
            .render(Rect::new(0, 0, 10, 3), &mut buffer);
        assert_eq!(buffer.to_string(), "┌────────┐\n│☑ on    │\n└────────┘");
    }

    #[test]
    fn theme_maps_text_and_accent_slots() {
        let theme = Theme::dark();
        let mut buffer = buffer_12x1();
        Checkbox::new("ab")
            .theme(&theme)
            .render(Rect::new(0, 0, 12, 1), &mut buffer);
        assert_eq!(buffer.get(2, 0).unwrap().style.fg, theme.text.fg);
        let mut buffer = buffer_12x1();
        Checkbox::new("ab")
            .checked(true)
            .theme(&theme)
            .render(Rect::new(0, 0, 12, 1), &mut buffer);
        assert_eq!(buffer.get(2, 0).unwrap().style.fg, theme.accent.fg);
    }

    #[test]
    fn tall_area_uses_only_the_first_row() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 8, 3));
        Checkbox::new("hi").render(Rect::new(0, 0, 8, 3), &mut buffer);
        assert_eq!(buffer.to_string(), "☐ hi    \n        \n        ");
    }

    #[test]
    fn renders_through_the_widget_trait() {
        let mut buffer = buffer_12x1();
        Widget::render(Checkbox::new("a"), Rect::new(0, 0, 12, 1), &mut buffer);
        assert_eq!(buffer.to_string(), "☐ a         ");
    }

    #[test]
    fn empty_area_is_a_noop() {
        let mut buffer = buffer_12x1();
        Checkbox::new("a").render(Rect::ZERO, &mut buffer);
        assert!(
            buffer
                .content
                .iter()
                .all(tuvix_core::buffer::Cell::is_empty)
        );
    }

    #[test]
    fn zero_width_area_is_a_noop() {
        let mut buffer = buffer_12x1();
        Checkbox::new("a").render(Rect::new(3, 0, 0, 1), &mut buffer);
        assert!(
            buffer
                .content
                .iter()
                .all(tuvix_core::buffer::Cell::is_empty)
        );
    }
}
