//! Spinner: the animated indicator, the M13 motion layer's first widget.
//!
//! The glyph set and the period are the paint configuration ([`Spinner`]);
//! the clock is app state ([`SpinnerState`]) advanced with elapsed time.
//! The frame index is a pure function of elapsed time — the M13 "the
//! clock owns the value" discipline — so a 60 fps loop and a 5 fps one
//! land on the same glyph, and a stopped spinner freezes because its
//! clock stops accumulating. Like `Checkbox` and `LineGauge` this is a
//! one-row widget: a tall area only uses its first row.
//!
//! # Examples
//!
//! ```
//! use std::time::Duration;
//!
//! use tuvix_core::buffer::Buffer;
//! use tuvix_core::geometry::Rect;
//! use tuvix_widgets::{Spinner, SpinnerState};
//!
//! let mut state = SpinnerState::new();
//! state.advance(Duration::from_millis(160)); // two 80 ms periods
//! let mut buffer = Buffer::empty(Rect::new(0, 0, 12, 1));
//! Spinner::dots()
//!     .label("loading")
//!     .render(Rect::new(0, 0, 12, 1), &mut buffer, &mut state);
//! assert_eq!(buffer.to_string(), "⠹ loading   ");
//! ```

use std::time::Duration;

use tuvix_core::buffer::Buffer;
use tuvix_core::geometry::Rect;
use tuvix_core::style::Style;
use tuvix_core::text::{Line, Span};
use tuvix_core::theme::Theme;
use tuvix_core::widget::RenderWidget;

use crate::block::Block;

/// The app-owned clock a [`Spinner`] reads.
///
/// A fresh state is running with a zeroed clock: advancing accumulates
/// elapsed time, stopping freezes it (advance becomes a no-op), resetting
/// zeroes it. The state is `Copy` cheap data — clone it, share it, the
/// widget never owns it.
///
/// ```
/// use std::time::Duration;
///
/// use tuvix_widgets::SpinnerState;
///
/// let mut state = SpinnerState::new();
/// assert!(state.is_running());
/// state.advance(Duration::from_millis(80));
/// assert_eq!(state.elapsed(), Duration::from_millis(80));
/// state.stop();
/// state.advance(Duration::from_secs(9));
/// assert_eq!(state.elapsed(), Duration::from_millis(80));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpinnerState {
    elapsed: Duration,
    running: bool,
}

impl Default for SpinnerState {
    fn default() -> Self {
        Self {
            elapsed: Duration::ZERO,
            running: true,
        }
    }
}

impl SpinnerState {
    /// A running state with a zeroed clock.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Accumulates `dt` into the clock — only while running. Saturates at
    /// [`Duration::MAX`] instead of overflowing.
    pub fn advance(&mut self, dt: Duration) {
        if self.running {
            self.elapsed = self.elapsed.saturating_add(dt);
        }
    }

    /// Freezes the clock: advances become no-ops, the frame stays put.
    pub fn stop(&mut self) {
        self.running = false;
    }

    /// Resumes the clock from the frozen frame.
    pub fn start(&mut self) {
        self.running = true;
    }

    /// Zeroes the clock; the run state is unchanged.
    pub fn reset(&mut self) {
        self.elapsed = Duration::ZERO;
    }

    /// The accumulated running time.
    #[must_use]
    pub const fn elapsed(&self) -> Duration {
        self.elapsed
    }

    /// Whether the clock is accumulating.
    #[must_use]
    pub const fn is_running(&self) -> bool {
        self.running
    }
}

