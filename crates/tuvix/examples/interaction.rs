//! M11 interaction showcase: the `Scrollable` viewport, the `FocusRing`
//! and the `Keymap` working as one app. Keys:
//! - `Tab`/`Shift+Tab` move the focus ring (log pane, name, filter),
//! - `j`/`k` or arrows scroll the focused log viewport one line,
//!   `Ctrl+f`/`Ctrl+b` page it, the mouse wheel scrolls it,
//! - `g g` (a chord!) jumps to the top, `G` to the bottom,
//! - `Ctrl+q` quits — every binding lives in one `Keymap`,
//! - typing edits the focused input.
//!
//! Run with: cargo run -p tuvix --example interaction

use std::io::Result;
use std::time::{Duration, Instant};

use tuvix::prelude::*;
use tuvix::{CrosstermEvents, try_init};
use tuvix_widgets::{Input, InputState};

/// Which pane the focus ring is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pane {
    Log,
    Name,
    Filter,
}

/// What a resolved keymap binding means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Quit,
    FocusNext,
    FocusPrevious,
    ToTop,
    ToBottom,
    PageDown,
    PageUp,
}

struct App {
    keymap: Keymap<Command>,
    focus: FocusRing<Pane>,
    viewport: ViewportState<()>,
    lines: Vec<String>,
    name: InputState,
    filter: InputState,
    quit: bool,
}

fn main() -> Result<()> {
    main_app()
}

fn main_app() -> Result<()> {
    let mut runner = try_init()?;

    let (mut reader, mut events) = CrosstermEvents::spawn()?;
    let keymap = dashboard_keymap();
    let lines = generate_log(160);
    let content_width =
        u16::try_from(lines.iter().map(String::len).max().unwrap_or(1)).unwrap_or(u16::MAX);
    let content_height = u16::try_from(lines.len()).unwrap_or(u16::MAX);
    let mut app = App {
        keymap,
        focus: FocusRing::with_items([Pane::Log, Pane::Name, Pane::Filter]),
        viewport: ViewportState::new(()),
        lines,
        name: InputState::new(),
        filter: InputState::new(),
        quit: false,
    };
    app.viewport
        .set_content(Size::new(content_width, content_height));

    while !app.quit {
        runner.draw(|frame| draw_dashboard(frame, &mut app, content_height))?;
        pump_events(&mut app, &mut events);
    }

    reader.stop();
    runner.restore()?;
    Ok(())
}

fn dashboard_keymap() -> Keymap<Command> {
    let mut keymap = Keymap::new();
    keymap.bind(&[KeyPattern::new(KeyCode::Char('c')).ctrl()], Command::Quit);
    keymap.bind(&[KeyPattern::new(KeyCode::Tab)], Command::FocusNext);
    keymap.bind(&[KeyPattern::new(KeyCode::BackTab)], Command::FocusPrevious);
    // The chord: `g g` jumps to the top. `G` takes the bottom directly.
    keymap.bind(
        &[
            KeyPattern::new(KeyCode::Char('g')),
            KeyPattern::new(KeyCode::Char('g')),
        ],
        Command::ToTop,
    );
    keymap.bind(
        &[KeyPattern::new(KeyCode::Char('G')).shift()],
        Command::ToBottom,
    );
    keymap.bind(
        &[KeyPattern::new(KeyCode::Char('f')).ctrl()],
        Command::PageDown,
    );
    keymap.bind(
        &[KeyPattern::new(KeyCode::Char('b')).ctrl()],
        Command::PageUp,
    );
    keymap
}

/// The log pane's content widget: every line, at its full extent.
struct LogView<'a> {
    lines: &'a [String],
}

impl RenderWidget for LogView<'_> {
    type State = ();

    fn render(self, area: Rect, buf: &mut Buffer, _state: &mut ()) {
        for (index, line) in self.lines.iter().enumerate() {
            buf.set_string(
                area.x,
                area.y + u16::try_from(index).unwrap_or(u16::MAX),
                line,
                Style::new(),
            );
        }
    }
}

