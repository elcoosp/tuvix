//! Constraint-based layout splitting.
//!
//! M0 ships a deterministic O(n) solver with a precisely specified outcome —
//! no cassowary, no cache, no surprises. The spec:
//!
//! 1. `avail = dimension - spacing * (segments - 1)`, saturating at 0.
//! 2. Pass one: every `Min` segment claims its minimum.
//! 3. Pass two: every remaining segment claims its desired size
//!    ([`Constraint::Length`], [`Constraint::Percentage`] and
//!    [`Constraint::Ratio`] resolved against `avail`; [`Constraint::Max`] its
//!    cap), clamped to the budget left, in declaration order.
//! 4. Pass three: `Weight` segments split the entire remaining budget
//!    proportionally to their weights (weight 0 counts as 1), remainder
//!    cells to the earliest of them. Weights are shares *of the terminal
//!    dimension*: `[Weight(1), Weight(3)]` on a 100-wide terminal is 25/75
//!    of whatever the fixed segments left behind.
//! 5. Pass four: any budget the previous passes could not distribute (no
//!    `Weight` present) goes to `Fill` segments proportionally, or equally
//!    among `Min` segments otherwise (so `Min(0)` acts as a flexible
//!    filler). Remainder cells go to the earliest segments of the receiving
//!    group.
//! 6. Pass five: the segments are placed along the axis under the
//!    [`Flex`] policy. [`Flex::Start`] packs them flush from the leading
//!    edge (the M0 behavior); the other modes place the budget no
//!    constraint claimed around the solved segments — centered, flush to
//!    the trailing edge, or distributed as inter-segment gaps. A `Weight`
//!    segment always consumes the whole budget and pass four hands
//!    everything to `Fill`/`Min`, so with either present every flex mode
//!    degenerates to `Start`.
//! 7. Under `Flex::Start` unsatisfied budget stays a trailing gap;
//!    unsatisfiable claims truncate from the end.
//!
//! M1 adds a split-result cache on top: repeated [`Layout::split`] calls
//! with the same area reuse the solved segments instead of re-solving. The
//! kasuari constraint solver was evaluated and deliberately *not* adopted —
//! see the architecture decision log — the deterministic solver solves
//! UI-scale problems in tens of nanoseconds where a cassowary pass costs
//! microseconds.

use std::cell::RefCell;
use std::collections::VecDeque;

use crate::geometry::Rect;

/// Axis along which a layout splits its area.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    /// Splits top to bottom.
    #[default]
    Vertical,
    /// Splits left to right.
    Horizontal,
}

/// The desired size of one layout segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Constraint {
    /// Fixed size in cells.
    Length(u16),
    /// Minimum size; grows when pass three has budget left and no `Fill`
    /// segment exists.
    Min(u16),
    /// Maximum size; never grows beyond the cap.
    Max(u16),
    /// Percentage of the available dimension (0-100), rounded down.
    Percentage(u16),
    /// `a / b` of the available dimension, rounded down.
    Ratio(u32, u32),
    /// Proportional share of the leftover budget, weighted (weight 0 is
    /// treated as 1).
    Fill(u16),
    /// Weighted share of *the entire remaining terminal dimension* after
    /// the fixed segments claimed theirs (weight 0 is treated as 1). The
    /// shares always sum to exactly the remaining budget; combine with
    /// `Fill` only if you accept `Fill` receiving nothing.
    ///
    /// `Weight` differs from `Fill`: `Fill` shares only the budget `Min`
    /// segments did not claim and `Percentage`/`Ratio` did not grant,
    /// while `Weight` is the whole distribution — the terminal-weighted
    /// sibling of the classic TUI layout.
    ///
    /// # Examples
    ///
    /// ```
    /// use tuvix_core::geometry::Rect;
    /// use tuvix_core::layout::{Constraint, Layout};
    ///
    /// let area = Rect::new(0, 0, 100, 10);
    /// let [sidebar, main] = Layout::horizontal([
    ///     Constraint::Weight(1),
    ///     Constraint::Weight(3),
    /// ])
    /// .areas(&area);
    /// assert_eq!(sidebar.width, 25);
    /// assert_eq!(main.width, 75);
    /// ```
    Weight(u16),
}

