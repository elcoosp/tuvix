//! M14 mouse showcase: the `HitMap`, the `GestureRecognizer` and the
//! `MouseMap` working as one app. The pointer:
//! - click the log to focus it, the wheel scrolls it (anywhere),
//! - drag the floating panel by its title bar — it follows the pointer,
//! - double-click the title bar to pin (drag off) or unpin (drag on),
//! - click `+1` / `clear` inside the panel, click empty space for a
//!   status flash,
//! - `Ctrl+q` quits, `p` toggles the pin from the keyboard.
//!
//! Every binding lives in one `MouseMap`; the panel is dragged live from
//! the drag origin, the way the recognizer reports it.
//!
//! Run with: cargo run -p tuvix --example mouse

use std::io::Result;
use std::time::Instant;

use tuvix::prelude::*;
use tuvix::{CrosstermEvents, try_init};

/// The clickable surfaces of the dashboard, in paint order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Region {
    Log,
    Panel,
    Header,
    ButtonPlus,
    ButtonClear,
}

/// What a resolved binding (key or mouse) means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Quit,
    TogglePin,
    FocusLog,
    Scroll,
    PressPlus,
    PressClear,
    DragHeader,
    EmptyClick,
}

struct App {
    keymap: Keymap<Command>,
    mousemap: MouseMap<Region, Command>,
    gestures: GestureRecognizer,
    hits: HitMap<Region>,
    viewport: ViewportState<()>,
    lines: Vec<String>,
    panel: Rect,
    pinned: bool,
    drag_start: Option<Rect>,
    plus_count: u32,
    status: String,
    quit: bool,
}

fn main() -> Result<()> {
    let mut runner = try_init()?;
    runner.push_input_mode(InputMode::MouseCapture)?;

    let (mut reader, mut events) = CrosstermEvents::spawn()?;
    let lines = generate_log(120);
    let content_height = u16::try_from(lines.len()).unwrap_or(u16::MAX);
    let mut app = App {
        keymap: key_bindings(),
        mousemap: mouse_bindings(),
        gestures: GestureRecognizer::new(GestureConfig::default()),
        hits: HitMap::new(),
        viewport: ViewportState::new(()),
        lines,
        panel: Rect::new(38, 4, 26, 6),
        pinned: false,
        drag_start: None,
        plus_count: 0,
        status: String::from("try the pointer: drag, double-click, scroll"),
        quit: false,
    };
    app.viewport.set_content(Size::new(64, content_height));

    let started = Instant::now();
    while !app.quit {
        runner.draw(|frame| draw_dashboard(frame, &mut app))?;
        pump_events(&mut app, &mut events, started.elapsed());
    }

    reader.stop();
    runner.restore()?;
    Ok(())
}

fn key_bindings() -> Keymap<Command> {
    let mut keymap = Keymap::new();
    keymap.bind(&[KeyPattern::new(KeyCode::Char('c')).ctrl()], Command::Quit);
    keymap.bind(&[KeyPattern::new(KeyCode::Char('p'))], Command::TogglePin);
    keymap
}

