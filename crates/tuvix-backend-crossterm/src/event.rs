//! Lossless translation between crossterm and tuvix event/color vocabularies.
//!
//! Applications write against [`tuvix_core::event`] only; this module is the
//! adapter layer that keeps crossterm out of user code.

use crossterm::event::{
    Event as CrosstermEvent, KeyCode as CrosstermKeyCode, KeyEvent as CrosstermKeyEvent,
    KeyEventKind as CrosstermKeyEventKind, KeyEventState as CrosstermKeyEventState,
    KeyModifiers as CrosstermKeyModifiers, MediaKeyCode as CrosstermMediaKeyCode,
    ModifierKeyCode as CrosstermModifierKeyCode, MouseButton as CrosstermMouseButton,
    MouseEvent as CrosstermMouseEvent, MouseEventKind as CrosstermMouseEventKind,
};
use crossterm::style::Color as CrosstermColor;
use tuvix_core::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MediaKeyCode,
    ModifierKeyCode, MouseButton, MouseEvent, MouseEventKind,
};
use tuvix_core::style::Color;

/// Converts a crossterm event into a tuvix event.
#[must_use]
pub fn to_tuvix_event(event: CrosstermEvent) -> Event {
    match event {
        CrosstermEvent::FocusGained => Event::FocusGained,
        CrosstermEvent::FocusLost => Event::FocusLost,
        CrosstermEvent::Key(key) => Event::Key(to_tuvix_key_event(key)),
        CrosstermEvent::Mouse(mouse) => Event::Mouse(to_tuvix_mouse_event(mouse)),
        CrosstermEvent::Paste(text) => Event::Paste(text),
        CrosstermEvent::Resize(width, height) => Event::Resize(width, height),
    }
}

/// Converts a crossterm key event into a tuvix key event.
fn to_tuvix_key_event(event: CrosstermKeyEvent) -> KeyEvent {
    KeyEvent {
        code: to_tuvix_key_code(event.code),
        modifiers: to_tuvix_modifiers(event.modifiers),
        kind: match event.kind {
            CrosstermKeyEventKind::Press => KeyEventKind::Press,
            CrosstermKeyEventKind::Repeat => KeyEventKind::Repeat,
            CrosstermKeyEventKind::Release => KeyEventKind::Release,
        },
        state: to_tuvix_key_state(event.state),
    }
}

/// Converts a crossterm key code into a tuvix key code.
fn to_tuvix_key_code(code: CrosstermKeyCode) -> KeyCode {
    match code {
        CrosstermKeyCode::Backspace => KeyCode::Backspace,
        CrosstermKeyCode::Enter => KeyCode::Enter,
        CrosstermKeyCode::Left => KeyCode::Left,
        CrosstermKeyCode::Right => KeyCode::Right,
        CrosstermKeyCode::Up => KeyCode::Up,
        CrosstermKeyCode::Down => KeyCode::Down,
        CrosstermKeyCode::Home => KeyCode::Home,
        CrosstermKeyCode::End => KeyCode::End,
        CrosstermKeyCode::PageUp => KeyCode::PageUp,
        CrosstermKeyCode::PageDown => KeyCode::PageDown,
        CrosstermKeyCode::Tab => KeyCode::Tab,
        CrosstermKeyCode::BackTab => KeyCode::BackTab,
        CrosstermKeyCode::Delete => KeyCode::Delete,
        CrosstermKeyCode::Insert => KeyCode::Insert,
        CrosstermKeyCode::F(f) => KeyCode::F(f),
        CrosstermKeyCode::Char(c) => KeyCode::Char(c),
        CrosstermKeyCode::Null => KeyCode::Null,
        CrosstermKeyCode::Esc => KeyCode::Esc,
        CrosstermKeyCode::CapsLock => KeyCode::CapsLock,
        CrosstermKeyCode::ScrollLock => KeyCode::ScrollLock,
        CrosstermKeyCode::NumLock => KeyCode::NumLock,
        CrosstermKeyCode::PrintScreen => KeyCode::PrintScreen,
        CrosstermKeyCode::Pause => KeyCode::Pause,
        CrosstermKeyCode::Menu => KeyCode::Menu,
        CrosstermKeyCode::KeypadBegin => KeyCode::KeypadBegin,
        CrosstermKeyCode::Media(media) => KeyCode::Media(to_tuvix_media(media)),
        CrosstermKeyCode::Modifier(modifier) => KeyCode::Modifier(to_tuvix_modifier(modifier)),
    }
}

