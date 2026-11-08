//! Floating-surface placement: the geometry behind popups, dropdowns,
//! tooltips and modals.
//!
//! The M6 kit (`Rect::centered` + [`Clear`](crate::widget)) answers one
//! question — where does a modal go? — and M15 left the rest to apps:
//! every dropdown re-rolled "open below my trigger, flip above when the
//! edge is near", every tooltip re-rolled the edge clamp. [`place`] is
//! that decision as a pure function, in the same spirit as the layout
//! solver: deterministic passes, one documented remainder rule, no
//! allocation, no hidden state.
//!
//! Since M18 the routing half lives here too: an app that floats more
//! than one surface no longer hand-rolls the open/close flags, the
//! z-order and the dismissal precedence — [`SurfaceStack`] is that
//! routing as a type, and its [`SurfaceStack::order`] is the sequence
//! both the paint and the hit-map registration consume.
//!
//! # The rules
//!
//! - **Flip, then clamp.** The preferred side first ([`Placement::Below`]
//!   opens under the anchor, [`Placement::Above`] over it); when the edge
//!   is near, the mirror side; when neither fits, the surface sits flush
//!   with the nearest host edge (Below falls to the host's bottom, Above
//!   to its top — which, for a surface taller than the host, is the same
//!   pinned-to-origin row).
//! - **Size never shrinks** — except [`Placement::Centered`], which keeps
//!   the exact M6 [`Rect::centered`] rule (an oversized modal *is* the
//!   host). A dropdown taller than the screen must still open from its
//!   trigger and let the buffer's clip discipline truncate it, not
//!   collapse into the host; the caller sees honest geometry.
//! - **The rect never starts outside the host.** Whatever the anchor
//!   does — hang off the host's edge, sit outside entirely — the resolved
//!   origin is pulled back to the host's top-left.
//! - **Saturating corners are total.** All arithmetic runs in `u32` and
//!   saturates at the `u16` edges: any anchor, any size, any host
//!   produces a deterministic [`Rect`], never a panic.
//!
//! Sizing is the other half of the decision: [`place`] needs a
//! [`Size`], and the size a dropdown should ask for — the option count
//! capped by the roomiest gap — is [`dropdown_rows`]' answer, so the
//! flip ladder always receives a size one of its paths can open.
//!
//! # Examples
//!
//! ```
//! use tuvix_core::geometry::{Rect, Size};
//! use tuvix_core::layer::{Placement, place};
//!
//! let host = Rect::new(0, 0, 40, 10);
//! let trigger = Rect::new(5, 7, 10, 1);
//!
//! // A dropdown opens directly under its trigger...
//! assert_eq!(place(host, trigger, Size::new(8, 2), Placement::Below), Rect::new(5, 8, 8, 2));
//!
//! // ...flips above when the edge is near...
//! assert_eq!(place(host, trigger, Size::new(8, 3), Placement::Below), Rect::new(5, 4, 8, 3));
//!
//! // ...and a modal keeps the M6 rule.
//! assert_eq!(
//!     place(host, trigger, Size::new(30, 8), Placement::Centered),
//!     host.centered(30, 8),
//! );
//! ```

use crate::geometry::{Position, Rect, Size};

/// Where a floating surface wants to open, relative to its anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Placement {
    /// Directly below the anchor (a dropdown opening from its trigger);
    /// flips above when the edge is near.
    Below,
    /// Directly above the anchor (a menu opening upward); flips below
    /// when the edge is near.
    Above,
    /// Centered in the host (a modal dialog), under the exact M6
    /// [`Rect::centered`] rule — an oversized surface becomes the host.
    Centered,
    /// With its top-left corner at the position (a tooltip at the
    /// pointer); clamped into the host, never flipped.
    At(Position),
}

