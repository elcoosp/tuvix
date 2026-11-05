//! The widget contracts: [`Widget`] for stateless painting and
//! [`StatefulWidget`] for widgets that need cross-frame state.
//!
//! Since M3 stateful framework widgets implement [`RenderWidget`] — one
//! `render(self, area, buf, state)` entry point with the state type as
//! an associated type, so renderers and app loops call every stateful
//! widget through a single uniform method. A `RenderWidget` type with
//! `State != ()` is callable as `widget.render(area, buf, state)`
//! directly; `Widget` and `StatefulWidget` remain public so hand-rolled
//! app widgets keep compiling unchanged, and stateless widgets stay
//! plain [`Widget`]s.
//!
//! Widgets are pure functions of `(self, area, &mut Buffer)`; stateful
//! widgets additionally get exclusive access to their state, so a scroll
//! position or a selection can live across frames without the widget
//! itself becoming stateful.
//!
//! # Examples
//!
//! ```
//! use tuvix_core::buffer::Buffer;
//! use tuvix_core::geometry::Rect;
//! use tuvix_core::widget::{StatefulWidget, Widget};
//!
//! struct Banner(&'static str);
//!
//! impl Widget for Banner {
//!     fn render(self, area: Rect, buf: &mut Buffer) {
//!         buf.set_string(area.x, area.y, self.0, Default::default());
//!     }
//! }
//!
//! let mut buffer = Buffer::empty(Rect::new(0, 0, 5, 1));
//! Banner("hi").render(Rect::new(0, 0, 5, 1), &mut buffer);
//! assert_eq!(buffer.to_string(), "hi   ");
//! ```
//!
//! A stateful widget implementing [`RenderWidget`] is callable through
//! the contract method — the form the framework widgets use:
//!
//! ```
//! use tuvix_core::buffer::Buffer;
//! use tuvix_core::geometry::Rect;
//! use tuvix_core::widget::RenderWidget;
//!
//! struct Tally;
//!
//! impl RenderWidget for Tally {
//!     type State = usize;
//!
//!     fn render(self, area: Rect, buf: &mut Buffer, state: &mut usize) {
//!         *state += 1;
//!         buf.set_string(area.x, area.y, "#", Default::default());
//!     }
//! }
//!
//! let mut buffer = Buffer::empty(Rect::new(0, 0, 3, 1));
//! let mut count = 0_usize;
//! Tally.render(Rect::new(0, 0, 3, 1), &mut buffer, &mut count);
//! assert_eq!(count, 1);
//! assert_eq!(buffer.to_string(), "#  ");
//! ```

use crate::buffer::Buffer;
use crate::geometry::Rect;

/// A stateless painter: writes its appearance into `buf` restricted to
/// `area`.
pub trait Widget {
    /// Paints the widget into `buf`, clipped to `area`.
    fn render(self, area: Rect, buf: &mut Buffer);
}

/// A painter that needs cross-frame state (scroll offsets, selections, ...).
///
/// The state lives outside the widget: the caller owns it, mutates it in
/// response to events, and hands it back on every frame.
pub trait StatefulWidget {
    /// The state this widget reads and mutates while rendering.
    type State;

    /// Paints the widget into `buf`, clipped to `area`, updating `state`.
    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State);

    /// Explicit-state convenience mirror of [`StatefulWidget::render`].
    fn render_stateful(self, area: Rect, buf: &mut Buffer, state: &mut Self::State)
    where
        Self: Sized,
    {
        Self::render(self, area, buf, state);
    }
}

/// The unified stateful authoring contract (M3): one
/// `render(self, area, buf, state)` entry point with the state type as
/// an associated type.
///
/// A `RenderWidget` type with `State != ()` is callable as
/// `widget.render(area, buf, state)` — the trait is the widget's only
/// `render`-named impl, so the method call resolves to it directly.
/// Stateless widgets stay plain [`Widget`]s: a generic blanket
/// `impl<R: RenderWidget<State = ()>> Widget for R` would keep
/// `widget.render(area, buf)` a two-candidate call (E0034) for every
/// widget that implements both traits, so the contract deliberately
/// does not blanket-implement anything.
///
/// [`StatefulWidget`] remains available for hand-rolled widgets that
/// prefer the classic trait.
pub trait RenderWidget {
    /// The per-frame mutable state; `()` for stateless widgets.
    type State;

    /// Paints the widget into `buf`, clipped to `area`, updating `state`.
    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::Style;

    struct Stamp(&'static str, usize);

    impl Widget for Stamp {
        fn render(self, area: Rect, buf: &mut Buffer) {
            for index in 0..self.1.min(usize::from(area.width)) {
                buf.set_string(area.x + index as u16, area.y, self.0, Style::new());
            }
        }
    }

    struct Counter {
        draws: usize,
    }

    struct Counting;

    impl StatefulWidget for Counting {
        type State = Counter;

        fn render(self, area: Rect, buf: &mut Buffer, state: &mut Counter) {
            state.draws += 1;
            buf.set_string(area.x, area.y, "*", Style::new());
        }
    }

    #[test]
    fn stateless_widget_paints_through_the_trait() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 6, 1));
        Stamp("x", 3).render(Rect::new(0, 0, 6, 1), &mut buffer);
        assert_eq!(buffer.to_string(), "xxx   ");
    }

    #[test]
    fn rendering_clips_to_area() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 6, 1));
        Stamp("x", 99).render(Rect::new(0, 0, 2, 1), &mut buffer);
        assert_eq!(buffer.to_string(), "xx    ");
    }

    #[test]
    fn stateful_widget_mutates_state_per_frame() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 3, 1));
        let mut state = Counter { draws: 0 };
        Counting.render(Rect::new(0, 0, 3, 1), &mut buffer, &mut state);
        Counting.render(Rect::new(0, 0, 3, 1), &mut buffer, &mut state);
        assert_eq!(state.draws, 2);
        assert_eq!(buffer.to_string(), "*  ");
    }

    // M3 — the RenderWidget contract for stateful widgets.

    struct Tally {
        paints: usize,
    }

    struct Tallying;

    impl RenderWidget for Tallying {
        type State = Tally;

        fn render(self, area: Rect, buf: &mut Buffer, state: &mut Tally) {
            state.paints += 1;
            buf.set_string(area.x, area.y, "#", Style::new());
        }
    }

    #[test]
    fn stateful_render_widget_paints_through_the_contract() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 1, 1));
        let mut state = Tally { paints: 0 };
        RenderWidget::render(Tallying, Rect::new(0, 0, 1, 1), &mut buffer, &mut state);
        RenderWidget::render(Tallying, Rect::new(0, 0, 1, 1), &mut buffer, &mut state);
        assert_eq!(state.paints, 2);
        assert_eq!(buffer.to_string(), "#");
    }

    #[test]
    fn stateful_render_widget_is_method_callable() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 1, 1));
        let mut state = Tally { paints: 0 };
        Tallying.render(Rect::new(0, 0, 1, 1), &mut buffer, &mut state);
        assert_eq!(state.paints, 1);
        assert_eq!(buffer.to_string(), "#");
    }

    #[test]
    fn state_type_exposes_through_the_contract() {
        fn assert_state<S>(_w: impl RenderWidget<State = S>) {}
        assert_state::<Tally>(Tallying);
    }
}
