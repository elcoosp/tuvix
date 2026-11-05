//! The declarative input-mode stack.
//!
//! M2 introduces an *input stack*: a LIFO list of terminal input modes
//! (kitty keyboard protocol push/pop, bracketed paste, mouse capture, focus
//! reporting) that translates into concrete escape sequences. The model is
//! pure — [`InputStack::push`], [`InputStack::pop`] and
//! [`InputStack::restore`] return an [`InputTransition`] carrying the exact
//! sequences to write — so backends and runners apply them, while tests
//! assert on the sequences themselves.
//!
//! # Examples
//!
//! ```
//! use tuvix_core::runtime::input::{InputMode, InputStack};
//!
//! let mut stack = InputStack::new();
//! let enable = stack.push(InputMode::BracketedPaste);
//! assert_eq!(
//!     enable.enable_sequences(),
//!     ["\x1b[?2004h"].map(String::from).as_slice()
//! );
//! ```
//!
//! Sequences were verified first-hand against crossterm 0.29's own
//! `write_ansi` implementations (kitty push `CSI > flags u` and pop
//! `CSI < 1 u`, mouse capture, bracketed paste, focus reporting).

use std::borrow::Cow;

use bitflags::bitflags;

bitflags! {
    /// Kitty keyboard protocol enhancement flags.
    ///
    /// Bit values follow the kitty keyboard protocol specification, as
    /// implemented by crossterm 0.29's `KeyboardEnhancementFlags`.
    #[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct KittyEnhancementFlags: u8 {
        /// Represent Escape and modified keys as CSI-u sequences so they can
        /// be read unambiguously.
        const DISAMBIGUATE_ESCAPE_CODES = 0b0000_0001;
        /// Report repeat and release key events.
        const REPORT_EVENT_TYPES = 0b0000_0010;
        /// Send alternate keycodes in addition to the base keycode.
        const REPORT_ALTERNATE_KEYS = 0b0000_0100;
        /// Represent all keyboard events as CSI-u sequences.
        const REPORT_ALL_KEYS_AS_ESCAPE_CODES = 0b0000_1000;
        /// Send the Unicode codepoint alongside the keycode.
        const REPORT_ASSOCIATED_TEXT = 0b0001_0000;
    }
}

/// A terminal input mode that can be pushed onto an [`InputStack`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InputMode {
    /// Pushes the given enhancement flags onto the kitty keyboard protocol
    /// stack (`CSI > flags u`; popped with `CSI < 1 u`).
    KittyKeyboard(KittyEnhancementFlags),
    /// Enables bracketed paste (paste text arrives wrapped in markers and is
    /// delivered as a single [`Event::Paste`](crate::event::Event::Paste)).
    BracketedPaste,
    /// Enables mouse capture (normal + button-event + any-event tracking,
    /// SGR extended coordinates).
    MouseCapture,
    /// Enables focus gained/lost reporting.
    FocusChange,
}

impl InputMode {
    /// The sequences that switch this mode on.
    pub fn enable_sequences(&self) -> Vec<Cow<'static, str>> {
        match *self {
            InputMode::KittyKeyboard(flags) => vec![Cow::Owned(format!("\x1b[>{}u", flags.bits()))],
            InputMode::BracketedPaste => vec![Cow::Borrowed("\x1b[?2004h")],
            InputMode::MouseCapture => vec![
                // Normal tracking: send mouse X & Y on button press and release.
                Cow::Borrowed("\x1b[?1000h"),
                // Button-event tracking: report button motion (dragging).
                Cow::Borrowed("\x1b[?1002h"),
                // Any-event tracking: report all motion events.
                Cow::Borrowed("\x1b[?1003h"),
                // RXVT mouse mode: coordinates above column 223.
                Cow::Borrowed("\x1b[?1015h"),
                // SGR mouse mode: coordinates above column 223, preferred.
                Cow::Borrowed("\x1b[?1006h"),
            ],
            InputMode::FocusChange => vec![Cow::Borrowed("\x1b[?1004h")],
        }
    }

    /// The sequences that switch this mode off.
    pub fn disable_sequences(&self) -> Vec<Cow<'static, str>> {
        match *self {
            InputMode::KittyKeyboard(_) => vec![Cow::Borrowed("\x1b[<1u")],
            InputMode::BracketedPaste => vec![Cow::Borrowed("\x1b[?2004l")],
            InputMode::MouseCapture => vec![
                // The inverse commands of the enable set, in reverse order.
                Cow::Borrowed("\x1b[?1006l"),
                Cow::Borrowed("\x1b[?1015l"),
                Cow::Borrowed("\x1b[?1003l"),
                Cow::Borrowed("\x1b[?1002l"),
                Cow::Borrowed("\x1b[?1000l"),
            ],
            InputMode::FocusChange => vec![Cow::Borrowed("\x1b[?1004l")],
        }
    }
}