fn mouse_bindings() -> MouseMap<Region, Command> {
    let mut map = MouseMap::new();
    // The catch-alls bind first; region bindings shadow them by recency.
    map.bind(MousePattern::anywhere(MouseClass::Scroll), Command::Scroll);
    map.bind(
        MousePattern::anywhere(MouseClass::Click),
        Command::EmptyClick,
    );
    map.bind(
        MousePattern::on(Region::Log, MouseClass::Click),
        Command::FocusLog,
    );
    map.bind(
        MousePattern::on(Region::Header, MouseClass::DoubleClick),
        Command::TogglePin,
    );
    map.bind(
        MousePattern::on(Region::Header, MouseClass::DragStart),
        Command::DragHeader,
    );
    map.bind(
        MousePattern::on(Region::Header, MouseClass::Drag),
        Command::DragHeader,
    );
    map.bind(
        MousePattern::on(Region::Header, MouseClass::DragEnd),
        Command::DragHeader,
    );
    map.bind(
        MousePattern::on(Region::ButtonPlus, MouseClass::AnyClick),
        Command::PressPlus,
    );
    map.bind(
        MousePattern::on(Region::ButtonClear, MouseClass::AnyClick),
        Command::PressClear,
    );
    map
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

fn draw_dashboard(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(&area);

    frame.buffer.set_string(
        header.x,
        header.y,
        "mouse — hit map · gestures · mouse map",
        Style::new().bold(),
    );

    // The log pane paints first; the floating panel paints after it and
    // registers after it, so the same z-order resolves both paint and
    // hit-testing — one painter's algorithm.
    let log_block = Block::bordered().title("service log");
    let log_inner = log_block.inner(body);
    log_block.render(body, &mut *frame.buffer);
    app.hits.push(Region::Log, log_inner);
    let scroll = Scrollable::new(LogView { lines: &app.lines })
        .content(Size::new(
            64,
            u16::try_from(app.lines.len()).unwrap_or(u16::MAX),
        ))
        .scrollbar(Scrollbar::default());
    RenderWidget::render(scroll, log_inner, &mut *frame.buffer, &mut app.viewport);

    // The floating panel, clamped to the frame.
    let panel = clamp_to(app.panel, area);
    Clear.render(panel, &mut *frame.buffer);
    let pinned_note = if app.pinned { " [pinned]" } else { "" };
    let panel_block = Block::bordered()
        .title(format!("tool panel{pinned_note}"))
        .border_style(if app.pinned {
            Style::new().fg(Color::Cyan)
        } else {
            Style::new()
        });
    panel_block.render(panel, &mut *frame.buffer);
    app.hits.push(Region::Panel, panel);

    let header_row = Rect::new(panel.x + 1, panel.y + 1, panel.width.saturating_sub(2), 1);
    frame.buffer.set_string(
        header_row.x,
        header_row.y,
        "drag me · double-click pins",
        Style::new().fg(Color::Yellow),
    );
    app.hits.push(Region::Header, header_row);

    let plus_row = Rect::new(panel.x + 2, panel.y + 3, 9, 1);
    let clear_row = Rect::new(panel.x + 13, panel.y + 3, 9, 1);
    frame.buffer.set_string(
        plus_row.x,
        plus_row.y,
        "[ +1 ]",
        Style::new().fg(Color::Green).bold(),
    );
    frame.buffer.set_string(
        clear_row.x,
        clear_row.y,
        "[ clr ]",
        Style::new().fg(Color::Red).bold(),
    );
    app.hits.push(Region::ButtonPlus, plus_row);
    app.hits.push(Region::ButtonClear, clear_row);

    frame
        .buffer
        .set_string(footer.x, footer.y, app.status.as_str(), Style::new().dim());
}

fn clamp_to(rect: Rect, bounds: Rect) -> Rect {
    let max_x = bounds.right().saturating_sub(rect.width);
    let max_y = bounds.bottom().saturating_sub(rect.height);
    Rect::new(
        rect.x.min(max_x),
        rect.y.min(max_y),
        rect.width,
        rect.height,
    )
}

/// Event pump: keys through the keymap, mouse through classify ->
/// hit-test -> resolve.
fn pump_events(app: &mut App, events: &mut EventStream, at: std::time::Duration) {
    let deadline = Instant::now() + std::time::Duration::from_millis(120);
    while Instant::now() < deadline {
        match events.try_next() {
            Some(Event::Key(key)) => {
                if let KeymapOutcome::Matched(command) = app.keymap.step(&key) {
                    dispatch_key(app, command);
                }
            }
            Some(Event::Mouse(mouse)) => {
                if let Some(gesture) = app.gestures.feed(mouse, at) {
                    let hit = app.hits.hit_test(anchor(&gesture));
                    if let Some(command) = app.mousemap.resolve(hit, &gesture) {
                        dispatch_gesture(app, command, &gesture);
                    }
                }
            }
            Some(_) => {}
            None => break,
        }
    }
}

fn dispatch_key(app: &mut App, command: Command) {
    match command {
        Command::Quit => app.quit = true,
        Command::TogglePin => {
            app.pinned = !app.pinned;
            app.status = if app.pinned {
                String::from("panel pinned (keyboard)")
            } else {
                String::from("panel free to drag")
            };
        }
        _ => {}
    }
}

fn dispatch_gesture(app: &mut App, command: Command, gesture: &Gesture) {
    match command {
        Command::Quit | Command::TogglePin => dispatch_key(app, command),
        Command::FocusLog => app.status = String::from("log focused (click)"),
        Command::Scroll => match *gesture {
            Gesture::Scroll {
                kind: ScrollKind::Up,
                ..
            } => {
                app.viewport.scroll_up(1);
                app.status = String::from("log scrolled up (wheel)");
            }
            Gesture::Scroll {
                kind: ScrollKind::Down,
                ..
            } => {
                app.viewport.scroll_down(1);
                app.status = String::from("log scrolled down (wheel)");
            }
            _ => app.status = String::from("horizontal wheel — nothing to pan"),
        },
        Command::PressPlus => {
            app.plus_count = app.plus_count.saturating_add(1);
            app.status = format!("plus clicked {} times", app.plus_count);
        }
        Command::PressClear => {
            app.plus_count = 0;
            app.status = String::from("counter cleared");
        }
        Command::DragHeader => match *gesture {
            Gesture::DragStart { origin, .. } => {
                let panel = clamp_to(app.panel, Rect::new(0, 1, u16::MAX, u16::MAX));
                app.drag_start = Some(panel);
                app.status = format!("drag started at {origin}");
            }
            Gesture::Drag {
                origin, position, ..
            } => {
                if let Some(start) = app.drag_start {
                    app.panel.x = start.x.saturating_add(position.x.saturating_sub(origin.x));
                    app.panel.y = start.y.saturating_add(position.y.saturating_sub(origin.y));
                }
            }
            Gesture::DragEnd {
                origin, position, ..
            } => {
                app.drag_start = None;
                app.status = format!("dropped at {position} (from {origin})");
            }
            _ => {}
        },
        Command::EmptyClick => app.status = String::from("clicked empty space"),
    }
}

fn generate_log(count: usize) -> Vec<String> {
    (0..count)
        .map(|index| {
            format!(
                "boot {index:03}: service {} healthy — the quick brown fox",
                index % 7
            )
        })
        .collect()
}
