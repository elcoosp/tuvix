//! Scrollable viewports: clip any widget to a window over larger content.
//!
//! A [`Scrollable`] wraps any [`RenderWidget`], paints it into an offscreen
//! scratch surface sized to the declared content extent, and blits the
//! window the [`ViewportState`] currently scrolls over. The scratch surface
//! lives in the state and the child is painted **once** — between
//! [`ViewportState::invalidate`] calls the frame is a pure blit, so a
//! scroll storm costs the window, never the content. Every write goes
//! through the journal-recorded
//! [`Buffer::get_mut`](tuvix_core::buffer::Buffer::get_mut), so the
//! persistent-DOM diff still sees exactly the blitted region.
//!
//! Semantics the tests pin down:
//!
//! - offsets are clamped to `content − viewport` at render, so a resize can
//!   never leave the window outside the content;
//! - a wide glyph whose pair would be split by the right window edge is
//!   clipped (blanked), never emitted as a lone leading half — the same
//!   terminal-pair honesty the buffer's write path enforces;
//! - a window starting on a covered half snaps back one column to the
//!   leading edge, so horizontal scrolling always starts on a whole glyph;
//! - wheel events move one line or column per notch — a primitive the app
//!   composes, not a policy.

use tuvix_core::buffer::{Buffer, Cell};
use tuvix_core::event::{MouseEvent, MouseEventKind};
use tuvix_core::geometry::{Rect, Size};
use tuvix_core::widget::RenderWidget;

use crate::scrollbar::{Scrollbar, ScrollbarState};

/// The scroll state of a [`Scrollable`]: the window offsets plus the
/// offscreen scratch surface the inner widget paints into.
///
/// The child's paint is cached: a frame where nothing changed but the
/// window blits, a frame where the content changed needs
/// [`ViewportState::invalidate`] first — the same explicit-dirtiness
/// contract the persistent-DOM journal uses one level down.
///
/// The state also remembers the last rendered viewport and the declared
/// content extent, so the scroll operations clamp honestly. Rendering
/// syncs both; before the first frame the app can prime them with
/// [`ViewportState::set_content`] (the viewport starts at `0 x 0`, so
/// before priming the clamps derive from the content alone).
///
/// The inner widget's own state is carried along — the app reaches it
/// through [`ViewportState::inner`] and [`ViewportState::inner_mut`].
///
/// # Examples
///
/// ```
/// use tuvix_core::geometry::Size;
/// use tuvix_widgets::ViewportState;
///
/// let mut state = ViewportState::new(());
/// state.set_content(Size::new(40, 100));
/// assert!(state.scroll_down(30));
/// // The viewport is still 0 x 0, so the clamp is the content height:
/// assert_eq!(state.offset_y(), 30);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewportState<S> {
    offset_y: u16,
    offset_x: u16,
    content: Size,
    viewport: Size,
    scratch: Option<(Size, Buffer)>,
    valid: bool,
    inner: S,
}

impl<S> ViewportState<S> {
    /// Creates the state around the inner widget's own state.
    pub fn new(inner: S) -> Self {
        Self {
            offset_y: 0,
            offset_x: 0,
            content: Size::new(0, 0),
            viewport: Size::new(0, 0),
            scratch: None,
            valid: false,
            inner,
        }
    }

    /// The vertical scroll offset in content rows.
    pub fn offset_y(&self) -> u16 {
        self.offset_y
    }

    /// The horizontal scroll offset in content columns.
    pub fn offset_x(&self) -> u16 {
        self.offset_x
    }

    /// The last rendered viewport size (`0 x 0` before the first frame).
    pub fn viewport(&self) -> Size {
        self.viewport
    }

    /// The declared content extent this state clamps against.
    pub fn content(&self) -> Size {
        self.content
    }

    /// Declares the content extent the scroll offsets clamp against.
    ///
    /// Rendering syncs this from the widget; priming it up front makes the
    /// scroll operations honest before the first frame.
    pub fn set_content(&mut self, content: Size) -> &mut Self {
        self.content = content;
        self.clamp_offsets();
        self
    }

