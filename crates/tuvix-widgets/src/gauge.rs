//! Gauges: a ratio bar with full and eighth-block cells.
//!
//! The bar fills the full height of its area; the fractional end column
//! uses one-eighth blocks ([`Gauge::use_unicode`], on by default) so a
//! 200-column gauge has 1,600 visible steps. The label (default `NN%`)
//! is centered over the bar and overwrites the cells it covers.

use tuvix_core::buffer::Buffer;
use tuvix_core::geometry::Rect;
use tuvix_core::style::Style;
use tuvix_core::text::Line;
use tuvix_core::widget::Widget;

use crate::block::Block;

/// The one-eighth block glyphs used for the fractional end column.
///
/// Index 0 renders nothing (the column stays unfilled); indices 1-7 are
/// the left-eighth through left-seven-eighths blocks (an index of 8
/// clamps down to 7, since the full cells are already written).
const EIGHTH_BLOCKS: [&str; 8] = ["", "▏", "▎", "▍", "▌", "▋", "▊", "▉"];

/// A ratio bar filling its area, with a centered label.
///
/// # Examples
///
/// ```
/// use tuvix_core::buffer::Buffer;
/// use tuvix_core::geometry::Rect;
/// use tuvix_widgets::Gauge;
///
/// let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 1));
/// Gauge::percent(50).render(Rect::new(0, 0, 10, 1), &mut buffer);
/// // Three full cells, the centered default label "50%", then fill.
/// assert_eq!(buffer.to_string(), "███50%    ");
///
/// let mut silent = Buffer::empty(Rect::new(0, 0, 10, 1));
/// Gauge::new().ratio(0.25)
///     .label("")
///     .render(Rect::new(0, 0, 10, 1), &mut silent);
/// // Two full cells plus the one-half eighth block.
/// assert_eq!(silent.to_string(), "██▌       ");
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Gauge {
    block: Option<Block>,
    ratio: f64,
    label: Option<Line>,
    style: Style,
    use_unicode: bool,
}

impl Default for Gauge {
    fn default() -> Self {
        Self {
            block: None,
            ratio: 0.0,
            label: None,
            style: Style::new(),
            // Eighth-block precision is on unless explicitly disabled.
            use_unicode: true,
        }
    }
}

impl Gauge {
    /// A gauge at zero.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A gauge at `percent` (clamped to 0..=100).
    #[must_use]
    pub fn percent(percent: u16) -> Self {
        Self::new().ratio(f64::from(percent.min(100)) / 100.0)
    }

    /// A gauge at `ratio` (clamped to 0.0..=1.0; NaN reads as 0.0).
    #[must_use]
    pub fn ratio(mut self, ratio: f64) -> Self {
        self.ratio = if ratio.is_nan() {
            0.0
        } else {
            ratio.clamp(0.0, 1.0)
        };
        self
    }

    /// Overrides the label (default: `{:.0}%` of the ratio).
    #[must_use]
    pub fn label<T: Into<Line>>(mut self, label: T) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets the style of the filled portion of the bar.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Toggles eighth-block precision for the end column (default on).
    #[must_use]
    pub fn use_unicode(mut self, value: bool) -> Self {
        self.use_unicode = value;
        self
    }

    /// Wraps the gauge in a block; the bar fills the block's inner area.
    #[must_use]
    pub fn block(mut self, block: Block) -> Self {
        self.block = Some(block);
        self
    }

    /// The displayed label line: the override, else the auto `NN%`.
    fn effective_label(&self) -> Line {
        self.label
            .clone()
            .unwrap_or_else(|| Line::raw(format!("{:.0}%", self.ratio * 100.0)))
    }