/// The sequences produced by an [`InputStack`] operation, ready to be written
/// to the terminal by a backend.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct InputTransition {
    enable: Vec<Cow<'static, str>>,
    disable: Vec<Cow<'static, str>>,
}

impl InputTransition {
    /// Sequences to write to switch the transition's modes on.
    pub fn enable_sequences(&self) -> &[Cow<'static, str>] {
        &self.enable
    }

    /// Sequences to write to switch the transition's modes off.
    pub fn disable_sequences(&self) -> &[Cow<'static, str>] {
        &self.disable
    }

    /// Whether the transition carries no sequences at all.
    pub fn is_empty(&self) -> bool {
        self.enable.is_empty() && self.disable.is_empty()
    }
}

/// A LIFO stack of input modes with exact enable/disable transitions.
///
/// The stack mirrors the terminal-side kitty keyboard protocol stack: every
/// push is paired with a pop. `MouseCapture` and other DEC-private modes are
/// idempotent at the terminal, but the stack still tracks one entry per push
/// so that restoration is always the exact inverse of what was enabled.
///
/// # Examples
///
/// ```
/// use tuvix_core::runtime::input::{InputMode, InputStack};
///
/// let mut stack = InputStack::new();
/// stack.push(InputMode::FocusChange);
/// stack.push(InputMode::MouseCapture);
///
/// // Restoration unwinds in LIFO order: mouse first, then focus.
/// let restore = stack.restore();
/// assert_eq!(
///     restore.disable_sequences(),
///     [
///         "\x1b[?1006l",
///         "\x1b[?1015l",
///         "\x1b[?1003l",
///         "\x1b[?1002l",
///         "\x1b[?1000l",
///         "\x1b[?1004l",
///     ]
///     .map(String::from)
///     .as_slice()
/// );
/// assert!(stack.is_empty());
/// ```
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct InputStack {
    modes: Vec<InputMode>,
}

impl InputStack {
    /// Creates an empty stack.
    pub fn new() -> Self {
        Self::default()
    }

    /// Pushes `mode` and returns the sequences that enable it.
    ///
    /// Duplicate pushes are allowed and pair with equally many pops.
    pub fn push(&mut self, mode: InputMode) -> InputTransition {
        let enable = mode.enable_sequences();
        self.modes.push(mode);
        InputTransition {
            enable,
            disable: Vec::new(),
        }
    }

    /// Pops the most recently pushed mode and returns the sequences that
    /// disable it. Returns `None` when the stack is empty.
    pub fn pop(&mut self) -> Option<InputTransition> {
        let mode = self.modes.pop()?;
        let disable = mode.disable_sequences();
        Some(InputTransition {
            enable: Vec::new(),
            disable,
        })
    }

    /// Pops every mode in LIFO order and returns the combined disable
    /// sequences. The stack is left empty.
    pub fn restore(&mut self) -> InputTransition {
        let mut disable = Vec::new();
        while let Some(mode) = self.modes.pop() {
            disable.extend(mode.disable_sequences());
        }
        InputTransition {
            enable: Vec::new(),
            disable,
        }
    }

    /// The currently pushed modes, most recently pushed last.
    pub fn modes(&self) -> &[InputMode] {
        &self.modes
    }