/// The animated indicator: one cycling glyph plus an optional label.
///
/// The curated sets ([`Spinner::dots`], [`Spinner::line`],
/// [`Spinner::arc`], [`Spinner::toggle`], [`Spinner::bounce`]) are all
/// single-width glyphs; a custom set renders verbatim and an empty set
/// renders the label alone. Stateful through the [`RenderWidget`]
/// contract: `spinner.render(area, buf, &mut state)`.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
///
/// use tuvix_core::buffer::Buffer;
/// use tuvix_core::geometry::Rect;
/// use tuvix_core::theme::Theme;
/// use tuvix_widgets::{Spinner, SpinnerState};
///
/// let spinner = Spinner::arc().theme(&Theme::dark()).label("syncing");
/// let mut state = SpinnerState::new();
/// state.advance(Duration::from_millis(200));
///
/// let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 1));
/// spinner.render(Rect::new(0, 0, 10, 1), &mut buffer, &mut state);
/// assert!(buffer.to_string().starts_with('◝'));
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Spinner {
    glyphs: Vec<Span>,
    period: Duration,
    label: Line,
    style: Style,
    stopped_style: Style,
    block: Option<Block>,
}

/// The classic ten-glyph braille dots set.
fn dots_glyphs() -> Vec<Span> {
    ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]
        .into_iter()
        .map(Span::raw)
        .collect()
}

impl Default for Spinner {
    fn default() -> Self {
        Self {
            glyphs: dots_glyphs(),
            period: Duration::from_millis(80),
            label: Line::default(),
            style: Style::new(),
            stopped_style: Style::new(),
            block: None,
        }
    }
}

impl Spinner {
    /// The classic braille dots spinner (10 glyphs, 80 ms period).
    #[must_use]
    pub fn dots() -> Self {
        Self::default()
    }

    /// The ASCII line spinner: `| / - \` (4 glyphs).
    #[must_use]
    pub fn line() -> Self {
        Self {
            glyphs: ["|", "/", "-", "\\"].into_iter().map(Span::raw).collect(),
            ..Self::default()
        }
    }

    /// The six-glyph arc spinner: `◜ ◠ ◝ ◞ ◡ ◟`.
    #[must_use]
    pub fn arc() -> Self {
        Self {
            glyphs: ["◜", "◠", "◝", "◞", "◡", "◟"]
                .into_iter()
                .map(Span::raw)
                .collect(),
            ..Self::default()
        }
    }

    /// The four-glyph contrast spinner: `◐ ◓ ◑ ◒`.
    #[must_use]
    pub fn toggle() -> Self {
        Self {
            glyphs: ["◐", "◓", "◑", "◒"].into_iter().map(Span::raw).collect(),
            ..Self::default()
        }
    }

    /// The four-glyph braille bounce: `⠁ ⠂ ⠄ ⠂` — out and back.
    #[must_use]
    pub fn bounce() -> Self {
        Self {
            glyphs: ["⠁", "⠂", "⠄", "⠂"].into_iter().map(Span::raw).collect(),
            ..Self::default()
        }
    }