    /// Paints the gauge into `buffer`, clipped to `area`.
    ///
    /// The filled run spans `floor(ratio * width)` columns on every row
    /// of the gauge area; the next column carries the eighth block for
    /// the fractional remainder (unicode mode). The label is drawn last,
    /// centered, overwriting the cells it covers.
    pub fn render(&self, area: Rect, buffer: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        if let Some(block) = &self.block {
            block.render(area, buffer);
        }
        let gauge_area = self.block.as_ref().map_or(area, |block| block.inner(area));
        if gauge_area.is_empty() {
            return;
        }

        let filled_width = f64::from(gauge_area.width) * self.ratio;
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "the ratio is clamped to [0.0, 1.0] by the constructor, \
                      so the filled width is non-negative and below the \
                      gauge width; std has no lossless f64-to-u16 step"
        )]
        let end = gauge_area.x + filled_width.floor() as u16;
        for y in gauge_area.y..gauge_area.bottom() {
            for x in gauge_area.x..end {
                if let Some(cell) = buffer.get_mut(x, y) {
                    cell.set_symbol("█").set_style(self.style);
                }
            }
            if self.use_unicode && self.ratio < 1.0 && end < gauge_area.right() {
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "fract() is in [0.0, 1.0), so the eighth index is \
                              in [0, 8]; std has no lossless f64-to-usize step"
                )]
                let index = (filled_width.fract() * 8.0).round() as usize;
                if index > 0
                    && let Some(cell) = buffer.get_mut(end, y)
                {
                    cell.set_symbol(EIGHTH_BLOCKS[index.min(7)])
                        .set_style(self.style);
                }
            }
        }

        // Label: centered over the bar; every label cell takes the gauge
        // style patched with the span style (filled or not).
        let label = self.effective_label();
        let row = gauge_area.y + gauge_area.height / 2;
        let label_width = label.width().min(gauge_area.width);
        let mut x = gauge_area.x + (gauge_area.width - label_width) / 2;
        let right_limit = gauge_area.right();
        for span in &label.spans {
            if x >= right_limit {
                break;
            }
            let style = self.style.patch(span.style);
            let (end_x, _) =
                buffer.set_stringn(x, row, span.content.as_ref(), right_limit - x, style);
            x = end_x;
        }
    }
}

impl Widget for Gauge {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        Gauge::render(&self, area, buffer);
    }
}

impl Gauge {
    /// Maps the theme onto the gauge: the `gauge` slot styles the fill.
    #[must_use]
    pub fn theme(mut self, theme: &Theme) -> Self {
        self.style = theme.gauge;
        self
    }
}

use tuvix_core::theme::Theme;
#[cfg(test)]
mod tests {
    use super::*;
    use tuvix_core::style::Color;
    use tuvix_core::text::Span;
    use tuvix_core::widget::Widget;

    fn buffer_10x1() -> Buffer {
        Buffer::empty(Rect::new(0, 0, 10, 1))
    }

    #[test]
    fn percent_fills_full_cells_with_default_label() {
        let mut buffer = buffer_10x1();
        Gauge::percent(50).render(Rect::new(0, 0, 10, 1), &mut buffer);
        // Three full cells, then the centered default label "50%".
        assert_eq!(buffer.to_string(), "███50%    ");
    }

    #[test]
    fn ratio_clamps_out_of_range_input() {
        let mut buffer = buffer_10x1();
        Gauge::new()
            .ratio(2.0)
            .render(Rect::new(0, 0, 10, 1), &mut buffer);
        assert_eq!(buffer.to_string(), "███100%███");
        let mut buffer = buffer_10x1();
        Gauge::new()
            .ratio(-1.0)
            .render(Rect::new(0, 0, 10, 1), &mut buffer);
        assert_eq!(buffer.to_string(), "    0%    ");
        let mut buffer = buffer_10x1();
        Gauge::new()
            .ratio(f64::NAN)
            .render(Rect::new(0, 0, 10, 1), &mut buffer);
        assert_eq!(buffer.to_string(), "    0%    ");
    }

    #[test]
    fn percent_clamps_over_100() {
        let mut buffer = buffer_10x1();
        Gauge::percent(250).render(Rect::new(0, 0, 10, 1), &mut buffer);
        assert_eq!(buffer.to_string(), "███100%███");
    }

