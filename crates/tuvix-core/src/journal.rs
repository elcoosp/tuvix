//! The persistent-DOM frame journal.
//!
//! This is the M4 slice that turns the M1 damage tracker into the O(1)
//! no-op frame decision. A [`FrameJournal`] records every cell a
//! [`Buffer`](crate::buffer::Buffer)
//! mutation touched, as merged row bands; the [`crate::terminal::Terminal`]
//! persistent mode reads the journal at frame end and decides:
//!
//! - **empty journal** — nothing was painted: the frame is a no-op, the diff
//!   walk is skipped entirely, the backend receives no draw call. Cost is
//!   O(1) regardless of viewport size (the B8 budget, finally met at
//!   200x60);
//! - **partial journal** — only the journaled bands are diffed, so an
//!   untouched 190-of-200 rows cost nothing;
//! - **full journal** — everything changed: the classic full diff runs.
//!
//! The journal never changes what is rendered, only how much work a frame
//! costs. The differential gate (`tests/m4_journal.rs`) proves persistent
//! renders are byte-identical to immediate-mode renders for the same paint
//! sequence, wide characters and pair repairs included.
//!
//! # Examples
//!
//! ```
//! use tuvix_core::geometry::Rect;
//! use tuvix_core::journal::FrameJournal;
//!
//! let mut journal = FrameJournal::viewport(80, 24);
//! assert!(journal.is_empty());
//! journal.mark_cell(3, 0);
//! journal.mark_cell(5, 0);
//! // Bands on the same row merge across gaps of 2 (the damage model).
//! assert_eq!(journal.bands().len(), 1);
//! journal.mark_full();
//! assert!(journal.is_full());
//! journal.clear();
//! assert!(journal.is_empty());
//! ```

use crate::damage::Damage;
use crate::geometry::Rect;

/// Records the cell regions a frame's paint touched, as merged row bands.
///
/// The tracker flips to full damage once more than half the viewport was
/// touched (repainting everything is then cheaper than chasing bands), so
/// full-screen repaints stay linear instead of quadratic in band inserts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameJournal {
    damage: Damage,
}

impl FrameJournal {
    /// Creates a journal for a viewport of `width` x `height` cells.
    pub fn viewport(width: u16, height: u16) -> Self {
        Self {
            damage: Damage::viewport(width, height),
        }
    }

    /// Marks a single cell as painted this frame.
    pub fn mark_cell(&mut self, x: u16, y: u16) {
        self.damage.add_row_band(y, x, x.saturating_add(1));
    }

    /// Marks every cell of `rect` as painted this frame. Cells outside the
    /// viewport are clipped away.
    pub fn mark_rect(&mut self, rect: Rect) {
        self.damage.add_rect(rect);
    }

    /// Marks the whole viewport painted (also the state after a resize,
    /// where all content is lost and every cell must be considered new).
    pub fn mark_full(&mut self) {
        self.damage.mark_full();
    }

    /// Returns `true` when nothing was painted: the frame is a no-op.
    pub fn is_empty(&self) -> bool {
        !self.damage.is_full() && self.damage.bands().is_empty()
    }

    /// Returns `true` when the tracker decided the whole viewport changed.
    pub fn is_full(&self) -> bool {
        self.damage.is_full()
    }

    /// The merged, sorted row bands recorded so far (row-major, then by
    /// start column).
    pub fn bands(&self) -> &[crate::damage::Band] {
        self.damage.bands()
    }

    /// The number of cells the recorded bands cover (an upper bound while
    /// bands keep accumulating; exact once [`FrameJournal::is_full`] flips).
    pub fn area_upper_bound(&self) -> u64 {
        self.damage.area_upper_bound()
    }

    /// Clears the journal: the next frame starts with no recorded paint.
    pub fn clear(&mut self) {
        self.damage.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_journal_reports_no_work() {
        let journal = FrameJournal::viewport(80, 24);
        assert!(journal.is_empty());
        assert!(!journal.is_full());
        assert_eq!(journal.bands(), []);
    }

    #[test]
    fn cells_merge_into_row_bands() {
        let mut journal = FrameJournal::viewport(80, 24);
        journal.mark_cell(10, 3);
        journal.mark_cell(11, 3);
        journal.mark_cell(14, 3); // gap of 2: merges with the running band
        assert_eq!(journal.bands().len(), 1);
        assert_eq!(journal.bands()[0].row, 3);
        assert_eq!(journal.bands()[0].start, 10);
        assert_eq!(journal.bands()[0].end, 15);
    }

    #[test]
    fn different_rows_stay_independent() {
        let mut journal = FrameJournal::viewport(80, 24);
        journal.mark_cell(0, 0);
        journal.mark_cell(0, 10);
        assert_eq!(journal.bands().len(), 2);
    }

    #[test]
    fn rect_marks_are_clipped_to_the_viewport() {
        let mut journal = FrameJournal::viewport(10, 4);
        journal.mark_rect(Rect::new(8, 3, 10, 5));
        assert_eq!(journal.bands().len(), 1);
        assert_eq!(journal.bands()[0].start, 8);
        assert_eq!(journal.bands()[0].end, 10);
        assert_eq!(journal.bands()[0].row, 3);
    }

    #[test]
    fn full_flip_stops_tracking_bands() {
        let mut journal = FrameJournal::viewport(10, 4);
        journal.mark_full();
        assert!(journal.is_full());
        assert_eq!(journal.bands(), []);
        // Further marks cannot resurrect bands.
        journal.mark_cell(0, 0);
        assert_eq!(journal.bands(), []);
        assert!(journal.is_full());
    }

    #[test]
    fn clear_returns_to_empty() {
        let mut journal = FrameJournal::viewport(10, 4);
        journal.mark_cell(1, 1);
        assert!(!journal.is_empty());
        journal.clear();
        assert!(journal.is_empty());
        assert!(!journal.is_full());
    }
}
