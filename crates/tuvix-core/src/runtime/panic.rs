//! Panic restoration: what to undo when a panic happens mid-frame.
//!
//! A TUI that panics while in raw mode leaves the user's shell broken —
//! the classic ratatui failure mode. The fix (studied from ratatui's
//! `init()`/`restore()` implementation) has two halves:
//!
//! 1. **Global terminal modes are process-global** — raw mode, the
//!    alternate screen and the input modes can be restored *without* a
//!    handle on the `Terminal` object, straight from a panic hook.
//! 2. The hook must **chain** to the previously installed hook so the
//!    panic message still reaches the user.
//!
//! This module keeps the pure half: the [`RestoreSpec`] captured from an
//! [`InputStack`](crate::runtime::input::InputStack), the process-global
//! registry, and [`install_panic_hook`] which wires a platform restorer
//! into the panic-hook chain. The crossterm backend (and `tuvix::try_init`)
//! supplies the restorer that actually writes sequences and disables raw
//! mode.

use std::sync::Mutex;

use crate::runtime::input::InputMode;

/// The input-mode state to restore on a panic: a snapshot of the input
/// stack, most recently pushed mode last (so restoration unwinds LIFO).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RestoreSpec {
    modes: Vec<InputMode>,
}

impl RestoreSpec {
    /// Builds a spec from the stack's current modes (push order preserved;
    /// restoration is LIFO over this list).
    pub fn from_modes(modes: impl IntoIterator<Item = InputMode>) -> Self {
        Self {
            modes: modes.into_iter().collect(),
        }
    }

    /// The captured modes, in push order.
    #[must_use]
    pub fn modes(&self) -> &[InputMode] {
        &self.modes
    }

    /// The combined disable sequences: every mode's disable set, LIFO.
    ///
    /// The list ends with the show-cursor sequence (`ESC [ ? 25 h`) because
    /// a panicking terminal almost always hid the cursor while rendering.
    pub fn disable_sequences(&self) -> Vec<String> {
        let mut sequences: Vec<String> = self
            .modes
            .iter()
            .rev()
            .flat_map(InputMode::disable_sequences)
            .map(std::borrow::Cow::into_owned)
            .collect();
        sequences.push("\x1b[?25h".to_string());
        sequences
    }
}

static RESTORE_SPEC: Mutex<Option<RestoreSpec>> = Mutex::new(None);

/// Serializes tests that touch the process-global panic hook and registry.
#[cfg(test)]
pub(crate) static TEST_GLOBAL_LOCK: Mutex<()> = Mutex::new(());

/// Blocks until the global-state test lock is free; ignores poisoning (a
/// failing test must not cascade into unrelated tests).
#[cfg(test)]
pub(crate) fn test_global_guard() -> std::sync::MutexGuard<'static, ()> {
    match TEST_GLOBAL_LOCK.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Registers `spec` as the terminal state to restore on panic, replacing
/// any previous registration.
pub fn set_restore_spec(spec: RestoreSpec) {
    let mut guard = match RESTORE_SPEC.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    *guard = Some(spec);
}

/// Takes the registered spec, leaving nothing registered. Called when the
/// runner restores the terminal normally.
pub fn take_restore_spec() -> Option<RestoreSpec> {
    let mut guard = match RESTORE_SPEC.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    guard.take()
}

/// A snapshot of the currently registered spec (empty when none).
pub fn current_restore_spec() -> RestoreSpec {
    let guard = match RESTORE_SPEC.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    guard.clone().unwrap_or_default()
}

