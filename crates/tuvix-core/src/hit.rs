//! The hit map: the click surface as a pure, per-frame data structure.
//!
//! Terminal mouse events carry a cell position; an application needs the
//! answer to "what did the user click?". Hand-rolled `Rect::contains`
//! ladders rot with every new region — [`HitMap`] is that answer as a
//! type: the app registers the regions it painted (buttons, panes,
//! popups) in paint order while building the frame, and
//! [`hit_test`](HitMap::hit_test) resolves a position to the topmost
//! region plus local coordinates.
//!
//! The rules the tests pin:
//!
//! - **z-order is registration order** — the painter's algorithm. The
//!   last region registered that contains the position wins, exactly the
//!   way the frame painted it (popups paint after backdrops, so they
//!   register after them and win the overlap);
//! - **local coordinates are relative to the region origin**, so a
//!   handler sees `Position(1, 2)` for a click one column into the
//!   second row of its region — never a global cell;
//! - **`push_clipped` applies the buffer's clip discipline**: the region
//!   registered is the intersection with the clip, and an empty
//!   intersection registers nothing — the same "paint the part that
//!   lands inside" rule the mutation applier (M12) follows;
//! - the decision itself is a reverse scan over plain rects: no
//!   allocation, no hashing, O(regions) with a tiny constant (the B17
//!   gate keeps a 1,000-region decision inside a frame budget).
//!
//! A `HitMap` is rebuilt every frame like the frame itself; it borrows
//! nothing and owns no terminal state, so it is as testable as a `Rect`.
//!
//! # Examples
//!
//! ```
//! use tuvix_core::geometry::{Position, Rect};
//! use tuvix_core::hit::HitMap;
//!
//! let mut hits: HitMap<&str> = HitMap::new();
//! hits.push("backdrop", Rect::new(0, 0, 40, 20));
//! hits.push("panel", Rect::new(5, 5, 10, 6));
//!
//! // The panel painted last: it wins the overlap.
//! let hit = hits.hit_test(Position::new(6, 6)).unwrap();
//! assert_eq!(hit.id, "panel");
//! assert_eq!(hit.local, Position::new(1, 1));
//!
//! // Outside the panel the backdrop is on top; elsewhere, nothing.
//! assert_eq!(hits.hit_test(Position::new(2, 2)).map(|h| h.id), Some("backdrop"));
//! assert_eq!(hits.hit_test(Position::new(50, 25)), None);
//! ```

use crate::geometry::{Position, Rect};

/// One resolved click: which region, where its bounds are, where inside
/// it the pointer landed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Hit<ID> {
    /// The id the app registered for the region.
    pub id: ID,
    /// The region's bounds (post-clip, for `push_clipped` regions).
    pub area: Rect,
    /// The pointer position relative to the region origin.
    pub local: Position,
}

/// The per-frame click surface: regions in paint order, resolved by the
/// painter's algorithm.
///
/// Generic over the app's own region id — a pane enum, a widget key, a
/// `u32` slot index — the same honesty as the [`FocusRing`](crate::focus::FocusRing)
/// being a type over the app's panes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HitMap<ID> {
    regions: Vec<(ID, Rect)>,
}

impl<ID> Default for HitMap<ID> {
    /// An empty map.
    fn default() -> Self {
        Self {
            regions: Vec::new(),
        }
    }
}

impl<ID: Copy> HitMap<ID> {
    /// Creates an empty map.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `area` under `id` on top of everything registered so
    /// far. Zero-sized areas are registered as declared (they can never
    /// be hit); use [`push_clipped`](Self::push_clipped) to apply a clip.
    pub fn push(&mut self, id: ID, area: Rect) {
        self.regions.push((id, area));
    }

    /// Registers the intersection of `area` with `clip` — the part of
    /// the region that actually painted. An empty intersection registers
    /// nothing.
    pub fn push_clipped(&mut self, id: ID, area: Rect, clip: Rect) {
        if let Some(clipped) = area.intersection(clip)
            && !clipped.is_empty()
        {
            self.regions.push((id, clipped));
        }
    }

    /// Resolves `position` to the topmost region containing it.
    ///
    /// The scan runs in reverse registration order (the painter's
    /// algorithm) and stops at the first containment: O(depth of the
    /// overlap), no allocation.
    #[must_use]
    pub fn hit_test(&self, position: Position) -> Option<Hit<ID>> {
        self.regions
            .iter()
            .rev()
            .find(|(_, area)| area.contains(position))
            .map(|(id, area)| {
                let local = Position::new(
                    position.x.saturating_sub(area.x),
                    position.y.saturating_sub(area.y),
                );
                Hit {
                    id: *id,
                    area: *area,
                    local,
                }
            })
    }

    /// Forgets every region — the between-frames reset.
    pub fn clear(&mut self) {
        self.regions.clear();
    }

    /// How many regions are registered.
    #[must_use]
    pub fn len(&self) -> usize {
        self.regions.len()
    }

    /// Whether no region is registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.regions.is_empty()
    }

    /// The registered regions in paint order (bottom of the stack
    /// first). The slice borrows the map; the map stays the resolver.
    pub fn iter(&self) -> impl Iterator<Item = (ID, Rect)> + '_ {
        self.regions.iter().map(|(id, area)| (*id, *area))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reverse_scan_prefers_the_last_registration() {
        let mut hits: HitMap<u8> = HitMap::new();
        hits.push(1, Rect::new(0, 0, 10, 10));
        hits.push(2, Rect::new(0, 0, 10, 10));
        hits.push(3, Rect::new(0, 0, 10, 10));
        assert_eq!(hits.hit_test(Position::new(5, 5)).map(|h| h.id), Some(3));
    }

    #[test]
    fn iter_reports_paint_order() {
        let mut hits: HitMap<u8> = HitMap::new();
        hits.push(1, Rect::new(0, 0, 4, 4));
        hits.push(2, Rect::new(1, 1, 4, 4));
        let ids: Vec<u8> = hits.iter().map(|(id, _)| id).collect();
        assert_eq!(ids, vec![1, 2]);
    }

    #[test]
    fn clipped_push_of_a_covered_area_registers_nothing() {
        let mut hits: HitMap<u8> = HitMap::new();
        // The clip misses the area entirely (zero overlap at the corner).
        hits.push_clipped(1, Rect::new(0, 0, 5, 5), Rect::new(10, 10, 5, 5));
        assert!(hits.is_empty());
    }
}
