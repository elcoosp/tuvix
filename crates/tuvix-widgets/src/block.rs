//! Blocks: borders and titles around a rectangular area.
//!
//! Blocks are [`Widget`]s: `block.render(area, buf)` paints the frame.

use bitflags::bitflags;
use tuvix_core::buffer::Buffer;
use tuvix_core::geometry::Rect;
use tuvix_core::style::Style;
use tuvix_core::text::Line;
use tuvix_core::widget::Widget;

bitflags! {
    /// Which edges of a [`Block`] to draw.
    #[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Borders: u8 {
    /// Top edge.
    const TOP = 1 << 0;
    /// Right edge.
    const RIGHT = 1 << 1;
    /// Bottom edge.
    const BOTTOM = 1 << 2;
    /// Left edge.
    const LEFT = 1 << 3;
    /// All four edges.
    const ALL = Self::TOP.bits() | Self::RIGHT.bits() | Self::BOTTOM.bits() | Self::LEFT.bits();
    }
}

/// The character set used for borders.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BorderType {
    /// Single thin lines (`─ │ ┌ ┐ └ ┘`).
    #[default]
    Plain,
    /// Rounded corners (`╭ ╮ ╰ ╯`).
    Rounded,
    /// Double lines (`═ ║ ╔ ╗ ╚ ╝`).
    Double,
    /// Heavy thick lines (`━ ┃ ┏ ┓ ┗ ┛`).
    Thick,
}

impl BorderType {
    /// `(horizontal, vertical, top_left, top_right, bottom_left, bottom_right)`.
    const fn glyphs(
        self,
    ) -> (
        &'static str,
        &'static str,
        &'static str,
        &'static str,
        &'static str,
        &'static str,
    ) {
        match self {
            Self::Plain => ("─", "│", "┌", "┐", "└", "┘"),
            Self::Rounded => ("─", "│", "╭", "╮", "╰", "╯"),
            Self::Double => ("═", "║", "╔", "╗", "╚", "╝"),
            Self::Thick => ("━", "┃", "┏", "┓", "┗", "┛"),
        }
    }
}

/// A bordered, titled frame around a rectangular area.
///
/// # Examples
///
/// ```
/// use tuvix_core::buffer::Buffer;
/// use tuvix_core::geometry::Rect;
/// use tuvix_core::widget::Widget;
/// use tuvix_widgets::Block;
///
/// let mut buffer = Buffer::empty(Rect::new(0, 0, 5, 3));
/// Block::bordered().render(Rect::new(0, 0, 5, 3), &mut buffer);
/// assert_eq!(buffer.to_string(), "┌───┐\n│   │\n└───┘");
/// ```
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Block {
    borders: Borders,
    border_type: BorderType,
    border_style: Style,
    title: Option<Line>,
    title_style: Style,
}

impl Block {
    /// A block with no borders and no title.
    pub fn new() -> Self {
        Self::default()
    }

    /// A block with all four borders (plain style).
    pub fn bordered() -> Self {
        Self::new().borders(Borders::ALL)
    }

    /// Sets which borders to draw.
    pub fn borders(mut self, borders: Borders) -> Self {
        self.borders = borders;
        self
    }

    /// Sets the border character set.
    pub fn border_type(mut self, border_type: BorderType) -> Self {
        self.border_type = border_type;
        self
    }

    /// Sets the style applied to border glyphs.
    pub fn border_style(mut self, style: Style) -> Self {
        self.border_style = style;
        self
    }

