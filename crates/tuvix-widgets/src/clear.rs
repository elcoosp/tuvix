//! The popup paint primitive: erase an area back to blank cells.
//!
//! [`Clear`] resets every cell of its area — through the buffer's
//! pair-repairing clear, so no orphaned covered cells survive — giving a
//! widget painted afterwards a clean canvas. The classic popup pattern:
//! split out a centered rect (`Rect::centered`), clear it, paint the
//! popup's block and content over it. See the `inputs` example for the
//! whole sequence in a running app.

use tuvix_core::buffer::Buffer;
use tuvix_core::geometry::Rect;
use tuvix_core::widget::Widget;

/// Erases its area back to blank cells when rendered.
///
/// # Examples
///
/// ```
/// use tuvix_core::buffer::Buffer;
/// use tuvix_core::geometry::Rect;
/// use tuvix_core::style::Style;
/// use tuvix_widgets::Clear;
///
/// let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 1));
/// buffer.set_string(0, 0, "aaaaaaaaaa", Style::new());
/// Clear.render(Rect::new(4, 0, 4, 1), &mut buffer);
/// assert_eq!(buffer.to_string(), "aaaa    aa");
/// ```
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Clear;

impl Clear {
    /// Erases `area` back to blank cells, repairing wide-glyph pairs.
    pub fn render(&self, area: Rect, buffer: &mut Buffer) {
        buffer.clear(area);
    }
}

impl Widget for Clear {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        Clear::render(&self, area, buffer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tuvix_core::style::Style;

    fn buffer_10x1() -> Buffer {
        Buffer::empty(Rect::new(0, 0, 10, 1))
    }

    #[test]
    fn wipes_the_area_back_to_blank_cells() {
        let mut buffer = buffer_10x1();
        buffer.set_string(0, 0, "aaaaaaaaaa", Style::new());
        Clear.render(Rect::new(4, 0, 4, 1), &mut buffer);
        assert_eq!(buffer.to_string(), "aaaa    aa");
        // The wiped cells are truly empty: no style, no symbol.
        assert!(buffer.get(4, 0).unwrap().is_empty());
    }

    #[test]
    fn clears_repair_wide_glyph_pairs() {
        // 中 occupies cells 0-1; clearing only the covered half (cell 1)
        // must repair the pair, not leave an orphaned covered cell.
        let mut buffer = Buffer::empty(Rect::new(0, 0, 6, 1));
        buffer.set_string(0, 0, "中文文", Style::new());
        Clear.render(Rect::new(1, 0, 1, 1), &mut buffer);
        assert_eq!(buffer.to_string(), "  文·文·");
    }

    #[test]
    fn out_of_bounds_areas_are_ignored() {
        let mut buffer = buffer_10x1();
        buffer.set_string(0, 0, "aaaaaaaaaa", Style::new());
        Clear.render(Rect::new(100, 100, 5, 5), &mut buffer);
        assert_eq!(buffer.to_string(), "aaaaaaaaaa");
    }

    #[test]
    fn renders_through_the_widget_trait() {
        let mut buffer = buffer_10x1();
        buffer.set_string(0, 0, "aaaaaaaaaa", Style::new());
        Widget::render(Clear, Rect::new(0, 0, 10, 1), &mut buffer);
        assert_eq!(buffer.to_string(), "          ");
    }
}