/// How the budget no constraint claimed is placed around the solved
/// segments (pass five of the solver).
///
/// The modes only differ in *placement*: segment sizes come out of the
/// same passes regardless of the policy. With a `Weight` segment present
/// the budget is always fully consumed, and pass four hands everything to
/// `Fill`/`Min` segments — under either, every mode degenerates to
/// [`Flex::Start`].
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Flex {
    /// Segments pack flush from the leading edge; leftover trails. The
    /// M0-M5 behavior and the default.
    #[default]
    Start,
    /// Segments pack flush from the trailing edge; leftover leads.
    End,
    /// The leftover splits in half around the segments; the extra cell of
    /// an odd leftover trails.
    Center,
    /// The leftover becomes equal extra space *between* segments (never
    /// below `spacing`); rounding remainder goes to the earliest gaps.
    /// A single segment packs from the start.
    SpaceBetween,
    /// The leftover splits into `2n` equal half-gaps: one at each edge
    /// and a double one (plus `spacing`) between consecutive segments;
    /// the rounding remainder trails. A single segment ends up centered.
    SpaceAround,
    /// The leftover splits into `n + 1` equal gaps — one at each edge and
    /// one (plus `spacing`) between consecutive segments; the rounding
    /// remainder trails. A single segment ends up centered.
    SpaceEvenly,
}

/// A layout plan: direction, constraints and spacing, resolved against an
/// area by [`Layout::split`].
///
/// # Examples
///
/// ```
/// use tuvix_core::geometry::Rect;
/// use tuvix_core::layout::{Constraint, Layout};
///
/// let area = Rect::new(0, 0, 80, 30);
/// let [top, bottom] = Layout::vertical([Constraint::Length(10), Constraint::Min(0)]).areas(&area);
/// assert_eq!(top.height, 10);
/// assert_eq!(bottom.height, 20);
/// ```
#[derive(Debug)]
pub struct Layout {
    direction: Direction,
    constraints: Vec<Constraint>,
    spacing: u16,
    flex: Flex,
    /// Memoized split results (the constraints are fixed per instance, so
    /// the area alone is a complete cache key). Interior mutability keeps
    /// `split(&self)` signature-compatible with M0.
    cache: RefCell<SplitCache>,
}

impl Clone for Layout {
    /// Clones the layout *plan* but not the memoized results: the clone
    /// starts with a fresh, empty cache so different spacings (the builder
    /// clones) never observe each other's solutions.
    fn clone(&self) -> Self {
        Self {
            direction: self.direction,
            constraints: self.constraints.clone(),
            spacing: self.spacing,
            flex: self.flex,
            cache: RefCell::new(SplitCache::default()),
        }
    }
}

/// Capacity of the per-[`Layout`] split cache.
const SPLIT_CACHE_CAPACITY: usize = 4;

#[derive(Debug, Default, Clone)]
struct SplitCache {
    entries: VecDeque<(Rect, Vec<Rect>)>,
    hits: u64,
    misses: u64,
}

impl SplitCache {
    fn get(&mut self, area: Rect) -> Option<Vec<Rect>> {
        let cached = self
            .entries
            .iter()
            .find(|(cached_area, _)| *cached_area == area)
            .map(|(_, rects)| rects.clone());
        if cached.is_some() {
            self.hits += 1;
        } else {
            self.misses += 1;
        }
        cached
    }

    fn put(&mut self, area: Rect, rects: Vec<Rect>) {
        if self.entries.len() >= SPLIT_CACHE_CAPACITY {
            self.entries.pop_front();
        }
        self.entries.push_back((area, rects));
    }
}

impl Layout {
    /// Creates a vertical layout (top to bottom) from the constraints.
    pub fn vertical<C>(constraints: C) -> Self
    where
        C: IntoIterator<Item = Constraint>,
    {
        Self {
            direction: Direction::Vertical,
            constraints: constraints.into_iter().collect(),
            spacing: 0,
            flex: Flex::default(),
            cache: RefCell::new(SplitCache::default()),
        }
    }

    /// Creates a horizontal layout (left to right) from the constraints.
    pub fn horizontal<C>(constraints: C) -> Self
    where
        C: IntoIterator<Item = Constraint>,
    {
        Self {
            direction: Direction::Horizontal,
            constraints: constraints.into_iter().collect(),
            spacing: 0,
            flex: Flex::default(),
            cache: RefCell::new(SplitCache::default()),
        }
    }

    /// Inserts `spacing` empty cells between consecutive segments.
    pub fn spacing(mut self, spacing: u16) -> Self {
        self.spacing = spacing;
        self
    }

    /// Sets the pass-five placement policy (default [`Flex::Start`]).
    ///
    /// # Examples
    ///
    /// ```
    /// use tuvix_core::geometry::Rect;
    /// use tuvix_core::layout::{Constraint, Flex, Layout};
    ///
    /// let area = Rect::new(0, 0, 40, 1);
    /// let [left, right] = Layout::horizontal([
    ///     Constraint::Length(10),
    ///     Constraint::Length(10),
    /// ])
    /// .flex(Flex::SpaceBetween)
    /// .areas(&area);
    /// assert_eq!((left.x, right.x), (0, 30));
    /// ```
    pub fn flex(mut self, flex: Flex) -> Self {
        self.flex = flex;
        self
    }