    /// Marks the child's paint stale: the next frame re-paints the scratch
    /// surface before blitting.
    ///
    /// Call this whenever anything the inner widget reads changes — its
    /// state through [`ViewportState::inner_mut`], or the data it captured
    /// by value. A frame that only scrolls needs no invalidation.
    pub fn invalidate(&mut self) -> &mut Self {
        self.valid = false;
        self
    }

    /// Whether the cached child paint is current (no invalidation pending
    /// and the scratch surface exists at the right size).
    pub fn is_valid(&self) -> bool {
        self.valid
    }

    /// The inner widget's state.
    pub fn inner(&self) -> &S {
        &self.inner
    }

    /// The inner widget's state, mutably.
    pub fn inner_mut(&mut self) -> &mut S {
        &mut self.inner
    }

    /// The largest legal vertical offset for the current content/viewport.
    fn max_offset_y(&self) -> u16 {
        self.content.height.saturating_sub(self.viewport.height)
    }

    /// The largest legal horizontal offset for the current content/viewport.
    fn max_offset_x(&self) -> u16 {
        self.content.width.saturating_sub(self.viewport.width)
    }

    fn clamp_offsets(&mut self) {
        self.offset_y = self.offset_y.min(self.max_offset_y());
        self.offset_x = self.offset_x.min(self.max_offset_x());
    }

    fn shift_y(&mut self, delta: i32) -> bool {
        self.clamp_offsets();
        let current = i32::from(self.offset_y);
        let target = if delta >= 0 {
            current
                .saturating_add(delta)
                .min(i32::from(self.max_offset_y()))
        } else {
            current.saturating_add(delta).max(0)
        };
        let target = target.max(0) as u16;
        if target == self.offset_y {
            return false;
        }
        self.offset_y = target;
        true
    }

    fn shift_x(&mut self, delta: i32) -> bool {
        self.clamp_offsets();
        let current = i32::from(self.offset_x);
        let target = if delta >= 0 {
            current
                .saturating_add(delta)
                .min(i32::from(self.max_offset_x()))
        } else {
            current.saturating_add(delta).max(0)
        };
        let target = target.max(0) as u16;
        if target == self.offset_x {
            return false;
        }
        self.offset_x = target;
        true
    }

    /// Scrolls up by `rows` content rows; `true` when the offset changed.
    pub fn scroll_up(&mut self, rows: u16) -> bool {
        self.shift_y(-i32::from(rows))
    }

    /// Scrolls down by `rows` content rows; `true` when the offset changed.
    pub fn scroll_down(&mut self, rows: u16) -> bool {
        self.shift_y(i32::from(rows))
    }

    /// Scrolls left by `columns`; `true` when the offset changed.
    pub fn scroll_left(&mut self, columns: u16) -> bool {
        self.shift_x(-i32::from(columns))
    }

    /// Scrolls right by `columns`; `true` when the offset changed.
    pub fn scroll_right(&mut self, columns: u16) -> bool {
        self.shift_x(i32::from(columns))
    }

    /// Pages up by `viewport_height` rows; `true` when the offset changed.
    pub fn page_up(&mut self, viewport_height: u16) -> bool {
        self.shift_y(-i32::from(viewport_height))
    }

    /// Pages down by `viewport_height` rows; `true` when the offset changed.
    pub fn page_down(&mut self, viewport_height: u16) -> bool {
        self.shift_y(i32::from(viewport_height))
    }

    /// Jumps to the top of the content; `true` when the offset changed.
    pub fn to_top(&mut self) -> bool {
        self.shift_y(-i32::MAX)
    }

    /// Jumps to the bottom of the content; `true` when the offset changed.
    pub fn to_bottom(&mut self) -> bool {
        self.shift_y(i32::MAX)
    }

    /// Jumps to the left edge of the content; `true` when the offset changed.
    pub fn to_left_end(&mut self) -> bool {
        self.shift_x(-i32::MAX)
    }

