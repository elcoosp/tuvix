//! The dependency-free ANSI backend for the tuvix terminal UI framework.
//!
//! [`AnsiBackend`] writes the same persistent raster-state stream the
//! crossterm backend produces — memoized SGR, CHA cursor economy, EL
//! elision, OSC 8 hyperlinks — straight into any [`std::io::Write`] target,
//! with **no terminal library in between**. It is the "simple" platform
//! backend of the M5 roadmap: the renderer every platform can have, and
//! the building block platform-specific crates (termion) compose with.
//!
//! Because it owns no TTY, it never queries the terminal: the host
//! supplies the size at construction (and on resize), and cursor
//! positions are the backend's own virtual state. Pair it with any input
//! source — the runtime, a platform backend's reader, or tests.
//!
//! # Examples
//!
//! ```
//! use tuvix_backend_ansi::AnsiBackend;
//! use tuvix_core::geometry::Size;
//!
//! // Any `Write` target works; a `Vec<u8>` captures the ANSI stream.
//! let backend = AnsiBackend::new(Vec::<u8>::new(), Size::new(80, 24));
//! ```

pub mod backend;

pub use backend::AnsiBackend;