    /// Sets the title, rendered on the top border row.
    pub fn title(mut self, title: impl Into<Line>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Sets the style layered under the title line.
    pub fn title_style(mut self, style: Style) -> Self {
        self.title_style = style;
        self
    }

    /// The area inside the borders.
    pub fn inner(&self, area: Rect) -> Rect {
        let mut inner = area;
        if self.borders.intersects(Borders::LEFT) {
            inner.x = inner.x.saturating_add(1).min(area.right());
            inner.width = inner.width.saturating_sub(1);
        }
        if self.borders.intersects(Borders::TOP) || self.title.is_some() {
            inner.y = inner.y.saturating_add(1).min(area.bottom());
            inner.height = inner.height.saturating_sub(1);
        }
        if self.borders.intersects(Borders::RIGHT) {
            inner.width = inner.width.saturating_sub(1);
        }
        if self.borders.intersects(Borders::BOTTOM) {
            inner.height = inner.height.saturating_sub(1);
        }
        inner
    }

    /// Paints the block into `buffer`, clipped to `area`.
    ///
    /// On degenerate 1x1 areas every glyph lands on the same cell and the
    /// bottom-right corner wins (glyphs are drawn in
    /// edges -> top-left -> top-right -> bottom-left -> bottom-right order).
    pub fn render(&self, area: Rect, buffer: &mut Buffer) {
        self.paint(area, buffer);
    }

    /// Internal paint; the public entry points are [`Block::render`] and
    /// the [`Widget`] impl.
    fn paint(&self, area: Rect, buffer: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let (horizontal, vertical, top_left, top_right, bottom_left, bottom_right) =
            self.border_type.glyphs();

        if self.borders.intersects(Borders::LEFT) {
            for y in area.y..area.bottom() {
                buffer.set_string(area.x, y, vertical, self.border_style);
            }
        }
        if self.borders.intersects(Borders::TOP) {
            for x in area.x..area.right() {
                buffer.set_string(x, area.y, horizontal, self.border_style);
            }
        }
        if self.borders.intersects(Borders::RIGHT) {
            let x = area.right().saturating_sub(1);
            for y in area.y..area.bottom() {
                buffer.set_string(x, y, vertical, self.border_style);
            }
        }
        if self.borders.intersects(Borders::BOTTOM) {
            let y = area.bottom().saturating_sub(1);
            for x in area.x..area.right() {
                buffer.set_string(x, y, horizontal, self.border_style);
            }
        }

        // Corners overwrite the plain edges where two borders meet.
        if self.borders.contains(Borders::TOP | Borders::LEFT) {
            buffer.set_string(area.x, area.y, top_left, self.border_style);
        }
        if self.borders.contains(Borders::TOP | Borders::RIGHT) {
            buffer.set_string(
                area.right().saturating_sub(1),
                area.y,
                top_right,
                self.border_style,
            );
        }
        if self.borders.contains(Borders::BOTTOM | Borders::LEFT) {
            buffer.set_string(
                area.x,
                area.bottom().saturating_sub(1),
                bottom_left,
                self.border_style,
            );
        }
        if self.borders.contains(Borders::BOTTOM | Borders::RIGHT) {
            let position = (
                area.right().saturating_sub(1),
                area.bottom().saturating_sub(1),
            );
            buffer.set_string(position.0, position.1, bottom_right, self.border_style);
        }

        // Title sits on the top border row, after the left border glyph.
        if let Some(title) = &self.title {
            let title_style = self.border_style.patch(self.title_style).patch(title.style);
            let mut x = area.x;
            if self.borders.intersects(Borders::LEFT) {
                x = x.saturating_add(1);
            }
            let right_limit = if self.borders.intersects(Borders::RIGHT) {
                area.right().saturating_sub(1)
            } else {
                area.right()
            };
            let max_width = right_limit.saturating_sub(x);
            if max_width > 0 {
                render_line_clipped(title, x, area.y, max_width, title_style, buffer);
            }
        }
    }
}

impl Widget for Block {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        Block::render(&self, area, buffer);
    }
}

/// Writes `line`'s spans left to right, clipped to `max_width` columns.
fn render_line_clipped(
    line: &Line,
    x: u16,
    y: u16,
    max_width: u16,
    line_style: Style,
    buffer: &mut Buffer,
) {
    let mut cursor = x;
    let mut remaining = max_width;
    for span in &line.spans {
        if remaining == 0 {
            break;
        }
        let style = line_style.patch(span.style);
        let (end_x, _) = buffer.set_stringn(cursor, y, &span.content, remaining, style);
        let used = end_x.saturating_sub(cursor);
        remaining = remaining.saturating_sub(used);
        cursor = end_x;
    }
}

impl Block {
    /// Maps the theme onto the block: the `border` slot styles the borders,
    /// the `title` slot styles the title.
    pub fn theme(mut self, theme: &Theme) -> Self {
        self.border_style = theme.border;
        self.title_style = theme.title;
        self
    }
}

use tuvix_core::theme::Theme;
#[cfg(test)]
mod tests {
    use super::*;
    use tuvix_core::style::Color;
    use tuvix_core::text::Span;

    fn buffer_8x4() -> Buffer {
        Buffer::empty(Rect::new(0, 0, 8, 4))
    }

    #[test]
    fn bordered_block_draws_all_glyphs() {
        let mut buffer = buffer_8x4();
        Block::bordered().render(Rect::new(0, 0, 6, 3), &mut buffer);
        assert_eq!(buffer.to_string(), "┌────┐  \n│    │  \n└────┘  \n        ");
    }

