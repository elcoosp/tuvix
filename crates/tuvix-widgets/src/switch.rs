//! Switch: a single-row on/off control rendered as a labeled track.
//!
//! The label and the flag are the app's data; the [`Switch`] widget is
//! the paint configuration. The state reads from the knob position
//! inside the four-cell track (`[● ]` on, `[ ●]` off) and from the
//! styles: the on-track takes [`Switch::on_style`], the off-track
//! [`Switch::off_style`] (dimmed by default). One-row widget like
//! `Tabs` and `Checkbox`.

use tuvix_core::buffer::Buffer;
use tuvix_core::geometry::Rect;
use tuvix_core::style::{Modifier, Style};
use tuvix_core::text::Line;
use tuvix_core::widget::Widget;

use crate::block::Block;

/// The on-position track: knob at the leading edge.
pub const TRACK_ON: &str = "[● ]";
/// The off-position track: knob at the trailing edge.
pub const TRACK_OFF: &str = "[ ●]";

/// A single-row on/off control: label plus a knob-in-a-track glyph.
///
/// # Examples
///
/// ```
/// use tuvix_core::buffer::Buffer;
/// use tuvix_core::geometry::Rect;
/// use tuvix_core::style::{Color, Style};
/// use tuvix_widgets::Switch;
///
/// let mut buffer = Buffer::empty(Rect::new(0, 0, 16, 1));
/// Switch::new("dark mode")
///     .on(true)
///     .on_style(Style::new().fg(Color::Cyan))
///     .render(Rect::new(0, 0, 16, 1), &mut buffer);
/// assert_eq!(buffer.to_string(), "dark mode [● ]  ");
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Switch {
    label: Line,
    on: bool,
    style: Style,
    on_style: Style,
    off_style: Style,
    block: Option<Block>,
}

impl Default for Switch {
    fn default() -> Self {
        Self {
            label: Line::default(),
            on: false,
            style: Style::new(),
            on_style: Style::new(),
            off_style: Style::new().add_modifier(Modifier::DIM),
            block: None,
        }
    }
}

impl Switch {
    /// A switch with the given label, initially off.
    pub fn new<T: Into<Line>>(label: T) -> Self {
        Self {
            label: label.into(),
            ..Self::default()
        }
    }

    /// The state the app owns; `true` renders the on-position track.
    pub fn on(mut self, on: bool) -> Self {
        self.on = on;
        self
    }

    /// The base style of the label.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// The style of the track while on (patched over the base style).
    pub fn on_style(mut self, style: Style) -> Self {
        self.on_style = style;
        self
    }

    /// The style of the track while off (default: dimmed).
    pub fn off_style(mut self, style: Style) -> Self {
        self.off_style = style;
        self
    }

    /// Wraps the switch in a block; the row fills the block's inner area.
    pub fn block(mut self, block: Block) -> Self {
        self.block = Some(block);
        self
    }

    /// Maps the theme: the `text` slot styles the label, the `accent`
    /// slot the on-track and the `muted` slot the off-track.
    pub fn theme(mut self, theme: &Theme) -> Self {
        self.style = theme.text;
        self.on_style = theme.accent;
        self.off_style = theme.muted;
        self
    }

    /// Paints the switch into `buffer`, clipped to `area`.
    ///
    /// The row is `label + ' ' + track` on the area's first row; the
    /// track is [`TRACK_ON`] while on and [`TRACK_OFF`] while off, in
    /// the on/off style patched over the base style.
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
        let mut x = row_area.x;
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
                    self.style.patch(span.style),
                )
                .0;
        }
        if x >= right {
            return;
        }
        x = buffer.set_stringn(x, row, " ", right - x, self.style).0;
        if x >= right {
            return;
        }
        let track_style = if self.on {
            self.style.patch(self.on_style)
        } else {
            self.style.patch(self.off_style)
        };
        let track = if self.on { TRACK_ON } else { TRACK_OFF };
        buffer.set_stringn(x, row, track, right - x, track_style);
    }
}

impl Widget for Switch {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        Switch::render(&self, area, buffer);
    }
}

use tuvix_core::theme::Theme;
#[cfg(test)]
mod tests {
    use super::*;
    use tuvix_core::style::Color;

    fn buffer_16x1() -> Buffer {
        Buffer::empty(Rect::new(0, 0, 16, 1))
    }

    #[test]
    fn on_renders_the_leading_knob_track() {
        let mut buffer = buffer_16x1();
        Switch::new("dark mode")
            .on(true)
            .render(Rect::new(0, 0, 16, 1), &mut buffer);
        assert_eq!(buffer.to_string(), "dark mode [● ]  ");
    }

