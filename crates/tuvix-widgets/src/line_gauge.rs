//! LineGauge: a single-row gauge drawn with box-drawing lines.
//!
//! The label sits on the left, then the bar fills the rest of the row:
//! filled columns are `━` in the gauge style, the rest is `─` in the
//! gauge style dimmed. The line gauge is integer-precise —
//! [`Gauge`](crate::Gauge) owns the eighth-block fractional precision —
//! and renders on the first row of its area.

use tuvix_core::buffer::Buffer;
use tuvix_core::geometry::Rect;
use tuvix_core::style::{Modifier, Style};
use tuvix_core::text::Line;
use tuvix_core::widget::Widget;

use crate::block::Block;

/// A one-row ratio bar with a label, drawn with line glyphs.
///
/// # Examples
///
/// ```
/// use tuvix_core::buffer::Buffer;
/// use tuvix_core::geometry::Rect;
/// use tuvix_widgets::LineGauge;
///
/// let mut buffer = Buffer::empty(Rect::new(0, 0, 20, 1));
/// LineGauge::default()
///     .ratio(0.5)
///     .render(Rect::new(0, 0, 20, 1), &mut buffer);
/// assert_eq!(buffer.to_string(), "50% ━━━━━━━━────────");
/// ```
#[derive(Debug, Default, Clone, PartialEq)]
pub struct LineGauge {
    block: Option<Block>,
    ratio: f64,
    label: Option<Line>,
    style: Style,
}

impl LineGauge {
    /// A gauge at zero.
    pub fn new() -> Self {
        Self::default()
    }

    /// A gauge at `percent` (clamped to 0..=100).
    pub fn percent(percent: u16) -> Self {
        Self::new().ratio(f64::from(percent.min(100)) / 100.0)
    }

    /// A gauge at `ratio` (clamped to 0.0..=1.0; NaN reads as 0.0).
    pub fn ratio(mut self, ratio: f64) -> Self {
        self.ratio = if ratio.is_nan() {
            0.0
        } else {
            ratio.clamp(0.0, 1.0)
        };
        self
    }

    /// Overrides the label (default: `{:.0}%` of the ratio).
    pub fn label<T: Into<Line>>(mut self, label: T) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets the style of the filled portion of the bar; the unfilled
    /// portion carries the same style dimmed.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Wraps the gauge in a block; the row fills the block's inner area.
    pub fn block(mut self, block: Block) -> Self {
        self.block = Some(block);
        self
    }

    /// Maps the theme onto the gauge: the `gauge` slot styles the fill.
    pub fn theme(mut self, theme: &Theme) -> Self {
        self.style = theme.gauge;
        self
    }

    /// The displayed label line: the override, else the auto `NN%`.
    fn effective_label(&self) -> Line {
        self.label
            .clone()
            .unwrap_or_else(|| Line::raw(format!("{:.0}%", self.ratio * 100.0)))
    }

    /// Paints the gauge into `buffer`, on the first row of `area`.
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
        let row = gauge_area.y;
        let right = gauge_area.right();

        // The label leads; every label cell patches its span style over
        // the gauge style.
        let mut x = gauge_area.x;
        for span in &self.effective_label().spans {
            if x >= right {
                return;
            }
            x = buffer
                .set_stringn(
                    x,
                    row,
                    span.content.as_ref(),
                    right - x,
                    self.style.patch(span.style),
                )
                .0;
        }

        // One cell of breathing room after a label that wrote something,
        // then the bar fills the rest (an empty label leaves no gap).
        let bar_start = if x > gauge_area.x {
            x.saturating_add(1)
        } else {
            x
        };
        if bar_start >= right {
            return;
        }
        let bar_width = right - bar_start;
        let filled = (f64::from(bar_width) * self.ratio).floor() as u16;
        let dimmed = self.style.add_modifier(Modifier::DIM);
        for column in bar_start..bar_start + filled {
            if let Some(cell) = buffer.get_mut(column, row) {
                cell.set_symbol("━").set_style(self.style);
            }
        }
        for column in bar_start + filled..right {
            if let Some(cell) = buffer.get_mut(column, row) {
                cell.set_symbol("─").set_style(dimmed);
            }
        }
    }
}

impl Widget for LineGauge {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        LineGauge::render(&self, area, buffer);
    }
}

use tuvix_core::theme::Theme;
#[cfg(test)]
mod tests {
    use super::*;
    use tuvix_core::style::Color;

