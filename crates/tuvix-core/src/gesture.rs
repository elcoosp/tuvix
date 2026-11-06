//! The gesture recognizer: raw mouse events classified on an injected
//! clock.
//!
//! The event vocabulary (M0) reports what the terminal sent — `Down`,
//! `Up`, `Drag`, `Moved`, the four scroll kinds. Applications think in
//! semantics: *a click* (which multiplicity?), *a drag* (from where?),
//! *a scroll*. [`GestureRecognizer`] is that classification as a pure
//! state machine: feed it every mouse event plus the time it arrived,
//! and it hands back the [`Gesture`] the event completed, if any.
//!
//! The clock is injected, never read (the M13 discipline — the clock
//! owns the value): an app feeds `Instant::now()` deltas from its own
//! loop, tests feed scripted durations, and the classification is a
//! pure function of the stream.
//!
//! The rules the tests pin:
//!
//! - **clicks count on release, release-to-release**: a release matching
//!   the active press at the same cell as the previous release, inside
//!   the interval, increments its multiplicity (`clicks: 2` = double,
//!   `3` = triple, no cap); anything else — a different cell, a
//!   different button, a late release — restarts at one;
//! - **drags belong to their origin**: the first movement of a held
//!   button to a *different* cell emits [`DragStart`](Gesture::DragStart)
//!   carrying the press origin, every further movement emits
//!   [`Drag`](Gesture::Drag), and the matching release emits
//!   [`DragEnd`](Gesture::DragEnd) — never a click. Same-cell wobble
//!   while the button is held emits nothing;
//! - **a click after a drag is a fresh single click**: the multiplicity
//!   counter is cleared by the drag;
//! - **unmatched events are ignored**: a release or drag with no active
//!   press (the terminal started reporting mid-press) does nothing;
//! - **scroll and move pass through** without touching press state —
//!   except that plain motion while a button is held follows the drag
//!   rules (some terminals report `Moved` with buttons held);
//! - a second `Down` replaces the active press (terminals report one
//!   button at a time; a fresh press is the truth on the table) and
//!   abandons any drag;
//! - [`reset`](GestureRecognizer::reset) clears press *and* click
//!   state, so the next click is a fresh single.
//!
//! # Examples
//!
//! ```
//! use std::time::Duration;
//!
//! use tuvix_core::event::{MouseButton, MouseEvent, MouseEventKind};
//! use tuvix_core::gesture::{Gesture, GestureConfig, GestureRecognizer};
//!
//! let mut gestures = GestureRecognizer::new(GestureConfig::default());
//! let at = Duration::from_millis;
//! let event = |kind, x: u16, y: u16| MouseEvent {
//!     kind,
//!     column: x,
//!     row: y,
//!     modifiers: tuvix_core::event::KeyModifiers::empty(),
//! };
//!
//! gestures.feed(event(MouseEventKind::Down(MouseButton::Left), 4, 4), at(0));
//! let click = gestures.feed(event(MouseEventKind::Up(MouseButton::Left), 4, 4), at(40));
//! assert!(matches!(click, Some(Gesture::Click { clicks: 1, .. })));
//!
//! // Released again in the same cell inside 500 ms: a double click.
//! gestures.feed(event(MouseEventKind::Down(MouseButton::Left), 4, 4), at(90));
//! let double = gestures.feed(event(MouseEventKind::Up(MouseButton::Left), 4, 4), at(130));
//! assert!(matches!(double, Some(Gesture::Click { clicks: 2, .. })));
//! ```

use std::time::Duration;

use crate::event::{MouseButton, MouseEvent, MouseEventKind};
use crate::geometry::Position;

/// Which wheel direction a [`Gesture::Scroll`] reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScrollKind {
    /// Wheel up.
    Up,
    /// Wheel down.
    Down,
    /// Wheel left.
    Left,
    /// Wheel right.
    Right,
}

/// The semantic meaning of one raw mouse event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Gesture {
    /// A button went down (emitted immediately, so apps can highlight on
    /// press).
    Press {
        /// The button that went down.
        button: MouseButton,
        /// Where it went down.
        position: Position,
    },
    /// A press released without having moved: a click. `clicks` is the
    /// multiplicity within the double-click window (1 = single).
    Click {
        /// The button that was clicked.
        button: MouseButton,
        /// Where the release landed.
        position: Position,
        /// 1 for a single click, 2 for a double, 3 for a triple —
        /// counting continues for held-down mashers.
        clicks: u16,
    },
    /// A held button moved to a different cell for the first time; the
    /// drag is anchored at the press origin.
    DragStart {
        /// The button being dragged.
        button: MouseButton,
        /// Where the press started.
        origin: Position,
    },
    /// A held button moved again. The origin is the press origin, not
    /// the last position.
    Drag {
        /// The button being dragged.
        button: MouseButton,
        /// Where the press started.
        origin: Position,
        /// Where the pointer is now.
        position: Position,
    },
    /// The dragged button was released. Not a click — a drag consumed
    /// the press.
    DragEnd {
        /// The button that was dragged.
        button: MouseButton,
        /// Where the press started.
        origin: Position,
        /// Where the release landed.
        position: Position,
    },
    /// Plain pointer motion with no button held.
    Moved {
        /// Where the pointer is now.
        position: Position,
    },
    /// One wheel notch, in any of the four directions.
    Scroll {
        /// The wheel direction.
        kind: ScrollKind,
        /// Where the pointer was when the wheel turned.
        position: Position,
    },
}

