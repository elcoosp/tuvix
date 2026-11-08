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

    #[test]
    fn dropdown_rows_is_total_at_the_corners() {
        let host = Rect::new(0, 0, u16::MAX, u16::MAX);
        let anchor = Rect::new(0, 0, 1, 1);
        assert_eq!(dropdown_rows(host, anchor, usize::MAX), 65_534);
        let sunken = Rect::new(0, u16::MAX, 5, 1);
        assert_eq!(dropdown_rows(host, sunken, 9), 9, "saturating room above");
    }
}
