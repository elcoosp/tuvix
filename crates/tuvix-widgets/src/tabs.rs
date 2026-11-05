//! Tabs: a horizontal row of titled pages with one highlighted selection.
//!
//! The titles render left to right, separated by [`Tabs::divider`]
//! (default `│`), each padded by [`Tabs::padding`] (one space each side).
//! The selected title carries [`Tabs::highlight_style`]. Everything clips
//! at the area's right edge — tabs that do not fit are simply not drawn,
//! the layout stays stable.

use tuvix_core::buffer::Buffer;
use tuvix_core::geometry::Rect;
use tuvix_core::style::Style;
use tuvix_core::text::{Line, Span};
use tuvix_core::widget::Widget;

use crate::block::Block;

/// A horizontal row of titles with one highlighted selection.
///
/// # Examples
///
/// ```
/// use tuvix_core::buffer::Buffer;
/// use tuvix_core::geometry::Rect;
/// use tuvix_core::style::{Modifier, Style};
/// use tuvix_widgets::Tabs;
///
/// let mut buffer = Buffer::empty(Rect::new(0, 0, 22, 1));
/// Tabs::new(["Server", "Logs"])
///     .select(1)
///     .highlight_style(Style::new().add_modifier(Modifier::REVERSED))
///     .render(Rect::new(0, 0, 22, 1), &mut buffer);
/// assert_eq!(buffer.to_string(), " Server │ Logs        ");
/// ```
///
/// The selected `Logs` carries the reversed-video highlight in its three
/// cells plus the flanking padding spaces; the assertion only pins the
/// plain text layout.
#[derive(Debug, Clone, PartialEq)]
pub struct Tabs {
    titles: Vec<Line>,
    select: usize,
    style: Style,
    highlight_style: Style,
    divider: Span,
    padding_left: String,
    padding_right: String,
    block: Option<Block>,
}

impl Default for Tabs {
    fn default() -> Self {
        Self {
            titles: Vec::new(),
            select: 0,
            style: Style::new(),
            highlight_style: Style::new(),
            divider: Span::raw("│"),
            padding_left: " ".to_owned(),
            padding_right: " ".to_owned(),
            block: None,
        }
    }
}

impl Tabs {
    /// Tabs from anything that converts into [`Line`]s.
    pub fn new<T, C>(titles: C) -> Self
    where
        T: Into<Line>,
        C: IntoIterator<Item = T>,
    {
        Self {
            titles: titles.into_iter().map(Into::into).collect(),
            ..Self::default()
        }
    }

    /// The selected tab index (out-of-range selects nothing).
    pub fn select(mut self, index: usize) -> Self {
        self.select = index;
        self
    }

    /// The base style of every unselected title.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// The style of the selected title (patched over the base style).
    pub fn highlight_style(mut self, style: Style) -> Self {
        self.highlight_style = style;
        self
    }

    /// The separator drawn between adjacent titles (default `│`).
    pub fn divider<T: Into<Span>>(mut self, divider: T) -> Self {
        self.divider = divider.into();
        self
    }

    /// The padding strings flanking every title (default one space).
    pub fn padding<T: Into<String>>(mut self, left: T, right: T) -> Self {
        self.padding_left = left.into();
        self.padding_right = right.into();
        self
    }

    /// Wraps the tabs in a block; the row fills the block's inner area.
    pub fn block(mut self, block: Block) -> Self {
        self.block = Some(block);
        self
    }

    /// Maps the theme: the `text` slot styles unselected titles, the
    /// `highlight` slot the selected one.
    pub fn theme(mut self, theme: &Theme) -> Self {
        self.style = theme.text;
        self.highlight_style = theme.highlight;
        self
    }

    /// Paints the tab row into `buffer`, clipped to `area`.
    ///
    /// Each title renders as `padding + spans + padding`; the selected
    /// title's cells take `highlight_style` patched over the span styles.
    /// The divider sits between adjacent titles, in the base style.
    pub fn render(&self, area: Rect, buffer: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        if let Some(block) = &self.block {
            block.render(area, buffer);
        }
        let tabs_area = self.block.as_ref().map_or(area, |block| block.inner(area));
        if tabs_area.is_empty() {
            return;
        }
        let row = tabs_area.y;
        let right_limit = tabs_area.right();
        let mut x = tabs_area.x;

        for (index, title) in self.titles.iter().enumerate() {
            // The divider sits before every title except the first.
            if index > 0 {
                if x >= right_limit {
                    break;
                }
                let divider = Span {
                    content: self.divider.content.clone(),
                    style: self.style.patch(self.divider.style),
                };
                x = buffer
                    .set_stringn(
                        x,
                        row,
                        divider.content.as_ref(),
                        right_limit - x,
                        divider.style,
                    )
                    .0;
            }

            let selected = index == self.select;
            let padded_style = if selected {
                self.style.patch(self.highlight_style)
            } else {
                self.style
            };
            let mut spans: Vec<Span> = Vec::with_capacity(title.spans.len() + 2);
            spans.push(Span {
                content: self.padding_left.clone().into(),
                style: padded_style,
            });
            for span in &title.spans {
                spans.push(Span {
                    content: span.content.clone(),
                    style: padded_style.patch(span.style),
                });
            }
            spans.push(Span {
                content: self.padding_right.clone().into(),
                style: padded_style,
            });

            for span in spans {
                if x >= right_limit {
                    break;
                }
                x = buffer
                    .set_stringn(x, row, span.content.as_ref(), right_limit - x, span.style)
                    .0;
            }
        }
    }
}