    /// A spinner over the app's own glyphs, rendered in order. An empty
    /// set renders the label only.
    #[must_use]
    pub fn custom<I, T>(glyphs: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<Span>,
    {
        Self {
            glyphs: glyphs.into_iter().map(Into::into).collect(),
            ..Self::default()
        }
    }

    /// The clock period per glyph step (default 80 ms). A zero period
    /// pins frame 0.
    #[must_use]
    pub fn period(mut self, period: Duration) -> Self {
        self.period = period;
        self
    }

    /// The label rendered after the glyph; clips at the right edge.
    #[must_use]
    pub fn label<T: Into<Line>>(mut self, label: T) -> Self {
        self.label = label.into();
        self
    }

    /// The base style of the glyph and the label while running.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Patched over the base style while the spinner is stopped.
    #[must_use]
    pub fn stopped_style(mut self, style: Style) -> Self {
        self.stopped_style = style;
        self
    }

    /// Wraps the spinner in a block; the row fills the block's inner area.
    #[must_use]
    pub fn block(mut self, block: Block) -> Self {
        self.block = Some(block);
        self
    }

    /// Maps the theme: the `accent` slot styles the running spinner, the
    /// `muted` slot patches it while stopped.
    #[must_use]
    pub fn theme(mut self, theme: &Theme) -> Self {
        self.style = theme.accent;
        self.stopped_style = theme.muted;
        self
    }

    /// The frame index for the state's elapsed time: `elapsed / period`
    /// modulo the glyph count. Zero glyphs or a zero period pin frame 0;
    /// the result is always a valid index into the glyph set.
    #[must_use]
    pub fn frame(&self, state: &SpinnerState) -> usize {
        if self.glyphs.is_empty() || self.period.is_zero() {
            return 0;
        }
        let periods = state.elapsed().as_nanos() / self.period.as_nanos();
        usize::try_from(periods % self.glyphs.len() as u128).unwrap_or(0)
    }

    /// Paints the spinner into `buffer`, clipped to `area`.
    ///
    /// The row is `glyph + ' ' + label` on the area's first row (the
    /// space and the label only when a label is set). While stopped,
    /// [`Spinner::stopped_style`] patches every cell of the row.
    pub fn render(&self, area: Rect, buffer: &mut Buffer, state: &SpinnerState) {
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
        let style = if state.is_running() {
            self.style
        } else {
            self.style.patch(self.stopped_style)
        };

        let mut x = row_area.x;
        if !self.glyphs.is_empty() {
            let glyph = &self.glyphs[self.frame(state)];
            x = buffer
                .set_stringn(
                    x,
                    row,
                    glyph.content.as_ref(),
                    right - x,
                    style.patch(glyph.style),
                )
                .0;
            if !self.label.spans.is_empty() && x < right {
                // One separator column between the glyph and the label;
                // the label renders alone when the set is empty.
                x = buffer.set_stringn(x, row, " ", right - x, style).0;
            }
        }
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

impl RenderWidget for Spinner {
    type State = SpinnerState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        Spinner::render(&self, area, buf, state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tuvix_core::style::{Color, Modifier};
    use tuvix_core::stylize::Stylize as _;

    #[test]
    fn doctest_frame_matches_the_period_math() {
        let spinner = Spinner::line();
        let mut state = SpinnerState::new();
        state.advance(Duration::from_millis(250)); // 250/80 = 3 whole periods
        assert_eq!(spinner.frame(&state), 3);
    }

    #[test]
    fn named_sets_differ_in_length() {
        assert_eq!(Spinner::dots().glyphs.len(), 10);
        assert_eq!(Spinner::line().glyphs.len(), 4);
        assert_eq!(Spinner::arc().glyphs.len(), 6);
        assert_eq!(Spinner::toggle().glyphs.len(), 4);
        assert_eq!(Spinner::bounce().glyphs.len(), 4);
    }

    #[test]
    fn advance_while_stopped_is_a_noop_and_start_resumes() {
        let spinner = Spinner::dots();
        let mut state = SpinnerState::new();
        state.stop();
        state.advance(Duration::from_secs(1));
        assert_eq!(state.elapsed(), Duration::ZERO);
        state.start();
        state.advance(Duration::from_millis(80));
        assert_eq!(spinner.frame(&state), 1);
    }

    #[test]
    fn frame_is_always_a_valid_index() {
        let spinner = Spinner::custom(["a", "b"]);
        let mut state = SpinnerState::new();
        state.advance(Duration::from_secs(123_456));
        assert!(spinner.frame(&state) < 2);
    }

    #[test]
    fn label_styles_patch_over_the_base() {
        let spinner = Spinner::dots().style(Style::new().fg(Color::Cyan)).label(
            tuvix_core::text::Span::styled("hot", Style::new().fg(Color::Red)),
        );
        let mut state = SpinnerState::new();
        let mut buffer = Buffer::empty(Rect::new(0, 0, 6, 1));
        spinner.render(Rect::new(0, 0, 6, 1), &mut buffer, &mut state);
        assert_eq!(buffer.get(0, 0).unwrap().style.fg, Some(Color::Cyan));
        assert_eq!(buffer.get(2, 0).unwrap().style.fg, Some(Color::Red));
    }

    #[test]
    fn stopped_style_dim_applies() {
        let spinner = Spinner::dots().stopped_style(Style::new().dim());
        let mut state = SpinnerState::new();
        state.stop();
        let mut buffer = Buffer::empty(Rect::new(0, 0, 2, 1));
        spinner.render(Rect::new(0, 0, 2, 1), &mut buffer, &mut state);
        assert_eq!(buffer.get(0, 0).unwrap().style.add_modifier, Modifier::DIM);
    }
}