fn draw_dashboard(frame: &mut Frame, app: &mut App, content_height: u16) {
    let area = frame.area();
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(&area);
    let [log_area, side_area] =
        Layout::horizontal([Constraint::Min(0), Constraint::Length(34)]).areas(&body);
    let [name_row, filter_row] = Layout::vertical([Constraint::Length(3), Constraint::Length(3)])
        .flex(Flex::Start)
        .areas(&side_area);

    // Header: which pane has the ring.
    let title = match app.focus.current() {
        Some(Pane::Log) => "interaction — log pane (j/k scroll, gg top, G bottom)",
        Some(Pane::Name) => "interaction — name field",
        _ => "interaction — filter field",
    };
    frame
        .buffer
        .set_string(header.x, header.y, title, Style::new().bold());

    // The log viewport inside a border; the scrollbar rides the inner
    // edge. A frame that only scrolls blits the cached content.
    let log_block = Block::bordered().title("service log");
    let log_inner = log_block.inner(log_area);
    log_block.render(log_area, &mut *frame.buffer);
    let scroll = Scrollable::new(LogView { lines: &app.lines })
        .content(Size::new(
            u16::try_from(app.lines.iter().map(String::len).max().unwrap_or(1)).unwrap_or(u16::MAX),
            content_height,
        ))
        .scrollbar(Scrollbar::default());
    RenderWidget::render(scroll, log_inner, &mut *frame.buffer, &mut app.viewport);

    // The two inputs, highlighted when focused.
    let panes: [(Pane, &str, Rect, &mut InputState); 2] = [
        (Pane::Name, "name", name_row, &mut app.name),
        (Pane::Filter, "filter", filter_row, &mut app.filter),
    ];
    for (pane, title, row, state) in panes {
        let mut block = Block::bordered().title(title);
        if app.focus.is_focused(&pane) {
            block = block.border_style(Style::new().fg(Color::Cyan).bold());
        }
        RenderWidget::render(
            Input::new().placeholder("type here").block(block),
            row,
            &mut *frame.buffer,
            state,
        );
    }

    // Footer: the pending chord indicator plus a scroll ratio gauge.
    let mut hints = String::from("Ctrl+q quit · Tab focus · gg/G jumps ");
    if app.keymap.is_pending() {
        hints.push_str("· chord: g…");
    }
    frame
        .buffer
        .set_string(footer.x, footer.y, hints, Style::new().dim());
    let max_offset = content_height.saturating_sub(log_inner.height).max(1);
    let ratio = f64::from(app.viewport.offset_y()) / f64::from(max_offset);
    let gauge = LineGauge::default()
        .ratio(ratio.clamp(0.0, 1.0))
        .label(String::from("scroll"))
        .style(Style::new().fg(Color::Cyan));
    Widget::render(gauge, footer, &mut *frame.buffer);
}

/// Event pump: the keymap resolves first; whatever it misses feeds the
/// focused pane.
fn pump_events(app: &mut App, events: &mut tuvix_core::runtime::queue::EventStream) {
    let deadline = Instant::now() + Duration::from_millis(120);
    while Instant::now() < deadline {
        match events.try_next() {
            Some(Event::Key(key)) => match app.keymap.step(&key) {
                KeymapOutcome::Matched(command) => dispatch(app, command),
                KeymapOutcome::Pending => {}
                KeymapOutcome::Miss => {
                    // The keymap missed: editing keys feed the focused pane.
                    if key.kind == KeyEventKind::Press {
                        apply_missed(app, key);
                    }
                }
            },
            Some(Event::Mouse(mouse)) => {
                if app.focus.is_focused(&Pane::Log) {
                    app.viewport.apply_wheel(&mouse);
                }
            }
            Some(_) => {}
            None => break,
        }
    }
}

fn dispatch(app: &mut App, command: Command) {
    match command {
        Command::Quit => app.quit = true,
        Command::FocusNext => {
            app.focus.advance();
        }
        Command::FocusPrevious => {
            app.focus.retreat();
        }
        Command::ToTop => {
            app.viewport.to_top();
        }
        Command::ToBottom => {
            app.viewport.to_bottom();
        }
        Command::PageDown => {
            let page = app.viewport.viewport().height.max(1);
            app.viewport.scroll_down(page);
        }
        Command::PageUp => {
            let page = app.viewport.viewport().height.max(1);
            app.viewport.scroll_up(page);
        }
    }
}

/// Keys the keymap missed: scroll keys for the log pane, editing keys for
/// the inputs.
fn apply_missed(app: &mut App, key: KeyEvent) {
    let modifiers = key.modifiers;
    match key.code {
        KeyCode::Down | KeyCode::Char('j') => {
            if app.focus.is_focused(&Pane::Log) {
                app.viewport.scroll_down(1);
            }
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if app.focus.is_focused(&Pane::Log) {
                app.viewport.scroll_up(1);
            }
        }
        _ => match app.focus.current() {
            Some(Pane::Name) => edit(&mut app.name, key, modifiers),
            Some(Pane::Filter) => edit(&mut app.filter, key, modifiers),
            _ => {}
        },
    }
}

fn edit(state: &mut InputState, key: KeyEvent, modifiers: KeyModifiers) {
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

/// A deterministic fake service log.
fn generate_log(rows: usize) -> Vec<String> {
    (0..rows)
        .map(|row| {
            let level = match row % 11 {
                0 => "WARN ",
                3 => "ERROR",
                _ => "INFO ",
            };
            format!(
                "2026-11-05 14:{row:02}:{:02} {level} worker-{} request={} served in {}ms",
                row * 7 % 60,
                row % 5,
                1000 + row,
                (row * 13) % 120
            )
        })
        .collect()
}