impl Widget for Tabs {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        Tabs::render(&self, area, buffer);
    }
}

use tuvix_core::theme::Theme;
#[cfg(test)]
mod tests {
    use super::*;
    use tuvix_core::style::Color;

    fn buffer_22x1() -> Buffer {
        Buffer::empty(Rect::new(0, 0, 22, 1))
    }

    #[test]
    fn renders_titles_with_default_padding_and_divider() {
        let mut buffer = buffer_22x1();
        Tabs::new(["Server", "Logs"]).render(Rect::new(0, 0, 22, 1), &mut buffer);
        assert_eq!(buffer.to_string(), " Server │ Logs        ");
    }

    #[test]
    fn selection_highlights_title_and_padding() {
        let mut buffer = buffer_22x1();
        Tabs::new(["Server", "Logs"])
            .select(1)
            .highlight_style(Style::new().fg(Color::Red))
            .render(Rect::new(0, 0, 22, 1), &mut buffer);
        // The unselected title keeps the default style, and so does the
        // divider between the two titles.
        assert_eq!(buffer.get(1, 0).unwrap().style.fg, None);
        assert_eq!(buffer.get(8, 0).unwrap().style.fg, None);
        // The selected title and its padding carry the highlight.
        assert_eq!(buffer.get(9, 0).unwrap().style.fg, Some(Color::Red));
        assert_eq!(buffer.get(10, 0).unwrap().style.fg, Some(Color::Red));
        assert_eq!(buffer.get(14, 0).unwrap().style.fg, Some(Color::Red));
    }

    #[test]
    fn out_of_range_selection_highlights_nothing() {
        let mut buffer = buffer_22x1();
        Tabs::new(["Server", "Logs"])
            .select(9)
            .highlight_style(Style::new().fg(Color::Red))
            .render(Rect::new(0, 0, 22, 1), &mut buffer);
        assert!(buffer.content.iter().all(|cell| cell.style.fg.is_none()));
    }

    #[test]
    fn base_style_applies_to_unselected_titles_and_divider() {
        let mut buffer = buffer_22x1();
        Tabs::new(["Server", "Logs"])
            .style(Style::new().fg(Color::Blue))
            .render(Rect::new(0, 0, 22, 1), &mut buffer);
        assert_eq!(buffer.get(1, 0).unwrap().style.fg, Some(Color::Blue));
        // The divider inherits the base style too.
        assert_eq!(buffer.get(8, 0).unwrap().style.fg, Some(Color::Blue));
    }

    #[test]
    fn custom_divider_and_padding() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 16, 1));
        Tabs::new(["A", "B"])
            .divider(Span::raw("-"))
            .padding("", "")
            .render(Rect::new(0, 0, 16, 1), &mut buffer);
        assert_eq!(buffer.to_string(), "A-B             ");
    }

    #[test]
    fn titles_clip_at_the_right_edge() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 6, 1));
        Tabs::new(["Server", "Logs"]).render(Rect::new(0, 0, 6, 1), &mut buffer);
        assert_eq!(buffer.to_string(), " Serve");
    }

    #[test]
    fn block_wraps_and_fills_inner() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 12, 3));
        Tabs::new(["A", "B"])
            .block(Block::bordered())
            .render(Rect::new(0, 0, 12, 3), &mut buffer);
        assert_eq!(
            buffer.to_string(),
            "┌──────────┐\n│ A │ B    │\n└──────────┘"
        );
    }

    #[test]
    fn styled_spans_keep_their_own_style() {
        let mut buffer = buffer_22x1();
        Tabs::new([Line::from_spans(vec![
            Span::raw("Ser"),
            Span::styled("ver", Style::new().fg(Color::Green)),
        ])])
        .render(Rect::new(0, 0, 22, 1), &mut buffer);
        assert_eq!(buffer.get(1, 0).unwrap().style.fg, None);
        assert_eq!(buffer.get(4, 0).unwrap().style.fg, Some(Color::Green));
    }

    #[test]
    fn renders_through_the_widget_trait() {
        let mut buffer = buffer_22x1();
        Widget::render(Tabs::new(["A"]), Rect::new(0, 0, 22, 1), &mut buffer);
        assert_eq!(buffer.to_string(), " A                    ");
    }

    #[test]
    fn empty_area_is_a_noop() {
        let mut buffer = buffer_22x1();
        Tabs::new(["A"]).render(Rect::ZERO, &mut buffer);
        assert!(
            buffer
                .content
                .iter()
                .all(tuvix_core::buffer::Cell::is_empty)
        );
    }
}