    #[test]
    fn off_renders_the_trailing_knob_track() {
        let mut buffer = buffer_16x1();
        Switch::new("dark mode").render(Rect::new(0, 0, 16, 1), &mut buffer);
        assert_eq!(buffer.to_string(), "dark mode [ ●]  ");
    }

    #[test]
    fn track_glyphs_are_the_documented_constants() {
        assert_eq!(TRACK_ON, "[● ]");
        assert_eq!(TRACK_OFF, "[ ●]");
    }

    #[test]
    fn track_takes_the_on_or_off_style() {
        let mut buffer = buffer_16x1();
        Switch::new("ab")
            .on(true)
            .on_style(Style::new().fg(Color::Cyan))
            .off_style(Style::new().fg(Color::DarkGray))
            .render(Rect::new(0, 0, 16, 1), &mut buffer);
        assert_eq!(buffer.get(3, 0).unwrap().style.fg, Some(Color::Cyan));
        assert_eq!(buffer.get(6, 0).unwrap().style.fg, Some(Color::Cyan));

        let mut buffer = buffer_16x1();
        Switch::new("ab")
            .on_style(Style::new().fg(Color::Cyan))
            .off_style(Style::new().fg(Color::DarkGray))
            .render(Rect::new(0, 0, 16, 1), &mut buffer);
        assert_eq!(buffer.get(3, 0).unwrap().style.fg, Some(Color::DarkGray));
    }

    #[test]
    fn label_keeps_the_base_style() {
        let mut buffer = buffer_16x1();
        Switch::new("ab")
            .style(Style::new().fg(Color::Gray))
            .on_style(Style::new().fg(Color::Cyan))
            .render(Rect::new(0, 0, 16, 1), &mut buffer);
        assert_eq!(buffer.get(0, 0).unwrap().style.fg, Some(Color::Gray));
    }

    #[test]
    fn off_track_dims_by_default() {
        let mut buffer = buffer_16x1();
        Switch::new("ab").render(Rect::new(0, 0, 16, 1), &mut buffer);
        assert_eq!(buffer.get(3, 0).unwrap().style.add_modifier, Modifier::DIM);
    }

    #[test]
    fn row_clips_at_the_right_edge() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 8, 1));
        Switch::new("dark mode")
            .on(true)
            .render(Rect::new(0, 0, 8, 1), &mut buffer);
        assert_eq!(buffer.to_string(), "dark mod");
    }

    #[test]
    fn block_wraps_and_fills_inner() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 14, 3));
        Switch::new("on")
            .on(true)
            .block(Block::bordered())
            .render(Rect::new(0, 0, 14, 3), &mut buffer);
        assert_eq!(
            buffer.to_string(),
            "┌────────────┐\n│on [● ]     │\n└────────────┘"
        );
    }

    #[test]
    fn theme_maps_text_accent_and_muted_slots() {
        let theme = Theme::dark();
        let mut buffer = buffer_16x1();
        Switch::new("ab")
            .theme(&theme)
            .render(Rect::new(0, 0, 16, 1), &mut buffer);
        assert_eq!(buffer.get(0, 0).unwrap().style.fg, theme.text.fg);
        assert_eq!(buffer.get(3, 0).unwrap().style, theme.muted);
        let mut buffer = buffer_16x1();
        Switch::new("ab")
            .on(true)
            .theme(&theme)
            .render(Rect::new(0, 0, 16, 1), &mut buffer);
        assert_eq!(buffer.get(3, 0).unwrap().style, theme.accent);
    }

    #[test]
    fn tall_area_uses_only_the_first_row() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 2));
        Switch::new("hi").render(Rect::new(0, 0, 10, 2), &mut buffer);
        assert_eq!(buffer.to_string(), "hi [ ●]   \n          ");
    }

    #[test]
    fn renders_through_the_widget_trait() {
        let mut buffer = buffer_16x1();
        Widget::render(Switch::new("a"), Rect::new(0, 0, 16, 1), &mut buffer);
        assert_eq!(buffer.to_string(), "a [ ●]          ");
    }

    #[test]
    fn empty_area_is_a_noop() {
        let mut buffer = buffer_16x1();
        Switch::new("a").render(Rect::ZERO, &mut buffer);
        assert!(
            buffer
                .content
                .iter()
                .all(tuvix_core::buffer::Cell::is_empty)
        );
    }
}