    #[test]
    fn default_label_leads_and_the_bar_fills_the_row() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 20, 1));
        LineGauge::default()
            .ratio(0.5)
            .render(Rect::new(0, 0, 20, 1), &mut buffer);
        // Label "50%" (3), one gap, bar 16 wide: 8 filled, 8 dimmed.
        assert_eq!(buffer.to_string(), "50% ━━━━━━━━────────");
    }

    #[test]
    fn full_ratio_fills_every_bar_cell() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 1));
        LineGauge::percent(100)
            .label("")
            .render(Rect::new(0, 0, 10, 1), &mut buffer);
        assert_eq!(buffer.to_string(), "━━━━━━━━━━");
    }

    #[test]
    fn zero_ratio_is_all_dimmed_track() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 1));
        LineGauge::default()
            .label("")
            .ratio(0.0)
            .render(Rect::new(0, 0, 10, 1), &mut buffer);
        assert_eq!(buffer.to_string(), "──────────");
    }

    #[test]
    fn ratio_clamps_out_of_range_input() {
        // Over 1.0 fills everything; NaN and negatives read as zero.
        let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 1));
        LineGauge::default()
            .label("")
            .ratio(2.0)
            .render(Rect::new(0, 0, 10, 1), &mut buffer);
        assert_eq!(buffer.to_string(), "━━━━━━━━━━");
        for ratio in [f64::NAN, -1.0] {
            let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 1));
            LineGauge::default()
                .label("")
                .ratio(ratio)
                .render(Rect::new(0, 0, 10, 1), &mut buffer);
            assert_eq!(buffer.to_string(), "──────────");
        }
    }

    #[test]
    fn integer_precision_floors_the_fill() {
        // Bar width 10, ratio 0.49 -> floor(4.9) = 4 filled cells.
        let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 1));
        LineGauge::default()
            .label("")
            .ratio(0.49)
            .render(Rect::new(0, 0, 10, 1), &mut buffer);
        assert_eq!(buffer.to_string(), "━━━━──────");
    }

    #[test]
    fn unfilled_cells_carry_the_dimmed_style() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 1));
        LineGauge::default()
            .label("")
            .ratio(0.5)
            .style(Style::new().fg(Color::Red))
            .render(Rect::new(0, 0, 10, 1), &mut buffer);
        assert_eq!(buffer.get(2, 0).unwrap().style.fg, Some(Color::Red));
        assert_eq!(
            buffer.get(2, 0).unwrap().style.add_modifier,
            Modifier::empty()
        );
        assert_eq!(buffer.get(7, 0).unwrap().style.fg, Some(Color::Red));
        assert_eq!(buffer.get(7, 0).unwrap().style.add_modifier, Modifier::DIM);
    }

    #[test]
    fn label_override_keeps_span_styles() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 20, 1));
        LineGauge::default()
            .ratio(1.0)
            .label(Line::styled("FULL", Style::new().fg(Color::Green)))
            .render(Rect::new(0, 0, 20, 1), &mut buffer);
        assert_eq!(buffer.get(0, 0).unwrap().style.fg, Some(Color::Green));
        // Bar starts after label + gap: the gap cell is blank, the rest
        // of the 15 remaining cells fill completely at ratio 1.0.
        assert!(buffer.get(4, 0).unwrap().is_empty());
        assert_eq!(buffer.get(5, 0).unwrap().symbol, "━");
    }

    #[test]
    fn block_wraps_and_fills_inner() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 3));
        LineGauge::percent(50)
            .block(Block::bordered())
            .render(Rect::new(0, 0, 10, 3), &mut buffer);
        // Inner row: label "50%" + gap + bar of 4 (2 filled, 2 dimmed).
        assert_eq!(buffer.to_string(), "┌────────┐\n│50% ━━──│\n└────────┘");
    }

    #[test]
    fn taller_areas_use_only_the_first_row() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 3));
        LineGauge::default()
            .label("")
            .ratio(0.5)
            .render(Rect::new(0, 0, 10, 3), &mut buffer);
        assert!(buffer.get(0, 1).unwrap().is_empty());
        assert!(buffer.get(0, 2).unwrap().is_empty());
        assert_eq!(buffer.get(0, 0).unwrap().symbol, "━");
    }

    #[test]
    fn label_longer_than_the_row_clips_and_skips_the_bar() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 4, 1));
        LineGauge::default()
            .ratio(0.5)
            .label("Overflowing")
            .render(Rect::new(0, 0, 4, 1), &mut buffer);
        assert_eq!(buffer.to_string(), "Over");
    }

    #[test]
    fn renders_through_the_widget_trait() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 1));
        Widget::render(
            LineGauge::default().label("").ratio(0.5),
            Rect::new(0, 0, 10, 1),
            &mut buffer,
        );
        assert_eq!(buffer.to_string(), "━━━━━─────");
    }

    #[test]
    fn empty_area_is_a_noop() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 1));
        LineGauge::default().render(Rect::ZERO, &mut buffer);
        assert!(
            buffer
                .content
                .iter()
                .all(tuvix_core::buffer::Cell::is_empty)
        );
    }
}
