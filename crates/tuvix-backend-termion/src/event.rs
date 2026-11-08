//! The lossless termion → tuvix event bridge.
//!
//! Every event termion can report maps onto the tuvix event model, the
//! same contract the crossterm bridge keeps ("adapters are lossless,
//! tests pin it"). termion's model is smaller than crossterm's, so three
//! corners are documented and pinned:
//!
//! 1. `\r` and `\n` both arrive as [`Key::Char('\n')`](termion::event::Key)
//!    — both become [`KeyCode::Enter`].
//! 2. Mouse **releases carry no button** (the X10 and SGR protocols
//!    report releases button-less through termion) — mapped to
//!    [`MouseEventKind::Up`] with the left button.
//! 3. [`Unsupported`](termion::event::Event::Unsupported) byte sequences
//!    have no tuvix representation and are dropped (`None`).
//!
//! Mouse coordinates are one-based in termion and zero-based in tuvix;
//! the bridge subtracts one.

use tuvix_core::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

/// Translates one native termion event into a tuvix event, or `None` when
/// termion reported something the tuvix model cannot represent
/// (`Unsupported` byte runs, incomplete parses).
pub fn to_tuvix_event(event: &termion::event::Event) -> Option<Event> {
    match *event {
        termion::event::Event::Key(key) => to_tuvix_key(key).map(Event::Key),
        termion::event::Event::Mouse(mouse) => Some(Event::Mouse(to_tuvix_mouse(mouse))),
        termion::event::Event::Unsupported(_) => None,
    }
}

/// Translates a termion key.
fn to_tuvix_key(key: termion::event::Key) -> Option<KeyEvent> {
    use termion::event::Key as TKey;
    let key_event = match key {
        TKey::Backspace => KeyEvent::new(KeyCode::Backspace, KeyModifiers::empty()),
        TKey::Left => KeyEvent::new(KeyCode::Left, KeyModifiers::empty()),
        TKey::ShiftLeft => KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT),
        TKey::AltLeft => KeyEvent::new(KeyCode::Left, KeyModifiers::ALT),
        TKey::CtrlLeft => KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL),
        TKey::Right => KeyEvent::new(KeyCode::Right, KeyModifiers::empty()),
        TKey::ShiftRight => KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT),
        TKey::AltRight => KeyEvent::new(KeyCode::Right, KeyModifiers::ALT),
        TKey::CtrlRight => KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL),
        TKey::Up => KeyEvent::new(KeyCode::Up, KeyModifiers::empty()),
        TKey::ShiftUp => KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT),
        TKey::AltUp => KeyEvent::new(KeyCode::Up, KeyModifiers::ALT),
        TKey::CtrlUp => KeyEvent::new(KeyCode::Up, KeyModifiers::CONTROL),
        TKey::Down => KeyEvent::new(KeyCode::Down, KeyModifiers::empty()),
        TKey::ShiftDown => KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT),
        TKey::AltDown => KeyEvent::new(KeyCode::Down, KeyModifiers::ALT),
        TKey::CtrlDown => KeyEvent::new(KeyCode::Down, KeyModifiers::CONTROL),
        TKey::Home => KeyEvent::new(KeyCode::Home, KeyModifiers::empty()),
        TKey::CtrlHome => KeyEvent::new(KeyCode::Home, KeyModifiers::CONTROL),
        TKey::End => KeyEvent::new(KeyCode::End, KeyModifiers::empty()),
        TKey::CtrlEnd => KeyEvent::new(KeyCode::End, KeyModifiers::CONTROL),
        TKey::PageUp => KeyEvent::new(KeyCode::PageUp, KeyModifiers::empty()),
        TKey::PageDown => KeyEvent::new(KeyCode::PageDown, KeyModifiers::empty()),
        TKey::BackTab => KeyEvent::new(KeyCode::BackTab, KeyModifiers::empty()),
        TKey::Delete => KeyEvent::new(KeyCode::Delete, KeyModifiers::empty()),
        TKey::Insert => KeyEvent::new(KeyCode::Insert, KeyModifiers::empty()),
        TKey::F(number) => KeyEvent::new(KeyCode::F(number), KeyModifiers::empty()),
        // termion reports both \r and \n as Char('\n').
        TKey::Char('\n' | '\r') => KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
        TKey::Char('\t') => KeyEvent::new(KeyCode::Tab, KeyModifiers::empty()),
        TKey::Char(character) => KeyEvent::new(KeyCode::Char(character), KeyModifiers::empty()),
        TKey::Alt(character) => KeyEvent::new(KeyCode::Char(character), KeyModifiers::ALT),
        TKey::Ctrl(character) => KeyEvent::new(KeyCode::Char(character), KeyModifiers::CONTROL),
        TKey::Null => KeyEvent::new(KeyCode::Null, KeyModifiers::empty()),
        TKey::Esc => KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()),
        // Incomplete parses never escape the reader loop.
        TKey::__IsNotComplete => return None,
    };
    Some(key_event)
}

