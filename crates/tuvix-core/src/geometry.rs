//! Primitives for positions, sizes and rectangular regions on the terminal grid.
//!
//! All coordinates are zero-based; the origin is the top-left cell of the
//! terminal. Every type here is cheap to copy and safe to store by value.

use core::fmt;

/// A zero-based position on the terminal grid: column `x`, row `y`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Position {
    /// Column index, zero-based, left to right.
    pub x: u16,
    /// Row index, zero-based, top to bottom.
    pub y: u16,
}

impl Position {
    /// Creates a position from `x` (column) and `y` (row).
    pub const fn new(x: u16, y: u16) -> Self {
        Self { x, y }
    }
}

impl fmt::Display for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}, {})", self.x, self.y)
    }
}

/// A size in terminal cells: `width` columns by `height` rows.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Size {
    /// Number of columns.
    pub width: u16,
    /// Number of rows.
    pub height: u16,
}

impl Size {
    /// Creates a size from `width` and `height`.
    pub const fn new(width: u16, height: u16) -> Self {
        Self { width, height }
    }

    /// Returns `true` when either dimension is zero.
    pub const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }
}

impl fmt::Display for Size {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}×{}", self.width, self.height)
    }
}

/// A rectangular region anchored at `(x, y)` spanning `width` × `height` cells.
///
/// The right and bottom edges are exclusive: a rect at `x = 0` with
/// `width = 10` covers columns `0..10`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rect {
    /// Left edge, inclusive.
    pub x: u16,
    /// Top edge, inclusive.
    pub y: u16,
    /// Number of columns.
    pub width: u16,
    /// Number of rows.
    pub height: u16,
}

impl Rect {
    /// The empty rectangle anchored at the origin.
    pub const ZERO: Self = Self {
        x: 0,
        y: 0,
        width: 0,
        height: 0,
    };

    /// Creates a rect from an inclusive top-left corner and an exclusive size.
    pub const fn new(x: u16, y: u16, width: u16, height: u16) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Creates a rect anchored at the origin with the given size.
    pub const fn from_size(size: Size) -> Self {
        Self {
            x: 0,
            y: 0,
            width: size.width,
            height: size.height,
        }
    }

    /// Returns the size of the rect.
    pub const fn size(self) -> Size {
        Size {
            width: self.width,
            height: self.height,
        }
    }

    /// Returns the number of cells covered by the rect, computed in `u32` so
    /// that `width = height = u16::MAX` does not overflow.
    pub fn area(self) -> u32 {
        u32::from(self.width) * u32::from(self.height)
    }

    /// Returns `true` when either dimension is zero.
    pub const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// Returns the exclusive right edge, saturating at `u16::MAX`.
    pub const fn right(self) -> u16 {
        self.x.saturating_add(self.width)
    }

    /// Returns the exclusive bottom edge, saturating at `u16::MAX`.
    pub const fn bottom(self) -> u16 {
        self.y.saturating_add(self.height)
    }

    /// Returns `true` when `position` lies inside the rect: left/top edges are
    /// inclusive, right/bottom edges are exclusive.
    pub fn contains(self, position: impl Into<Position>) -> bool {
        let position = position.into();
        position.x >= self.x
            && position.x < self.right()
            && position.y >= self.y
            && position.y < self.bottom()
    }

    /// Returns the overlap of the two rects, or `None` when they do not share
    /// at least one cell (touching edges do not count as overlap).
    pub fn intersection(self, other: Self) -> Option<Self> {
        let left = u32::from(self.x.max(other.x));
        let top = u32::from(self.y.max(other.y));
        let right = self.right32().min(other.right32());
        let bottom = self.bottom32().min(other.bottom32());
        if right > left && bottom > top {
            Some(Self {
                x: left as u16,
                y: top as u16,
                width: (right - left) as u16,
                height: (bottom - top) as u16,
            })
        } else {
            None
        }
    }

    /// Returns the smallest rect covering both rects. Dimensions saturate at
    /// `u16::MAX`.
    pub fn union(self, other: Self) -> Self {
        let left = u32::from(self.x.min(other.x));
        let top = u32::from(self.y.min(other.y));
        let right = self.right32().max(other.right32());
        let bottom = self.bottom32().max(other.bottom32());
        Self {
            x: left as u16,
            y: top as u16,
            width: (right - left).min(u32::from(u16::MAX) - left) as u16,
            height: (bottom - top).min(u32::from(u16::MAX) - top) as u16,
        }
    }

