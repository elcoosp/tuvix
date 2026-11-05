//! Scrollbars: a position thumb over a styled track.
//!
//! The thumb geometry is pure integer math (no float, deterministic
//! across platforms): thumb length is `track * viewport / content`
//! (clamped to at least one cell), and the thumb start maps the scroll
//! position across the free track linearly. A [`Scrollbar`] is a
//! [`RenderWidget`] with `State = ScrollbarState`; render it with
//! `scrollbar.render(area, buf, &mut state)`.

use tuvix_core::buffer::Buffer;
use tuvix_core::geometry::Rect;
use tuvix_core::style::Modifier;
use tuvix_core::style::Style;
use tuvix_core::widget::RenderWidget;

/// Where a [`Scrollbar`] sits and which default glyphs it uses.
///
/// # Examples
///
/// ```
/// use tuvix_widgets::ScrollbarOrientation;
///
/// // Vertical orientations are the default; horizontal ones use the
/// // ─ / ← / → glyph family instead of │ / ↑ / ↓.
/// assert_eq!(ScrollbarOrientation::default(), ScrollbarOrientation::VerticalRight);
/// ```
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScrollbarOrientation {
    /// A vertical track on the right edge of the area.
    #[default]
    VerticalRight,
    /// A vertical track on the left edge of the area.
    VerticalLeft,
    /// A horizontal track on the top row of the area.
    HorizontalTop,
    /// A horizontal track on the bottom row of the area.
    HorizontalBottom,
}

impl ScrollbarOrientation {
    /// `(track, thumb, begin, end)` glyph defaults for the orientation.
    fn default_symbols(self) -> (&'static str, &'static str, &'static str, &'static str) {
        match self {
            Self::VerticalRight | Self::VerticalLeft => ("│", "█", "↑", "↓"),
            Self::HorizontalTop | Self::HorizontalBottom => ("─", "█", "←", "→"),
        }
    }

    /// `true` when the track runs top-to-bottom.
    fn is_vertical(self) -> bool {
        matches!(self, Self::VerticalRight | Self::VerticalLeft)
    }
}

/// The scroll geometry a [`Scrollbar`] renders from.
///
/// # Examples
///
/// ```
/// use tuvix_widgets::ScrollbarState;
///
/// let mut state = ScrollbarState::new(100);
/// state.position(25);
/// assert_eq!(state.get_position(), 25);
/// assert_eq!(state.get_content_length(), 100);
/// ```
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScrollbarState {
    content_length: usize,
    position: usize,
    viewport_content_length: usize,
}

impl ScrollbarState {
    /// Creates a state for content of `content_length` units.
    pub fn new(content_length: usize) -> Self {
        Self {
            content_length,
            position: 0,
            viewport_content_length: 0,
        }
    }

    /// Sets the total content length (builder style).
    pub fn content_length(&mut self, content_length: usize) -> &mut Self {
        self.content_length = content_length;
        self
    }

    /// Sets the scroll position in content units (clamped at render).
    pub fn position(&mut self, position: usize) -> &mut Self {
        self.position = position;
        self
    }

    /// Sets how many content units are visible (builder style). Zero
    /// means "unknown": the track length is used instead.
    pub fn viewport_content_length(&mut self, viewport_content_length: usize) -> &mut Self {
        self.viewport_content_length = viewport_content_length;
        self
    }

    /// The stored scroll position.
    pub fn get_position(&self) -> usize {
        self.position
    }

    /// The stored content length.
    pub fn get_content_length(&self) -> usize {
        self.content_length
    }

    /// The stored viewport content length.
    pub fn get_viewport_content_length(&self) -> usize {
        self.viewport_content_length
    }
}

/// A scrollbar track with a position thumb.
///
/// The scrollbar renders into one column (vertical orientations) or one
/// row (horizontal) of the given area — the rightmost column by
/// default. Pair it with a widget's inner area shrunk by the scrollbar
/// margin.
///
/// # Examples
///
/// ```
/// use tuvix_core::buffer::Buffer;
/// use tuvix_core::geometry::Rect;
/// use tuvix_core::widget::RenderWidget;
/// use tuvix_widgets::{Scrollbar, ScrollbarOrientation, ScrollbarState};
///
/// let mut buffer = Buffer::empty(Rect::new(0, 0, 1, 3));
/// let mut state = ScrollbarState::new(4);
/// state.position(0).viewport_content_length(2);
/// let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight);
/// scrollbar.render(Rect::new(0, 0, 1, 3), &mut buffer, &mut state);
/// // Track of 3, thumb = 3 * 2 / 4 = 1, position 0 pins it to the top
/// // below the begin symbol: "↑│↓".
/// assert_eq!(buffer.to_string(), "↑\n█\n↓");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Scrollbar {
    orientation: ScrollbarOrientation,
    track_symbol: Option<&'static str>,
    thumb_symbol: &'static str,
    begin_symbol: Option<&'static str>,
    end_symbol: Option<&'static str>,
    track_style: Style,
    thumb_style: Style,
    begin_style: Style,
    end_style: Style,
}