    /// Splits `area` and returns the resulting segment rects.
    ///
    /// Results are memoized per area: repeated calls with the same area
    /// reuse the previous solution (see [`Layout::cache_stats`]).
    pub fn split(&self, area: Rect) -> Vec<Rect> {
        if let Some(cached) = self.cache.borrow_mut().get(area) {
            return cached;
        }
        let rects = self.solve(area);
        self.cache.borrow_mut().put(area, rects.clone());
        rects
    }

    /// `(hits, misses)` of the split cache since this layout was created.
    pub fn cache_stats(&self) -> (u64, u64) {
        let cache = self.cache.borrow();
        (cache.hits, cache.misses)
    }

    /// The uncached solver: the M0 three-pass algorithm, unchanged.
    fn solve(&self, area: Rect) -> Vec<Rect> {
        let count = self.constraints.len();
        if count == 0 || area.is_empty() {
            return Vec::new();
        }
        let total = match self.direction {
            Direction::Vertical => area.height,
            Direction::Horizontal => area.width,
        };
        let gaps = self
            .spacing
            .saturating_mul(u16::try_from(count - 1).unwrap_or(u16::MAX));
        let avail = u32::from(total.saturating_sub(gaps));

        // Pass one: minimums claim budget first.
        let mins: Vec<u32> = self
            .constraints
            .iter()
            .map(|c| match c {
                Constraint::Min(m) => u32::from((*m).min(avail as u16)),
                _ => 0,
            })
            .collect();
        let mut sizes: Vec<u32> = Vec::with_capacity(count);
        let mut budget = avail.saturating_sub(mins.iter().sum::<u32>());

        // Pass two: desired sizes for everything else, clamped to budget.
        for (index, constraint) in self.constraints.iter().enumerate() {
            if mins[index] > 0 {
                sizes.push(mins[index]);
                continue;
            }
            let desired = match constraint {
                Constraint::Length(l) => u32::from(*l),
                Constraint::Max(m) => u32::from(*m),
                Constraint::Percentage(p) => avail * u32::from(*p) / 100,
                Constraint::Ratio(a, b) => avail
                    .checked_mul(*a)
                    .and_then(|scaled| scaled.checked_div(*b))
                    .unwrap_or(0),
                Constraint::Min(_) | Constraint::Fill(_) | Constraint::Weight(_) => 0,
            };
            let granted = desired.min(budget);
            budget -= granted;
            sizes.push(granted);
        }

        // Pass three: Weight segments split the entire remaining budget
        // proportionally to their weights (remainder cells to the earliest
        // of them). After this pass the budget is always zero when any
        // Weight segment exists.
        self.apply_weight_pass(&mut sizes, &mut budget);

        // Pass four: distribute whatever the earlier passes left (only
        // possible when no Weight segment exists).
        if budget > 0 {
            let fills: Vec<usize> = self
                .constraints
                .iter()
                .enumerate()
                .filter(|(_, c)| matches!(c, Constraint::Fill(_)))
                .map(|(index, _)| index)
                .collect();
            if fills.is_empty() {
                let mins: Vec<usize> = self
                    .constraints
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| matches!(c, Constraint::Min(_)))
                    .map(|(index, _)| index)
                    .collect();
                distribute_equally(&mut sizes, &mins, budget);
            } else {
                let weights: Vec<u32> = fills
                    .iter()
                    .map(|&index| match self.constraints[index] {
                        Constraint::Fill(w) => u32::from(w.max(1)),
                        _ => 1,
                    })
                    .collect();
                distribute_proportionally(&mut sizes, &fills, &weights, budget);
            }
        }