    /// Number of pushed modes.
    pub fn len(&self) -> usize {
        self.modes.len()
    }

    /// Whether no modes are pushed.
    pub fn is_empty(&self) -> bool {
        self.modes.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sequences(mode: InputMode) -> Vec<String> {
        mode.enable_sequences()
            .into_iter()
            .chain(mode.disable_sequences())
            .map(Cow::into_owned)
            .collect()
    }

    mod kitty_flags {
        use super::*;

        #[test]
        fn bit_values_match_the_kitty_protocol() {
            assert_eq!(
                KittyEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES.bits(),
                0b0000_0001
            );
            assert_eq!(
                KittyEnhancementFlags::REPORT_EVENT_TYPES.bits(),
                0b0000_0010
            );
            assert_eq!(
                KittyEnhancementFlags::REPORT_ALTERNATE_KEYS.bits(),
                0b0000_0100
            );
            assert_eq!(
                KittyEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES.bits(),
                0b0000_1000
            );
            assert_eq!(
                KittyEnhancementFlags::REPORT_ASSOCIATED_TEXT.bits(),
                0b0001_0000
            );
        }

        #[test]
        fn combinable() {
            let flags = KittyEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                | KittyEnhancementFlags::REPORT_EVENT_TYPES;
            assert_eq!(flags.bits(), 0b0000_0011);
        }
    }

    mod input_mode {
        use super::*;

        #[test]
        fn kitty_push_uses_stack_syntax() {
            let mode = InputMode::KittyKeyboard(
                KittyEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                    | KittyEnhancementFlags::REPORT_EVENT_TYPES,
            );
            let all = sequences(mode);
            assert_eq!(all[0], "\x1b[>3u", "push must be CSI > flags u");
            assert_eq!(all[1], "\x1b[<1u", "pop must be CSI < 1 u");
        }

        #[test]
        fn kitty_pop_is_constant_regardless_of_flags() {
            for flags in [
                KittyEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES,
                KittyEnhancementFlags::REPORT_ASSOCIATED_TEXT,
                KittyEnhancementFlags::empty(),
            ] {
                let mode = InputMode::KittyKeyboard(flags);
                let all = sequences(mode);
                assert_eq!(all[1], "\x1b[<1u");
            }
        }

        #[test]
        fn bracketed_paste_sequences() {
            let all = sequences(InputMode::BracketedPaste);
            assert_eq!(all, vec!["\x1b[?2004h", "\x1b[?2004l"]);
        }

        #[test]
        fn focus_sequences() {
            let all = sequences(InputMode::FocusChange);
            assert_eq!(all, vec!["\x1b[?1004h", "\x1b[?1004l"]);
        }

        #[test]
        fn mouse_capture_enable_disable_mirror_crossterm() {
            let all = sequences(InputMode::MouseCapture);
            assert_eq!(
                all,
                vec![
                    "\x1b[?1000h",
                    "\x1b[?1002h",
                    "\x1b[?1003h",
                    "\x1b[?1015h",
                    "\x1b[?1006h",
                    "\x1b[?1006l",
                    "\x1b[?1015l",
                    "\x1b[?1003l",
                    "\x1b[?1002l",
                    "\x1b[?1000l",
                ]
            );
        }

        #[test]
        fn mode_equality_includes_flags() {
            assert_ne!(
                InputMode::KittyKeyboard(KittyEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES),
                InputMode::KittyKeyboard(KittyEnhancementFlags::REPORT_EVENT_TYPES)
            );
            assert_eq!(InputMode::FocusChange, InputMode::FocusChange);
        }
    }

    mod input_stack {
        use super::*;

        #[test]
        fn new_stack_is_empty() {
            let stack = InputStack::new();
            assert!(stack.is_empty());
            assert_eq!(stack.len(), 0);
            assert_eq!(stack.modes().len(), 0);
        }

        #[test]
        fn push_returns_only_enable_sequences() {
            let mut stack = InputStack::new();
            let transition = stack.push(InputMode::BracketedPaste);
            assert_eq!(
                transition.enable_sequences(),
                ["\x1b[?2004h"].map(Cow::Borrowed)
            );
            assert!(!transition.is_empty());
            assert_eq!(stack.len(), 1);
        }

        #[test]
        fn pop_returns_disable_of_last_pushed() {
            let mut stack = InputStack::new();
            stack.push(InputMode::FocusChange);
            stack.push(InputMode::BracketedPaste);
            let popped = stack.pop().unwrap();
            assert_eq!(
                popped.disable_sequences(),
                ["\x1b[?2004l"].map(Cow::Borrowed)
            );
            assert_eq!(stack.modes(), [InputMode::FocusChange]);
        }

        #[test]
        fn pop_on_empty_stack_is_none() {
            let mut stack = InputStack::new();
            assert!(stack.pop().is_none());
            stack.push(InputMode::FocusChange);
            stack.pop().unwrap();
            assert!(stack.pop().is_none());
        }

        #[test]
        fn restore_unwinds_in_lifo_order() {
            let mut stack = InputStack::new();
            stack.push(InputMode::KittyKeyboard(
                KittyEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES,
            ));
            stack.push(InputMode::BracketedPaste);
            stack.push(InputMode::FocusChange);

            let restore = stack.restore();
            assert_eq!(
                restore.disable_sequences(),
                ["\x1b[?1004l", "\x1b[?2004l", "\x1b[<1u",].map(Cow::Borrowed)
            );
            assert_eq!(restore.enable_sequences().len(), 0);
            assert!(stack.is_empty());
        }

        #[test]
        fn restore_combines_all_mouse_disable_sequences() {
            let mut stack = InputStack::new();
            stack.push(InputMode::MouseCapture);
            stack.push(InputMode::MouseCapture);
            let restore = stack.restore();
            // Two pushes -> the disable set appears twice, LIFO order keeps
            // the reverse-order list intact.
            let disable: Vec<String> = restore
                .disable_sequences()
                .iter()
                .map(|sequence| sequence.clone().into_owned())
                .collect();
            assert_eq!(disable.len(), 10);
            assert_eq!(disable[0], "\x1b[?1006l");
            assert_eq!(disable[9], "\x1b[?1000l");
            assert_eq!(disable[5], "\x1b[?1006l");
        }

        #[test]
        fn restore_on_empty_stack_is_an_empty_transition() {
            let mut stack = InputStack::new();
            let restore = stack.restore();
            assert!(restore.is_empty());
        }

        #[test]
        fn duplicate_pushes_pair_with_duplicate_pops() {
            let mut stack = InputStack::new();
            stack.push(InputMode::FocusChange);
            stack.push(InputMode::FocusChange);
            stack.pop().unwrap();
            assert_eq!(stack.modes(), [InputMode::FocusChange]);
            stack.pop().unwrap();
            assert!(stack.is_empty());
        }

        #[test]
        fn modes_read_back_in_push_order() {
            let mut stack = InputStack::new();
            stack.push(InputMode::BracketedPaste);
            stack.push(InputMode::MouseCapture);
            stack.push(InputMode::FocusChange);
            assert_eq!(
                stack.modes(),
                [
                    InputMode::BracketedPaste,
                    InputMode::MouseCapture,
                    InputMode::FocusChange,
                ]
            );
        }
    }

    mod transition {
        use super::*;

        #[test]
        fn default_is_empty() {
            let transition = InputTransition::default();
            assert!(transition.is_empty());
            assert_eq!(transition.enable_sequences().len(), 0);
            assert_eq!(transition.disable_sequences().len(), 0);
        }

        #[test]
        fn sequences_round_trip_through_cow() {
            let mut stack = InputStack::new();
            let transition = stack.push(InputMode::BracketedPaste);
            let owned: Vec<String> = transition
                .enable_sequences()
                .iter()
                .map(|sequence| sequence.clone().into_owned())
                .collect();
            assert_eq!(owned, vec!["\x1b[?2004h"]);
        }
    }
}