    /// Jumps to the right edge of the content; `true` when the offset changed.
    pub fn to_right_end(&mut self) -> bool {
        self.shift_x(i32::MAX)
    }

    /// Applies a mouse wheel event: one row or column per wheel notch.
    ///
    /// All other mouse events are ignored (`false`).
    pub fn apply_wheel(&mut self, event: &MouseEvent) -> bool {
        match event.kind {
            MouseEventKind::ScrollUp => self.scroll_up(1),
            MouseEventKind::ScrollDown => self.scroll_down(1),
            MouseEventKind::ScrollLeft => self.scroll_left(1),
            MouseEventKind::ScrollRight => self.scroll_right(1),
            _ => false,
        }
    }
}

impl<S: Default> Default for ViewportState<S> {
    fn default() -> Self {
        Self::new(S::default())
    }
}

/// A viewport over larger content: any [`RenderWidget`] plus a declared
/// content extent, optionally with an attached vertical [`Scrollbar`].
///
/// The inner widget is painted at its full declared size into the state's
/// scratch surface, then the window [`ViewportState`] scrolls over is
/// blitted into the real buffer. When the content is taller than the
/// viewport and a scrollbar is attached, the scrollbar takes the last
/// column and the viewport shrinks by it.
///
/// # Examples
///
/// ```
/// use tuvix_core::buffer::Buffer;
/// use tuvix_core::geometry::{Rect, Size};
/// use tuvix_core::style::Style;
/// use tuvix_core::widget::RenderWidget;
/// use tuvix_widgets::{Scrollable, ViewportState};
///
/// struct Lines<'a>(&'a [&'a str]);
///
/// impl RenderWidget for Lines<'_> {
///     type State = ();
///     fn render(self, area: Rect, buf: &mut Buffer, _state: &mut ()) {
///         for (index, line) in self.0.iter().enumerate() {
///             buf.set_string(area.x, area.y + index as u16, *line, Style::new());
///         }
///     }
/// }
///
/// let content = ["one", "two", "three", "four"];
/// let mut buffer = Buffer::empty(Rect::new(0, 0, 5, 2));
/// let mut state = ViewportState::new(());
/// Scrollable::new(Lines(&content))
///     .content(Size::new(5, 4))
///     .render(Rect::new(0, 0, 5, 2), &mut buffer, &mut state);
/// assert_eq!(buffer.to_string(), "one  \ntwo  ");
///
/// state.scroll_down(2);
/// Scrollable::new(Lines(&content))
///     .content(Size::new(5, 4))
///     .render(Rect::new(0, 0, 5, 2), &mut buffer, &mut state);
/// assert_eq!(buffer.to_string(), "three\nfour ");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scrollable<W> {
    inner: W,
    content: Size,
    scrollbar: Option<Scrollbar>,
}

impl<W> Scrollable<W> {
    /// Wraps `inner`, whose painted extent is declared by `.content(...)`.
    ///
    /// Without a declared content extent the viewport cannot scroll
    /// (the clamps derive from `content − viewport`).
    pub fn new(inner: W) -> Self {
        Self {
            inner,
            content: Size::new(0, 0),
            scrollbar: None,
        }
    }

    /// Declares the painted extent of the inner widget.
    pub fn content(mut self, content: Size) -> Self {
        self.content = content;
        self
    }

    /// Attaches a vertical scrollbar (rendered in the last column when the
    /// content is taller than the viewport).
    pub fn scrollbar(mut self, scrollbar: Scrollbar) -> Self {
        self.scrollbar = Some(scrollbar);
        self
    }

    /// The declared content extent.
    pub fn get_content(&self) -> Size {
        self.content
    }

    /// The attached scrollbar, if any.
    pub fn get_scrollbar(&self) -> Option<&Scrollbar> {
        self.scrollbar.as_ref()
    }
}

impl<W: RenderWidget> RenderWidget for Scrollable<W> {
    type State = ViewportState<W::State>;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        if area.is_empty() {
            return;
        }

