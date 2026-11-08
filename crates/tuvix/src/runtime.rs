//! The one-call terminal runtime, ratatui-style.
//!
//! [`try_init`] installs the panic hook, enables raw mode, enters the
//! alternate screen and hands back a [`DefaultRunner`] whose input stack is
//! ready for additional modes. Drop the runner (or call
//! [`AppRunner::restore`](tuvix_core::runtime::runner::AppRunner::restore))
//! and everything is undone in LIFO order — on the happy path, on errors
//! and on panics.
//!
//! Sequences and ordering follow the pattern studied in ratatui's
//! `init`/`restore`: restore raw mode first (widest side effects), then the
//! screen, then the cursor.

use std::io::stdout;
use std::io::{self, Write};

use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use tuvix_backend_crossterm::CrosstermBackend;
use tuvix_core::terminal::Terminal;

pub use tuvix_core::runtime::*;

/// The runner returned by [`try_init`]: an [`AppRunner`] over the crossterm
/// backend writing to stdout.
pub type DefaultRunner = AppRunner<CrosstermBackend<std::io::Stdout>>;

/// Initializes the terminal (raw mode + alternate screen) and returns the
/// runner, installing the panic-restore hook first.
///
/// The default input stack is intentionally minimal — apps push the modes
/// they want (mouse capture, bracketed paste, kitty keyboard flags) through
/// [`AppRunner::push_input_mode`].
///
/// # Examples
///
/// ```no_run
/// # use std::io::Result;
/// use tuvix::prelude::*;
///
/// # fn main() -> Result<()> {
/// let mut runner = tuvix::try_init()?;
/// runner.push_input_mode(InputMode::BracketedPaste)?;
///
/// runner.draw(|frame| {
///     frame.buffer.set_string(0, 0, "hello", Style::default());
/// })?;
///
/// // Restoration happens here — or on drop, or on panic.
/// runner.restore()?;
/// # Ok(())
/// # }
/// ```
///
/// # Errors
///
/// Fails when raw mode, the alternate screen or the terminal size cannot be
/// set up; whatever was enabled is rolled back before the error returns.
pub fn try_init() -> io::Result<DefaultRunner> {
    install_panic_restore();
    enable_raw_mode()?;
    match init_with_raw_mode() {
        Ok(runner) => Ok(runner),
        Err(error) => {
            // Roll back what was enabled so a failed init never leaves the
            // terminal in a half-touched state.
            rollback_partially_initialized();
            Err(error)
        }
    }
}

/// Rolls back a partially-initialized terminal: leaves the alternate screen
/// and disables raw mode. Errors are swallowed (best effort).
fn rollback_partially_initialized() {
    let mut out = stdout();
    let _ = execute!(out, LeaveAlternateScreen);
    let _ = disable_raw_mode();
}

/// Like [`try_init`] but panics with a descriptive message on failure.
///
/// # Panics
///
/// Panics when raw mode or the alternate screen cannot be set up.
#[must_use]
pub fn init() -> DefaultRunner {
    try_init().expect("failed to initialize the terminal")
}

/// The half of init that runs once raw mode is on: alternate screen, then
/// the runner.
fn init_with_raw_mode() -> io::Result<DefaultRunner> {
    execute!(stdout(), EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout());
    let terminal = Terminal::new(backend)?;
    Ok(AppRunner::new(terminal))
}

/// Restores the process-global terminal state: disables raw mode, pops the
/// registered input modes, leaves the alternate screen and shows the
/// cursor. Used by the panic hook and available for global cleanup.
///
/// # Errors
///
/// Returns the first restoration error; later steps still run.
pub fn try_restore_global() -> io::Result<()> {
    let spec = tuvix_core::runtime::panic::current_restore_spec();
    disable_raw_mode()?;
    write_restore_sequences(&spec)?;
    let mut out = stdout();
    execute!(out, LeaveAlternateScreen, crossterm::cursor::Show)?;
    let _ = tuvix_core::runtime::panic::take_restore_spec();
    Ok(())
}

/// Like [`try_restore_global`] but prints a warning on failure instead of
/// returning the error.
pub fn restore_global() {
    if let Err(error) = try_restore_global() {
        // There is not much to do when restoration fails; make the failure
        // visible without panicking inside a hook.
        eprintln!("tuvix: failed to restore the terminal: {error}");
    }
}

