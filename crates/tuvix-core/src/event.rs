//! Backend-agnostic input event model.
//!
//! tuvix defines its own event vocabulary so applications never depend on a
//! specific terminal library; backends translate their native events with a
//! lossless mapping (see `tuvix-backend-crossterm`).

use bitflags::bitflags;

bitflags! {
    /// Modifier keys held during a key or mouse event.
    #[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct KeyModifiers: u16 {
        /// Shift.
        const SHIFT = 1 << 0;
        /// Control.
        const CONTROL = 1 << 1;
        /// Alt (Option).
        const ALT = 1 << 2;
        /// Super (Command / Windows key).
        const SUPER = 1 << 3;
        /// Hyper.
        const HYPER = 1 << 4;
        /// Meta.
        const META = 1 << 5;
    }
}

bitflags! {
    /// Extra state attached to a [`KeyEvent`].
    #[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct KeyEventState: u16 {
        /// The key came from the keypad.
        const KEYPAD = 1 << 0;
        /// Caps lock was active.
        const CAPS_LOCK = 1 << 1;
        /// Num lock was active.
        const NUM_LOCK = 1 << 2;
    }
}

/// A physical or logical key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum KeyCode {
    /// Placeholder default, never matches a real key report.
    #[default]
    Null,
    /// Backspace.
    Backspace,
    /// Enter.
    Enter,
    /// Left arrow.
    Left,
    /// Right arrow.
    Right,
    /// Up arrow.
    Up,
    /// Down arrow.
    Down,
    /// Home.
    Home,
    /// End.
    End,
    /// Page up.
    PageUp,
    /// Page down.
    PageDown,
    /// Tab.
    Tab,
    /// Shift + Tab.
    BackTab,
    /// Delete.
    Delete,
    /// Insert.
    Insert,
    /// Function key F1-F12.
    F(u8),
    /// A character key.
    Char(char),
    /// Escape.
    Esc,
    /// Caps lock.
    CapsLock,
    /// Scroll lock.
    ScrollLock,
    /// Num lock.
    NumLock,
    /// Print screen.
    PrintScreen,
    /// Pause.
    Pause,
    /// Menu.
    Menu,
    /// Begin on the keypad.
    KeypadBegin,
    /// A media key.
    Media(MediaKeyCode),
    /// A modifier key itself.
    Modifier(ModifierKeyCode),
}

/// Media control keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MediaKeyCode {
    /// Play.
    Play,
    /// Pause.
    Pause,
    /// Toggle play/pause.
    PlayPause,
    /// Reverse.
    Reverse,
    /// Stop.
    Stop,
    /// Fast forward.
    FastForward,
    /// Rewind.
    Rewind,
    /// Next track.
    TrackNext,
    /// Previous track.
    TrackPrevious,
    /// Record.
    Record,
    /// Lower volume.
    LowerVolume,
    /// Raise volume.
    RaiseVolume,
    /// Mute.
    Mute,
}

/// The modifier keys themselves, when reported as keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ModifierKeyCode {
    /// Left shift.
    LeftShift,
    /// Left control.
    LeftControl,
    /// Left alt.
    LeftAlt,
    /// Left super.
    LeftSuper,
    /// Left hyper.
    LeftHyper,
    /// Left meta.
    LeftMeta,
    /// Right shift.
    RightShift,
    /// Right control.
    RightControl,
    /// Right alt.
    RightAlt,
    /// Right super.
    RightSuper,
    /// Right hyper.
    RightHyper,
    /// Right meta.
    RightMeta,
    /// ISO level 3 shift.
    IsoLevel3Shift,
    /// ISO level 5 shift.
    IsoLevel5Shift,
}

/// Whether a key was pressed, released, or is auto-repeating.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyEventKind {
    /// The key went down.
    #[default]
    Press,
    /// The key is auto-repeating.
    Repeat,
    /// The key went up (only reported when the terminal supports it).
    Release,
}

/// A keyboard event.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyEvent {
    /// The key itself.
    pub code: KeyCode,
    /// Modifier keys held during the event.
    pub modifiers: KeyModifiers,
    /// Press, repeat or release.
    pub kind: KeyEventKind,
    /// Extra state (keypad, caps lock, num lock).
    pub state: KeyEventState,
}

impl KeyEvent {
    /// Creates a key-press event with no extra state.
    #[must_use]
    pub fn new(code: KeyCode, modifiers: KeyModifiers) -> Self {
        Self {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::empty(),
        }
    }

