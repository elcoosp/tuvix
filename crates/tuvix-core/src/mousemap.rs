//! The mouse map: declarative mouse bindings — the `Keymap`'s mouse
//! twin.
//!
//! M11 shipped the declarative surface for the keyboard; the mouse had
//! nothing but hand-rolled `match` ladders over raw events. [`MouseMap`]
//! is the same translation for the pointer: bind [`MousePattern`]s — a
//! target region, a gesture class, a button — to commands, and resolve
//! classified [`Gesture`]s (from the [`GestureRecognizer`](crate::gesture::GestureRecognizer))
//! against the frame's [`HitMap`](crate::hit::HitMap) hits.
//!
//! Resolution is **stateless** — a gesture is atomic, so unlike the
//! keymap's chords there is no `Pending`: the answer is
//! `Some(command)` or `None`, and it is the same answer every time.
//!
//! The rules the tests pin:
//!
//! - **later bindings shadow earlier ones** — the painter's-algorithm
//!   intuition the `HitMap` already follows, one rule for the whole
//!   mouse layer. Rebinding is reprioritizing;
//! - **region bindings match by id only**: the pattern `on("panel")`
//!   fires wherever the hit map says the pointer is on `panel`, and
//!   nowhere else; `anywhere` bindings fire on empty space too;
//! - **drags resolve against their origin** — a drag belongs to the
//!   region it started in, not wherever the pointer has wandered; the
//!   drop target resolves the landing cell itself;
//! - **the class matrix is exact**: `Click` matches single clicks,
//!   `DoubleClick` matches `clicks: 2`, `AnyClick` matches every
//!   multiplicity — there is no implicit triple-click class; apps that
//!   want it bind `AnyClick`;
//! - button patterns are exact or any; modifier keys live on the raw
//!   event, not the gesture, and stay the app's business.
//!
//! # Examples
//!
//! ```
//! use tuvix_core::event::MouseButton;
//! use tuvix_core::geometry::{Position, Rect};
//! use tuvix_core::gesture::Gesture;
//! use tuvix_core::hit::HitMap;
//! use tuvix_core::mousemap::{MouseClass, MouseMap, MousePattern};
//!
//! #[derive(Debug, Clone, Copy, PartialEq, Eq)]
//! enum Command { FocusLog, Scroll, Dismiss }
//!
//! let mut map = MouseMap::new();
//! // Catch-alls bind first; the region binding registers later and
//! // shadows them inside the log region.
//! map.bind(MousePattern::anywhere(MouseClass::Scroll), Command::Scroll);
//! map.bind(MousePattern::anywhere(MouseClass::AnyClick), Command::Dismiss);
//! map.bind(MousePattern::on("log", MouseClass::Click), Command::FocusLog);
//!
//! let mut hits: HitMap<&str> = HitMap::new();
//! hits.push("log", Rect::new(0, 0, 30, 10));
//! let hit = hits.hit_test(Position::new(3, 3));
//! let click = Gesture::Click {
//!     button: MouseButton::Left,
//!     position: Position::new(3, 3),
//!     clicks: 1,
//! };
//! // Later bindings shadow earlier ones — but only where they match.
//! assert_eq!(map.resolve(hit, &click), Some(Command::FocusLog));
//! assert_eq!(map.resolve(None, &click), Some(Command::Dismiss));
//! ```

use crate::event::MouseButton;
use crate::geometry::Position;
use crate::gesture::Gesture;
use crate::hit::Hit;

/// Where a [`MousePattern`] applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseTarget<ID> {
    /// Every position, empty space included.
    Any,
    /// Only when the hit map resolved this region id.
    Region(ID),
}

/// Which [`Gesture`] a [`MousePattern`] matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseClass {
    /// A button went down.
    Press,
    /// A single click (`clicks == 1`).
    Click,
    /// A double click (`clicks == 2`).
    DoubleClick,
    /// Any click multiplicity — the catch-all for triple-and-beyond.
    AnyClick,
    /// The first movement of a held button.
    DragStart,
    /// A held button moving.
    Drag,
    /// A dragged button released.
    DragEnd,
    /// Plain pointer motion.
    Moved,
    /// A wheel notch, any direction.
    Scroll,
}

/// Which button a [`MousePattern`] requires.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ButtonPattern {
    /// Any button.
    Any,
    /// Exactly this button.
    Exact(MouseButton),
}

/// One binding's match rule: where, what kind of gesture, which button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MousePattern<ID> {
    /// The region scope.
    pub target: MouseTarget<ID>,
    /// The gesture class.
    pub class: MouseClass,
    /// The button requirement.
    pub button: ButtonPattern,
}

impl<ID: Copy + PartialEq> MousePattern<ID> {
    /// A pattern for `class` anywhere on the screen, any button.
    pub fn anywhere(class: MouseClass) -> Self {
        Self {
            target: MouseTarget::Any,
            class,
            button: ButtonPattern::Any,
        }
    }

    /// A pattern for `class` scoped to the region `id`, any button.
    pub fn on(id: ID, class: MouseClass) -> Self {
        Self {
            target: MouseTarget::Region(id),
            class,
            button: ButtonPattern::Any,
        }
    }

    /// Requires an exact button.
    pub fn button(mut self, button: MouseButton) -> Self {
        self.button = ButtonPattern::Exact(button);
        self
    }

    /// Accepts any button (the default, spelled for intent).
    pub fn any_button(mut self) -> Self {
        self.button = ButtonPattern::Any;
        self
    }