        // Pass five: place the segments along the axis under the flex
        // policy. The leftover is the budget the size passes could not
        // distribute; the placement never overflows the area because the
        // leading space plus sizes plus gaps is exactly `avail`.
        let total: u32 = sizes.iter().sum();
        let leftover = avail.saturating_sub(total);
        let (lead, gaps) = self.placement(count, leftover);
        let lead = lead.min(u32::from(u16::MAX)) as u16;
        let start = match self.direction {
            Direction::Vertical => area.y,
            Direction::Horizontal => area.x,
        };
        let mut placed = Vec::with_capacity(count);
        let mut cursor = start.saturating_add(lead);
        for (index, &size) in sizes.iter().enumerate() {
            let size = size.min(u32::from(u16::MAX)) as u16;
            let rect = match self.direction {
                Direction::Vertical => Rect::new(area.x, cursor, area.width, size),
                Direction::Horizontal => Rect::new(cursor, area.y, size, area.height),
            };
            placed.push(rect);
            // The trailing edge rides in the gap vector as one extra
            // entry after the last segment.
            let gap = gaps.get(index).copied().unwrap_or(0);
            let gap = gap.min(u32::from(u16::MAX)) as u16;
            cursor = cursor.saturating_add(size).saturating_add(gap);
        }
        placed
    }

    /// Pass five of the solver: how much space leads the first segment
    /// and the gap after every segment (the trailing edge rides in the
    /// vector as one extra entry) for the given undistributed `leftover`.
    ///
    /// Every mode places `spacing` between consecutive segments and grows
    /// from there; the remainder rules follow the module convention
    /// (earliest receivers first, except the trailing edge under
    /// `Center`/`SpaceAround`/`SpaceEvenly`, where the extra cell trails).
    fn placement(&self, count: usize, leftover: u32) -> (u32, Vec<u32>) {
        let spacing = u32::from(self.spacing);
        match self.flex {
            Flex::Start => (0, vec![spacing; count.saturating_sub(1)]),
            Flex::End => (leftover, vec![spacing; count.saturating_sub(1)]),
            Flex::Center => (leftover / 2, vec![spacing; count.saturating_sub(1)]),
            Flex::SpaceBetween => {
                if count <= 1 {
                    return (0, Vec::new());
                }
                let slots = (count - 1) as u32;
                let base = leftover / slots;
                let remainder = (leftover % slots) as usize;
                let gaps = (0..count - 1)
                    .map(|index| spacing + base + u32::from(index < remainder))
                    .collect();
                (0, gaps)
            }
            Flex::SpaceAround => {
                let slots = 2 * count as u32;
                let side = leftover / slots;
                let remainder = leftover - slots * side;
                let mut gaps = vec![2 * side + spacing; count.saturating_sub(1)];
                gaps.push(side + remainder);
                (side, gaps)
            }
            Flex::SpaceEvenly => {
                let slots = count as u32 + 1;
                let each = leftover / slots;
                let remainder = leftover - slots * each;
                let mut gaps = vec![spacing + each; count.saturating_sub(1)];
                gaps.push(each + remainder);
                (each, gaps)
            }
        }
    }

    /// Pass three of the solver: Weight segments split the entire
    /// remaining `budget` proportionally to their weights (weight 0 counts
    /// as 1; remainder cells go to the earliest Weight segment). When at
    /// least one Weight segment exists the budget is always consumed.
    fn apply_weight_pass(&self, sizes: &mut [u32], budget: &mut u32) {
        if *budget == 0 {
            return;
        }
        let weighted: Vec<usize> = self
            .constraints
            .iter()
            .enumerate()
            .filter(|(_, c)| matches!(c, Constraint::Weight(_)))
            .map(|(index, _)| index)
            .collect();
        if weighted.is_empty() {
            return;
        }
        let weights: Vec<u32> = weighted
            .iter()
            .map(|&index| match self.constraints[index] {
                Constraint::Weight(w) => u32::from(w.max(1)),
                _ => 1,
            })
            .collect();
        distribute_proportionally(sizes, &weighted, &weights, *budget);
        *budget = 0;
    }

    /// Splits `area` into exactly the number of constraints as a fixed-size
    /// array; panics when the lengths differ.
    ///
    /// Convenience for `let [a, b] = layout.areas(&area);`.
    pub fn areas<const N: usize>(&self, area: &Rect) -> [Rect; N] {
        let rects = self.split(*area);
        let count = rects.len();
        let rects: [Rect; N] = rects
            .try_into()
            .unwrap_or_else(|_| panic!("layout produced {count} rects, expected {N}"));
        rects
    }
}

/// Distributes `budget` equally over `indices`, earliest indices get the
/// rounding remainder first.
fn distribute_equally(sizes: &mut [u32], indices: &[usize], budget: u32) {
    if indices.is_empty() {
        return;
    }
    let share = budget / indices.len() as u32;
    let remainder = (budget % indices.len() as u32) as usize;
    for (position, &index) in indices.iter().enumerate() {
        let extra = u32::from(position < remainder);
        sizes[index] += share + extra;
    }
}