    /// Returns a `width` × `height` rect centered inside `self`, clamped
    /// to fit (a popup never exceeds its host).
    ///
    /// The centering truncates toward the leading edge — half of any
    /// odd remainder stays trailing, matching [`Flex::Center`](crate::layout::Flex::Center)'s rule.
    ///
    /// ```
    /// use tuvix_core::geometry::Rect;
    ///
    /// let host = Rect::new(0, 0, 80, 24);
    /// let popup = host.centered(30, 10);
    /// assert_eq!(popup, Rect::new(25, 7, 30, 10));
    /// // Oversized requests clamp to the host.
    /// assert_eq!(host.centered(100, 30), host);
    /// ```
    pub fn centered(self, width: u16, height: u16) -> Self {
        let width = width.min(self.width);
        let height = height.min(self.height);
        Self {
            x: self.x + (self.width - width) / 2,
            y: self.y + (self.height - height) / 2,
            width,
            height,
        }
    }

    /// Exclusive right edge as `u32`, immune to saturation for exact math.
    fn right32(self) -> u32 {
        u32::from(self.x) + u32::from(self.width)
    }

    /// Exclusive bottom edge as `u32`, immune to saturation for exact math.
    fn bottom32(self) -> u32 {
        u32::from(self.y) + u32::from(self.height)
    }
}