impl Default for Scrollbar {
    /// The default scrollbar: a vertical track on the right edge.
    fn default() -> Self {
        Self::new(ScrollbarOrientation::VerticalRight)
    }
}

impl Scrollbar {
    /// Creates a scrollbar for the orientation, with its default glyphs.
    pub fn new(orientation: ScrollbarOrientation) -> Self {
        let (track, thumb, begin, end) = orientation.default_symbols();
        Self {
            orientation,
            track_symbol: Some(track),
            thumb_symbol: thumb,
            begin_symbol: Some(begin),
            end_symbol: Some(end),
            track_style: Style::new(),
            thumb_style: Style::new(),
            begin_style: Style::new(),
            end_style: Style::new(),
        }
    }

    /// Overrides the track glyph (`None` hides the track cells).
    pub fn track_symbol(mut self, symbol: Option<&'static str>) -> Self {
        self.track_symbol = symbol;
        self
    }

    /// Overrides the thumb glyph.
    pub fn thumb_symbol(mut self, symbol: &'static str) -> Self {
        self.thumb_symbol = symbol;
        self
    }

    /// Overrides the begin glyph (`None` removes the begin cell).
    pub fn begin_symbol(mut self, symbol: Option<&'static str>) -> Self {
        self.begin_symbol = symbol;
        self
    }

    /// Overrides the end glyph (`None` removes the end cell).
    pub fn end_symbol(mut self, symbol: Option<&'static str>) -> Self {
        self.end_symbol = symbol;
        self
    }

    /// Sets the style of the track cells.
    pub fn track_style(mut self, style: Style) -> Self {
        self.track_style = style;
        self
    }

    /// Sets the style of the thumb cells.
    pub fn thumb_style(mut self, style: Style) -> Self {
        self.thumb_style = style;
        self
    }

    /// Sets the style of the begin glyph.
    pub fn begin_style(mut self, style: Style) -> Self {
        self.begin_style = style;
        self
    }

    /// Sets the style of the end glyph.
    pub fn end_style(mut self, style: Style) -> Self {
        self.end_style = style;
        self
    }

