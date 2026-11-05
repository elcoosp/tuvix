//! The application runner: input modes, panic hooks, restore-on-drop.
//!
//! [`AppRunner`] wraps a [`Terminal`] and owns the terminal *lifecycle* on
//! top of it:
//!
//! - **input modes** — [`push_input_mode`](AppRunner::push_input_mode) /
//!   [`pop_input_mode`](AppRunner::pop_input_mode) drive the
//!   [`InputStack`] and write each
//!   transition through [`Backend::write_control`], flushing immediately so
//!   modes reach the terminal before the next frame;
//! - **restore** — [`restore`](AppRunner::restore) unwinds every pushed
//!   mode in LIFO order, shows the cursor and flushes; it is idempotent;
//! - **restore-on-drop** — `Drop` runs the same restore, so panics on the
//!   runner's thread unwind through it and the terminal survives;
//! - **panic hooks** — [`install_panic_hook`](AppRunner::install_panic_hook)
//!   registers the current input stack as a [`RestoreSpec`] and chains a
//!   platform restorer in front of the previous panic hook, covering the
//!   threads the runner does not own.
//!
//! # Examples
//!
//! ```
//! use tuvix_core::backend::TestBackend;
//! use tuvix_core::geometry::Size;
//! use tuvix_core::runtime::input::InputMode;
//! use tuvix_core::runtime::runner::AppRunner;
//! use tuvix_core::terminal::Terminal;
//!
//! let terminal = Terminal::new(TestBackend::new(Size::new(8, 2))).unwrap();
//! let mut runner = AppRunner::new(terminal);
//! runner.push_input_mode(InputMode::BracketedPaste).unwrap();
//!
//! runner.draw(|frame| {
//!     frame.buffer.set_string(0, 0, "hello", Default::default());
//! }).unwrap();
//!
//! runner.restore().unwrap();
//! ```

use crate::backend::Backend;
use crate::cancellation::CancellationToken;
use crate::runtime::input::{InputMode, InputStack, InputTransition};
use crate::runtime::panic::{self, RestoreSpec};
use crate::terminal::{DrawOutcome, Terminal};

/// A [`Terminal`] plus lifecycle: input modes, panic restoration and
/// restore-on-drop.
pub struct AppRunner<B: Backend> {
    terminal: Terminal<B>,
    input: InputStack,
    restored: bool,
    panic_spec_registered: bool,
}

impl<B: Backend> core::fmt::Debug for AppRunner<B> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("AppRunner")
            .field("input", &self.input)
            .field("restored", &self.restored)
            .finish_non_exhaustive()
    }
}

impl<B: Backend> AppRunner<B> {
    /// Wraps `terminal` in a runner.
    pub fn new(terminal: Terminal<B>) -> Self {
        Self {
            terminal,
            input: InputStack::new(),
            restored: false,
            panic_spec_registered: false,
        }
    }

    /// Shared access to the wrapped terminal.
    pub fn terminal(&self) -> &Terminal<B> {
        &self.terminal
    }

    /// Exclusive access to the wrapped terminal.
    pub fn terminal_mut(&mut self) -> &mut Terminal<B> {
        &mut self.terminal
    }

    /// The input-mode stack managed by this runner.
    pub fn input_stack(&self) -> &InputStack {
        &self.input
    }

    /// Pushes `mode` onto the input stack and writes its enable sequences to
    /// the terminal immediately.
    ///
    /// On a backend error the stack is rolled back, so restore-on-drop never
    /// disables a mode that never fully applied.
    pub fn push_input_mode(&mut self, mode: InputMode) -> Result<(), B::Error> {
        let transition = self.input.push(mode);
        if let Err(error) = self.apply(&transition) {
            self.input.pop();
            return Err(error);
        }
        Ok(())
    }

    /// Pops the most recently pushed mode and writes its disable sequences.
    /// Returns the popped mode, or `None` when the stack is empty.
    ///
    /// On a backend error the mode is pushed back onto the stack, so
    /// restore-on-drop still tries to disable it.
    pub fn pop_input_mode(&mut self) -> Result<Option<InputMode>, B::Error> {
        let Some(mode) = self.input.modes().last().copied() else {
            return Ok(None);
        };
        let transition = self
            .input
            .pop()
            .expect("the last mode was just read from the stack");
        if let Err(error) = self.apply(&transition) {
            self.input.push(mode);
            return Err(error);
        }
        Ok(Some(mode))
    }

