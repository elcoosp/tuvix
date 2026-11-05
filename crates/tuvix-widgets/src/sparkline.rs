//! Sparklines: single-row-per-column bar charts from `u64` series.
//!
//! The last `area.width` values are rendered as stacked column bars with
//! eighth-block precision (the `▁▂▃▄▅▆▇█` ladder): a column of bar-height
//! full blocks plus one fractional block on top, measured in eighths of
//! a row.

use tuvix_core::buffer::Buffer;
use tuvix_core::geometry::Rect;
use tuvix_core::style::Style;
use tuvix_core::widget::Widget;

use crate::block::Block;

/// Bar glyphs from one eighth of a row to a full row (`▁▂▃▄▅▆▇█`).
const SPARK_LEVELS: [&str; 8] = ["▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];

/// A compact bar chart of the last N values of a series.
///
/// # Examples
///
/// ```
/// use tuvix_core::buffer::Buffer;
/// use tuvix_core::geometry::Rect;
/// use tuvix_widgets::Sparkline;
///
/// let mut buffer = Buffer::empty(Rect::new(0, 0, 5, 2));
/// Sparkline::new([1_u64, 2, 0, 4, 8, 16]).render(Rect::new(0, 0, 5, 2), &mut buffer);
/// // Last 5 values [2, 0, 4, 8, 16] with max 16, bottom-anchored:
/// // eighth bars, a gap, one full row, then two full rows.
/// assert_eq!(buffer.to_string(), "    █\n▂ ▄██");
/// ```
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Sparkline {
    data: Vec<u64>,
    max: Option<u64>,
    style: Style,
    block: Option<Block>,
}

impl Sparkline {
    /// Creates a sparkline from a series of values.
    pub fn new<T, I>(data: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<u64>,
    {
        Self {
            data: data.into_iter().map(Into::into).collect(),
            max: None,
            style: Style::new(),
            block: None,
        }
    }

    /// Pins the maximum of the scale instead of the data maximum.
    pub fn max(mut self, max: u64) -> Self {
        self.max = Some(max);
        self
    }

    /// Sets the style of the bar cells.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Wraps the sparkline in a block; bars fill the block's inner area.
    pub fn block(mut self, block: Block) -> Self {
        self.block = Some(block);
        self
    }

    /// Paints the sparkline into `buffer`, clipped to `area`.
    ///
    /// Column heights are `(value * height * 8 / max)` eighths, clamped
    /// to the area height. When neither a custom nor a data maximum
    /// exists (empty data or an all-zero series), nothing is drawn.
    pub fn render(&self, area: Rect, buffer: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        if let Some(block) = &self.block {
            block.render(area, buffer);
        }
        let spark_area = self.block.as_ref().map_or(area, |block| block.inner(area));
        if spark_area.is_empty() {
            return;
        }

        let max = self.max.or_else(|| self.data.iter().copied().max());
        let Some(max) = max.filter(|max| *max > 0) else {
            return;
        };

        let visible = usize::from(spark_area.width);
        let start = self.data.len().saturating_sub(visible);
        let scale = u64::from(spark_area.height) * 8;
        for (column, value) in self.data[start..].iter().copied().enumerate() {
            let eighths = (value.min(max) * scale / max).min(scale);
            if eighths == 0 {
                continue;
            }
            let x = spark_area.x + column as u16;
            let full_rows = eighths / 8;
            let fraction = eighths % 8;
            for row in 0..full_rows {
                // Full blocks stack from the bottom row upward.
                let y = spark_area.bottom() - 1 - row as u16;
                if let Some(cell) = buffer.get_mut(x, y) {
                    cell.set_symbol("█").set_style(self.style);
                }
            }
            if fraction > 0 {
                let y = spark_area.bottom() - 1 - full_rows as u16;
                if let Some(cell) = buffer.get_mut(x, y) {
                    cell.set_symbol(SPARK_LEVELS[(fraction - 1) as usize])
                        .set_style(self.style);
                }
            }
        }
    }
}

impl Widget for Sparkline {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        Sparkline::render(&self, area, buffer);
    }
}

impl Sparkline {
    /// Maps the theme onto the sparkline: the `sparkline` slot styles the bars.
    pub fn theme(mut self, theme: &Theme) -> Self {
        self.style = theme.sparkline;
        self
    }
}

use tuvix_core::theme::Theme;
#[cfg(test)]
mod tests {
    use super::*;
    use tuvix_core::buffer::Cell;
    use tuvix_core::style::Color;
    use tuvix_core::widget::Widget;

    fn buffer_5x2() -> Buffer {
        Buffer::empty(Rect::new(0, 0, 5, 2))
    }

    #[test]
    fn renders_the_last_width_values() {
        let mut buffer = buffer_5x2();
        // Max = 16; the last 5 values are [2, 0, 4, 8, 16]. Scale =
        // 2 rows * 8 = 16 eighths, so the columns are [2/8, blank, 4/8,
        // 1 row, 2 rows], bottom-anchored.
        Sparkline::new([1_u64, 2, 0, 4, 8, 16]).render(Rect::new(0, 0, 5, 2), &mut buffer);
        assert_eq!(buffer.to_string(), "    █\n▂ ▄██");
    }

    #[test]
    fn single_row_uses_eighth_levels() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 4, 1));
        Sparkline::new([0_u64, 2, 5, 8])
            .max(8)
            .render(Rect::new(0, 0, 4, 1), &mut buffer);
        // Heights in eighths: [0, 2, 5, 8] -> ["", "▂", "▅", "█"].
        assert_eq!(buffer.to_string(), " ▂▅█");
    }

    #[test]
    fn custom_max_pins_the_scale() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 2, 1));
        Sparkline::new([1_u64, 1])
            .max(4)
            .render(Rect::new(0, 0, 2, 1), &mut buffer);
        // 1 * 8 / 4 = 2 eighths -> ▂.
        assert_eq!(buffer.to_string(), "▂▂");
    }

    #[test]
    fn values_above_max_are_clamped() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 2, 1));
        Sparkline::new([9_u64, 3])
            .max(8)
            .render(Rect::new(0, 0, 2, 1), &mut buffer);
        assert_eq!(buffer.to_string(), "█▃");
    }

    #[test]
    fn all_zero_or_empty_data_renders_nothing() {
        let mut buffer = buffer_5x2();
        Sparkline::new([0_u64, 0, 0]).render(Rect::new(0, 0, 5, 2), &mut buffer);
        assert!(buffer.content.iter().all(Cell::is_empty));
        let mut buffer = buffer_5x2();
        Sparkline::new(Vec::<u64>::new()).render(Rect::new(0, 0, 5, 2), &mut buffer);
        assert!(buffer.content.iter().all(Cell::is_empty));
    }

    #[test]
    fn taller_areas_stack_full_blocks() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 1, 4));
        Sparkline::new([12_u64])
            .max(16)
            .render(Rect::new(0, 0, 1, 4), &mut buffer);
        // 12 * 32 / 16 = 24 eighths = 3 full rows, bottom-anchored.
        assert_eq!(buffer.to_string(), " \n█\n█\n█");
    }

    #[test]
    fn fractional_top_block_sits_above_full_blocks() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 1, 4));
        Sparkline::new([13_u64])
            .max(16)
            .render(Rect::new(0, 0, 1, 4), &mut buffer);
        // 13 * 32 / 16 = 26 eighths = 3 full rows + 2/8 on top (▂).
        assert_eq!(buffer.to_string(), "▂\n█\n█\n█");
    }

    #[test]
    fn style_is_applied_to_bar_cells() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 2, 1));
        Sparkline::new([8_u64, 8])
            .style(Style::new().fg(Color::Cyan))
            .render(Rect::new(0, 0, 2, 1), &mut buffer);
        assert_eq!(buffer.get(0, 0).unwrap().style.fg, Some(Color::Cyan));
        assert_eq!(buffer.get(1, 0).unwrap().style.fg, Some(Color::Cyan));
    }

    #[test]
    fn block_wraps_and_bars_fill_inner() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 6, 3));
        Sparkline::new([4_u64])
            .max(4)
            .block(Block::bordered())
            .render(Rect::new(0, 0, 6, 3), &mut buffer);
        // Inner = (1, 1, 4, 1): one full-height bar in the first column.
        assert_eq!(buffer.to_string(), "┌────┐\n│█   │\n└────┘");
    }

    #[test]
    fn rendering_into_empty_area_is_noop() {
        let mut buffer = buffer_5x2();
        Sparkline::new([1_u64]).render(Rect::ZERO, &mut buffer);
        assert!(buffer.content.iter().all(Cell::is_empty));
    }

    #[test]
    fn sparkline_renders_through_the_widget_trait() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 1, 1));
        Widget::render(
            Sparkline::new([8_u64]).max(8),
            Rect::new(0, 0, 1, 1),
            &mut buffer,
        );
        assert_eq!(buffer.to_string(), "█");
    }
}