/// Converts a crossterm media key code into a tuvix media key code.
fn to_tuvix_media(code: CrosstermMediaKeyCode) -> MediaKeyCode {
    match code {
        CrosstermMediaKeyCode::Play => MediaKeyCode::Play,
        CrosstermMediaKeyCode::Pause => MediaKeyCode::Pause,
        CrosstermMediaKeyCode::PlayPause => MediaKeyCode::PlayPause,
        CrosstermMediaKeyCode::Reverse => MediaKeyCode::Reverse,
        CrosstermMediaKeyCode::Stop => MediaKeyCode::Stop,
        CrosstermMediaKeyCode::FastForward => MediaKeyCode::FastForward,
        CrosstermMediaKeyCode::Rewind => MediaKeyCode::Rewind,
        CrosstermMediaKeyCode::TrackNext => MediaKeyCode::TrackNext,
        CrosstermMediaKeyCode::TrackPrevious => MediaKeyCode::TrackPrevious,
        CrosstermMediaKeyCode::Record => MediaKeyCode::Record,
        CrosstermMediaKeyCode::LowerVolume => MediaKeyCode::LowerVolume,
        CrosstermMediaKeyCode::RaiseVolume => MediaKeyCode::RaiseVolume,
        CrosstermMediaKeyCode::MuteVolume => MediaKeyCode::Mute,
    }
}

/// Converts a crossterm modifier key code into a tuvix modifier key code.
fn to_tuvix_modifier(code: CrosstermModifierKeyCode) -> ModifierKeyCode {
    match code {
        CrosstermModifierKeyCode::LeftShift => ModifierKeyCode::LeftShift,
        CrosstermModifierKeyCode::LeftControl => ModifierKeyCode::LeftControl,
        CrosstermModifierKeyCode::LeftAlt => ModifierKeyCode::LeftAlt,
        CrosstermModifierKeyCode::LeftSuper => ModifierKeyCode::LeftSuper,
        CrosstermModifierKeyCode::LeftHyper => ModifierKeyCode::LeftHyper,
        CrosstermModifierKeyCode::LeftMeta => ModifierKeyCode::LeftMeta,
        CrosstermModifierKeyCode::RightShift => ModifierKeyCode::RightShift,
        CrosstermModifierKeyCode::RightControl => ModifierKeyCode::RightControl,
        CrosstermModifierKeyCode::RightAlt => ModifierKeyCode::RightAlt,
        CrosstermModifierKeyCode::RightSuper => ModifierKeyCode::RightSuper,
        CrosstermModifierKeyCode::RightHyper => ModifierKeyCode::RightHyper,
        CrosstermModifierKeyCode::RightMeta => ModifierKeyCode::RightMeta,
        CrosstermModifierKeyCode::IsoLevel3Shift => ModifierKeyCode::IsoLevel3Shift,
        CrosstermModifierKeyCode::IsoLevel5Shift => ModifierKeyCode::IsoLevel5Shift,
    }
}

/// Converts crossterm modifiers into tuvix modifiers.
fn to_tuvix_modifiers(modifiers: CrosstermKeyModifiers) -> KeyModifiers {
    let mut converted = KeyModifiers::empty();
    if modifiers.contains(CrosstermKeyModifiers::SHIFT) {
        converted |= KeyModifiers::SHIFT;
    }
    if modifiers.contains(CrosstermKeyModifiers::CONTROL) {
        converted |= KeyModifiers::CONTROL;
    }
    if modifiers.contains(CrosstermKeyModifiers::ALT) {
        converted |= KeyModifiers::ALT;
    }
    if modifiers.contains(CrosstermKeyModifiers::SUPER) {
        converted |= KeyModifiers::SUPER;
    }
    if modifiers.contains(CrosstermKeyModifiers::HYPER) {
        converted |= KeyModifiers::HYPER;
    }
    if modifiers.contains(CrosstermKeyModifiers::META) {
        converted |= KeyModifiers::META;
    }
    converted
}

