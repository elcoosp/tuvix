//! The M5 "simple" backend in action: the dependency-free ANSI renderer
//! painting a demo screen (weighted layout, styled text, an OSC 8
//! hyperlink) straight to stdout — no terminal library involved.
//!
//! This example does not touch terminal modes; it just writes a frame and
//! exits, so it is safe to run anywhere:
//!
//! Run with: cargo run -p tuvix --example simple_backend

use std::io::{self, Write};

use tuvix::prelude::*;

fn main() -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    // Ask the terminal for its size the cheap way: assume 80x24 (or read
    // $COLUMNS/$LINES in a real app; the backend itself never queries).
    let size = Size::new(80, 24);
    let mut backend = AnsiBackend::new(&mut stdout, size);

    let area = Rect::from_size(size);
    let [header, sidebar, main] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Weight(1),
        Constraint::Weight(3),
    ])
    .areas(&area);

    backend.draw([])?; // establish the raster state machine
    backend.clear(ClearRegion::All)?;

    let mut frame = Buffer::empty(area);
    frame.set_string(
        1,
        header.y,
        "tuvix — simple ANSI backend",
        Style::new().bold(),
    );
    frame.set_string(1, sidebar.y + 1, "sidebar", Style::new().fg(Color::Cyan));
    frame.set_string(1, sidebar.y + 2, "(weight 1)", Style::new().dim());
    frame.set_string(1, main.y + 1, "main pane", Style::new().fg(Color::Green));
    frame.set_string(1, main.y + 2, "(weight 3)", Style::new().dim());
    frame.set_string_with_hyperlink(
        1,
        main.y + 4,
        "https://tuvix.dev",
        Style::new().fg(Color::Blue).underlined(),
        Hyperlink::new("https://tuvix.dev"),
    );

    let blank = Buffer::empty(area);
    let runs: Vec<DiffRun<'_>> = blank.diff_runs(&frame).collect();
    backend.draw(runs)?;
    backend.flush()?;
    stdout.write_all(b"\r\n")?;
    stdout.flush()?;
    Ok(())
}
