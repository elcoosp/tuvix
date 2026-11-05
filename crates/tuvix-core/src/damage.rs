//! The row-band damage tracker.
//!
//! This is the M1 slice of the hybrid damage model: changed regions are
//! accumulated as per-row `[start, end)` bands, bands on the same row merge
//! when their gap is small, and different rows stay independent — beating
//! both whole-row repaints (a one-column edit dirties only a few cells, not
//! the row) and single-bounding-box merges (two diagonal edits do not dirty
//! the viewport).
//!
//! When the accumulated damage grows beyond a threshold of the viewport
//! area, the tracker flips to *full damage*: repainting everything is
//! cheaper than chasing bands.
//!
//! # Examples
//!
//! ```
//! use tuvix_core::damage::Damage;
//! use tuvix_core::geometry::Rect;
//!
//! let mut damage = Damage::viewport(20, 4);
//! damage.add_rect(Rect::new(0, 0, 2, 1));
//! damage.add_rect(Rect::new(4, 0, 2, 1));
//! // Bands on the same row with a gap of 2 columns merge.
//! assert_eq!(damage.bands().len(), 1);
//! assert!(!damage.is_full());
//! ```

use crate::geometry::Rect;

/// Two bands on the same row merge when the gap between them is at most
/// this many cells. (The architecture pins the value: merging across gaps
/// of 1-2 cells saves more cursor moves than it costs in extra paint.)
const BAND_MERGE_GAP: u16 = 2;

/// A damaged cell range on one row: `[start, end)` columns, `end` exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Band {
    /// The row the band lives on.
    pub row: u16,
    /// First damaged column.
    pub start: u16,
    /// One past the last damaged column.
    pub end: u16,
}

impl Band {
    /// Number of cells covered by the band.
    pub fn len(&self) -> u16 {
        self.end.saturating_sub(self.start)
    }

    /// Returns `true` when the band covers no cells.
    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }
}

/// Accumulates damaged regions and reports them as merged row bands.
///
/// Bands are per-row and merged when their gap is at most
/// `BAND_MERGE_GAP` columns; rows are tracked independently. Adding
/// damage whose accumulated area exceeds half the viewport flips the
/// tracker to full damage ([`Damage::is_full`]).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Damage {
    viewport: Rect,
    bands: Vec<Band>,
    full: bool,
    damaged_area: u64,
}

impl Damage {
    /// Creates a tracker for a viewport of `width` x `height` cells.
    pub fn viewport(width: u16, height: u16) -> Self {
        Self {
            viewport: Rect::new(0, 0, width, height),
            bands: Vec::new(),
            full: false,
            damaged_area: 0,
        }
    }

    /// Marks the whole viewport damaged.
    pub fn mark_full(&mut self) {
        self.full = true;
        self.bands.clear();
    }

    /// Adds the damage caused by a rectangular change. The rect is clipped
    /// to the viewport; an empty intersection adds nothing.
    pub fn add_rect(&mut self, rect: Rect) {
        let Some(clipped) = self.viewport.intersection(rect) else {
            return;
        };
        if clipped.is_empty() {
            return;
        }
        for row in clipped.y..clipped.bottom() {
            self.add_row_band(row, clipped.x, clipped.right());
        }
    }

    /// Adds the half-open column range `[start, end)` of `row` as damage,
    /// clipped to the viewport row bounds.
    pub fn add_row_band(&mut self, row: u16, start: u16, end: u16) {
        if self.full || row >= self.viewport.height {
            return;
        }
        let start = start.max(self.viewport.x);
        let end = end.min(self.viewport.right());
        if start >= end {
            return;
        }
        self.damaged_area += u64::from(end - start);
        if self.area_upper_bound() * 2 > u64::from(self.viewport.area()) {
            self.full = true;
            self.bands.clear();
            return;
        }
        self.insert_merged(Band { row, start, end });
    }

    /// Upper bound of the damaged area in cells: the sum of band lengths
    /// (bands never overlap within a row).
    pub fn area_upper_bound(&self) -> u64 {
        self.damaged_area
    }

    /// Returns `true` when the tracker decided everything must repaint.
    pub fn is_full(&self) -> bool {
        self.full
    }

    /// The merged, sorted bands (row-major, then by start column).
    pub fn bands(&self) -> &[Band] {
        &self.bands
    }

    /// Resets the tracker to a clean (empty, not full) state.
    pub fn clear(&mut self) {
        self.bands.clear();
        self.full = false;
        self.damaged_area = 0;
    }