/// Converts crossterm key state into tuvix key state.
fn to_tuvix_key_state(state: CrosstermKeyEventState) -> KeyEventState {
    let mut converted = KeyEventState::empty();
    if state.contains(CrosstermKeyEventState::KEYPAD) {
        converted |= KeyEventState::KEYPAD;
    }
    if state.contains(CrosstermKeyEventState::CAPS_LOCK) {
        converted |= KeyEventState::CAPS_LOCK;
    }
    if state.contains(CrosstermKeyEventState::NUM_LOCK) {
        converted |= KeyEventState::NUM_LOCK;
    }
    converted
}

/// Converts a crossterm mouse event into a tuvix mouse event.
fn to_tuvix_mouse_event(event: CrosstermMouseEvent) -> MouseEvent {
    MouseEvent {
        kind: match event.kind {
            CrosstermMouseEventKind::Down(button) => MouseEventKind::Down(to_tuvix_button(button)),
            CrosstermMouseEventKind::Up(button) => MouseEventKind::Up(to_tuvix_button(button)),
            CrosstermMouseEventKind::Drag(button) => MouseEventKind::Drag(to_tuvix_button(button)),
            CrosstermMouseEventKind::Moved => MouseEventKind::Moved,
            CrosstermMouseEventKind::ScrollDown => MouseEventKind::ScrollDown,
            CrosstermMouseEventKind::ScrollUp => MouseEventKind::ScrollUp,
            CrosstermMouseEventKind::ScrollLeft => MouseEventKind::ScrollLeft,
            CrosstermMouseEventKind::ScrollRight => MouseEventKind::ScrollRight,
        },
        column: event.column,
        row: event.row,
        modifiers: to_tuvix_modifiers(event.modifiers),
    }
}

/// Converts a crossterm mouse button into a tuvix mouse button.
fn to_tuvix_button(button: CrosstermMouseButton) -> MouseButton {
    match button {
        CrosstermMouseButton::Left => MouseButton::Left,
        CrosstermMouseButton::Right => MouseButton::Right,
        CrosstermMouseButton::Middle => MouseButton::Middle,
    }
}

/// Converts a crossterm color into a tuvix color.
///
/// The mapping follows the ANSI palette: `Red` maps to crossterm's
/// `DarkRed` (ANSI 1) and `LightRed` to crossterm's `Red` (ANSI 9).
#[must_use]
pub fn to_tuvix_color(color: CrosstermColor) -> Color {
    match color {
        CrosstermColor::Reset => Color::Reset,
        CrosstermColor::Black => Color::Black,
        CrosstermColor::DarkGrey => Color::DarkGray,
        CrosstermColor::Red => Color::LightRed,
        CrosstermColor::DarkRed => Color::Red,
        CrosstermColor::Green => Color::LightGreen,
        CrosstermColor::DarkGreen => Color::Green,
        CrosstermColor::Yellow => Color::LightYellow,
        CrosstermColor::DarkYellow => Color::Yellow,
        CrosstermColor::Blue => Color::LightBlue,
        CrosstermColor::DarkBlue => Color::Blue,
        CrosstermColor::Magenta => Color::LightMagenta,
        CrosstermColor::DarkMagenta => Color::Magenta,
        CrosstermColor::Cyan => Color::LightCyan,
        CrosstermColor::DarkCyan => Color::Cyan,
        CrosstermColor::Grey => Color::Gray,
        CrosstermColor::White => Color::White,
        CrosstermColor::Rgb { r, g, b } => Color::Rgb { r, g, b },
        CrosstermColor::AnsiValue(value) => Color::Indexed(value),
    }
}