    /// Renders one frame through the wrapped [`Terminal::draw`].
    pub fn draw(
        &mut self,
        render: impl FnOnce(&mut crate::terminal::Frame<'_>),
    ) -> Result<(), B::Error> {
        self.terminal.draw(render)
    }

    /// Renders one journal-driven frame through the wrapped
    /// [`Terminal::draw_persistent`].
    pub fn draw_persistent(
        &mut self,
        render: impl FnOnce(&mut crate::terminal::Frame<'_>),
    ) -> Result<(), B::Error> {
        self.terminal.draw_persistent(render)
    }

    /// Renders one frame that input is allowed to preempt, through the
    /// wrapped [`Terminal::draw_cancellable`].
    pub fn draw_cancellable(
        &mut self,
        token: &CancellationToken,
        render: impl FnOnce(&mut crate::terminal::Frame<'_>),
    ) -> Result<DrawOutcome, B::Error> {
        self.terminal.draw_cancellable(token, render)
    }

    /// Renders one journal-driven frame that input is allowed to preempt,
    /// through the wrapped [`Terminal::draw_persistent_cancellable`].
    pub fn draw_persistent_cancellable(
        &mut self,
        token: &CancellationToken,
        render: impl FnOnce(&mut crate::terminal::Frame<'_>),
    ) -> Result<DrawOutcome, B::Error> {
        self.terminal.draw_persistent_cancellable(token, render)
    }

    /// Restores the terminal: every pushed input mode is disabled in LIFO
    /// order, the cursor is shown, everything is flushed and the panic
    /// restoration spec is unregistered.
    ///
    /// Idempotent: calls after the first are a no-op.
    pub fn restore(&mut self) -> Result<(), B::Error> {
        if self.restored {
            return Ok(());
        }
        let transition = self.input.restore();
        self.apply(&transition)?;
        self.terminal.show_cursor()?;
        self.terminal.backend_mut().flush()?;
        self.restored = true;
        if self.panic_spec_registered {
            let _ = panic::take_restore_spec();
            self.panic_spec_registered = false;
        }
        Ok(())
    }

    /// Registers the current input stack as the process-wide panic
    /// [`RestoreSpec`] and installs a chained panic hook running `restorer`
    /// (see [`panic::install_panic_hook`]).
    ///
    /// [`restore`](Self::restore) (explicitly or through `Drop`)
    /// unregisters the spec.
    pub fn install_panic_hook<F>(&mut self, restorer: F)
    where
        F: Fn(&RestoreSpec) + Send + Sync + 'static,
    {
        panic::set_restore_spec(RestoreSpec::from_modes(self.input.modes().iter().copied()));
        panic::install_panic_hook(restorer);
        self.panic_spec_registered = true;
    }

    /// Writes every sequence of `transition` and flushes when non-empty.
    fn apply(&mut self, transition: &InputTransition) -> Result<(), B::Error> {
        for sequence in transition.enable_sequences() {
            self.terminal.backend_mut().write_control(sequence)?;
        }
        for sequence in transition.disable_sequences() {
            self.terminal.backend_mut().write_control(sequence)?;
        }
        if !transition.is_empty() {
            self.terminal.backend_mut().flush()?;
        }
        Ok(())
    }
}

impl<B: Backend> Drop for AppRunner<B> {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{Backend, ClearRegion};
    use crate::buffer::DiffRun;
    use crate::geometry::{Position, Size};
    use crate::style::Style;
    use std::sync::{Arc, Mutex};

    /// A backend that records every operation through a shared handle, so
    /// tests can inspect the log even after the runner (and therefore the
    /// backend) has been dropped.
    #[derive(Debug, Clone, Default)]
    struct RecordingBackend {
        log: Arc<Mutex<Vec<String>>>,
        fail_at: usize,
        writes: usize,
    }

    impl RecordingBackend {
        fn new() -> Self {
            Self::default()
        }

        /// Fails the `n`-th `write_control` call (1-based); 0 never fails.
        fn fail_control_at(mut self, n: usize) -> Self {
            self.fail_at = n;
            self
        }
    }

    impl Backend for RecordingBackend {
        type Error = std::io::Error;

        fn draw<'a>(
            &mut self,
            content: impl IntoIterator<Item = DiffRun<'a>>,
        ) -> Result<(), Self::Error> {
            let runs = content.into_iter().count();
            self.log.lock().unwrap().push(format!("draw {runs} runs"));
            Ok(())
        }

        fn flush(&mut self) -> Result<(), Self::Error> {
            self.log.lock().unwrap().push("flush".to_string());
            Ok(())
        }

        fn size(&self) -> Result<Size, Self::Error> {
            Ok(Size::new(8, 2))
        }

        fn get_cursor_position(&self) -> Result<Position, Self::Error> {
            Ok(Position::new(0, 0))
        }

        fn set_cursor_position(&mut self, _position: Position) -> Result<(), Self::Error> {
            Ok(())
        }

        fn hide_cursor(&mut self) -> Result<(), Self::Error> {
            self.log.lock().unwrap().push("hide_cursor".to_string());
            Ok(())
        }

        fn show_cursor(&mut self) -> Result<(), Self::Error> {
            self.log.lock().unwrap().push("show_cursor".to_string());
            Ok(())
        }

        fn clear(&mut self, _region: ClearRegion) -> Result<(), Self::Error> {
            Ok(())
        }

        fn write_control(&mut self, sequence: &str) -> Result<(), Self::Error> {
            self.writes += 1;
            if self.fail_at == self.writes {
                return Err(std::io::Error::other("injected backend failure"));
            }
            self.log.lock().unwrap().push(format!("control {sequence}"));
            Ok(())
        }
    }

    fn recording_runner() -> (AppRunner<RecordingBackend>, Arc<Mutex<Vec<String>>>) {
        let backend = RecordingBackend::new();
        let log = Arc::clone(&backend.log);
        let terminal = Terminal::new(backend).unwrap();
        (AppRunner::new(terminal), log)
    }

    mod push_and_pop {
        use super::*;
        use crate::runtime::input::KittyEnhancementFlags;

        #[test]
        fn push_writes_enable_sequences_and_flushes() {
            let (mut runner, log) = recording_runner();
            runner.push_input_mode(InputMode::BracketedPaste).unwrap();
            assert_eq!(
                log.lock().unwrap().clone(),
                vec!["control \x1b[?2004h".to_string(), "flush".to_string()]
            );
            assert_eq!(runner.input_stack().modes(), [InputMode::BracketedPaste]);
        }

        #[test]
        fn push_kitty_writes_stack_push_sequence() {
            let (mut runner, log) = recording_runner();
            runner
                .push_input_mode(InputMode::KittyKeyboard(
                    KittyEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES,
                ))
                .unwrap();
            assert_eq!(
                log.lock().unwrap().clone(),
                vec!["control \x1b[>1u".to_string(), "flush".to_string()]
            );
        }

        #[test]
        fn pop_writes_disable_sequences() {
            let (mut runner, log) = recording_runner();
            runner.push_input_mode(InputMode::FocusChange).unwrap();
            log.lock().unwrap().clear();
            let popped = runner.pop_input_mode().unwrap();
            assert_eq!(popped, Some(InputMode::FocusChange));
            assert_eq!(
                log.lock().unwrap().clone(),
                vec!["control \x1b[?1004l".to_string(), "flush".to_string()]
            );
            assert!(runner.input_stack().is_empty());
        }

        #[test]
        fn pop_on_empty_stack_writes_nothing() {
            let (mut runner, log) = recording_runner();
            assert_eq!(runner.pop_input_mode().unwrap(), None);
            assert!(log.lock().unwrap().is_empty());
        }

        #[test]
        fn push_failure_rolls_back_the_stack() {
            let backend = RecordingBackend::new().fail_control_at(1);
            let log = Arc::clone(&backend.log);
            let terminal = Terminal::new(backend).unwrap();
            let mut runner = AppRunner::new(terminal);
            let error = runner.push_input_mode(InputMode::MouseCapture).unwrap_err();
            assert_eq!(error.to_string(), "injected backend failure");
            assert!(runner.input_stack().is_empty());
            assert_eq!(log.lock().unwrap().len(), 0);
        }

        #[test]
        fn pop_failure_pushes_the_mode_back() {
            let (mut runner, log) = recording_runner();
            runner.push_input_mode(InputMode::BracketedPaste).unwrap();
            // Fail the *next* control write (the pop's disable sequence).
            runner.terminal_mut().backend_mut().fail_at = 2;
            log.lock().unwrap().clear();
            let error = runner.pop_input_mode().unwrap_err();
            assert_eq!(error.to_string(), "injected backend failure");
            assert_eq!(runner.input_stack().modes(), [InputMode::BracketedPaste]);
            // Drop restores it anyway (idempotent at the terminal).
            drop(runner);
            let log = log.lock().unwrap().clone();
            assert!(log.contains(&"control \x1b[?2004l".to_string()));
        }
    }

    mod restore {
        use super::*;

        #[test]
        fn restore_unwinds_lifo_shows_cursor_and_flushes() {
            let (mut runner, log) = recording_runner();
            // A draw hides the cursor, so restore must show it again.
            runner.draw(|_| {}).unwrap();
            runner.push_input_mode(InputMode::FocusChange).unwrap();
            runner.push_input_mode(InputMode::BracketedPaste).unwrap();
            log.lock().unwrap().clear();

            runner.restore().unwrap();
            assert_eq!(
                log.lock().unwrap().clone(),
                vec![
                    "control \x1b[?2004l".to_string(),
                    "control \x1b[?1004l".to_string(),
                    "flush".to_string(),
                    "show_cursor".to_string(),
                    "flush".to_string(),
                ]
            );
            assert!(runner.input_stack().is_empty());
        }

        #[test]
        fn restore_without_a_draw_skips_the_memoized_show_cursor() {
            // The cursor was never hidden, so Terminal::show_cursor is a
            // no-op and must not reach the backend.
            let (mut runner, log) = recording_runner();
            runner.push_input_mode(InputMode::FocusChange).unwrap();
            log.lock().unwrap().clear();

            runner.restore().unwrap();
            assert_eq!(
                log.lock().unwrap().clone(),
                vec![
                    "control \x1b[?1004l".to_string(),
                    "flush".to_string(),
                    "flush".to_string(),
                ]
            );
        }

        #[test]
        fn restore_is_idempotent() {
            let (mut runner, log) = recording_runner();
            runner.push_input_mode(InputMode::FocusChange).unwrap();
            runner.restore().unwrap();
            log.lock().unwrap().clear();
            runner.restore().unwrap();
            assert!(log.lock().unwrap().is_empty());
        }

        #[test]
        fn restore_on_never_pushed_runner_still_flushes() {
            let (mut runner, log) = recording_runner();
            runner.restore().unwrap();
            assert_eq!(log.lock().unwrap().clone(), vec!["flush".to_string()]);
        }

        #[test]
        fn drop_restores_the_terminal() {
            let log;
            {
                let (mut runner, shared) = recording_runner();
                runner.push_input_mode(InputMode::MouseCapture).unwrap();
                runner.push_input_mode(InputMode::FocusChange).unwrap();
                log = shared;
                drop(runner);
            }
            let entries = log.lock().unwrap().clone();
            assert!(entries.contains(&"control \x1b[?1004l".to_string()));
            assert!(entries.contains(&"control \x1b[?1006l".to_string()));
            assert_eq!(entries.last().map(String::as_str), Some("flush"));
        }

        #[test]
        fn drop_after_explicit_restore_does_not_double_restore() {
            let (mut runner, log) = recording_runner();
            runner.push_input_mode(InputMode::FocusChange).unwrap();
            runner.restore().unwrap();
            log.lock().unwrap().clear();
            drop(runner);
            assert!(log.lock().unwrap().is_empty());
        }
    }

    mod draw {
        use super::*;
        use crate::backend::TestBackend;
        use crate::geometry::Size as GeometrySize;

        #[test]
        fn draw_passes_through_to_the_terminal() {
            let terminal = Terminal::new(TestBackend::new(GeometrySize::new(8, 2))).unwrap();
            let mut runner = AppRunner::new(terminal);
            runner
                .draw(|frame| {
                    frame.buffer.set_string(0, 0, "hi", Style::default());
                })
                .unwrap();
            assert_eq!(
                runner.terminal().backend().buffer().to_string(),
                "hi      \n        "
            );
        }

        #[test]
        fn draw_and_control_sequences_coexist() {
            let terminal = Terminal::new(TestBackend::new(GeometrySize::new(8, 2))).unwrap();
            let mut runner = AppRunner::new(terminal);
            runner.push_input_mode(InputMode::FocusChange).unwrap();
            runner.draw(|_| {}).unwrap();
            assert_eq!(
                runner.terminal().backend().control_log(),
                ["\x1b[?1004h"].map(String::from)
            );
            assert!(runner.terminal().backend().is_cursor_hidden());
        }

        #[test]
        fn draw_cancellable_passes_through_with_the_outcome() {
            use crate::cancellation::CancellationToken;
            use crate::terminal::DrawOutcome;

            let terminal = Terminal::new(TestBackend::new(GeometrySize::new(8, 2))).unwrap();
            let mut runner = AppRunner::new(terminal);
            let token = CancellationToken::new();
            let outcome = runner
                .draw_cancellable(&token, |frame| {
                    frame.buffer.set_string(0, 0, "hi", Style::default());
                })
                .unwrap();
            assert_eq!(outcome, DrawOutcome::Committed);
            assert_eq!(
                runner.terminal().backend().buffer().to_string(),
                "hi      \n        "
            );

            token.cancel();
            let outcome = runner
                .draw_cancellable(&token, |frame| {
                    frame.buffer.set_string(0, 0, "junk", Style::default());
                })
                .unwrap();
            assert_eq!(outcome, DrawOutcome::Cancelled);
            assert_eq!(
                runner.terminal().backend().buffer().to_string(),
                "hi      \n        "
            );
        }

        #[test]
        fn draw_persistent_cancellable_discards_on_the_runner_too() {
            use crate::cancellation::CancellationToken;
            use crate::terminal::DrawOutcome;

            let terminal =
                Terminal::new_persistent(TestBackend::new(GeometrySize::new(8, 2))).unwrap();
            let mut runner = AppRunner::new(terminal);
            runner
                .draw_persistent(|frame| {
                    frame.buffer.set_string(0, 0, "hi", Style::default());
                })
                .unwrap();

            let token = CancellationToken::new();
            let outcome = runner
                .draw_persistent_cancellable(&token, |frame| {
                    frame.buffer.set_string(0, 0, "junk", Style::default());
                    if let Some(stage) = frame.cancellation() {
                        stage.cancel();
                    }
                })
                .unwrap();
            assert_eq!(outcome, DrawOutcome::Cancelled);
            assert_eq!(
                runner.terminal().backend().buffer().to_string(),
                "hi      \n        "
            );
        }
    }

    mod panic_registration {
        use super::*;
        use crate::runtime::input::KittyEnhancementFlags;
        use crate::runtime::panic::test_global_guard;

        #[test]
        fn install_panic_hook_registers_the_current_stack() {
            let _guard = test_global_guard();
            let (mut runner, _log) = recording_runner();
            runner.install_panic_hook(|_spec| {});
            let spec = panic::current_restore_spec();
            assert_eq!(spec.modes(), runner.input_stack().modes());
            panic::take_restore_spec();
        }

        #[test]
        fn install_panic_hook_replaces_the_spec_when_called_again() {
            let _guard = test_global_guard();
            let (mut runner, _log) = recording_runner();
            runner.install_panic_hook(|_spec| {});
            runner
                .push_input_mode(InputMode::KittyKeyboard(
                    KittyEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES,
                ))
                .unwrap();
            runner.install_panic_hook(|_spec| {});
            let spec = panic::current_restore_spec();
            assert_eq!(spec.modes(), runner.input_stack().modes());
            panic::take_restore_spec();
        }

        #[test]
        fn restore_unregisters_the_spec() {
            let _guard = test_global_guard();
            let (mut runner, _log) = recording_runner();
            runner.push_input_mode(InputMode::FocusChange).unwrap();
            runner.install_panic_hook(|_spec| {});
            assert_eq!(panic::current_restore_spec().modes().len(), 1);
            runner.restore().unwrap();
            assert_eq!(panic::current_restore_spec().modes().len(), 0);
        }

        #[test]
        fn drop_unregisters_the_spec() {
            let _guard = test_global_guard();
            {
                let (mut runner, _log) = recording_runner();
                runner.push_input_mode(InputMode::BracketedPaste).unwrap();
                runner.install_panic_hook(|_spec| {});
                assert_eq!(panic::current_restore_spec().modes().len(), 1);
            }
            assert_eq!(panic::current_restore_spec().modes().len(), 0);
        }

        #[test]
        fn runners_without_a_hook_leave_a_foreign_spec_alone() {
            let _guard = test_global_guard();
            let foreign = RestoreSpec::from_modes([InputMode::FocusChange]);
            panic::set_restore_spec(foreign);
            {
                let (mut runner, _log) = recording_runner();
                runner.push_input_mode(InputMode::BracketedPaste).unwrap();
                runner.restore().unwrap();
            }
            assert_eq!(
                panic::current_restore_spec(),
                RestoreSpec::from_modes([InputMode::FocusChange])
            );
            panic::take_restore_spec();
        }
    }

    mod debug {
        use super::*;

        #[test]
        fn debug_impl_mentions_state() {
            let (runner, _log) = recording_runner();
            let rendered = format!("{runner:?}");
            assert!(rendered.contains("AppRunner"));
            assert!(rendered.contains("restored"));
        }
    }
}
