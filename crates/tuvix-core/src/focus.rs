//! The focus ring: which widget receives the editing keys.
//!
//! tuvix widget states stay key-agnostic — the app translates events into
//! state methods (the M6 `Input` contract). What apps still hand-roll is
//! the *routing*: "which of my three fields is focused, and where does Tab
//! go next?" [`FocusRing`] is that routing as a type: an ordered ring of
//! focus targets with wrap-around cycling, lookup by position or by value,
//! and an explicit unfocused state.
//!
//! The ring is generic: focus targets are whatever the app names its
//! fields with — an enum of widgets, `usize` indices, `&'static str` tags.
//!
//! # Examples
//!
//! ```
//! use tuvix_core::focus::FocusRing;
//!
//! #[derive(Debug, Clone, Copy, PartialEq, Eq)]
//! enum Field {
//!     Name,
//!     Email,
//!     Bio,
//! }
//!
//! let mut ring = FocusRing::with_items([Field::Name, Field::Email, Field::Bio]);
//! assert!(ring.is_focused(&Field::Name));
//! ring.advance();
//! assert!(ring.is_focused(&Field::Email));
//! ring.advance();
//! ring.advance(); // wraps
//! assert!(ring.is_focused(&Field::Name));
//! assert!(ring.set(&Field::Bio));
//! assert_eq!(ring.current(), Some(&Field::Bio));
//! ```

/// An ordered ring of focus targets with wrap-around cycling.
///
/// The ring starts focused on its first item (or unfocused when empty);
/// [`FocusRing::unfocus`] clears the focus explicitly, and the next cycle
/// restarts from the beginning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusRing<T> {
    items: Vec<T>,
    index: Option<usize>,
}

impl<T> Default for FocusRing<T> {
    /// An empty, unfocused ring — `T` needs no default of its own.
    fn default() -> Self {
        Self::new()
    }
}