        // The scrollbar claims the last column only when it can help
        // (attached, content taller than the window, room for both).
        let scroll_slot = self
            .scrollbar
            .filter(|_| self.content.height > area.height)
            .filter(|_| area.width > 1);
        let (viewport_area, scrollbar) = match scroll_slot {
            Some(scrollbar) => {
                let column = Rect::new(area.right() - 1, area.y, 1, area.height);
                let viewport = Rect::new(area.x, area.y, area.width - 1, area.height);
                (viewport, Some((scrollbar, column)))
            }
            None => (area, None),
        };

        // Sync the clamping world and enforce it.
        state.content = self.content;
        state.viewport = viewport_area.size();
        state.clamp_offsets();

        // Paint the inner widget at its full extent into the scratch
        // surface — once per invalidation, not once per frame. A frame
        // that only scrolls blits the cached paint.
        let paint = Rect::new(
            0,
            0,
            self.content.width.max(viewport_area.width),
            self.content.height.max(viewport_area.height),
        );
        let needs_paint = !state.valid
            || state
                .scratch
                .as_ref()
                .is_none_or(|(size, _)| *size != paint.size());
        state
            .scratch
            .get_or_insert_with(|| (paint.size(), Buffer::empty(paint)));
        if needs_paint {
            let (size, scratch) = state.scratch.as_mut().expect("just inserted");
            *size = paint.size();
            scratch.resize(paint);
            scratch.reset();
            self.inner.render(paint, scratch, &mut state.inner);
        }
        state.valid = true;
        let scratch = &state.scratch.as_ref().expect("just inserted").1;

        // Snap a window starting on a covered half back to the leading
        // edge so horizontal scrolling always begins on a whole glyph.
        let mut offset_x = state.offset_x;
        if offset_x > 0
            && scratch
                .get(offset_x, state.offset_y)
                .is_some_and(|cell| cell.skip)
        {
            offset_x -= 1;
        }

        let window = Rect::new(
            offset_x,
            state.offset_y,
            viewport_area.width,
            viewport_area.height,
        );
        for row in 0..viewport_area.height {
            let y = window.y + row;
            for column in 0..viewport_area.width {
                let x = window.x + column;
                let Some(cell) = scratch.get(x, y) else {
                    continue;
                };
                let mut cell = cell.clone();
                // A wide glyph whose covered half falls outside the window
                // would arrive as a lone leading half mid-row: clip it.
                if !cell.skip
                    && column + 1 == viewport_area.width
                    && scratch.get(x + 1, y).is_some_and(|next| next.skip)
                {
                    cell = Cell::empty();
                }
                if let Some(target) = buf.get_mut(viewport_area.x + column, viewport_area.y + row) {
                    *target = cell;
                }
            }
        }

        if let Some((scrollbar, column)) = scrollbar {
            let mut thumb = ScrollbarState::new(usize::from(self.content.height));
            thumb
                .position(usize::from(state.offset_y))
                .viewport_content_length(usize::from(viewport_area.height));
            scrollbar.render(column, buf, &mut thumb);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tuvix_core::event::KeyModifiers;
    use tuvix_core::style::Style;

    /// A numbered-lines widget: paints `rows` rows of `r00`, `r01`, ...
    #[derive(Clone, Copy)]
    struct Lines {
        rows: u16,
    }

    impl RenderWidget for Lines {
        type State = ();

        fn render(self, area: Rect, buf: &mut Buffer, _state: &mut ()) {
            for row in 0..self.rows {
                buf.set_string(area.x, area.y + row, format!("r{row:02}"), Style::new());
            }
        }
    }

    /// A widget that paints one fixed string per row.
    #[derive(Clone, Copy)]
    struct Grid(&'static [&'static str]);

    impl RenderWidget for Grid {
        type State = ();

        fn render(self, area: Rect, buf: &mut Buffer, _state: &mut ()) {
            for (row, line) in self.0.iter().enumerate() {
                buf.set_string(area.x, area.y + row as u16, *line, Style::new());
            }
        }
    }

    fn wheel(kind: MouseEventKind) -> MouseEvent {
        MouseEvent {
            kind,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::empty(),
        }
    }