/// Resolves the area of a floating surface of `size` inside `host`.
///
/// `anchor` is the rect the surface opens from — a trigger row, a
/// button, the pointer's cell. It may sit anywhere (even outside the
/// host); the rules never inspect containment, they only read its
/// edges. See the [module docs](self) for the flip-then-clamp rules.
///
/// # Examples
///
/// ```
/// use tuvix_core::geometry::{Position, Rect, Size};
/// use tuvix_core::layer::{Placement, place};
///
/// let host = Rect::new(0, 0, 80, 24);
/// let trigger = Rect::new(60, 20, 10, 1);
///
/// // Left-aligning would run past the host: the surface right-aligns
/// // to the trigger's right edge instead.
/// let placed = place(host, trigger, Size::new(40, 3), Placement::Below);
/// assert_eq!(placed, Rect::new(30, 21, 40, 3));
///
/// // A tooltip at the pointer, clamped at the host's edges.
/// let tooltip = place(host, trigger, Size::new(10, 1), Placement::At(Position::new(75, 23)));
/// assert_eq!(tooltip, Rect::new(70, 23, 10, 1));
/// ```
#[must_use]
pub fn place(host: Rect, anchor: Rect, size: Size, placement: Placement) -> Rect {
    let width = u32::from(size.width);
    let height = u32::from(size.height);
    let host_x = u32::from(host.x);
    let host_y = u32::from(host.y);
    let host_right = u32::from(host.right());
    let host_bottom = u32::from(host.bottom());
    let anchor_x = u32::from(anchor.x);
    let anchor_y = u32::from(anchor.y);
    let anchor_right = u32::from(anchor.right());
    let anchor_bottom = u32::from(anchor.bottom());

    // The host's last legal origin per axis: a rect of `size` starting
    // there ends exactly at the host's edge (or pins to the origin when
    // the size exceeds the host).
    let last_x = host_right.saturating_sub(width).max(host_x);
    let last_y = host_bottom.saturating_sub(height).max(host_y);
    let fits_below = host_bottom >= anchor_bottom.saturating_add(height);
    let fits_above = anchor_y >= host_y.saturating_add(height);

    let (x, y) = match placement {
        Placement::Centered => {
            let centered = host.centered(size.width, size.height);
            return centered;
        }
        Placement::At(position) => {
            let x = u32::from(position.x);
            let y = u32::from(position.y);
            (x.min(last_x), y.min(last_y))
        }
        Placement::Below => {
            let y = if fits_below {
                anchor_bottom
            } else if fits_above {
                anchor_y - height
            } else {
                last_y
            };
            (
                horizontal_origin(anchor_x, anchor_right, width, host_x, host_right, last_x),
                y,
            )
        }
        Placement::Above => {
            let y = if fits_above {
                anchor_y - height
            } else if fits_below {
                anchor_bottom
            } else {
                host_y
            };
            (
                horizontal_origin(anchor_x, anchor_right, width, host_x, host_right, last_x),
                y,
            )
        }
    };

    // The rect never starts outside the host, whatever the anchor does.
    Rect::new(
        u16::try_from(x.max(host_x)).unwrap_or(u16::MAX),
        u16::try_from(y.max(host_y)).unwrap_or(u16::MAX),
        size.width,
        size.height,
    )
}

/// The rows a dropdown-style floating surface should request from
/// [`place`]: the content's row count, capped by the roomiest of the
/// two gaps the flip ladder can open into (below the anchor, above
/// it).
///
/// [`place`] answers where a surface of a given size goes;
/// `dropdown_rows` answers the size a dropdown should ask for, so an
/// app never re-rolls "how many of my options fit?" by hand. The pair
/// composes: size with `dropdown_rows`, then [`Placement::Below`] —
/// when the surface does not fit below the anchor, the same size fits
/// the roomier side above, which is exactly the side the cap measured.
///
/// Zero content requests zero rows (a dropdown with nothing to show
/// opens no surface); non-empty content floors at one row — the caller
/// decides not to open, the sizing never hides the want.
///
/// # Examples
///
/// ```
/// use tuvix_core::geometry::{Rect, Size};
/// use tuvix_core::layer::{Placement, dropdown_rows, place};
///
/// let host = Rect::new(0, 0, 40, 20);
/// let trigger = Rect::new(5, 10, 10, 1);
///
/// // Eighteen options in a 20-row host: nine rows free below the
/// // trigger, ten above — the surface borrows the roomier side.
/// assert_eq!(dropdown_rows(host, trigger, 18), 10);
///
/// // Sized, then placed: the ten rows do not fit below, so the
/// // surface opens above and ends at the trigger.
/// let surface = place(
///     host,
///     trigger,
///     Size::new(12, dropdown_rows(host, trigger, 18)),
///     Placement::Below,
/// );
/// assert_eq!(surface.bottom(), trigger.y);
/// ```
#[must_use]
pub fn dropdown_rows(host: Rect, anchor: Rect, content_rows: usize) -> u16 {
    let wanted = u32::try_from(content_rows).unwrap_or(u32::MAX);
    if wanted == 0 {
        return 0;
    }
    let room_below = u32::from(host.bottom()).saturating_sub(u32::from(anchor.bottom()));
    let room_above = u32::from(anchor.y).saturating_sub(u32::from(host.y));
    let rows = wanted.min(room_below.max(room_above)).max(1);
    u16::try_from(rows).unwrap_or(u16::MAX)
}