/// Installs a panic hook that first runs `restorer` with the registered
/// [`RestoreSpec`], then delegates to the previously installed hook.
///
/// Restorer errors are swallowed — a panic hook must not panic.
pub fn install_panic_hook<F>(restorer: F)
where
    F: Fn(&RestoreSpec) + Send + Sync + 'static,
{
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let spec = current_restore_spec();
        restorer(&spec);
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::input::{InputMode, InputStack, KittyEnhancementFlags};
    use std::sync::{Arc, Mutex as StdMutex};

    mod restore_spec {
        use super::*;

        #[test]
        fn default_spec_has_only_the_cursor_sequence() {
            let spec = RestoreSpec::default();
            assert_eq!(spec.modes(), []);
            assert_eq!(spec.disable_sequences(), vec!["\x1b[?25h"]);
        }

        #[test]
        fn disable_sequences_unwind_lifo_and_show_cursor_last() {
            let spec = RestoreSpec::from_modes([
                InputMode::KittyKeyboard(KittyEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES),
                InputMode::BracketedPaste,
                InputMode::FocusChange,
            ]);
            assert_eq!(
                spec.disable_sequences(),
                vec!["\x1b[?1004l", "\x1b[?2004l", "\x1b[<1u", "\x1b[?25h",]
            );
        }

        #[test]
        fn full_mouse_disable_is_included_verbatim() {
            let spec = RestoreSpec::from_modes([InputMode::MouseCapture]);
            assert_eq!(
                spec.disable_sequences(),
                vec![
                    "\x1b[?1006l",
                    "\x1b[?1015l",
                    "\x1b[?1003l",
                    "\x1b[?1002l",
                    "\x1b[?1000l",
                    "\x1b[?25h",
                ]
            );
        }

        #[test]
        fn modes_read_back_in_push_order() {
            let spec = RestoreSpec::from_modes([InputMode::FocusChange, InputMode::BracketedPaste]);
            assert_eq!(
                spec.modes(),
                &[InputMode::FocusChange, InputMode::BracketedPaste]
            );
        }

        #[test]
        fn spec_matches_an_input_stack_snapshot() {
            let mut stack = InputStack::new();
            stack.push(InputMode::BracketedPaste);
            stack.push(InputMode::MouseCapture);
            let spec = RestoreSpec::from_modes(stack.modes().iter().copied());
            assert_eq!(spec.modes(), stack.modes());
            let mut expected: Vec<String> = stack
                .restore()
                .disable_sequences()
                .iter()
                .map(|sequence| sequence.clone().into_owned())
                .collect();
            expected.push("\x1b[?25h".to_string());
            assert_eq!(spec.disable_sequences(), expected);
        }
    }

    mod registry {
        use super::*;

        #[test]
        fn set_then_take_round_trips() {
            let _guard = test_global_guard();
            let spec = RestoreSpec::from_modes([InputMode::FocusChange]);
            set_restore_spec(spec.clone());
            assert_eq!(current_restore_spec(), spec);
            assert_eq!(take_restore_spec(), Some(spec));
            assert_eq!(take_restore_spec(), None);
            assert_eq!(current_restore_spec(), RestoreSpec::default());
        }

        #[test]
        fn set_replaces_the_previous_spec() {
            let _guard = test_global_guard();
            set_restore_spec(RestoreSpec::from_modes([InputMode::FocusChange]));
            set_restore_spec(RestoreSpec::from_modes([InputMode::MouseCapture]));
            let current = current_restore_spec();
            assert_eq!(current.modes(), &[InputMode::MouseCapture]);
            take_restore_spec();
        }
    }

    mod panic_hook {
        use super::*;

        /// Runs `panic!("tuvix-hook-test")` under `catch_unwind` (the hook
        /// still runs — that is what we assert on).
        fn trigger_panic() {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                panic!("tuvix-hook-test");
            }));
        }

        #[test]
        fn hook_chains_restorer_then_previous_hook() {
            let _guard = test_global_guard();
            let original = std::panic::take_hook();
            let log: Arc<StdMutex<Vec<String>>> = Arc::new(StdMutex::new(Vec::new()));

            // First hook: restorer A chained onto the original hook.
            {
                let log = Arc::clone(&log);
                install_panic_hook(move |_spec| {
                    log.lock().unwrap().push("restore-A".to_string());
                });
            }
            // Second hook: restorer B chained onto hook A.
            {
                let log = Arc::clone(&log);
                install_panic_hook(move |_spec| {
                    log.lock().unwrap().push("restore-B".to_string());
                });
            }

            set_restore_spec(RestoreSpec::from_modes([InputMode::FocusChange]));
            trigger_panic();

            let entries = log.lock().unwrap().clone();
            assert_eq!(
                entries,
                vec!["restore-B".to_string(), "restore-A".to_string()],
                "most recent restorer runs first, then the chained ones"
            );

            // Cleanup for the rest of the suite.
            take_restore_spec();
            std::panic::set_hook(original);
        }

        #[test]
        fn restorer_receives_the_registered_spec() {
            let _guard = test_global_guard();
            let original = std::panic::take_hook();
            let seen: Arc<StdMutex<Option<RestoreSpec>>> = Arc::new(StdMutex::new(None));
            {
                let seen = Arc::clone(&seen);
                install_panic_hook(move |spec: &RestoreSpec| {
                    *seen.lock().unwrap() = Some(spec.clone());
                });
            }

            let spec =
                RestoreSpec::from_modes([InputMode::MouseCapture, InputMode::BracketedPaste]);
            set_restore_spec(spec);
            trigger_panic();

            let captured = seen.lock().unwrap().clone().unwrap();
            assert_eq!(
                captured.modes(),
                &[InputMode::MouseCapture, InputMode::BracketedPaste]
            );

            take_restore_spec();
            std::panic::set_hook(original);
        }

        #[test]
        fn restorer_runs_even_without_a_registered_spec() {
            let _guard = test_global_guard();
            let original = std::panic::take_hook();
            let seen: Arc<StdMutex<Option<RestoreSpec>>> = Arc::new(StdMutex::new(None));
            {
                let seen = Arc::clone(&seen);
                install_panic_hook(move |spec: &RestoreSpec| {
                    *seen.lock().unwrap() = Some(spec.clone());
                });
            }

            trigger_panic();
            let captured = seen.lock().unwrap().clone();
            assert_eq!(captured, Some(RestoreSpec::default()));

            std::panic::set_hook(original);
        }
    }
}
