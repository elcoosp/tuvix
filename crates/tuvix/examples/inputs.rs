//! M6 interaction showcase: Tabs, Input fields, LineGauge and the
//! Clear + Rect::centered popup pattern over Flex layouts. Keys:
//! - `Tab`/`Shift+Tab` switch the focused field,
//! - arrows move the cursor, `Ctrl+Left`/`Ctrl+Right` jump by words,
//!   `Home`/`End` jump to the edges, `Backspace`/`Delete` edit, every
//!   other character types,
//! - `Enter` opens a centered confirmation popup (any key closes it),
//! - `Ctrl+q` quits.
//!
//! Run with: cargo run -p tuvix --example inputs

use std::io::Result;
use std::time::Duration;

use tuvix::prelude::*;
use tuvix::{CrosstermEvents, try_init};
use tuvix_widgets::{Input, InputState, LineGauge, Tabs};

/// One editable form field: title, placeholder hint and editing state.
struct Field {
    title: &'static str,
    placeholder: &'static str,
    state: InputState,
}

struct App {
    focused: usize,
    fields: Vec<Field>,
    popup: Option<&'static str>,
    quit: bool,
}

fn main() -> Result<()> {
    main_app()
}

fn main_app() -> Result<()> {
    let mut runner = try_init()?;

    let (mut reader, mut events) = CrosstermEvents::spawn()?;
    let mut app = App {
        focused: 0,
        fields: vec![
            Field {
                title: "name",
                placeholder: "your name",
                state: InputState::new(),
            },
            Field {
                title: "endpoint",
                placeholder: "https://example.dev",
                state: InputState::new(),
            },
            Field {
                title: "token",
                placeholder: "paste a token",
                state: InputState::new(),
            },
        ],
        popup: None,
        quit: false,
    };

    while !app.quit {
        runner.draw(|frame| draw_form(frame, &mut app))?;
        handle_events(&mut app, &mut events);
    }

    reader.stop();
    runner.restore()?;
    Ok(())
}

/// One frame: tab row, the three fields, a completion gauge, and the
/// popup when open.
fn draw_form(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let [header, _body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(&area);

    // The tab row centers itself over the header.
    let tabs = Tabs::new(["Name", "Endpoint", "Token"])
        .select(app.focused)
        .highlight_style(Style::new().fg(Color::Black).bg(Color::Cyan).bold());
    Widget::render(tabs, header, &mut *frame.buffer);

    // The three fields share a centered column: Flex::SpaceEvenly
    // distributes the leftover rows around them.
    let column = area.centered(area.width.saturating_sub(4).min(64), 9);
    let [name_row, endpoint_row, token_row] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Length(3),
    ])
    .flex(Flex::SpaceEvenly)
    .areas(&column);

    let focus_ring = Style::new().fg(Color::Cyan);
    let field_rows = [name_row, endpoint_row, token_row];
    for (index, field) in app.fields.iter_mut().enumerate() {
        let mut block = Block::bordered().title(field.title);
        if index == app.focused {
            block = block.border_style(focus_ring);
        }
        let input = Input::new().placeholder(field.placeholder).block(block);
        RenderWidget::render(
            input,
            field_rows[index],
            &mut *frame.buffer,
            &mut field.state,
        );
    }

    // Footer: how full the focused field is, as a one-line gauge.
    let focused_field = &app.fields[app.focused];
    let filled = f64::from(focused_field.state.value().chars().count() as u16) / 24.0;
    let gauge = LineGauge::default()
        .ratio(filled)
        .label(focused_field.title.to_owned())
        .style(Style::new().fg(Color::Cyan));
    Widget::render(gauge, footer, &mut *frame.buffer);

    // The popup pattern: a centered rect, cleared, then painted over.
    if let Some(message) = app.popup {
        let popup_area = area.centered(44, 5);
        Clear.render(popup_area, &mut *frame.buffer);
        let block = Block::bordered()
            .title("confirm")
            .border_style(Style::new().fg(Color::Yellow));
        block.render(popup_area, &mut *frame.buffer);
        frame
            .buffer
            .set_string(popup_area.x + 2, popup_area.y + 2, message, Style::new());
    }
}

/// Event pump for the form.
fn handle_events(app: &mut App, events: &mut tuvix_core::runtime::queue::EventStream) {
    let deadline = std::time::Instant::now() + Duration::from_millis(120);
    while std::time::Instant::now() < deadline {
        match events.try_next() {
            Some(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                if app.popup.is_some() {
                    app.popup = None;
                    continue;
                }
                let modifiers = key.modifiers;
                match key.code {
                    KeyCode::Char('q') if modifiers.contains(KeyModifiers::CONTROL) => {
                        app.quit = true;
                    }
                    KeyCode::Tab => app.focused = (app.focused + 1) % app.fields.len(),
                    KeyCode::BackTab => {
                        app.focused = (app.focused + app.fields.len() - 1) % app.fields.len();
                    }
                    KeyCode::Enter => {
                        app.popup = Some("Form accepted (any key closes this popup)");
                    }
                    _ => {
                        let state = &mut app.fields[app.focused].state;
                        apply_editing(key, modifiers, state);
                    }
                }
            }
            Some(_) => {}
            None => break,
        }
    }
}

/// Editing keys feed the focused `InputState`; everything else types.
fn apply_editing(key: KeyEvent, modifiers: KeyModifiers, state: &mut InputState) {
    match key.code {
        KeyCode::Backspace => state.delete_backward(),
        KeyCode::Delete => state.delete_forward(),
        KeyCode::Left if modifiers.contains(KeyModifiers::CONTROL) => state.move_word_left(),
        KeyCode::Right if modifiers.contains(KeyModifiers::CONTROL) => state.move_word_right(),
        KeyCode::Left => state.move_left(),
        KeyCode::Right => state.move_right(),
        KeyCode::Home => state.move_home(),
        KeyCode::End => state.move_end(),
        KeyCode::Char(character) => state.insert(character),
        _ => {}
    }
}