/// The tunable timing of a [`GestureRecognizer`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GestureConfig {
    /// The release-to-release window in which two clicks of the same
    /// button at the same cell count as a multi-click.
    pub double_click_interval: Duration,
}

impl Default for GestureConfig {
    /// 500 ms — the conventional double-click window, cell-granular
    /// terminal pointers included.
    fn default() -> Self {
        Self {
            double_click_interval: Duration::from_millis(500),
        }
    }
}

impl GestureConfig {
    /// A config with a custom double-click window.
    pub fn new(double_click_interval: Duration) -> Self {
        Self {
            double_click_interval,
        }
    }
}

/// The click/drag state machine over a stream of mouse events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GestureRecognizer {
    config: GestureConfig,
    pressed: Option<(MouseButton, Position)>,
    dragging: bool,
    last_click: Option<(MouseButton, Position, u16, Duration)>,
}

impl Default for GestureRecognizer {
    /// A recognizer with the default config.
    fn default() -> Self {
        Self::new(GestureConfig::default())
    }
}

impl GestureRecognizer {
    /// A recognizer with `config`.
    pub fn new(config: GestureConfig) -> Self {
        Self {
            config,
            pressed: None,
            dragging: false,
            last_click: None,
        }
    }

    /// Feeds one mouse event that arrived at time `at` (monotonic, in
    /// the app's own clock domain) and answers the gesture it completed,
    /// if any. Most events complete nothing — a lone `Down` completes
    /// [`Gesture::Press`], everything else waits for its release.
    pub fn feed(&mut self, event: MouseEvent, at: Duration) -> Option<Gesture> {
        let position = Position::new(event.column, event.row);
        match event.kind {
            MouseEventKind::Down(button) => {
                // A fresh press is the truth on the table: it replaces
                // (and abandons) any active press or drag.
                self.pressed = Some((button, position));
                self.dragging = false;
                Some(Gesture::Press { button, position })
            }
            MouseEventKind::Up(button) => {
                let Some((press_button, origin)) = self.pressed else {
                    return None; // mid-stream release: nothing to complete
                };
                if press_button != button {
                    return None; // not the pressed button
                }
                self.pressed = None;
                if self.dragging {
                    self.dragging = false;
                    return Some(Gesture::DragEnd {
                        button,
                        origin,
                        position,
                    });
                }
                // A click: multiplicity from the previous release.
                let clicks = match self.last_click {
                    Some((last_button, last_at_cell, count, released_at))
                        if last_button == button
                            && last_at_cell == position
                            && at.saturating_sub(released_at)
                                <= self.config.double_click_interval =>
                    {
                        count.saturating_add(1)
                    }
                    _ => 1,
                };
                self.last_click = Some((button, position, clicks, at));
                Some(Gesture::Click {
                    button,
                    position,
                    clicks,
                })
            }
            MouseEventKind::Drag(button) => self.movement(button, position),
            MouseEventKind::Moved => match self.pressed {
                // Some terminals report plain motion while a button is
                // held: it follows the same drag rules.
                Some((button, _)) => self.movement(button, position),
                None => Some(Gesture::Moved { position }),
            },
            MouseEventKind::ScrollDown => Some(Gesture::Scroll {
                kind: ScrollKind::Down,
                position,
            }),
            MouseEventKind::ScrollUp => Some(Gesture::Scroll {
                kind: ScrollKind::Up,
                position,
            }),
            MouseEventKind::ScrollLeft => Some(Gesture::Scroll {
                kind: ScrollKind::Left,
                position,
            }),
            MouseEventKind::ScrollRight => Some(Gesture::Scroll {
                kind: ScrollKind::Right,
                position,
            }),
        }
    }

    /// Drag-state transition for a movement of `button` to `position`.
    /// A movement to the press origin's cell is wobble, not a drag.
    fn movement(&mut self, button: MouseButton, position: Position) -> Option<Gesture> {
        let (press_button, origin) = self.pressed?;
        if press_button != button {
            return None; // not the pressed button
        }
        if position == origin {
            return None; // same-cell wobble
        }
        if self.dragging {
            return Some(Gesture::Drag {
                button,
                origin,
                position,
            });
        }
        self.dragging = true;
        self.last_click = None; // a drag consumes the click sequence
        Some(Gesture::DragStart { button, origin })
    }

    /// Whether a button is currently held.
    pub fn is_pressed(&self) -> bool {
        self.pressed.is_some()
    }

    /// Whether the active press has become a drag.
    pub fn is_dragging(&self) -> bool {
        self.dragging
    }

    /// Forgets everything: the active press, any drag and the click
    /// multiplicity. The next click is a fresh single.
    pub fn reset(&mut self) {
        self.pressed = None;
        self.dragging = false;
        self.last_click = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::KeyModifiers;

    fn event(kind: MouseEventKind, x: u16, y: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column: x,
            row: y,
            modifiers: KeyModifiers::empty(),
        }
    }

    #[test]
    fn moved_without_press_reports_moved() {
        let mut gestures = GestureRecognizer::default();
        assert_eq!(
            gestures.feed(event(MouseEventKind::Moved, 3, 3), Duration::ZERO),
            Some(Gesture::Moved {
                position: Position::new(3, 3)
            })
        );
    }

    #[test]
    fn the_default_interval_is_five_hundred_milliseconds() {
        assert_eq!(
            GestureConfig::default().double_click_interval,
            Duration::from_millis(500)
        );
    }
}