/// The floating-surface routing: which surfaces are open, and in what
/// z-order they paint.
///
/// M16 answered *where* a surface goes ([`place`]) and *how* it paints
/// (the widget-side `Overlay`); M17 closed the single-dropdown seam.
/// The routing an app with more than one surface still hand-rolls —
/// the open flags, the z-order, the dismissal precedence, the hit-map
/// registration order — is this stack: the `FocusRing`/`Keymap`
/// treatment for floating surfaces. The keys are the app's own pane
/// enum (like the ring's focus targets); the widgets and their state
/// stay app-side, the stack only routes.
///
/// The one sequence both halves of the frame consume is
/// [`SurfaceStack::order`] — bottom-to-top, the painter's algorithm:
/// paint the base, then walk `order()` resolving each surface's
/// geometry (`dropdown_rows` + `place`) and painting it erased
/// (`Overlay`); then register the base and the same `order()` into the
/// `HitMap`. Screen and mouse agree on z-order by construction.
///
/// # The rules
///
/// - **`push` opens or raises.** A new key opens on top (`true` — the
///   app initializes the surface's state); an already-open key is
///   raised to the top without duplicating (`false` — the state stays).
/// - **`close` is stable.** Removing a surface anywhere in the stack
///   keeps the relative order of the rest.
/// - **`close_top` is the Esc precedence.** The top surface dismisses
///   first — a modal over an open dropdown, then the dropdown. The
///   hand-rolled version of this rule is where apps quietly close the
///   surface underneath the one the user sees.
/// - **Total everywhere.** `close` of an absent key is `false`,
///   `close_top` of an empty stack is `None`, `clear` empties; nothing
///   panics, nothing hides state.
///
/// # Examples
///
/// ```
/// use tuvix_core::layer::SurfaceStack;
///
/// #[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// enum Surface { Dropdown, Modal }
///
/// use Surface::{Dropdown, Modal};
///
/// let mut stack = SurfaceStack::new();
/// assert!(stack.push(Dropdown), "the dropdown opens");
/// assert!(stack.push(Modal), "the modal opens over it");
/// assert_eq!(stack.order(), &[Dropdown, Modal]);
///
/// // Esc: the modal is on top, so the modal dismisses first — the
/// // dropdown underneath stays open.
/// assert_eq!(stack.close_top(), Some(Modal));
/// assert_eq!(stack.top(), Some(&Dropdown));
///
/// // Reopening an open surface raises it instead of duplicating it.
/// assert!(!stack.push(Dropdown), "already open — raised");
/// assert_eq!(stack.order(), &[Dropdown]);
/// assert_eq!(stack.close_top(), Some(Dropdown));
/// assert_eq!(stack.close_top(), None, "an empty stack dismisses nothing");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SurfaceStack<K> {
    /// The open surfaces, bottom-to-top: the paint order, the hit-map
    /// registration order, the dismissal order's mirror.
    surfaces: Vec<K>,
}

impl<K> SurfaceStack<K> {
    /// Creates an empty stack — no surface floats.
    #[must_use]
    pub fn new() -> Self {
        Self {
            surfaces: Vec::new(),
        }
    }

    /// Opens `key` on top of the stack, or raises an already-open `key`
    /// to the top. Returns `true` when the surface was newly opened
    /// (the app initializes its state) and `false` when it was raised
    /// (the state stays exactly as it was).
    pub fn push(&mut self, key: K) -> bool
    where
        K: PartialEq,
    {
        if let Some(position) = self.surfaces.iter().position(|open| *open == key) {
            let raised = self.surfaces.remove(position);
            self.surfaces.push(raised);
            false
        } else {
            self.surfaces.push(key);
            true
        }
    }

    /// Closes `key` wherever it sits, keeping the relative order of the
    /// surfaces above and below it. Returns `false` when the surface
    /// was not open.
    pub fn close(&mut self, key: &K) -> bool
    where
        K: PartialEq,
    {
        if let Some(position) = self.surfaces.iter().position(|open| open == key) {
            self.surfaces.remove(position);
            true
        } else {
            false
        }
    }

    /// Dismisses the top surface — the Esc precedence — and returns
    /// its key (`None` on an empty stack). The app maps the key back to
    /// its own state (close the select, forget the toast).
    pub fn close_top(&mut self) -> Option<K> {
        self.surfaces.pop()
    }

    /// The top surface — the one the keys and the clicks go to first.
    #[must_use]
    pub fn top(&self) -> Option<&K> {
        self.surfaces.last()
    }

    /// The open surfaces bottom-to-top: the paint order and the
    /// hit-map registration order in one sequence. Empty when nothing
    /// floats.
    #[must_use]
    pub fn order(&self) -> &[K] {
        &self.surfaces
    }