    /// The configured begin glyph (for layout math and assertions).
    pub fn begin_glyph(&self) -> Option<&'static str> {
        self.begin_symbol
    }

    /// The configured end glyph (for layout math and assertions).
    pub fn end_glyph(&self) -> Option<&'static str> {
        self.end_symbol
    }

    /// The configured orientation.
    pub fn orientation(&self) -> ScrollbarOrientation {
        self.orientation
    }

    /// Paints the scrollbar into `buffer`, clipped to `area`, reading
    /// the state (position clamping happens here, not in the state).
    ///
    /// Called through [`RenderWidget::render`]; the method-call form is
    /// `scrollbar.render(area, buffer, &mut state)`.
    fn paint(self, area: Rect, buffer: &mut Buffer, state: &ScrollbarState) {
        if area.is_empty() {
            return;
        }
        let vertical = self.orientation.is_vertical();
        let (axis_start, axis_len) = if vertical {
            (area.y, area.height)
        } else {
            (area.x, area.width)
        };
        if axis_len == 0 {
            return;
        }

        // Begin / end symbols bookend the track.
        let mut cells_start = u32::from(axis_start);
        let mut cells_len = u32::from(axis_len);
        if self.begin_symbol.is_some() {
            cells_start += 1;
            cells_len = cells_len.saturating_sub(1);
        }
        if self.end_symbol.is_some() {
            cells_len = cells_len.saturating_sub(1);
        }

        // Thumb geometry: pure integer math over the usable track.
        let view = if state.viewport_content_length == 0 {
            cells_len
        } else {
            u32::try_from(state.viewport_content_length).unwrap_or(u32::MAX)
        };
        let content = u32::try_from(state.content_length).unwrap_or(u32::MAX);
        let (thumb_start, thumb_len) = if content == 0 || cells_len == 0 {
            (0, cells_len)
        } else {
            let thumb_len = (cells_len * view / content).clamp(1, cells_len);
            let max_scroll = content.saturating_sub(view);
            let position = u32::try_from(state.position)
                .unwrap_or(u32::MAX)
                .min(max_scroll);
            let start = position
                .saturating_mul(cells_len - thumb_len)
                .checked_div(max_scroll)
                .unwrap_or(0);
            (start, thumb_len)
        };

        for offset in 0..cells_len {
            let in_thumb = offset >= thumb_start && offset < thumb_start + thumb_len;
            let (symbol, style) = if in_thumb {
                (self.thumb_symbol, self.thumb_style)
            } else {
                match self.track_symbol {
                    Some(symbol) => (symbol, self.track_style),
                    None => continue,
                }
            };
            self.write_cell(buffer, area, cells_start + offset, symbol, style);
        }
        if let Some(symbol) = self.begin_symbol {
            self.write_cell(
                buffer,
                area,
                u32::from(axis_start),
                symbol,
                self.begin_style,
            );
        }
        if let Some(symbol) = self.end_symbol {
            let end_cell = u32::from(axis_start) + u32::from(axis_len) - 1;
            self.write_cell(buffer, area, end_cell, symbol, self.end_style);
        }
    }

    /// Writes one cell of the track; `cell` is an absolute position along
    /// the orientation axis.
    fn write_cell(&self, buffer: &mut Buffer, area: Rect, cell: u32, symbol: &str, style: Style) {
        let (x, y) = if self.orientation.is_vertical() {
            let x = match self.orientation {
                ScrollbarOrientation::VerticalLeft => area.x,
                _ => area.right().saturating_sub(1),
            };
            (x, u16::try_from(cell).unwrap_or(u16::MAX))
        } else {
            let y = match self.orientation {
                ScrollbarOrientation::HorizontalTop => area.y,
                _ => area.bottom().saturating_sub(1),
            };
            (u16::try_from(cell).unwrap_or(u16::MAX), y)
        };
        if let Some(target) = buffer.get_mut(x, y) {
            target.set_symbol(symbol).set_style(style);
        }
    }
}

impl RenderWidget for Scrollbar {
    type State = ScrollbarState;

    fn render(self, area: Rect, buffer: &mut Buffer, state: &mut Self::State) {
        Scrollbar::paint(self, area, buffer, state);
    }
}

impl Scrollbar {
    /// Maps the theme onto the scrollbar: the `scrollbar` slot styles the
    /// track; the thumb inverts it so it stays visible over the track.
    pub fn theme(mut self, theme: &Theme) -> Self {
        self.track_style = theme.scrollbar;
        self.thumb_style = theme.scrollbar.add_modifier(Modifier::REVERSED);
        self
    }
}

use tuvix_core::theme::Theme;
#[cfg(test)]
mod tests {
    use super::*;
    use tuvix_core::style::Color;

    fn render_v(buffer: &mut Buffer, state: &mut ScrollbarState, scrollbar: Scrollbar) -> String {
        RenderWidget::render(scrollbar, Rect::new(0, 0, 1, 5), buffer, state);
        buffer.to_string()
    }