/// Converts a tuvix color into a crossterm color.
///
/// The mapping follows the ANSI palette: `Red` maps to crossterm's
/// `DarkRed` (ANSI 1) and `LightRed` to crossterm's `Red` (ANSI 9).
#[must_use]
pub fn to_crossterm_color(color: Color) -> CrosstermColor {
    match color {
        Color::Reset => CrosstermColor::Reset,
        Color::Black => CrosstermColor::Black,
        Color::Red => CrosstermColor::DarkRed,
        Color::Green => CrosstermColor::DarkGreen,
        Color::Yellow => CrosstermColor::DarkYellow,
        Color::Blue => CrosstermColor::DarkBlue,
        Color::Magenta => CrosstermColor::DarkMagenta,
        Color::Cyan => CrosstermColor::DarkCyan,
        Color::Gray => CrosstermColor::Grey,
        Color::DarkGray => CrosstermColor::DarkGrey,
        Color::LightRed => CrosstermColor::Red,
        Color::LightGreen => CrosstermColor::Green,
        Color::LightYellow => CrosstermColor::Yellow,
        Color::LightBlue => CrosstermColor::Blue,
        Color::LightMagenta => CrosstermColor::Magenta,
        Color::LightCyan => CrosstermColor::Cyan,
        Color::White => CrosstermColor::White,
        Color::Rgb { r, g, b } => CrosstermColor::Rgb { r, g, b },
        Color::Indexed(value) => CrosstermColor::AnsiValue(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_codes_map_losslessly() {
        let cases = [
            (CrosstermKeyCode::Backspace, KeyCode::Backspace),
            (CrosstermKeyCode::Enter, KeyCode::Enter),
            (CrosstermKeyCode::Left, KeyCode::Left),
            (CrosstermKeyCode::Right, KeyCode::Right),
            (CrosstermKeyCode::Up, KeyCode::Up),
            (CrosstermKeyCode::Down, KeyCode::Down),
            (CrosstermKeyCode::Home, KeyCode::Home),
            (CrosstermKeyCode::End, KeyCode::End),
            (CrosstermKeyCode::PageUp, KeyCode::PageUp),
            (CrosstermKeyCode::PageDown, KeyCode::PageDown),
            (CrosstermKeyCode::Tab, KeyCode::Tab),
            (CrosstermKeyCode::BackTab, KeyCode::BackTab),
            (CrosstermKeyCode::Delete, KeyCode::Delete),
            (CrosstermKeyCode::Insert, KeyCode::Insert),
            (CrosstermKeyCode::F(5), KeyCode::F(5)),
            (CrosstermKeyCode::Char('q'), KeyCode::Char('q')),
            (CrosstermKeyCode::Null, KeyCode::Null),
            (CrosstermKeyCode::Esc, KeyCode::Esc),
            (CrosstermKeyCode::CapsLock, KeyCode::CapsLock),
            (CrosstermKeyCode::ScrollLock, KeyCode::ScrollLock),
            (CrosstermKeyCode::NumLock, KeyCode::NumLock),
            (CrosstermKeyCode::PrintScreen, KeyCode::PrintScreen),
            (CrosstermKeyCode::Pause, KeyCode::Pause),
            (CrosstermKeyCode::Menu, KeyCode::Menu),
            (CrosstermKeyCode::KeypadBegin, KeyCode::KeypadBegin),
            (
                CrosstermKeyCode::Media(CrosstermMediaKeyCode::Play),
                KeyCode::Media(MediaKeyCode::Play),
            ),
            (
                CrosstermKeyCode::Modifier(CrosstermModifierKeyCode::LeftShift),
                KeyCode::Modifier(ModifierKeyCode::LeftShift),
            ),
        ];
        for (from, to) in cases {
            assert_eq!(to_tuvix_key_code(from), to);
        }
    }

    #[test]
    fn key_event_maps_all_fields() {
        let key =
            CrosstermKeyEvent::new(CrosstermKeyCode::Char('c'), CrosstermKeyModifiers::CONTROL);
        let converted = to_tuvix_key_event(key);
        assert_eq!(converted.code, KeyCode::Char('c'));
        assert_eq!(converted.modifiers, KeyModifiers::CONTROL);
        assert_eq!(converted.kind, KeyEventKind::Press);
    }

    #[test]
    fn modifiers_map_losslessly() {
        let all = CrosstermKeyModifiers::SHIFT
            | CrosstermKeyModifiers::CONTROL
            | CrosstermKeyModifiers::ALT
            | CrosstermKeyModifiers::SUPER
            | CrosstermKeyModifiers::HYPER
            | CrosstermKeyModifiers::META;
        let converted = to_tuvix_modifiers(all);
        assert!(converted.contains(KeyModifiers::SHIFT));
        assert!(converted.contains(KeyModifiers::CONTROL));
        assert!(converted.contains(KeyModifiers::ALT));
        assert!(converted.contains(KeyModifiers::SUPER));
        assert!(converted.contains(KeyModifiers::HYPER));
        assert!(converted.contains(KeyModifiers::META));
    }

    #[test]
    fn mouse_event_maps_all_fields() {
        let mouse = CrosstermMouseEvent {
            kind: CrosstermMouseEventKind::Down(CrosstermMouseButton::Left),
            column: 10,
            row: 20,
            modifiers: CrosstermKeyModifiers::SHIFT,
        };
        let converted = to_tuvix_mouse_event(mouse);
        assert_eq!(converted.kind, MouseEventKind::Down(MouseButton::Left));
        assert_eq!(converted.column, 10);
        assert_eq!(converted.row, 20);
        assert_eq!(converted.modifiers, KeyModifiers::SHIFT);
    }

    #[test]
    fn event_variants_map() {
        assert_eq!(
            to_tuvix_event(CrosstermEvent::FocusGained),
            Event::FocusGained
        );
        assert_eq!(to_tuvix_event(CrosstermEvent::FocusLost), Event::FocusLost);
        assert_eq!(
            to_tuvix_event(CrosstermEvent::Resize(80, 24)),
            Event::Resize(80, 24)
        );
        assert_eq!(
            to_tuvix_event(CrosstermEvent::Paste("text".to_string())),
            Event::Paste("text".to_string())
        );
    }

    #[test]
    fn ansi_color_mapping_is_correct() {
        assert_eq!(to_crossterm_color(Color::Red), CrosstermColor::DarkRed);
        assert_eq!(to_crossterm_color(Color::LightRed), CrosstermColor::Red);
        assert_eq!(to_crossterm_color(Color::Gray), CrosstermColor::Grey);
        assert_eq!(
            to_crossterm_color(Color::DarkGray),
            CrosstermColor::DarkGrey
        );
        assert_eq!(
            to_crossterm_color(Color::Rgb { r: 9, g: 8, b: 7 }),
            CrosstermColor::Rgb { r: 9, g: 8, b: 7 }
        );
        assert_eq!(
            to_crossterm_color(Color::Indexed(201)),
            CrosstermColor::AnsiValue(201)
        );
    }

    #[test]
    fn color_mapping_round_trips() {
        for color in [
            Color::Reset,
            Color::Black,
            Color::Red,
            Color::Green,
            Color::Yellow,
            Color::Blue,
            Color::Magenta,
            Color::Cyan,
            Color::Gray,
            Color::DarkGray,
            Color::LightRed,
            Color::LightGreen,
            Color::LightYellow,
            Color::LightBlue,
            Color::LightMagenta,
            Color::LightCyan,
            Color::White,
            Color::Indexed(7),
        ] {
            assert_eq!(to_tuvix_color(to_crossterm_color(color)), color);
        }
        let rgb = Color::Rgb { r: 1, g: 2, b: 3 };
        assert_eq!(to_tuvix_color(to_crossterm_color(rgb)), rgb);
    }
}