impl<T> FocusRing<T> {
    /// Creates an empty, unfocused ring.
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            index: None,
        }
    }

    /// Creates a ring over `items`, focused on the first one.
    pub fn with_items(items: impl IntoIterator<Item = T>) -> Self {
        let items: Vec<T> = items.into_iter().collect();
        let index = if items.is_empty() { None } else { Some(0) };
        Self { items, index }
    }

    /// Appends a target; an empty ring focuses it.
    pub fn push(&mut self, item: T) {
        let was_empty = self.items.is_empty();
        self.items.push(item);
        if was_empty {
            self.index = Some(0);
        }
    }

    /// The number of focus targets.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether the ring holds no targets.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// All targets in cycle order.
    pub fn items(&self) -> &[T] {
        &self.items
    }

    /// The currently focused target, if any.
    pub fn current(&self) -> Option<&T> {
        self.items.get(self.index?)
    }

    /// The focused position, if any.
    pub fn index(&self) -> Option<usize> {
        self.index
    }

    /// Whether `item` is the focused target.
    ///
    /// Note the empty-ring edge: `is_focused` on any item is `false`.
    pub fn is_focused(&self, item: &T) -> bool
    where
        T: PartialEq,
    {
        self.current().is_some_and(|current| current == item)
    }

    /// Focuses the target at `index`; out-of-range indices are rejected
    /// and leave the focus unchanged (`false`).
    pub fn set_index(&mut self, index: usize) -> bool {
        if index >= self.items.len() {
            return false;
        }
        self.index = Some(index);
        true
    }

    /// Focuses the first target equal to `item` (`false` when absent).
    pub fn set(&mut self, item: &T) -> bool
    where
        T: PartialEq,
    {
        let found = self.items.iter().position(|candidate| candidate == item);
        match found {
            Some(index) => {
                self.index = Some(index);
                true
            }
            None => false,
        }
    }

    /// Advances to the next target, wrapping around; an empty or
    /// unfocused ring restarts at the beginning. Returns the new target.
    pub fn advance(&mut self) -> Option<&T> {
        if self.items.is_empty() {
            return None;
        }
        self.index = Some(match self.index {
            Some(index) => (index + 1) % self.items.len(),
            None => 0,
        });
        self.items.get(self.index?)
    }

    /// Steps back to the previous target, wrapping around; an empty or
    /// unfocused ring restarts at the end. Returns the new target.
    pub fn retreat(&mut self) -> Option<&T> {
        if self.items.is_empty() {
            return None;
        }
        self.index = Some(match self.index {
            Some(index) => (index + self.items.len() - 1) % self.items.len(),
            None => self.items.len() - 1,
        });
        self.items.get(self.index?)
    }

    /// Clears the focus; [`FocusRing::current`] becomes `None` until the
    /// next cycle or [`FocusRing::set`] call.
    pub fn unfocus(&mut self) {
        self.index = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Field {
        Name,
        Email,
        Bio,
    }

    use Field::{Bio, Email, Name};

    #[test]
    fn with_items_focuses_the_first_item() {
        let ring = FocusRing::with_items([Name, Email, Bio]);
        assert!(ring.is_focused(&Name));
        assert_eq!(ring.index(), Some(0));
        assert_eq!(ring.len(), 3);
    }

    #[test]
    fn next_wraps_around_the_ring() {
        let mut ring = FocusRing::with_items([Name, Email, Bio]);
        assert_eq!(ring.advance(), Some(&Email));
        assert_eq!(ring.advance(), Some(&Bio));
        assert_eq!(ring.advance(), Some(&Name));
    }

    #[test]
    fn previous_goes_back_with_wrap() {
        let mut ring = FocusRing::with_items([Name, Email, Bio]);
        assert_eq!(ring.retreat(), Some(&Bio));
        assert_eq!(ring.retreat(), Some(&Email));
        assert_eq!(ring.retreat(), Some(&Name));
    }

    #[test]
    fn single_item_ring_is_stationary() {
        let mut ring = FocusRing::with_items([Name]);
        assert_eq!(ring.advance(), Some(&Name));
        assert_eq!(ring.retreat(), Some(&Name));
        assert!(ring.is_focused(&Name));
    }

    #[test]
    fn empty_ring_returns_none_everywhere() {
        let mut ring: FocusRing<Field> = FocusRing::new();
        assert!(ring.is_empty());
        assert_eq!(ring.current(), None);
        assert_eq!(ring.advance(), None);
        assert_eq!(ring.retreat(), None);
        assert!(!ring.is_focused(&Name));
        assert!(!ring.set(&Name));
        assert!(!ring.set_index(0));
    }

    #[test]
    fn is_focused_matches_only_the_current_item() {
        let ring = FocusRing::with_items([Name, Email, Bio]);
        assert!(ring.is_focused(&Name));
        assert!(!ring.is_focused(&Email));
    }

    #[test]
    fn set_focuses_by_value() {
        let mut ring = FocusRing::with_items([Name, Email, Bio]);
        assert!(ring.set(&Bio));
        assert!(ring.is_focused(&Bio));
        assert_eq!(ring.index(), Some(2));
        assert!(ring.set(&Name));
        assert!(ring.is_focused(&Name));
        assert_eq!(ring.index(), Some(0));
    }

    #[test]
    fn set_of_an_absent_item_is_rejected() {
        let mut ring = FocusRing::with_items([Name, Email]);
        assert!(!ring.set(&Bio));
        assert!(ring.is_focused(&Name));
    }

    #[test]
    fn set_index_out_of_range_is_rejected() {
        let mut ring = FocusRing::with_items([Name, Email]);
        assert!(!ring.set_index(2));
        assert!(ring.is_focused(&Name));
        assert!(ring.set_index(1));
        assert!(ring.is_focused(&Email));
    }

    #[test]
    fn push_focuses_into_an_empty_ring() {
        let mut ring: FocusRing<Field> = FocusRing::new();
        ring.push(Name);
        assert!(ring.is_focused(&Name));
        ring.push(Email);
        assert!(ring.is_focused(&Name));
        assert_eq!(ring.advance(), Some(&Email));
    }

    #[test]
    fn unfocus_clears_and_cycles_restart_from_the_beginning() {
        let mut ring = FocusRing::with_items([Name, Email, Bio]);
        assert!(ring.set(&Bio));
        ring.unfocus();
        assert_eq!(ring.current(), None);
        assert!(!ring.is_focused(&Bio));
        assert_eq!(ring.advance(), Some(&Name));
        ring.unfocus();
        assert_eq!(ring.retreat(), Some(&Bio));
    }

    #[test]
    fn default_ring_is_empty_and_unfocused() {
        let ring: FocusRing<Field> = FocusRing::default();
        assert!(ring.is_empty());
        assert_eq!(ring.current(), None);
    }
}