    #[test]
    fn zero_content_fills_the_track() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 1, 5));
        let rendered = render_v(
            &mut buffer,
            &mut ScrollbarState::new(0),
            Scrollbar::default(),
        );
        assert_eq!(rendered, "↑\n█\n█\n█\n↓");
    }

    #[test]
    fn thumb_scales_with_viewport_ratio() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 1, 5));
        // Track 5, usable 3; content 4, view 2 -> thumb = 3 * 2 / 4 = 1.
        let mut state = ScrollbarState::new(4);
        state.viewport_content_length(2);
        let rendered = render_v(&mut buffer, &mut state, Scrollbar::default());
        assert_eq!(rendered, "↑\n█\n│\n│\n↓");
    }

    #[test]
    fn position_maps_across_the_free_track() {
        // Usable track 3, thumb 1, max_scroll = 2: positions 0/1/2 pin
        // the thumb to top/middle/bottom.
        for (position, expected) in [
            (0_usize, "↑\n█\n│\n│\n↓"),
            (1_usize, "↑\n│\n█\n│\n↓"),
            (2_usize, "↑\n│\n│\n█\n↓"),
        ] {
            let mut buffer = Buffer::empty(Rect::new(0, 0, 1, 5));
            let mut state = ScrollbarState::new(4);
            state.viewport_content_length(2).position(position);
            let rendered = render_v(&mut buffer, &mut state, Scrollbar::default());
            assert_eq!(rendered, expected, "position {position}");
        }
    }

    #[test]
    fn position_clamps_to_max_scroll() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 1, 5));
        let mut state = ScrollbarState::new(4);
        state.viewport_content_length(2).position(999);
        let rendered = render_v(&mut buffer, &mut state, Scrollbar::default());
        assert_eq!(rendered, "↑\n│\n│\n█\n↓");
    }

    #[test]
    fn view_ge_content_means_full_thumb() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 1, 5));
        let mut state = ScrollbarState::new(2);
        state.viewport_content_length(2);
        let rendered = render_v(&mut buffer, &mut state, Scrollbar::default());
        assert_eq!(rendered, "↑\n█\n█\n█\n↓");
    }

    #[test]
    fn default_viewport_uses_the_track_length() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 1, 5));
        // No viewport: view = usable track 3; thumb = 3 * 3 / 9 = 1.
        let mut state = ScrollbarState::new(9);
        state.position(0);
        let rendered = render_v(&mut buffer, &mut state, Scrollbar::default());
        assert_eq!(rendered, "↑\n█\n│\n│\n↓");
    }

    #[test]
    fn symbols_can_be_removed_and_replaced() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 1, 5));
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(None)
            .thumb_symbol("▓");
        let mut state = ScrollbarState::new(0);
        RenderWidget::render(scrollbar, Rect::new(0, 0, 1, 5), &mut buffer, &mut state);
        assert_eq!(buffer.to_string(), "▓\n▓\n▓\n▓\n▓");
    }

    #[test]
    fn horizontal_bottom_renders_the_last_row() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 5, 2));
        let mut state = ScrollbarState::new(0);
        RenderWidget::render(
            Scrollbar::new(ScrollbarOrientation::HorizontalBottom),
            Rect::new(0, 0, 5, 2),
            &mut buffer,
            &mut state,
        );
        assert_eq!(buffer.to_string(), "     \n←███→");
    }

    #[test]
    fn vertical_left_renders_the_first_column() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 2, 3));
        let mut state = ScrollbarState::new(0);
        RenderWidget::render(
            Scrollbar::new(ScrollbarOrientation::VerticalLeft),
            Rect::new(0, 0, 2, 3),
            &mut buffer,
            &mut state,
        );
        assert_eq!(buffer.to_string(), "↑ \n█ \n↓ ");
    }

    #[test]
    fn styles_are_applied() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 1, 5));
        let scrollbar = Scrollbar::default()
            .begin_style(Style::new().fg(Color::Red))
            .thumb_style(Style::new().fg(Color::Green))
            .track_style(Style::new().fg(Color::Blue))
            .end_style(Style::new().fg(Color::Yellow));
        let mut state = ScrollbarState::new(4);
        state.viewport_content_length(2);
        RenderWidget::render(scrollbar, Rect::new(0, 0, 1, 5), &mut buffer, &mut state);
        assert_eq!(buffer.get(0, 0).unwrap().style.fg, Some(Color::Red));
        assert_eq!(buffer.get(0, 1).unwrap().style.fg, Some(Color::Green));
        assert_eq!(buffer.get(0, 2).unwrap().style.fg, Some(Color::Blue));
        assert_eq!(buffer.get(0, 4).unwrap().style.fg, Some(Color::Yellow));
    }

    #[test]
    fn empty_area_is_noop() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 4, 4));
        RenderWidget::render(
            Scrollbar::default(),
            Rect::ZERO,
            &mut buffer,
            &mut ScrollbarState::new(0),
        );
        assert!(
            buffer
                .content
                .iter()
                .all(tuvix_core::buffer::Cell::is_empty)
        );
    }

    #[test]
    fn state_builders_and_getters() {
        let mut state = ScrollbarState::new(10);
        state
            .position(3)
            .viewport_content_length(4)
            .content_length(20);
        assert_eq!(state.get_position(), 3);
        assert_eq!(state.get_content_length(), 20);
        assert_eq!(state.get_viewport_content_length(), 4);
    }

    #[test]
    fn scrollbar_renders_through_render_widget_contract() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 1, 3));
        let mut state = ScrollbarState::new(0);
        RenderWidget::render(
            Scrollbar::default(),
            Rect::new(0, 0, 1, 3),
            &mut buffer,
            &mut state,
        );
        assert_eq!(buffer.to_string(), "↑\n█\n↓");
    }
}