    /// Whether `key` is currently open (at any height of the stack).
    #[must_use]
    pub fn is_open(&self, key: &K) -> bool
    where
        K: PartialEq,
    {
        self.surfaces.contains(key)
    }

    /// The number of open surfaces.
    #[must_use]
    pub fn len(&self) -> usize {
        self.surfaces.len()
    }

    /// Whether no surface floats.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.surfaces.is_empty()
    }

    /// Dismisses every surface at once.
    pub fn clear(&mut self) {
        self.surfaces.clear();
    }
}

/// The shared horizontal rule of `Below` and `Above`: left-align to the
/// anchor; when that runs past the host, right-align to the anchor's
/// right edge; when that does not fit either, flush to the host's
/// right edge.
fn horizontal_origin(
    anchor_x: u32,
    anchor_right: u32,
    width: u32,
    host_x: u32,
    host_right: u32,
    last_x: u32,
) -> u32 {
    if anchor_x + width <= host_right {
        anchor_x
    } else if anchor_right >= host_x + width {
        anchor_right - width
    } else {
        last_x
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn below_and_above_are_exact_mirrors_on_the_vertical_axis() {
        let host = Rect::new(0, 0, 40, 20);
        let trigger = Rect::new(5, 10, 10, 1);
        let size = Size::new(8, 3);
        let below = place(host, trigger, size, Placement::Below);
        let above = place(host, trigger, size, Placement::Above);
        assert_eq!(below.y, trigger.bottom(), "below opens under the trigger");
        assert_eq!(
            above.y,
            trigger.y - size.height,
            "above opens over the trigger"
        );
        assert_eq!(below.x, above.x, "the horizontal rule is shared");
    }

    #[test]
    fn the_anchor_never_reads_containment() {
        // A zero anchor at the origin: the rules still resolve — the
        // surface opens "below" the empty anchor's bottom edge.
        let host = Rect::new(0, 0, 40, 10);
        let anchor = Rect::ZERO;
        let placed = place(host, anchor, Size::new(4, 2), Placement::Below);
        assert_eq!(placed, Rect::new(0, 0, 4, 2));
    }

    // M17 — the sizing half of the decision.

    #[test]
    fn dropdown_rows_caps_at_the_roomiest_gap() {
        let host = Rect::new(0, 0, 40, 20);
        let trigger = Rect::new(5, 10, 10, 1);
        // Nine rows below, ten above: eighteen options borrow the ten.
        assert_eq!(dropdown_rows(host, trigger, 18), 10);
        assert_eq!(dropdown_rows(host, trigger, 6), 6, "content under the cap");
    }

    #[test]
    fn dropdown_rows_floors_at_one_row_for_non_empty_content() {
        let host = Rect::new(0, 0, 40, 1);
        let trigger = Rect::new(0, 0, 40, 1);
        assert_eq!(dropdown_rows(host, trigger, 9), 1);
        assert_eq!(dropdown_rows(host, trigger, 0), 0, "no content, no rows");
    }

    // M18 — the routing half of the floating surfaces.

    #[test]
    fn the_stack_pushes_new_surfaces_and_raises_open_ones() {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        enum Surface {
            Dropdown,
            Tooltip,
            Modal,
        }
        use Surface::{Dropdown, Modal, Tooltip};

        let mut stack = SurfaceStack::new();
        assert!(stack.push(Dropdown));
        assert!(stack.push(Tooltip));
        assert!(stack.push(Modal));
        assert_eq!(stack.order(), &[Dropdown, Tooltip, Modal]);

        assert!(
            !stack.push(Dropdown),
            "an open surface is raised, not duplicated"
        );
        assert_eq!(stack.order(), &[Tooltip, Modal, Dropdown]);
        assert_eq!(stack.len(), 3);
        assert_eq!(stack.top(), Some(&Dropdown));
    }

    #[test]
    fn the_stack_dismisses_the_top_surface_first() {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        enum Surface {
            Dropdown,
            Modal,
        }
        use Surface::{Dropdown, Modal};

        let mut stack = SurfaceStack::new();
        stack.push(Dropdown);
        stack.push(Modal);

        assert_eq!(stack.close_top(), Some(Modal), "Esc dismisses the modal");
        assert_eq!(stack.order(), &[Dropdown], "the dropdown stays open");

        assert!(stack.close(&Dropdown), "closing by value also works");
        assert!(
            !stack.close(&Dropdown),
            "closing a closed surface is a no-op"
        );
        assert!(stack.is_empty());
        assert_eq!(stack.close_top(), None, "an empty stack dismisses nothing");
    }
}