/// Writes the spec's disable sequences to stdout and flushes.
fn write_restore_sequences(spec: &RestoreSpec) -> io::Result<()> {
    let mut out = stdout();
    for sequence in spec.disable_sequences() {
        out.write_all(sequence.as_bytes())?;
    }
    out.flush()
}

/// Chains the global restorer in front of the current panic hook.
fn install_panic_restore() {
    tuvix_core::runtime::panic::install_panic_hook(|spec| {
        // Best-effort restoration from inside a hook: never propagate.
        let _ = disable_raw_mode();
        if let Err(error) = write_restore_sequences(spec) {
            eprintln!("tuvix: panic restore failed: {error}");
        }
        let mut out = stdout();
        let _ = execute!(out, LeaveAlternateScreen, crossterm::cursor::Show);
    });
}

/// The termion runtime (unix only): everything [`try_init`] does, driven by
/// the termion backend instead of crossterm.
///
/// Raw mode is termios state owned by the backend's raw-terminal handle:
/// it is restored when the runner unwinds (drop), and the panic hook below
/// restores everything sequence-based — input modes, the alternate screen,
/// the cursor. Unlike the crossterm restorer it cannot disable termios
/// from the hook (no handle, and the workspace forbids unsafe); a panic
/// therefore restores raw mode one step later, during unwinding.
#[cfg(unix)]
pub mod termion_runtime {
    use std::io::{self, Write, stdout};

    use tuvix_backend_termion::TermionBackend;
    use tuvix_core::backend::Backend as _;
    use tuvix_core::runtime::panic::{self, RestoreSpec};
    use tuvix_core::runtime::runner::AppRunner;
    use tuvix_core::terminal::Terminal;

    /// The runner returned by [`try_init_termion`].
    pub type TermionRunner = AppRunner<TermionBackend<std::io::Stdout>>;

    /// Initializes the terminal through termion (raw mode + alternate
    /// screen) and returns the runner, installing the panic-restore hook
    /// first.
    ///
    /// # Errors
    ///
    /// Fails when raw mode or the terminal size cannot be set up; whatever
    /// was enabled is rolled back before the error returns.
    pub fn try_init_termion() -> io::Result<TermionRunner> {
        install_panic_restore_termion();
        let mut backend = TermionBackend::new(stdout())?;
        let initialized = enter_alternate_screen(&mut backend);
        match initialized {
            Ok(()) => {
                let terminal = Terminal::new(backend)?;
                Ok(AppRunner::new(terminal))
            }
            Err(error) => {
                // Rolling back raw mode happens when the backend drops.
                Err(error)
            }
        }
    }

    /// Like [`try_init_termion`] but panics with a descriptive message on
    /// failure.
    ///
    /// # Panics
    ///
    /// Panics when raw mode or the alternate screen cannot be set up.
    #[must_use]
    pub fn init_termion() -> TermionRunner {
        try_init_termion().expect("failed to initialize the termion terminal")
    }

    /// The alternate screen is two control sequences; the runner owns the
    /// lifecycle, so the backend writes them like any input mode.
    fn enter_alternate_screen(backend: &mut TermionBackend<std::io::Stdout>) -> io::Result<()> {
        backend.write_control("\x1b[?1049h")?;
        backend.flush()
    }

    /// Writes the sequence-based half of restoration to stdout: input-mode
    /// disables, alternate screen leave, cursor show. Raw mode is restored
    /// by unwinding (the `RawTerminal` inside the runner drops).
    fn restore_sequences(spec: &RestoreSpec) {
        let mut out = stdout();
        let mut failed = false;
        for sequence in spec.disable_sequences() {
            failed |= out.write_all(sequence.as_bytes()).is_err();
        }
        // Leave the alternate screen and show the cursor.
        failed |= out.write_all(b"\x1b[?1049l\x1b[?25h").is_err();
        failed |= out.flush().is_err();
        if failed {
            eprintln!("tuvix: termion panic restore failed");
        }
    }

    /// Chains the termion restorer in front of the current panic hook.
    fn install_panic_restore_termion() {
        panic::install_panic_hook(restore_sequences);
    }
}

#[cfg(unix)]
pub use termion_runtime::{TermionRunner, init_termion, try_init_termion};
