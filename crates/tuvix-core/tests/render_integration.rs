//! End-to-end rendering scenarios through `Terminal` + `TestBackend`,
//! mirroring how real applications drive the stack.

use tuvix_core::backend::TestBackend;
use tuvix_core::geometry::{Position, Rect, Size};
use tuvix_core::layout::{Constraint, Layout};
use tuvix_core::style::{Color, Modifier, Style};
use tuvix_core::terminal::Terminal;

#[test]
fn dashboard_layout_and_frames_flow() {
    let mut terminal = Terminal::new(TestBackend::new(Size::new(40, 10))).unwrap();

    // Frame 1: header + body via the layout solver, styled text.
    terminal
        .draw(|frame| {
            let [header, body] =
                Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(&frame.area());
            frame.buffer.set_string(
                header.x,
                header.y,
                "tuvix dashboard",
                Style::new().fg(Color::Cyan),
            );
            frame
                .buffer
                .set_style(header, Style::new().add_modifier(Modifier::BOLD));
            frame.buffer.set_string(
                body.x,
                body.y,
                "hello from tuvix",
                Style::new().fg(Color::Green),
            );
        })
        .unwrap();

    let rendered = terminal.backend().buffer().to_string();
    let rows: Vec<&str> = rendered.lines().collect();
    assert_eq!(rows[0], "tuvix dashboard".to_string() + &" ".repeat(25));
    assert_eq!(rows[1], "hello from tuvix".to_string() + &" ".repeat(24));

    // Frame 2: cursor request shows the cursor at a position.
    terminal
        .draw(|frame| {
            frame.set_cursor_position(Position::new(16, 0));
        })
        .unwrap();
    assert_eq!(terminal.backend().cursor_position(), Position::new(16, 0));
    assert!(!terminal.backend().is_cursor_hidden());
}

#[test]
fn resizing_terminal_repaints_fully() {
    let mut terminal = Terminal::new(TestBackend::new(Size::new(10, 2))).unwrap();
    terminal
        .draw(|frame| {
            frame.buffer.set_string(0, 0, "wide content!", Style::new());
        })
        .unwrap();

    terminal.backend_mut().resize(Size::new(20, 2));
    terminal
        .draw(|frame| {
            frame
                .buffer
                .set_string(0, 0, "wider content!!", Style::new());
        })
        .unwrap();

    let rendered = terminal.backend().buffer().to_string();
    let rows: Vec<&str> = rendered.lines().collect();
    assert_eq!(rows[0], "wider content!!     ");
    assert_eq!(rows[1], "                    ");
}

#[test]
fn style_only_frames_repaint_styles_without_touching_symbols() {
    let mut terminal = Terminal::new(TestBackend::new(Size::new(8, 1))).unwrap();
    terminal
        .draw(|frame| {
            frame.buffer.set_string(0, 0, "styled", Style::new());
        })
        .unwrap();
    terminal
        .draw(|frame| {
            frame
                .buffer
                .set_string(0, 0, "styled", Style::new().fg(Color::Red));
            frame.buffer.set_style(
                Rect::new(0, 0, 8, 1),
                Style::new().add_modifier(Modifier::BOLD),
            );
        })
        .unwrap();

    let visible = terminal.backend().buffer();
    let cell = visible.get(0, 0).unwrap();
    assert_eq!(cell.symbol, "s");
    assert_eq!(cell.style.fg, Some(Color::Red));
    assert!(cell.style.add_modifier.contains(Modifier::BOLD));
}
