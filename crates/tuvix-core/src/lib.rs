//! Core of the tuvix terminal UI framework.
//!
//! This crate is dependency-light and I/O-free: it contains the geometry,
//! styling, text, buffer, layout, event and terminal-runtime primitives that
//! every tuvix backend and widget builds upon.
//!
//! # Rendering pipeline
//!
//! ```text
//! widgets -> Buffer -> diff_runs() -> DiffRun slices -> Backend -> terminal
//! ```
//!
//! See `docs/ARCHITECTURE.md` in the repository for the full design.

pub mod backend;
pub mod buffer;
pub mod cancellation;
pub mod clipboard;
pub mod damage;
pub mod event;
pub mod focus;
pub mod geometry;
pub mod journal;
pub mod keymap;
pub mod layout;
pub mod markup;
pub mod preview;
pub mod replay;
pub mod runtime;
pub mod scheduler;
pub mod screen;
pub mod session;
pub mod style;
pub mod stylize;
pub mod terminal;
pub mod text;
pub mod theme;
pub mod widget;