    /// Whether the pattern accepts `hit` (by id) and `gesture` (by
    /// class, resolved at the gesture's own anchor position).
    fn matches(&self, hit: Option<Hit<ID>>, gesture: &Gesture) -> bool {
        if !self.class_matches(gesture) {
            return false;
        }
        if !self.button_matches(gesture) {
            return false;
        }
        match self.target {
            MouseTarget::Any => true,
            MouseTarget::Region(id) => hit.is_some_and(|hit| hit.id == id),
        }
    }

    /// The class matrix: one-to-one except the click family.
    fn class_matches(&self, gesture: &Gesture) -> bool {
        matches!(
            (self.class, gesture),
            (MouseClass::Press, Gesture::Press { .. })
                | (MouseClass::Click, Gesture::Click { clicks: 1, .. })
                | (MouseClass::DoubleClick, Gesture::Click { clicks: 2, .. })
                | (MouseClass::AnyClick, Gesture::Click { .. })
                | (MouseClass::DragStart, Gesture::DragStart { .. })
                | (MouseClass::Drag, Gesture::Drag { .. })
                | (MouseClass::DragEnd, Gesture::DragEnd { .. })
                | (MouseClass::Moved, Gesture::Moved { .. })
                | (MouseClass::Scroll, Gesture::Scroll { .. })
        )
    }

    fn button_matches(&self, gesture: &Gesture) -> bool {
        let gesture_button = match gesture {
            Gesture::Press { button, .. }
            | Gesture::Click { button, .. }
            | Gesture::DragStart { button, .. }
            | Gesture::Drag { button, .. }
            | Gesture::DragEnd { button, .. } => Some(*button),
            Gesture::Moved { .. } | Gesture::Scroll { .. } => None,
        };
        match self.button {
            ButtonPattern::Any => true,
            ButtonPattern::Exact(expected) => gesture_button == Some(expected),
        }
    }
}

/// The declarative mouse router: patterns bound to commands.
///
/// Resolution scans the bindings newest-first (later bindings shadow
/// earlier ones) and returns the first match's command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MouseMap<ID, C> {
    bindings: Vec<(MousePattern<ID>, C)>,
}

impl<ID, C> Default for MouseMap<ID, C> {
    /// An empty map.
    fn default() -> Self {
        Self {
            bindings: Vec::new(),
        }
    }
}

impl<ID, C> MouseMap<ID, C> {
    /// Creates an empty map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Binds `pattern` to `command`. A later binding of an overlapping
    /// pattern shadows the earlier one wherever both match.
    pub fn bind(&mut self, pattern: MousePattern<ID>, command: C) {
        self.bindings.push((pattern, command));
    }

    /// How many bindings are registered.
    pub fn len(&self) -> usize {
        self.bindings.len()
    }

    /// Whether no binding is registered.
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }
}

impl<ID: Copy + PartialEq, C: Clone> MouseMap<ID, C> {
    /// Resolves `gesture` — already classified by the
    /// [`GestureRecognizer`](crate::gesture::GestureRecognizer) —
    /// against the frame's hit map. `hit` is what the
    /// [`HitMap`](crate::hit::HitMap) answered for the gesture's anchor
    /// position (the pointer for clicks and motion, the drag origin for
    /// drags — see [`anchor`]); `None` is empty space.
    ///
    /// Stateful by nothing: the same call answers the same command
    /// forever.
    pub fn resolve(&self, hit: Option<Hit<ID>>, gesture: &Gesture) -> Option<C> {
        self.bindings
            .iter()
            .rev()
            .find(|(pattern, _)| pattern.matches(hit, gesture))
            .map(|(_, command)| command.clone())
    }
}

/// The gesture's anchor position: where a drag belongs (its origin) and
/// where everything else happened (the pointer). Exposed for callers
/// that resolve the hit map themselves — [`MouseMap::resolve`] takes the
/// resolved hit, so apps call [`HitMap::hit_test`](crate::hit::HitMap::hit_test) with this.
pub fn anchor(gesture: &Gesture) -> Position {
    match *gesture {
        Gesture::Press { position, .. }
        | Gesture::Click { position, .. }
        | Gesture::Moved { position }
        | Gesture::Scroll { position, .. }
        | Gesture::DragStart {
            origin: position, ..
        }
        | Gesture::Drag {
            origin: position, ..
        }
        | Gesture::DragEnd {
            origin: position, ..
        } => position,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Rect;

    #[test]
    fn anchor_is_the_pointer_for_clicks_and_the_origin_for_drags() {
        let click = Gesture::Click {
            button: MouseButton::Left,
            position: Position::new(5, 6),
            clicks: 1,
        };
        assert_eq!(anchor(&click), Position::new(5, 6));
        let drag = Gesture::Drag {
            button: MouseButton::Left,
            origin: Position::new(1, 2),
            position: Position::new(30, 40),
        };
        assert_eq!(anchor(&drag), Position::new(1, 2));
    }

    #[test]
    fn empty_map_resolves_nothing() {
        let map: MouseMap<u8, u8> = MouseMap::new();
        assert!(map.is_empty());
        let press = Gesture::Press {
            button: MouseButton::Left,
            position: Position::ZERO,
        };
        assert_eq!(
            map.resolve(
                Some(Hit {
                    id: 1,
                    area: Rect::new(0, 0, 1, 1),
                    local: Position::ZERO
                }),
                &press
            ),
            None
        );
    }
}
