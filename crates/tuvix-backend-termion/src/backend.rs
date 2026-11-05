//! The termion-powered [`Backend`]: the dependency-free ANSI raster
//! machine over termion's raw mode.
//!
//! [`TermionBackend`] composes the two other layers of the framework: it
//! enters termion raw mode (termios — restored when the backend drops),
//! and delegates every emission to
//! [`AnsiBackend`]. Terminal size is
//! queried live through termion on every `size()` call, so resize handling
//! needs no plumbing.

use std::io::{self, Write};
use std::os::fd::AsFd;

use termion::raw::{IntoRawMode, RawTerminal};
use tuvix_backend_ansi::AnsiBackend;
use tuvix_core::backend::{Backend, ClearRegion};
use tuvix_core::buffer::DiffRun;
use tuvix_core::geometry::{Position, Size};

/// A [`Backend`] writing ANSI through termion's raw mode to any
/// `Write + AsFd` target (unix only).
///
/// Constructing the backend **enables raw mode**; dropping it (or calling
/// [`TermionBackend::into_raw_terminal`]) restores the previous termios state.
/// The alternate screen is deliberately *not* entered here: it is two
/// control sequences (`ESC[?1049h` / `ESC[?1049l`), and the runner owns
/// that lifecycle.
///
/// # Examples
///
/// ```no_run
/// use tuvix_backend_termion::TermionBackend;
///
/// # fn demo() -> std::io::Result<()> {
/// let backend = TermionBackend::new(std::io::stdout())?;
/// // ... raw mode is on; Terminal::new(backend) drives it.
/// # Ok(())
/// # }
/// ```
pub struct TermionBackend<W: Write + AsFd> {
    inner: AnsiBackend<RawTerminal<W>>,
}

impl<W: Write + AsFd> std::fmt::Debug for TermionBackend<W> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // RawTerminal is not Debug (it owns termios state); report the
        // shape without the inner handle.
        f.debug_struct("TermionBackend").finish_non_exhaustive()
    }
}

impl<W: Write + AsFd> TermionBackend<W> {
    /// Creates the backend over `writer`, entering raw mode.
    ///
    /// # Errors
    ///
    /// Fails when the terminal size cannot be queried or termios cannot be
    /// switched to raw mode; the mode is rolled back before returning.
    pub fn new(writer: W) -> io::Result<Self> {
        let size = Self::query_size()?;
        match writer.into_raw_mode() {
            Ok(raw) => Ok(Self {
                inner: AnsiBackend::new(raw, size),
            }),
            Err(error) => Err(error),
        }
    }

    /// Returns the raw terminal, consuming the backend. Raw mode is
    /// restored when the returned [`RawTerminal`] drops.
    pub fn into_raw_terminal(self) -> RawTerminal<W> {
        self.inner.into_writer()
    }

    /// Queries the terminal size through termion.
    fn query_size() -> io::Result<Size> {
        let (width, height) = termion::terminal_size()?;
        Ok(Size::new(width, height))
    }
}

impl<W: Write + AsFd> Backend for TermionBackend<W> {
    type Error = io::Error;

    fn draw<'a>(
        &mut self,
        content: impl IntoIterator<Item = DiffRun<'a>>,
    ) -> Result<(), Self::Error> {
        self.inner.draw(content)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.inner.flush()
    }

    fn size(&self) -> Result<Size, Self::Error> {
        Self::query_size()
    }

    fn get_cursor_position(&self) -> Result<Position, Self::Error> {
        self.inner.get_cursor_position()
    }

    fn set_cursor_position(&mut self, position: Position) -> Result<(), Self::Error> {
        self.inner.set_cursor_position(position)
    }

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        self.inner.hide_cursor()
    }

    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        self.inner.show_cursor()
    }

    fn clear(&mut self, region: ClearRegion) -> Result<(), Self::Error> {
        self.inner.clear(region)
    }

    fn write_control(&mut self, sequence: &str) -> Result<(), Self::Error> {
        self.inner.write_control(sequence)
    }

    fn set_clipboard(&mut self, contents: &str) -> Result<(), Self::Error> {
        self.inner.set_clipboard(contents)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Termion without a TTY cannot switch modes; the composition contract
    /// here is that construction over a non-tty fails cleanly instead of
    /// corrupting state.
    #[test]
    fn raw_mode_over_a_non_tty_fails_cleanly() {
        // A connected socket pair is not a tty: into_raw_mode must fail.
        let (_read, write) = std::os::unix::net::UnixStream::pair().unwrap();
        let result = TermionBackend::new(write);
        assert!(result.is_err(), "a non-tty must reject raw mode");
    }
}