/// Distributes `budget` over `indices` proportional to `weights`, rounding
/// remainders going to the earliest receiving segments.
fn distribute_proportionally(sizes: &mut [u32], indices: &[usize], weights: &[u32], budget: u32) {
    let total_weight: u32 = weights.iter().sum();
    if total_weight == 0 || indices.len() != weights.len() {
        distribute_equally(sizes, indices, budget);
        return;
    }
    let mut floored_total = 0;
    for (&index, &weight) in indices.iter().zip(weights.iter()) {
        let share = budget * weight / total_weight;
        floored_total += share;
        sizes[index] += share;
    }
    let remainder = (budget - floored_total) as usize;
    for &index in indices.iter().take(remainder) {
        sizes[index] += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Size;

    fn area() -> Rect {
        Rect::new(0, 0, 80, 30)
    }

    fn heights(rects: &[Rect]) -> Vec<u16> {
        rects.iter().map(|r| r.height).collect()
    }

    fn widths(rects: &[Rect]) -> Vec<u16> {
        rects.iter().map(|r| r.width).collect()
    }

    mod direction {
        use super::*;

        #[test]
        fn vertical_splits_by_height() {
            let rects =
                Layout::vertical([Constraint::Length(10), Constraint::Min(0)]).split(area());
            assert_eq!(heights(&rects), vec![10, 20]);
            assert!(rects.iter().all(|r| r.width == 80));
        }

        #[test]
        fn horizontal_splits_by_width() {
            let rects =
                Layout::horizontal([Constraint::Length(30), Constraint::Min(0)]).split(area());
            assert_eq!(widths(&rects), vec![30, 50]);
            assert!(rects.iter().all(|r| r.height == 30));
        }

        #[test]
        fn segments_are_adjacent() {
            let rects = Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(area());
            assert_eq!(rects[0].bottom(), rects[1].y);
            assert_eq!(rects[1].bottom(), 30);
        }
    }

    mod constraints {
        use super::*;

        #[test]
        fn length_is_exact() {
            let rects = Layout::vertical([Constraint::Length(7), Constraint::Min(0)]).split(area());
            assert_eq!(heights(&rects), vec![7, 23]);
        }

        #[test]
        fn percentage_of_available() {
            let rects = Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(area());
            assert_eq!(heights(&rects), vec![15, 15]);
        }

        #[test]
        fn percentage_rounds_down() {
            let rects = Layout::vertical([Constraint::Percentage(33), Constraint::Percentage(33)])
                .split(area());
            assert_eq!(heights(&rects), vec![9, 9]);
        }

        #[test]
        fn ratio_of_available() {
            // Ratio floors per segment; the leftover cell stays unassigned
            // (a trailing gap), as specified for M0.
            let rects = Layout::horizontal([Constraint::Ratio(1, 3), Constraint::Ratio(2, 3)])
                .split(area());
            assert_eq!(widths(&rects), vec![26, 53]);
        }

        #[test]
        fn ratio_with_zero_denominator_is_zero() {
            let rects =
                Layout::horizontal([Constraint::Ratio(1, 0), Constraint::Min(0)]).split(area());
            assert_eq!(widths(&rects), vec![0, 80]);
        }

        #[test]
        fn fill_takes_proportional_share() {
            let rects = Layout::vertical([
                Constraint::Length(10),
                Constraint::Fill(1),
                Constraint::Fill(2),
            ])
            .split(area());
            // 20 leftover cells at weights 1:2 with the rounding remainder
            // going to the earliest segment: 7 + 13.
            assert_eq!(heights(&rects), vec![10, 7, 13]);
        }

        #[test]
        fn weight_splits_the_whole_remaining_dimension() {
            let rects = Layout::vertical([
                Constraint::Length(10),
                Constraint::Weight(1),
                Constraint::Weight(3),
            ])
            .split(area());
            // 20 remaining cells at 1:3 = 5 + 15, exactly, no remainder.
            assert_eq!(heights(&rects), vec![10, 5, 15]);
        }

        #[test]
        fn weight_remainder_goes_to_the_earliest_segment() {
            // 30 cells, weights 1:1:1 -> floors 10,10,10 (remainder 0).
            let rects = Layout::vertical([
                Constraint::Weight(1),
                Constraint::Weight(1),
                Constraint::Weight(1),
            ])
            .split(area());
            assert_eq!(heights(&rects), vec![10, 10, 10]);

            // 100 cells, weights 1:2 -> floors 33,66 (remainder 1 -> earliest).
            let rects = Layout::horizontal([Constraint::Weight(1), Constraint::Weight(2)])
                .split(Rect::new(0, 0, 100, 10));
            assert_eq!(widths(&rects), vec![34, 66]);
        }

        #[test]
        fn weight_zero_counts_as_one() {
            let rects =
                Layout::vertical([Constraint::Weight(0), Constraint::Weight(3)]).split(area());
            assert_eq!(heights(&rects), vec![8, 22]);
        }

        #[test]
        fn weight_after_min_claims() {
            let rects = Layout::vertical([
                Constraint::Min(30),
                Constraint::Weight(1),
                Constraint::Weight(1),
            ])
            .split(Rect::new(0, 0, 80, 100));
            // Min(30) claims its minimum first; the other 70 split evenly.
            assert_eq!(heights(&rects), vec![30, 35, 35]);
        }

        #[test]
        fn weight_consumes_what_fill_would_get() {
            let rects = Layout::vertical([
                Constraint::Length(10),
                Constraint::Weight(1),
                Constraint::Fill(1),
            ])
            .split(area());
            // Weight takes the whole remaining budget; Fill gets nothing.
            assert_eq!(heights(&rects), vec![10, 20, 0]);
        }

        #[test]
        fn single_weight_takes_all_remaining() {
            let rects =
                Layout::vertical([Constraint::Length(5), Constraint::Weight(7)]).split(area());
            assert_eq!(heights(&rects), vec![5, 25]);
        }

        #[test]
        fn weight_respects_spacing() {
            let rects = Layout::vertical([
                Constraint::Weight(1),
                Constraint::Weight(1),
                Constraint::Weight(1),
            ])
            .spacing(1)
            .split(area());
            // 30 - 2 gaps = 28, weights 1:1:1 -> 10,9,9 (earliest remainder).
            assert_eq!(heights(&rects), vec![10, 9, 9]);
        }

        #[test]
        fn weight_is_hashable_and_comparable() {
            use std::collections::HashSet;
            let mut set = HashSet::new();
            set.insert(Constraint::Weight(2));
            set.insert(Constraint::Weight(2));
            set.insert(Constraint::Fill(2));
            assert_eq!(set.len(), 2);
        }

        #[test]
        fn weight_results_are_cached_like_every_constraint() {
            let layout = Layout::vertical([Constraint::Weight(1), Constraint::Weight(2)]);
            let first = layout.split(area());
            let second = layout.split(area());
            assert_eq!(first, second);
            let (hits, _) = layout.cache_stats();
            assert!(hits > 0);
        }

        #[test]
        fn min_grows_when_no_fill() {
            let rects = Layout::vertical([Constraint::Min(10), Constraint::Min(20)]).split(area());
            assert_eq!(heights(&rects), vec![10, 20]);
        }

        #[test]
        fn min_zero_acts_as_filler() {
            let rects =
                Layout::vertical([Constraint::Length(10), Constraint::Min(0)]).split(area());
            assert_eq!(heights(&rects), vec![10, 20]);
        }

        #[test]
        fn max_is_capped_and_min_grows() {
            let rects = Layout::vertical([Constraint::Max(5), Constraint::Min(10)]).split(area());
            assert_eq!(heights(&rects), vec![5, 25]);
        }

        #[test]
        fn equal_mins_split_evenly() {
            let rects =
                Layout::vertical([Constraint::Min(0), Constraint::Min(0), Constraint::Min(0)])
                    .split(area());
            assert_eq!(heights(&rects), vec![10, 10, 10]);
        }
    }

    mod edge_cases {
        use super::*;

        #[test]
        fn shortage_truncates_from_the_end() {
            let rects = Layout::vertical([Constraint::Length(20), Constraint::Length(20)])
                .split(Rect::new(0, 0, 80, 30));
            assert_eq!(heights(&rects), vec![20, 10]);
        }

        #[test]
        fn shortage_clamps_to_remaining_budget() {
            let rects = Layout::vertical([Constraint::Length(20), Constraint::Length(20)])
                .split(Rect::new(0, 0, 80, 10));
            assert_eq!(heights(&rects), vec![10, 0]);
        }

        #[test]
        fn spacing_shrinks_available() {
            let rects =
                Layout::vertical([Constraint::Min(0), Constraint::Min(0), Constraint::Min(0)])
                    .spacing(1)
                    .split(area());
            // 30 - 2 gaps = 28, split equally.
            assert_eq!(heights(&rects), vec![10, 9, 9]);
        }

        #[test]
        fn no_constraints_yields_no_rects() {
            assert_eq!(
                Layout::vertical(Vec::<Constraint>::new())
                    .split(area())
                    .len(),
                0
            );
        }

        #[test]
        fn empty_area_yields_no_rects() {
            assert_eq!(
                Layout::vertical([Constraint::Length(5)])
                    .split(Rect::ZERO)
                    .len(),
                0
            );
        }

        #[test]
        fn oversized_length_clamps_to_available() {
            let rects = Layout::vertical([Constraint::Length(1000), Constraint::Min(0)])
                .split(Rect::new(0, 0, 80, 30));
            assert_eq!(heights(&rects), vec![30, 0]);
        }

        #[test]
        fn zero_area_input_produces_nothing() {
            let rects = Layout::vertical([Constraint::Min(0)]).split(Rect::new(5, 5, 0, 0));
            assert_eq!(rects.len(), 0);
        }
    }

    mod split_cache {
        use super::*;

        #[test]
        fn repeated_split_with_same_area_hits_the_cache() {
            let layout = Layout::vertical([Constraint::Length(10), Constraint::Min(0)]);
            let first = layout.split(area());
            let second = layout.split(area());
            assert_eq!(first, second);
            assert_eq!(layout.cache_stats(), (1, 1));
        }

        #[test]
        fn different_areas_are_distinct_cache_entries() {
            let layout = Layout::horizontal([Constraint::Fill(1), Constraint::Fill(1)]);
            let _ = layout.split(Rect::new(0, 0, 80, 10));
            let _ = layout.split(Rect::new(0, 0, 80, 20));
            let _ = layout.split(Rect::new(0, 0, 80, 10));
            assert_eq!(layout.cache_stats(), (1, 2));
        }

        #[test]
        fn cached_result_is_identical_to_fresh_solve() {
            let layout = Layout::vertical([Constraint::Percentage(20), Constraint::Min(0)]);
            let fresh = layout.solve(area());
            let cached = layout.split(area());
            assert_eq!(fresh, cached);
        }

        #[test]
        fn cache_evicts_oldest_beyond_capacity() {
            let layout = Layout::vertical([Constraint::Fill(1), Constraint::Fill(1)]);
            let areas: [Rect; 5] = [
                Rect::new(0, 0, 10, 10),
                Rect::new(0, 0, 20, 10),
                Rect::new(0, 0, 30, 10),
                Rect::new(0, 0, 40, 10),
                Rect::new(0, 0, 50, 10),
            ];
            for a in areas {
                let _ = layout.split(a);
            }
            // All 5 were misses; the oldest (10x10) fell out of the 4 slots.
            let _ = layout.split(areas[0]);
            assert_eq!(layout.cache_stats(), (0, 6));
        }

        #[test]
        fn mutated_spacing_changes_the_solution_for_the_same_area() {
            // spacing() clones the builder into a NEW Layout, and clones get
            // a fresh cache, so per-instance caches never collide across
            // different spacings.
            let plain = Layout::vertical([Constraint::Min(0), Constraint::Min(0)]);
            let spaced = plain.clone().spacing(2);
            let a = plain.split(Rect::new(0, 0, 20, 4));
            let b = spaced.split(Rect::new(0, 0, 20, 4));
            assert_eq!(a[0].height, 2);
            // 4 rows minus 2 gap cells = 2 to distribute.
            assert_eq!(b[0].height, 1);
            assert_eq!(b[1].y, b[0].bottom() + 2);
        }

        #[test]
        fn clone_gets_a_fresh_cache() {
            let original = Layout::vertical([Constraint::Length(5), Constraint::Min(0)]);
            let _ = original.split(area());
            let clone = original.clone();
            assert_eq!(clone.cache_stats(), (0, 0), "clone must not inherit stats");
        }
    }

    mod areas_helper {
        use super::*;

        #[test]
        fn deconstructs_into_fixed_array() {
            let [top, bottom] =
                Layout::vertical([Constraint::Length(10), Constraint::Min(0)]).areas(&area());
            assert_eq!(top.size(), Size::new(80, 10));
            assert_eq!(bottom.size(), Size::new(80, 20));
        }

        #[test]
        #[should_panic(expected = "expected")]
        fn panics_on_constraint_count_mismatch() {
            let _: [Rect; 3] =
                Layout::vertical([Constraint::Length(10), Constraint::Min(0)]).areas(&area());
        }
    }

    mod flex {
        use super::*;

        /// Three 10-wide segments in a 40-wide row: sizes claim 30,
        /// leftover 20 — the classic flex playground.
        fn segments(width: u16, flex: Flex) -> Vec<Rect> {
            Layout::horizontal([
                Constraint::Length(10),
                Constraint::Length(10),
                Constraint::Length(10),
            ])
            .flex(flex)
            .split(Rect::new(0, 0, width, 1))
        }

        #[test]
        fn default_is_start() {
            assert_eq!(Flex::default(), Flex::Start);
        }

        #[test]
        fn start_packs_from_the_leading_edge() {
            // Regression guard: the M0-M5 placement is untouched.
            let rects = segments(40, Flex::Start);
            assert_eq!(
                rects.iter().map(|r| r.x).collect::<Vec<_>>(),
                vec![0, 10, 20]
            );
            // And the default is Start: no `.flex()` call at all.
            let plain = Layout::horizontal([Constraint::Length(10), Constraint::Length(10)])
                .split(Rect::new(0, 0, 40, 1));
            assert_eq!(plain.iter().map(|r| r.x).collect::<Vec<_>>(), vec![0, 10]);
        }

        #[test]
        fn end_packs_from_the_trailing_edge() {
            // 3×10 in a 50-wide row: leftover 20 leads.
            let rects = segments(50, Flex::End);
            assert_eq!(
                rects.iter().map(|r| r.x).collect::<Vec<_>>(),
                vec![20, 30, 40]
            );
            assert_eq!(rects[2].right(), 50);
        }

        #[test]
        fn center_splits_the_leftover_and_the_odd_cell_trails() {
            let rects = segments(50, Flex::Center);
            assert_eq!(
                rects.iter().map(|r| r.x).collect::<Vec<_>>(),
                vec![10, 20, 30]
            );
            // Odd leftover: [Length(10)] in a 41-wide row -> lead 15,
            // trailing 16.
            let single = Layout::horizontal([Constraint::Length(10)])
                .flex(Flex::Center)
                .split(Rect::new(0, 0, 41, 1));
            assert_eq!(single[0].x, 15);
            assert_eq!(single[0].right(), 25);
        }

        #[test]
        fn space_between_spreads_equal_gaps() {
            // Leftover 20 over two gaps: 10 cells between each pair;
            // the row ends flush at the trailing edge.
            let rects = segments(50, Flex::SpaceBetween);
            assert_eq!(
                rects.iter().map(|r| r.x).collect::<Vec<_>>(),
                vec![0, 20, 40]
            );
            assert_eq!(rects[2].right(), 50);
        }

        #[test]
        fn space_between_remainder_goes_to_the_earliest_gaps() {
            // Leftover 21 over two gaps: 11 then 10.
            let rects = segments(51, Flex::SpaceBetween);
            assert_eq!(
                rects.iter().map(|r| r.x).collect::<Vec<_>>(),
                vec![0, 21, 41]
            );
            assert_eq!(rects[2].right(), 51);
        }

        #[test]
        fn space_between_grows_over_a_fixed_spacing() {
            // spacing is the minimum gap: leftover 10 over one gap adds
            // to the requested 2.
            let rects = Layout::horizontal([Constraint::Length(10), Constraint::Length(10)])
                .flex(Flex::SpaceBetween)
                .spacing(2)
                .split(Rect::new(0, 0, 32, 1));
            assert_eq!(rects[1].x, 22, "10 + (2 spacing + 10 leftover)");
        }

        #[test]
        fn space_around_places_half_gaps_at_the_edges() {
            // Leftover 20 over 2n = 6 slots: side 3, remainder 2 trails.
            // lead 3 + sizes 30 + double gaps 2×6 + trailing 5 = 50.
            let rects = segments(50, Flex::SpaceAround);
            assert_eq!(
                rects.iter().map(|r| r.x).collect::<Vec<_>>(),
                vec![3, 19, 35]
            );
            assert_eq!(rects[2].right(), 45);
        }

        #[test]
        fn space_evenly_places_equal_gaps_everywhere() {
            // Leftover 20 over n+1 = 4 slots: 5 each, remainder 0.
            let rects = segments(50, Flex::SpaceEvenly);
            assert_eq!(
                rects.iter().map(|r| r.x).collect::<Vec<_>>(),
                vec![5, 20, 35]
            );
            // Odd leftover: 21 -> each 5, remainder 1 trails.
            let rects = segments(51, Flex::SpaceEvenly);
            assert_eq!(
                rects.iter().map(|r| r.x).collect::<Vec<_>>(),
                vec![5, 20, 35]
            );
        }

        #[test]
        fn single_segment_centers_under_space_around_and_evenly() {
            for flex in [Flex::SpaceAround, Flex::SpaceEvenly] {
                let rects = Layout::horizontal([Constraint::Length(10)])
                    .flex(flex)
                    .split(Rect::new(0, 0, 41, 1));
                assert_eq!(rects[0].x, 15, "{flex:?} centers a single segment");
            }
            // SpaceBetween has nothing to distribute between: packs start.
            let rects = Layout::horizontal([Constraint::Length(10)])
                .flex(Flex::SpaceBetween)
                .split(Rect::new(0, 0, 41, 1));
            assert_eq!(rects[0].x, 0);
        }

        #[test]
        fn vertical_flex_centers_rows() {
            let rects = Layout::vertical([Constraint::Length(5), Constraint::Length(5)])
                .flex(Flex::Center)
                .split(Rect::new(0, 0, 10, 30));
            assert_eq!(rects.iter().map(|r| r.y).collect::<Vec<_>>(), vec![10, 15]);
        }

        #[test]
        fn flex_is_a_noop_when_weight_consumes_the_budget() {
            let area = Rect::new(0, 0, 100, 1);
            let rects = Layout::horizontal([Constraint::Weight(1), Constraint::Weight(3)])
                .flex(Flex::Center)
                .split(area);
            assert_eq!(
                rects.iter().map(|r| r.x).collect::<Vec<_>>(),
                vec![0, 25],
                "weights leave no leftover; every mode degenerates to Start"
            );
        }

        #[test]
        fn flex_is_a_noop_when_min_segments_absorb_the_budget() {
            let area = Rect::new(0, 0, 80, 1);
            let rects = Layout::horizontal([Constraint::Length(10), Constraint::Min(0)])
                .flex(Flex::Center)
                .split(area);
            assert_eq!(rects.iter().map(|r| r.x).collect::<Vec<_>>(), vec![0, 10]);
        }

        #[test]
        fn flex_survives_the_split_cache() {
            // The flex mode is part of the plan, so a cloned layout that
            // changes flex must not observe the original's cached solve.
            let base = Layout::horizontal([Constraint::Length(10), Constraint::Length(10)]);
            let started = base.split(Rect::new(0, 0, 40, 1));
            let centered = base.clone().flex(Flex::Center);
            let rects = centered.split(Rect::new(0, 0, 40, 1));
            assert_eq!(started[0].x, 0);
            assert_eq!(rects[0].x, 10);
        }
    }
}