    #[test]
    fn viewport_shows_the_top_window_of_the_content() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 4, 2));
        let mut state = ViewportState::new(());
        Scrollable::new(Lines { rows: 4 })
            .content(Size::new(4, 4))
            .render(Rect::new(0, 0, 4, 2), &mut buffer, &mut state);
        assert_eq!(buffer.to_string(), "r00 \nr01 ");
        assert_eq!(state.offset_y(), 0);
        assert_eq!(state.viewport(), Size::new(4, 2));
    }

    #[test]
    fn scroll_down_moves_the_window() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 4, 2));
        let mut state = ViewportState::new(());
        state.set_content(Size::new(4, 4));
        state.viewport = Size::new(4, 2);
        assert!(state.scroll_down(2));
        Scrollable::new(Lines { rows: 4 })
            .content(Size::new(4, 4))
            .render(Rect::new(0, 0, 4, 2), &mut buffer, &mut state);
        assert_eq!(buffer.to_string(), "r02 \nr03 ");
        assert_eq!(state.offset_y(), 2);
    }

    #[test]
    fn scroll_clamps_to_content_bottom() {
        let mut state = ViewportState::new(());
        state.set_content(Size::new(4, 4));
        state.viewport = Size::new(4, 2);
        assert!(state.scroll_down(100));
        assert_eq!(state.offset_y(), 2);
        assert!(!state.scroll_down(1));
    }

    #[test]
    fn scroll_up_reports_false_at_the_top() {
        let mut state = ViewportState::new(());
        state.set_content(Size::new(4, 4));
        state.viewport = Size::new(4, 2);
        assert!(!state.scroll_up(3));
        assert_eq!(state.offset_y(), 0);
    }

    #[test]
    fn to_bottom_then_to_top_round_trip() {
        let mut state = ViewportState::new(());
        state.set_content(Size::new(4, 9));
        state.viewport = Size::new(4, 2);
        assert!(state.to_bottom());
        assert_eq!(state.offset_y(), 7);
        assert!(state.to_top());
        assert_eq!(state.offset_y(), 0);
    }

    #[test]
    fn page_down_moves_a_viewport_and_clamps() {
        let mut state = ViewportState::new(());
        state.set_content(Size::new(4, 9));
        state.viewport = Size::new(4, 4);
        assert!(state.page_down(4));
        assert_eq!(state.offset_y(), 4);
        assert!(state.page_down(4));
        assert_eq!(state.offset_y(), 5);
    }

    #[test]
    fn horizontal_scroll_shifts_columns() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 3, 1));
        let mut state = ViewportState::new(());
        state.set_content(Size::new(6, 1));
        state.viewport = Size::new(3, 1);
        assert!(state.scroll_right(1));
        Scrollable::new(Grid(&["abcdef"]))
            .content(Size::new(6, 1))
            .render(Rect::new(0, 0, 3, 1), &mut buffer, &mut state);
        assert_eq!(buffer.to_string(), "bcd");
    }

    #[test]
    fn wide_glyph_crossing_the_right_edge_is_clipped_not_split() {
        // "ab汉c" — 汉 covers columns 2-3. A 3-wide window from column 0
        // ends on the leading half; the pair must be clipped, never split.
        let mut buffer = Buffer::empty(Rect::new(0, 0, 3, 1));
        let mut state = ViewportState::new(());
        Scrollable::new(Grid(&["ab汉c"]))
            .content(Size::new(5, 1))
            .render(Rect::new(0, 0, 3, 1), &mut buffer, &mut state);
        assert_eq!(buffer.to_string(), "ab ");
    }

    #[test]
    fn window_start_snaps_to_wide_glyph_leading_edge() {
        // "a汉b" — 汉 covers columns 1-2. A 2-wide window at offset 2 would
        // start on the covered half; it snaps back to the leading edge and
        // shows the whole pair.
        let mut buffer = Buffer::empty(Rect::new(0, 0, 2, 1));
        let mut state = ViewportState::new(());
        state.set_content(Size::new(4, 1));
        state.viewport = Size::new(2, 1);
        assert!(state.scroll_right(2));
        Scrollable::new(Grid(&["a汉b"]))
            .content(Size::new(4, 1))
            .render(Rect::new(0, 0, 2, 1), &mut buffer, &mut state);
        // The window shows the whole pair; the covered half prints as `·`.
        assert_eq!(buffer.to_string(), "汉·");
    }

    #[test]
    fn attached_scrollbar_renders_in_last_column() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 3, 4));
        let mut state = ViewportState::new(());
        Scrollable::new(Lines { rows: 8 })
            .content(Size::new(2, 8))
            .scrollbar(Scrollbar::default())
            .render(Rect::new(0, 0, 3, 4), &mut buffer, &mut state);
        let rendered = buffer.to_string();
        let rows: Vec<&str> = rendered.lines().collect();
        // Viewport is 2 columns wide; the last column holds the scrollbar:
        // begin glyph, a 1-cell thumb (usable track 2 * viewport 4 / content
        // 8 = 1) at position 0, track, end glyph.
        assert_eq!(rows[0], "r0↑");
        assert_eq!(rows[1], "r0█");
        assert_eq!(rows[2], "r0│");
        assert_eq!(rows[3], "r0↓");
    }

    #[test]
    fn scrollbar_hidden_when_content_fits() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 3, 2));
        let mut state = ViewportState::new(());
        Scrollable::new(Lines { rows: 2 })
            .content(Size::new(3, 2))
            .scrollbar(Scrollbar::default())
            .render(Rect::new(0, 0, 3, 2), &mut buffer, &mut state);
        assert_eq!(buffer.to_string(), "r00\nr01");
    }

    #[test]
    fn scrollbar_hidden_when_it_cannot_fit() {
        // A 1-column area has no room for both content and scrollbar.
        let mut buffer = Buffer::empty(Rect::new(0, 0, 1, 2));
        let mut state = ViewportState::new(());
        Scrollable::new(Lines { rows: 4 })
            .content(Size::new(1, 4))
            .scrollbar(Scrollbar::default())
            .render(Rect::new(0, 0, 1, 2), &mut buffer, &mut state);
        // Content only, no scrollbar glyphs; 1-wide rows show one letter.
        assert_eq!(buffer.to_string(), "r\nr");
    }

    #[test]
    fn wheel_events_scroll_the_viewport() {
        let mut state = ViewportState::new(());
        state.set_content(Size::new(10, 10));
        state.viewport = Size::new(5, 5);
        assert!(!state.apply_wheel(&wheel(MouseEventKind::ScrollUp))); // at the top
        assert!(state.apply_wheel(&wheel(MouseEventKind::ScrollDown)));
        assert_eq!(state.offset_y(), 1);
        assert!(!state.apply_wheel(&wheel(MouseEventKind::ScrollLeft)));
        assert!(state.apply_wheel(&wheel(MouseEventKind::ScrollRight)));
        assert_eq!(state.offset_x(), 1);
        let click = MouseEvent {
            kind: MouseEventKind::Down(tuvix_core::event::MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: KeyModifiers::empty(),
        };
        assert!(!state.apply_wheel(&click));
    }

    #[test]
    fn inner_state_is_reachable_and_mutated() {
        struct Counting;

        impl RenderWidget for Counting {
            type State = usize;

            fn render(self, area: Rect, buf: &mut Buffer, state: &mut usize) {
                *state += 1;
                buf.set_string(area.x, area.y, format!("n{state}"), Style::new());
            }
        }

        let mut buffer = Buffer::empty(Rect::new(0, 0, 3, 1));
        let mut state = ViewportState::new(0_usize);
        Scrollable::new(Counting).content(Size::new(3, 1)).render(
            Rect::new(0, 0, 3, 1),
            &mut buffer,
            &mut state,
        );
        assert_eq!(*state.inner(), 1);
        assert_eq!(buffer.to_string(), "n1 ");
        *state.inner_mut() = 9;
        assert_eq!(*state.inner(), 9);
    }

    #[test]
    fn empty_area_is_a_noop() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 2, 2));
        let mut state = ViewportState::new(());
        Scrollable::new(Lines { rows: 4 })
            .content(Size::new(4, 4))
            .render(Rect::new(0, 0, 0, 2), &mut buffer, &mut state);
        assert_eq!(buffer.to_string(), "  \n  ");
        assert_eq!(state.viewport(), Size::new(0, 0));
    }

    #[test]
    fn invalidation_repaints_the_child() {
        struct Counting;

        impl RenderWidget for Counting {
            type State = usize;

            fn render(self, area: Rect, buf: &mut Buffer, state: &mut usize) {
                *state += 1;
                buf.set_string(area.x, area.y, format!("n{state}"), Style::new());
            }
        }

        let area = Rect::new(0, 0, 3, 1);
        let mut buffer = Buffer::empty(area);
        let mut state = ViewportState::new(0_usize);
        Scrollable::new(Counting)
            .content(Size::new(3, 1))
            .render(area, &mut buffer, &mut state);
        Scrollable::new(Counting)
            .content(Size::new(3, 1))
            .render(area, &mut buffer, &mut state);
        // The cached paint: the child was not re-run on the second frame.
        assert_eq!(*state.inner(), 1);
        assert!(state.is_valid());
        state.invalidate();
        assert!(!state.is_valid());
        Scrollable::new(Counting)
            .content(Size::new(3, 1))
            .render(area, &mut buffer, &mut state);
        assert_eq!(*state.inner(), 2);
        assert_eq!(buffer.to_string(), "n2 ");
    }

    #[test]
    fn content_resize_repaints_without_invalidation() {
        let area = Rect::new(0, 0, 4, 2);
        let mut buffer = Buffer::empty(area);
        let mut state = ViewportState::new(());
        Scrollable::new(Lines { rows: 4 })
            .content(Size::new(4, 4))
            .render(area, &mut buffer, &mut state);
        assert!(state.is_valid());
        // A different content extent resizes the scratch surface, which
        // loses content: the next frame must repaint from scratch even
        // without an explicit invalidation.
        Scrollable::new(Lines { rows: 6 })
            .content(Size::new(4, 6))
            .render(area, &mut buffer, &mut state);
        assert!(state.is_valid());
        assert_eq!(buffer.to_string(), "r00 \nr01 ");
    }

    #[test]
    fn second_frame_matches_a_fresh_scroll() {
        // Differential gate: reusing the scratch surface across frames must
        // render byte-identically to a fresh state scrolled to the same
        // offset.
        const CONTENT: [&str; 4] = ["row zero ", "row one  ", "row two  ", "row three"];
        let build = || Scrollable::new(Grid(&CONTENT)).content(Size::new(9, 4));

        let mut reused = Buffer::empty(Rect::new(0, 0, 9, 2));
        let mut reused_state = ViewportState::new(());
        build().render(Rect::new(0, 0, 9, 2), &mut reused, &mut reused_state);
        reused_state.scroll_down(2);
        build().render(Rect::new(0, 0, 9, 2), &mut reused, &mut reused_state);

        let mut fresh = Buffer::empty(Rect::new(0, 0, 9, 2));
        let mut fresh_state = ViewportState::new(());
        fresh_state.set_content(Size::new(9, 4));
        fresh_state.viewport = Size::new(9, 2);
        fresh_state.scroll_down(2);
        build().render(Rect::new(0, 0, 9, 2), &mut fresh, &mut fresh_state);

        assert_eq!(reused, fresh);
        assert_eq!(reused.to_string(), "row two  \nrow three");
    }

    #[test]
    fn offsets_clamp_to_content_minus_viewport() {
        let mut state = ViewportState::new(());
        state.set_content(Size::new(8, 12));
        // No viewport yet: the clamp is the bare content height.
        assert!(state.to_bottom());
        assert_eq!(state.offset_y(), 12);
        state.viewport = Size::new(8, 4);
        // The clamp itself brings the offset back into range.
        assert!(!state.to_bottom());
        assert_eq!(state.offset_y(), 8);
    }
}