    /// Returns `true` when the modifiers of `self` exactly match `modifiers`.
    #[must_use]
    pub fn modifiers_eq(&self, modifiers: KeyModifiers) -> bool {
        self.modifiers == modifiers
    }
}

/// A mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    /// Left button.
    Left,
    /// Right button.
    Right,
    /// Middle button.
    Middle,
}

/// What happened with the mouse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MouseEventKind {
    /// Button went down.
    Down(MouseButton),
    /// Button went up.
    Up(MouseButton),
    /// Button held while moving.
    Drag(MouseButton),
    /// Movement without buttons.
    Moved,
    /// Wheel scrolled down.
    ScrollDown,
    /// Wheel scrolled up.
    ScrollUp,
    /// Wheel scrolled left.
    ScrollLeft,
    /// Wheel scrolled right.
    ScrollRight,
}

/// A mouse event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MouseEvent {
    /// What happened.
    pub kind: MouseEventKind,
    /// Column of the cursor, zero-based.
    pub column: u16,
    /// Row of the cursor, zero-based.
    pub row: u16,
    /// Modifier keys held during the event.
    pub modifiers: KeyModifiers,
}

/// Anything the terminal can report to the application.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Event {
    /// The terminal gained focus.
    FocusGained,
    /// The terminal lost focus.
    FocusLost,
    /// A keyboard event.
    Key(KeyEvent),
    /// A mouse event.
    Mouse(MouseEvent),
    /// A bracketed paste of arbitrary text.
    Paste(String),
    /// The terminal was resized to `(width, height)` cells.
    Resize(u16, u16),
}

#[cfg(test)]
mod tests {
    use super::*;

    mod key_modifiers {
        use super::*;

        #[test]
        fn empty_by_default() {
            assert_eq!(KeyModifiers::default(), KeyModifiers::empty());
        }

        #[test]
        fn combinable() {
            let m = KeyModifiers::CONTROL | KeyModifiers::ALT;
            assert!(m.contains(KeyModifiers::CONTROL));
            assert!(m.contains(KeyModifiers::ALT));
            assert!(!m.contains(KeyModifiers::SHIFT));
        }
    }

    mod key_event {
        use super::*;

        #[test]
        fn new_is_press_with_no_state() {
            let event = KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL);
            assert_eq!(event.code, KeyCode::Enter);
            assert_eq!(event.kind, KeyEventKind::Press);
            assert_eq!(event.state, KeyEventState::empty());
        }

        #[test]
        fn modifiers_eq_checks_exact_match() {
            let event = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
            assert!(event.modifiers_eq(KeyModifiers::CONTROL));
            assert!(!event.modifiers_eq(KeyModifiers::empty()));
        }

        #[test]
        fn equality_covers_all_fields() {
            let a = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::SHIFT);
            let mut b = a;
            b.kind = KeyEventKind::Release;
            assert_ne!(a, b);
        }
    }

    mod key_code {
        use super::*;

        #[test]
        fn char_and_function_keys_carry_payload() {
            assert_eq!(KeyCode::Char('q'), KeyCode::Char('q'));
            assert_ne!(KeyCode::F(1), KeyCode::F(2));
        }

        #[test]
        fn media_and_modifier_variants_exist() {
            assert_eq!(
                KeyCode::Media(MediaKeyCode::Play),
                KeyCode::Media(MediaKeyCode::Play)
            );
            assert_eq!(
                KeyCode::Modifier(ModifierKeyCode::LeftShift),
                KeyCode::Modifier(ModifierKeyCode::LeftShift)
            );
        }
    }

    mod mouse_event {
        use super::*;

        #[test]
        fn carries_position_and_kind() {
            let event = MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 12,
                row: 34,
                modifiers: KeyModifiers::SHIFT,
            };
            assert_eq!(event.column, 12);
            assert_eq!(event.row, 34);
            assert_eq!(event.kind, MouseEventKind::Down(MouseButton::Left));
        }
    }

    mod event {
        use super::*;

        #[test]
        fn paste_carries_text() {
            let event = Event::Paste("pasted".to_string());
            assert_eq!(event, Event::Paste("pasted".to_string()));
        }

        #[test]
        fn resize_carries_dimensions() {
            let event = Event::Resize(80, 24);
            assert_eq!(event, Event::Resize(80, 24));
        }

        #[test]
        fn focus_events_are_distinct() {
            assert_ne!(Event::FocusGained, Event::FocusLost);
        }
    }
}