    /// Inserts a band merging it with same-row neighbors whose gap is at
    /// most [`BAND_MERGE_GAP`], keeping the list sorted.
    fn insert_merged(&mut self, band: Band) {
        // Find the first band that could merge with `band` on the same row.
        let mut start = band.start;
        let mut end = band.end;
        let mut first_overlap = self.bands.len();
        let mut last_overlap_exclusive = self.bands.len();
        for (index, existing) in self.bands.iter().enumerate() {
            if existing.row != band.row {
                continue;
            }
            let touches = existing.start <= end.saturating_add(BAND_MERGE_GAP)
                && start <= existing.end.saturating_add(BAND_MERGE_GAP);
            if touches {
                first_overlap = first_overlap.min(index);
                last_overlap_exclusive = index + 1;
                start = start.min(existing.start);
                end = end.max(existing.end);
            }
        }
        if first_overlap == last_overlap_exclusive {
            // No merge partner: insert in sorted position.
            let position = self
                .bands
                .partition_point(|b| (b.row, b.start) < (band.row, band.start));
            self.bands.insert(position, band);
        } else {
            let merged = Band {
                row: band.row,
                start,
                end,
            };
            self.bands
                .splice(first_overlap..last_overlap_exclusive, [merged]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bands_of(damage: &Damage) -> Vec<(u16, u16, u16)> {
        damage
            .bands()
            .iter()
            .map(|b| (b.row, b.start, b.end))
            .collect()
    }

    #[test]
    fn empty_tracker_has_no_bands_and_is_not_full() {
        let damage = Damage::viewport(20, 4);
        assert_eq!(damage.bands(), []);
        assert!(!damage.is_full());
        assert_eq!(damage.area_upper_bound(), 0);
    }

    #[test]
    fn same_row_bands_merge_within_gap_two() {
        let mut damage = Damage::viewport(20, 2);
        damage.add_row_band(0, 0, 2);
        damage.add_row_band(0, 4, 6); // gap = 2 -> merge
        assert_eq!(bands_of(&damage), vec![(0, 0, 6)]);
    }

    #[test]
    fn same_row_bands_beyond_gap_two_stay_separate() {
        let mut damage = Damage::viewport(20, 2);
        damage.add_row_band(0, 0, 2);
        damage.add_row_band(0, 5, 7); // gap = 3 -> no merge
        assert_eq!(bands_of(&damage), vec![(0, 0, 2), (0, 5, 7)]);
    }

    #[test]
    fn different_rows_stay_independent() {
        let mut damage = Damage::viewport(20, 4);
        damage.add_row_band(1, 0, 2);
        damage.add_row_band(3, 0, 2);
        assert_eq!(bands_of(&damage), vec![(1, 0, 2), (3, 0, 2)]);
    }

    #[test]
    fn rects_split_into_row_bands_and_clip_to_viewport() {
        let mut damage = Damage::viewport(10, 4);
        damage.add_rect(Rect::new(8, 2, 10, 1)); // columns 8..18 -> clip to 8..10
        assert_eq!(bands_of(&damage), vec![(2, 8, 10)]);
        damage.add_rect(Rect::new(0, 10, 5, 5)); // rows 10.. -> outside
        assert_eq!(damage.bands().len(), 1);
    }

    #[test]
    fn overlapping_adds_coalesce() {
        let mut damage = Damage::viewport(20, 2);
        damage.add_row_band(0, 2, 5);
        damage.add_row_band(0, 3, 8);
        assert_eq!(bands_of(&damage), vec![(0, 2, 8)]);
        assert_eq!(damage.area_upper_bound(), 8); // raw accumulation (3 + 5), pre-merge
    }

    #[test]
    fn chain_of_small_gaps_merges_into_one_band() {
        let mut damage = Damage::viewport(40, 2);
        for col in (0..10).step_by(3) {
            damage.add_row_band(0, col, col + 1);
        }
        // 0-1, 3-4, 6-7, 9-10: consecutive gaps of 2 -> single band.
        assert_eq!(bands_of(&damage), vec![(0, 0, 10)]);
    }

    #[test]
    fn area_over_half_the_viewport_flips_full() {
        let mut damage = Damage::viewport(10, 10); // 100 cells
        for row in 0..5_u16 {
            damage.add_row_band(row, 0, 10); // 50 cells = exactly 50%: not yet
        }
        assert!(!damage.is_full());
        damage.add_row_band(5, 0, 1); // 51 > 50%: flips
        assert!(damage.is_full());
        // Further adds are ignored once full.
        damage.add_row_band(6, 0, 10);
        assert_eq!(damage.bands(), []);
    }

    #[test]
    fn mark_full_forces_full_and_ignores_adds() {
        let mut damage = Damage::viewport(20, 4);
        damage.add_row_band(0, 0, 4);
        damage.mark_full();
        assert!(damage.is_full());
        damage.add_row_band(1, 0, 4);
        assert_eq!(damage.bands(), []);
    }

    #[test]
    fn clear_resets_everything() {
        let mut damage = Damage::viewport(20, 4);
        damage.add_row_band(0, 0, 4);
        damage.mark_full();
        damage.clear();
        assert_eq!(damage.bands(), []);
        assert!(!damage.is_full());
        assert_eq!(damage.area_upper_bound(), 0);
        damage.add_row_band(2, 1, 3);
        assert_eq!(bands_of(&damage), vec![(2, 1, 3)]);
    }

    #[test]
    fn bands_are_sorted_row_major_then_by_column() {
        let mut damage = Damage::viewport(30, 5);
        damage.add_row_band(2, 5, 7);
        damage.add_row_band(1, 9, 11);
        damage.add_row_band(2, 0, 2);
        damage.add_row_band(0, 0, 1);
        assert_eq!(
            bands_of(&damage),
            vec![(0, 0, 1), (1, 9, 11), (2, 0, 2), (2, 5, 7)]
        );
    }

    #[test]
    fn band_lengths_report_cells() {
        assert_eq!(
            Band {
                row: 0,
                start: 3,
                end: 7
            }
            .len(),
            4
        );
        assert!(
            Band {
                row: 0,
                start: 5,
                end: 5
            }
            .is_empty()
        );
    }
}