    #[test]
    fn border_type_changes_glyphs() {
        for (border_type, expected) in [
            (BorderType::Rounded, "╭──╮    \n│  │    \n╰──╯    "),
            (BorderType::Double, "╔══╗    \n║  ║    \n╚══╝    "),
            (BorderType::Thick, "┏━━┓    \n┃  ┃    \n┗━━┛    "),
        ] {
            let mut buffer = buffer_8x4();
            Block::bordered()
                .border_type(border_type)
                .render(Rect::new(0, 0, 4, 3), &mut buffer);
            assert_eq!(buffer.to_string(), format!("{expected}\n        "));
        }
    }

    #[test]
    fn partial_borders_render_only_selected_edges() {
        let mut buffer = buffer_8x4();
        Block::new()
            .borders(Borders::TOP)
            .render(Rect::new(0, 0, 4, 2), &mut buffer);
        assert_eq!(buffer.to_string(), "────    \n        \n        \n        ");
    }

    #[test]
    fn border_style_is_applied() {
        let mut buffer = buffer_8x4();
        Block::bordered()
            .border_style(Style::new().fg(Color::Red))
            .render(Rect::new(0, 0, 4, 3), &mut buffer);
        assert_eq!(buffer.get(0, 0).unwrap().style.fg, Some(Color::Red));
        assert_eq!(buffer.get(1, 0).unwrap().style.fg, Some(Color::Red));
    }

    #[test]
    fn title_renders_after_left_border() {
        let mut buffer = buffer_8x4();
        Block::bordered()
            .title("ab")
            .render(Rect::new(0, 0, 8, 3), &mut buffer);
        let rendered = buffer.to_string();
        let rows: Vec<&str> = rendered.lines().collect();
        assert_eq!(rows[0], "┌ab────┐");
    }

    #[test]
    fn title_is_truncated_to_inner_width() {
        let mut buffer = buffer_8x4();
        Block::bordered()
            .title("abcdefghij")
            .render(Rect::new(0, 0, 8, 3), &mut buffer);
        let rendered = buffer.to_string();
        let rows: Vec<&str> = rendered.lines().collect();
        // Inner width between borders = 6 columns.
        assert_eq!(rows[0], "┌abcdef┐");
    }

    #[test]
    fn title_without_left_border_starts_at_edge() {
        let mut buffer = buffer_8x4();
        Block::new()
            .borders(Borders::TOP)
            .title("hi")
            .render(Rect::new(0, 0, 6, 2), &mut buffer);
        let rendered = buffer.to_string();
        let rows: Vec<&str> = rendered.lines().collect();
        assert_eq!(rows[0], "hi────  ");
    }

    #[test]
    fn title_style_layers_over_border_style() {
        let mut buffer = buffer_8x4();
        Block::bordered()
            .border_style(Style::new().fg(Color::Red))
            .title(Span::styled("t", Style::new().fg(Color::Green)))
            .render(Rect::new(0, 0, 6, 3), &mut buffer);
        assert_eq!(buffer.get(1, 0).unwrap().style.fg, Some(Color::Green));
        assert_eq!(buffer.get(0, 0).unwrap().style.fg, Some(Color::Red));
    }

    #[test]
    fn inner_respects_borders() {
        let block = Block::bordered();
        assert_eq!(block.inner(Rect::new(0, 0, 10, 5)), Rect::new(1, 1, 8, 3));
        let bare = Block::new();
        assert_eq!(bare.inner(Rect::new(0, 0, 10, 5)), Rect::new(0, 0, 10, 5));
        let sides = Block::new().borders(Borders::LEFT | Borders::RIGHT);
        assert_eq!(sides.inner(Rect::new(0, 0, 10, 5)), Rect::new(1, 0, 8, 5));
    }

    #[test]
    fn rendering_into_empty_area_is_noop() {
        let mut buffer = buffer_8x4();
        Block::bordered().render(Rect::ZERO, &mut buffer);
        assert!(
            buffer
                .content
                .iter()
                .all(tuvix_core::buffer::Cell::is_empty)
        );
    }

    #[test]
    fn single_cell_area_renders_corner() {
        let mut buffer = buffer_8x4();
        Block::bordered().render(Rect::new(0, 0, 1, 1), &mut buffer);
        assert_eq!(buffer.to_string(), "┘       \n        \n        \n        ");
    }
}
