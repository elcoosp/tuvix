//! Termion backend for the tuvix terminal UI framework (unix platforms).
//!
//! The M5 roadmap's platform backend beyond crossterm: [`TermionBackend`]
//! drives the dependency-free ANSI raster machine over termion's raw mode
//! (termios, restored on drop), and [`TermionEvents`] pumps termion input
//! through the lossless [`to_tuvix_event`] bridge into the async event
//! stream — the whole M2 runtime for free, exactly as the architecture
//! decision promises for any new backend.
//!
//! **Platform support**: Linux, macOS and other unix terminals (everything
//! termion supports). The crate compiles to an empty library on non-unix
//! targets so workspaces including it stay portable; gate termion code
//! with `#[cfg(unix)]`.
//!
//! **Input coverage** follows termion: no focus events, no bracketed
//! paste, and mouse releases carry no button (SGR/X10 releases are
//! button-less) — mapped to [`MouseEventKind::Up`](tuvix_core::event::MouseEventKind::Up) with the left button
//! and documented. `Unsupported` byte sequences are dropped.
//!
//! # Examples
//!
//! ```no_run
//! # #[cfg(unix)] fn demo() -> std::io::Result<()> {
//! use tuvix_backend_termion::{TermionBackend, TermionEvents};
//! use tuvix_core::geometry::Size;
//! use tuvix_core::terminal::Terminal;
//!
//! // Raw mode is entered on construction and restored on drop.
//! let backend = TermionBackend::new(std::io::stdout())?;
//! let mut terminal = Terminal::new(backend)?;
//! let (_reader, mut events) = TermionEvents::spawn()?;
//! while let Some(event) = events.try_next() {
//!     std::hint::black_box(event);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! On non-unix targets the example body is not compiled; the crate is
//! empty there.

#[cfg(unix)]
pub mod backend;
#[cfg(unix)]
pub mod event;
#[cfg(unix)]
pub mod events;

#[cfg(unix)]
pub use backend::TermionBackend;
#[cfg(unix)]
pub use event::to_tuvix_event;
#[cfg(unix)]
pub use events::TermionEvents;