impl fmt::Display for Rect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}x{} at {}, {}",
            self.width, self.height, self.x, self.y
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod position {
        use super::*;

        #[test]
        fn new_sets_fields() {
            let p = Position::new(3, 7);
            assert_eq!(p.x, 3);
            assert_eq!(p.y, 7);
        }

        #[test]
        fn default_is_origin() {
            assert_eq!(Position::default(), Position::new(0, 0));
        }

        #[test]
        fn display_shows_coordinates() {
            assert_eq!(Position::new(1, 2).to_string(), "(1, 2)");
        }
    }

    mod size {
        use super::*;

        #[test]
        fn new_sets_fields() {
            let s = Size::new(80, 24);
            assert_eq!(s.width, 80);
            assert_eq!(s.height, 24);
        }

        #[test]
        fn is_empty_when_any_dimension_is_zero() {
            assert!(Size::new(0, 10).is_empty());
            assert!(Size::new(10, 0).is_empty());
            assert!(!Size::new(10, 10).is_empty());
        }

        #[test]
        fn display_shows_dimensions() {
            assert_eq!(Size::new(80, 24).to_string(), "80×24");
        }
    }

    mod rect {
        use super::*;

        #[test]
        fn new_sets_fields() {
            let r = Rect::new(1, 2, 80, 24);
            assert_eq!((r.x, r.y, r.width, r.height), (1, 2, 80, 24));
        }

        #[test]
        fn zero_is_empty_and_at_origin() {
            assert_eq!(Rect::ZERO, Rect::new(0, 0, 0, 0));
            assert!(Rect::ZERO.is_empty());
            assert_eq!(Rect::ZERO.area(), 0);
        }

        #[test]
        fn from_size_anchors_at_origin() {
            assert_eq!(Rect::from_size(Size::new(80, 24)), Rect::new(0, 0, 80, 24));
        }

        #[test]
        fn size_roundtrip() {
            let r = Rect::new(5, 6, 80, 24);
            assert_eq!(r.size(), Size::new(80, 24));
        }

        #[test]
        fn area_is_width_times_height() {
            assert_eq!(Rect::new(0, 0, 80, 24).area(), 1_920);
        }

        #[test]
        fn area_does_not_overflow_u32() {
            assert_eq!(Rect::new(0, 0, u16::MAX, u16::MAX).area(), 4_294_836_225);
        }

        #[test]
        fn edges_are_exclusive() {
            let r = Rect::new(2, 3, 80, 24);
            assert_eq!(r.right(), 82);
            assert_eq!(r.bottom(), 27);
        }

        #[test]
        fn edges_saturate_at_u16_max() {
            let r = Rect::new(65_530, 65_530, 100, 100);
            assert_eq!(r.right(), u16::MAX);
            assert_eq!(r.bottom(), u16::MAX);
        }

        #[test]
        fn contains_is_left_top_inclusive_right_bottom_exclusive() {
            let r = Rect::new(10, 10, 10, 10);
            assert!(r.contains(Position::new(10, 10)));
            assert!(r.contains(Position::new(19, 19)));
            assert!(!r.contains(Position::new(20, 19)));
            assert!(!r.contains(Position::new(19, 20)));
            assert!(!r.contains(Position::new(9, 10)));
            assert!(!r.contains(Position::new(10, 9)));
        }

        #[test]
        fn is_empty_when_any_dimension_zero() {
            assert!(Rect::new(0, 0, 0, 5).is_empty());
            assert!(Rect::new(0, 0, 5, 0).is_empty());
        }

        #[test]
        fn intersection_of_overlapping_rects() {
            let a = Rect::new(0, 0, 10, 10);
            let b = Rect::new(5, 5, 10, 10);
            assert_eq!(a.intersection(b), Some(Rect::new(5, 5, 5, 5)));
        }

        #[test]
        fn intersection_of_disjoint_rects_is_none() {
            let a = Rect::new(0, 0, 5, 5);
            let b = Rect::new(10, 10, 5, 5);
            assert_eq!(a.intersection(b), None);
        }

        #[test]
        fn intersection_of_touching_rects_is_none() {
            let a = Rect::new(0, 0, 5, 5);
            let b = Rect::new(5, 0, 5, 5);
            assert_eq!(a.intersection(b), None);
        }

        #[test]
        fn intersection_with_self_is_self() {
            let r = Rect::new(1, 1, 8, 8);
            assert_eq!(r.intersection(r), Some(r));
        }

        #[test]
        fn intersection_of_nested_rects_is_inner() {
            let outer = Rect::new(0, 0, 20, 20);
            let inner = Rect::new(5, 5, 4, 4);
            assert_eq!(outer.intersection(inner), Some(inner));
        }

        #[test]
        fn intersection_handles_saturated_edges() {
            let a = Rect::new(65_500, 0, 100, 10);
            let b = Rect::new(0, 0, u16::MAX, 10);
            assert_eq!(a.intersection(b), Some(Rect::new(65_500, 0, 35, 10)));
        }

        #[test]
        fn union_spans_disjoint_rects() {
            let a = Rect::new(0, 0, 5, 5);
            let b = Rect::new(10, 10, 5, 5);
            assert_eq!(a.union(b), Rect::new(0, 0, 15, 15));
        }

        #[test]
        fn union_of_nested_is_outer() {
            let outer = Rect::new(0, 0, 20, 20);
            let inner = Rect::new(5, 5, 4, 4);
            assert_eq!(outer.union(inner), outer);
        }

        #[test]
        fn union_saturates_at_u16_max() {
            let a = Rect::new(65_500, 0, 100, 5);
            let b = Rect::new(0, 0, 10, 5);
            assert_eq!(a.union(b), Rect::new(0, 0, u16::MAX, 5));
        }

        #[test]
        fn centered_places_the_exact_size_in_the_middle() {
            let host = Rect::new(0, 0, 80, 24);
            assert_eq!(host.centered(30, 10), Rect::new(25, 7, 30, 10));
            // Offsets accumulate from the host origin.
            let offset_host = Rect::new(10, 5, 80, 24);
            assert_eq!(offset_host.centered(30, 10), Rect::new(35, 12, 30, 10));
        }

        #[test]
        fn centered_odd_remainder_leads_by_one_less() {
            // 80 - 31 = 49: leading edge gets 24, trailing 25.
            let host = Rect::new(0, 0, 80, 24);
            let popup = host.centered(31, 1);
            assert_eq!(popup.x, 24);
            assert_eq!(popup.right(), 55);
            // Same rule vertically: 24 - 5 = 19 -> y offset 9.
            assert_eq!(host.centered(31, 5).y, 9);
        }

        #[test]
        fn centered_clamps_to_the_host() {
            let host = Rect::new(0, 0, 80, 24);
            assert_eq!(host.centered(100, 30), host);
            // Partial clamps: width fits, height does not.
            assert_eq!(host.centered(20, 100), Rect::new(30, 0, 20, 24));
            // Zero requests stay zero-sized and never panic.
            assert_eq!(host.centered(0, 0), Rect::new(40, 12, 0, 0));
        }

        #[test]
        fn display_shows_geometry() {
            assert_eq!(Rect::new(0, 0, 80, 24).to_string(), "80x24 at 0, 0");
        }
    }
}