    #[test]
    fn unicode_fraction_uses_eighth_blocks() {
        // 10 columns * 0.25 = 2.5 filled -> two full cells + a 1/2 block;
        // the label is silenced to keep the fractional column visible.
        let mut buffer = buffer_10x1();
        Gauge::new()
            .ratio(0.25)
            .label("")
            .render(Rect::new(0, 0, 10, 1), &mut buffer);
        assert_eq!(buffer.to_string(), "██▌       ");
        // 10 * 0.55 = 5.5 -> five full cells + fraction 0.5 -> ▌.
        let mut buffer = buffer_10x1();
        Gauge::new()
            .ratio(0.55)
            .label("")
            .render(Rect::new(0, 0, 10, 1), &mut buffer);
        assert_eq!(buffer.to_string(), "█████▌    ");
        // 8 * 0.7 = 5.6 -> five full cells + fraction 0.6 -> ▋.
        let mut buffer = Buffer::empty(Rect::new(0, 0, 8, 1));
        Gauge::new()
            .ratio(0.7)
            .label("")
            .render(Rect::new(0, 0, 8, 1), &mut buffer);
        assert_eq!(buffer.to_string(), "█████▋  ");
        // 10 * 0.05 = 0.5 -> zero full cells, fraction 0.5 -> ▌ alone.
        let mut buffer = buffer_10x1();
        Gauge::new()
            .ratio(0.05)
            .label("")
            .render(Rect::new(0, 0, 10, 1), &mut buffer);
        assert_eq!(buffer.to_string(), "▌         ");
    }

    #[test]
    fn ascii_mode_fills_floor_only() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 40, 1));
        Gauge::percent(25)
            .use_unicode(false)
            .render(Rect::new(0, 0, 40, 1), &mut buffer);
        // Exactly 10.0 filled columns; no fractional cell in ascii mode.
        assert_eq!(
            buffer.to_string(),
            format!("{}{}25%{}", "█".repeat(10), " ".repeat(8), " ".repeat(19))
        );
    }

    #[test]
    fn full_ratio_fills_every_cell_without_fraction() {
        let mut buffer = buffer_10x1();
        Gauge::percent(100)
            .label("")
            .render(Rect::new(0, 0, 10, 1), &mut buffer);
        assert_eq!(buffer.to_string(), "██████████");
    }

    #[test]
    fn default_label_is_centered_percentage() {
        let mut buffer = buffer_10x1();
        Gauge::percent(50).render(Rect::new(0, 0, 10, 1), &mut buffer);
        // 50% label (3 cells) centered at x = (10-3)/2 = 3, over the fill.
        assert_eq!(buffer.to_string(), "███50%    ");
    }

    #[test]
    fn label_overrides_default_and_patches_styles() {
        let mut buffer = buffer_10x1();
        Gauge::percent(100)
            .label(Span::styled("FULL", Style::new().fg(Color::Green)))
            .style(Style::new().fg(Color::Red))
            .render(Rect::new(0, 0, 10, 1), &mut buffer);
        // Label starts at x = (10-4)/2 = 3; cells 0-2 gauge-red, 3-6
        // gauge-red patched with green.
        assert_eq!(buffer.get(0, 0).unwrap().style.fg, Some(Color::Red));
        assert_eq!(buffer.get(3, 0).unwrap().style.fg, Some(Color::Green));
        assert_eq!(buffer.get(6, 0).unwrap().style.fg, Some(Color::Green));
        assert_eq!(buffer.get(7, 0).unwrap().style.fg, Some(Color::Red));
    }

    #[test]
    fn gauge_fills_full_height() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 4, 3));
        Gauge::percent(50).render(Rect::new(0, 0, 4, 3), &mut buffer);
        // Label row is the middle one; the other rows are pure fill.
        assert_eq!(buffer.to_string(), "██  \n50% \n██  ");
    }

    #[test]
    fn block_wraps_and_fills_inner() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 6, 3));
        Gauge::percent(100)
            .block(Block::bordered())
            .render(Rect::new(0, 0, 6, 3), &mut buffer);
        // Inner area is (1, 1, 4, 1): the label "100%" fills it exactly.
        assert_eq!(buffer.to_string(), "┌────┐\n│100%│\n└────┘");
    }

    #[test]
    fn rendering_into_empty_area_is_noop() {
        let mut buffer = buffer_10x1();
        Gauge::percent(50).render(Rect::ZERO, &mut buffer);
        assert!(
            buffer
                .content
                .iter()
                .all(tuvix_core::buffer::Cell::is_empty)
        );
    }

    #[test]
    fn gauge_renders_through_the_widget_trait() {
        let mut buffer = buffer_10x1();
        Widget::render(Gauge::percent(30), Rect::new(0, 0, 10, 1), &mut buffer);
        assert_eq!(buffer.to_string(), "███30%    ");
    }
}
