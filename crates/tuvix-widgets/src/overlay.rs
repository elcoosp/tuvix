//! The floating-surface compositor: erase, then paint.
//!
//! [`Overlay`] is the M6 popup pattern's second half as a type. The app
//! resolves where the surface goes with
//! [`place`](tuvix_core::layer::place) — a dropdown below its trigger, a
//! modal centered, a tooltip at the pointer — and hands the widget to
//! [`Overlay`]: the area is erased through the buffer's pair-repairing
//! clear (so no orphaned wide-glyph halves survive under the surface)
//! and the wrapped widget paints on top. Z-order is paint order, the
//! painter's algorithm the rest of the framework speaks — overlays
//! painted last win the overlap, and a [`HitMap`](tuvix_core::hit::HitMap)
//! registers them after the base for the same result on the mouse side.
//!
//! The wrapper adds no state of its own: a stateful widget keeps its
//! state type, and the app drives it exactly as it would unpainted.
//!
//! # Examples
//!
//! ```
//! use tuvix_core::buffer::Buffer;
//! use tuvix_core::geometry::Rect;
//! use tuvix_core::style::Style;
//! use tuvix_core::widget::Widget;
//! use tuvix_widgets::Overlay;
//!
//! let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 1));
//! buffer.set_string(0, 0, "aaaaaaaaaa", Style::new());
//!
//! // A stateless surface: whatever sat under rows 0-4 is gone.
//! Overlay::new(tuvix_widgets::Clear).render(Rect::new(0, 0, 5, 1), &mut buffer);
//! assert_eq!(buffer.to_string(), "     aaaaa");
//! ```
//!
//! A stateful widget passes through the [`RenderWidget`] contract with
//! its own state — a floating select is still a select:
//!
//! ```
//! use tuvix_core::buffer::Buffer;
//! use tuvix_core::geometry::Rect;
//! use tuvix_core::widget::RenderWidget;
//! use tuvix_widgets::{Overlay, Select, SelectState};
//!
//! let mut buffer = Buffer::empty(Rect::new(0, 0, 20, 4));
//! let mut state = SelectState::new();
//! state.open();
//! Overlay::new(Select::new(["eu-west", "eu-north"]))
//!     .render(Rect::new(0, 0, 20, 4), &mut buffer, &mut state);
//! assert!(buffer.to_string().contains("eu-west"));
//! ```

use tuvix_core::buffer::Buffer;
use tuvix_core::geometry::Rect;
use tuvix_core::widget::{RenderWidget, Widget};

/// Paints one widget over a freshly erased area.
///
/// `Overlay` renders the same discipline every popup needs — clear the
/// region, then paint — so an app never re-rolls it and never forgets
/// the wide-glyph pair repair the clear performs. It owns nothing: no
/// position (the caller resolved it with `place`), no state (the
/// wrapped widget keeps its own).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Overlay<W> {
    /// The widget painted on top of the erased area.
    widget: W,
}

impl<W> Overlay<W> {
    /// Wraps `widget`: rendering the overlay erases the area, then
    /// paints `widget` into it.
    pub fn new(widget: W) -> Self {
        Self { widget }
    }

    /// The wrapped widget.
    pub fn inner(&self) -> &W {
        &self.widget
    }

    /// The wrapped widget, mutably.
    pub fn inner_mut(&mut self) -> &mut W {
        &mut self.widget
    }

    /// Unwraps the overlay, giving back the widget.
    pub fn into_inner(self) -> W {
        self.widget
    }
}

impl<W: Widget> Widget for Overlay<W> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        buf.clear(area);
        self.widget.render(area, buf);
    }
}

impl<W: RenderWidget> RenderWidget for Overlay<W> {
    type State = W::State;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        buf.clear(area);
        self.widget.render(area, buf, state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tuvix_core::style::Style;

    struct Label(&'static str);

    impl Widget for Label {
        fn render(self, area: Rect, buf: &mut Buffer) {
            buf.set_string(area.x, area.y, self.0, Style::new());
        }
    }

    #[test]
    fn erases_then_paints_in_one_render() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 6, 1));
        buffer.set_string(0, 0, "aaaaaa", Style::new());
        Overlay::new(Label("b")).render(Rect::new(2, 0, 2, 1), &mut buffer);
        assert_eq!(buffer.to_string(), "aab aa");
    }

    #[test]
    fn accessors_reach_the_wrapped_widget() {
        let overlay = Overlay::new(Label("x"));
        assert_eq!(overlay.inner().0, "x");
        let mut overlay = overlay;
        overlay.inner_mut().0 = "y";
        assert_eq!(overlay.into_inner().0, "y");
    }
}