/// Translates a termion mouse event; coordinates go one-based → zero-based.
fn to_tuvix_mouse(mouse: termion::event::MouseEvent) -> MouseEvent {
    use termion::event::{MouseButton as TButton, MouseEvent as TMouse};
    match mouse {
        TMouse::Press(button, x, y) => {
            // Wheel buttons are press-shaped in termion but scroll events
            // in the tuvix model.
            let kind = match button {
                TButton::WheelUp => MouseEventKind::ScrollUp,
                TButton::WheelDown => MouseEventKind::ScrollDown,
                TButton::WheelLeft => MouseEventKind::ScrollLeft,
                TButton::WheelRight => MouseEventKind::ScrollRight,
                TButton::Left => MouseEventKind::Down(MouseButton::Left),
                TButton::Right => MouseEventKind::Down(MouseButton::Right),
                TButton::Middle => MouseEventKind::Down(MouseButton::Middle),
            };
            MouseEvent {
                kind,
                column: x.saturating_sub(1),
                row: y.saturating_sub(1),
                modifiers: KeyModifiers::empty(),
            }
        }
        // termion's release carries no button: the X10 byte is consumed by
        // the release marker and SGR sends only the coordinates.
        TMouse::Release(x, y) => MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: x.saturating_sub(1),
            row: y.saturating_sub(1),
            modifiers: KeyModifiers::empty(),
        },
        TMouse::Hold(x, y) => MouseEvent {
            kind: MouseEventKind::Moved,
            column: x.saturating_sub(1),
            row: y.saturating_sub(1),
            modifiers: KeyModifiers::empty(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use termion::event::Key as TKey;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> Event {
        Event::Key(KeyEvent::new(code, modifiers))
    }

    #[test]
    fn plain_characters_map_without_modifiers() {
        assert_eq!(
            to_tuvix_event(&termion::event::Event::Key(TKey::Char('q'))),
            Some(key(KeyCode::Char('q'), KeyModifiers::empty()))
        );
    }

    #[test]
    fn enter_tab_and_backspace_map() {
        // termion reports both \r and \n as Char('\n').
        assert_eq!(
            to_tuvix_event(&termion::event::Event::Key(TKey::Char('\n'))),
            Some(key(KeyCode::Enter, KeyModifiers::empty()))
        );
        assert_eq!(
            to_tuvix_event(&termion::event::Event::Key(TKey::Char('\t'))),
            Some(key(KeyCode::Tab, KeyModifiers::empty()))
        );
        assert_eq!(
            to_tuvix_event(&termion::event::Event::Key(TKey::Backspace)),
            Some(key(KeyCode::Backspace, KeyModifiers::empty()))
        );
    }

    #[test]
    fn modified_characters_carry_their_modifier() {
        assert_eq!(
            to_tuvix_event(&termion::event::Event::Key(TKey::Ctrl('c'))),
            Some(key(KeyCode::Char('c'), KeyModifiers::CONTROL))
        );
        assert_eq!(
            to_tuvix_event(&termion::event::Event::Key(TKey::Alt('x'))),
            Some(key(KeyCode::Char('x'), KeyModifiers::ALT))
        );
    }

    #[test]
    fn modified_arrows_carry_their_modifier() {
        assert_eq!(
            to_tuvix_event(&termion::event::Event::Key(TKey::ShiftLeft)),
            Some(key(KeyCode::Left, KeyModifiers::SHIFT))
        );
        assert_eq!(
            to_tuvix_event(&termion::event::Event::Key(TKey::CtrlDown)),
            Some(key(KeyCode::Down, KeyModifiers::CONTROL))
        );
        assert_eq!(
            to_tuvix_event(&termion::event::Event::Key(TKey::AltUp)),
            Some(key(KeyCode::Up, KeyModifiers::ALT))
        );
    }

    #[test]
    fn navigation_and_function_keys_map() {
        assert_eq!(
            to_tuvix_event(&termion::event::Event::Key(TKey::Home)),
            Some(key(KeyCode::Home, KeyModifiers::empty()))
        );
        assert_eq!(
            to_tuvix_event(&termion::event::Event::Key(TKey::CtrlEnd)),
            Some(key(KeyCode::End, KeyModifiers::CONTROL))
        );
        assert_eq!(
            to_tuvix_event(&termion::event::Event::Key(TKey::PageUp)),
            Some(key(KeyCode::PageUp, KeyModifiers::empty()))
        );
        assert_eq!(
            to_tuvix_event(&termion::event::Event::Key(TKey::BackTab)),
            Some(key(KeyCode::BackTab, KeyModifiers::empty()))
        );
        assert_eq!(
            to_tuvix_event(&termion::event::Event::Key(TKey::Delete)),
            Some(key(KeyCode::Delete, KeyModifiers::empty()))
        );
        assert_eq!(
            to_tuvix_event(&termion::event::Event::Key(TKey::Insert)),
            Some(key(KeyCode::Insert, KeyModifiers::empty()))
        );
        assert_eq!(
            to_tuvix_event(&termion::event::Event::Key(TKey::F(5))),
            Some(key(KeyCode::F(5), KeyModifiers::empty()))
        );
        assert_eq!(
            to_tuvix_event(&termion::event::Event::Key(TKey::Esc)),
            Some(key(KeyCode::Esc, KeyModifiers::empty()))
        );
        assert_eq!(
            to_tuvix_event(&termion::event::Event::Key(TKey::Null)),
            Some(key(KeyCode::Null, KeyModifiers::empty()))
        );
    }

    #[test]
    fn incomplete_sequences_are_dropped() {
        assert_eq!(
            to_tuvix_event(&termion::event::Event::Key(TKey::__IsNotComplete)),
            None
        );
    }

    #[test]
    fn unsupported_bytes_are_dropped() {
        assert_eq!(
            to_tuvix_event(&termion::event::Event::Unsupported(vec![b'\x1b', b'?'])),
            None
        );
    }

    #[test]
    fn mouse_press_maps_down_with_zero_based_coords() {
        assert_eq!(
            to_tuvix_event(&termion::event::Event::Mouse(
                termion::event::MouseEvent::Press(termion::event::MouseButton::Left, 1, 1)
            )),
            Some(Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 0,
                row: 0,
                modifiers: KeyModifiers::empty(),
            }))
        );
    }

    #[test]
    fn mouse_wheels_map_to_scroll_kinds() {
        assert_eq!(
            to_tuvix_event(&termion::event::Event::Mouse(
                termion::event::MouseEvent::Press(termion::event::MouseButton::WheelUp, 10, 4)
            )),
            Some(Event::Mouse(MouseEvent {
                kind: MouseEventKind::ScrollUp,
                column: 9,
                row: 3,
                modifiers: KeyModifiers::empty(),
            }))
        );
    }

    #[test]
    fn mouse_release_has_no_button_left_button_is_assumed() {
        assert_eq!(
            to_tuvix_event(&termion::event::Event::Mouse(
                termion::event::MouseEvent::Release(12, 7)
            )),
            Some(Event::Mouse(MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: 11,
                row: 6,
                modifiers: KeyModifiers::empty(),
            }))
        );
    }

    #[test]
    fn mouse_hold_maps_to_moved() {
        assert_eq!(
            to_tuvix_event(&termion::event::Event::Mouse(
                termion::event::MouseEvent::Hold(3, 3)
            )),
            Some(Event::Mouse(MouseEvent {
                kind: MouseEventKind::Moved,
                column: 2,
                row: 2,
                modifiers: KeyModifiers::empty(),
            }))
        );
    }
}
