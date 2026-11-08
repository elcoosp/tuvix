//! M15 form showcase: the selection family — `Checkbox`, `RadioGroup`,
//! `Switch` and the `Select` dropdown — wired through the interaction
//! stack. The keyboard routes through a `FocusRing` over the app's own
//! panes (M11) and a `Keymap`; the pointer routes through the M14 stack
//! (`HitMap` + `GestureRecognizer` + `MouseMap`), registering regions
//! exactly where the controls painted. The widgets stay pure paint
//! functions: every toggle and every choice is the app's state.
//!
//! Keys:
//! - `Tab`/`Shift+Tab` move the focus ring, `Space`/`Enter` toggle or
//!   open the focused control, arrows navigate the radio group and the
//!   open dropdown, `Esc` dismisses the dropdown, `Ctrl+q` quits.
//!
//! Mouse: click a checkbox or the switch to toggle it, click a protocol
//! row to pick it, click the region trigger to open the dropdown and an
//! option to commit it.
//!
//! Run with: cargo run -p tuvix --example forms

use std::io::Result;
use std::time::Duration;

use tuvix::prelude::*;
use tuvix::{CrosstermEvents, try_init};
use tuvix_widgets::{Checkbox, RadioGroup, Select, SelectState, Switch};

/// One focusable control on the settings screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pane {
    AutoSave,
    Telemetry,
    Protocol,
    DarkMode,
    Region,
}

/// Where the pointer can land; registered during paint, M14 style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Region {
    AutoSave,
    Telemetry,
    Protocol,
    DarkMode,
    Trigger,
    Choice(usize),
}

/// What a key or a resolved click means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Quit,
    FocusNext,
    FocusPrev,
    ToggleAutoSave,
    ToggleTelemetry,
    ToggleDarkMode,
    PickProtocol,
    ToggleRegionList,
    PickRegion(usize),
}

/// The three boolean settings the form edits; grouped so the app struct
/// carries one record instead of a field per flag.
struct Settings {
    auto_save: bool,
    telemetry: bool,
    dark_mode: bool,
}

struct App {
    focus: FocusRing<Pane>,
    keymap: Keymap<Command>,
    mousemap: MouseMap<Region, Command>,
    gestures: GestureRecognizer,
    hits: HitMap<Region>,
    settings: Settings,
    protocol: usize,
    region: SelectState,
    protocols: [&'static str; 3],
    regions: [&'static str; 4],
    status: String,
    quit: bool,
}

fn main() -> Result<()> {
    let mut runner = try_init()?;
    runner.push_input_mode(InputMode::MouseCapture)?;

    let (mut reader, mut events) = CrosstermEvents::spawn()?;
    let mut app = App {
        focus: FocusRing::with_items([
            Pane::AutoSave,
            Pane::Telemetry,
            Pane::Protocol,
            Pane::DarkMode,
            Pane::Region,
        ]),
        keymap: key_bindings(),
        mousemap: mouse_bindings(),
        gestures: GestureRecognizer::new(GestureConfig::default()),
        hits: HitMap::new(),
        settings: Settings {
            auto_save: true,
            telemetry: false,
            dark_mode: false,
        },
        protocol: 1,
        region: SelectState::new(),
        protocols: ["rsa", "ed25519", "hybrid"],
        regions: ["eu-west", "eu-north", "us-east", "ap-south"],
        status: String::from("Tab to move, Space to toggle, q inside the dropdown picks"),
        quit: false,
    };

    while !app.quit {
        runner.draw(|frame| draw_form(frame, &mut app))?;
        pump_events(&mut app, &mut events);
    }

    reader.stop();
    runner.restore()?;
    Ok(())
}

fn key_bindings() -> Keymap<Command> {
    let mut keymap = Keymap::new();
    keymap.bind(&[KeyPattern::new(KeyCode::Char('q')).ctrl()], Command::Quit);
    keymap.bind(&[KeyPattern::new(KeyCode::Tab)], Command::FocusNext);
    keymap.bind(&[KeyPattern::new(KeyCode::BackTab)], Command::FocusPrev);
    keymap
}

fn mouse_bindings() -> MouseMap<Region, Command> {
    let mut map = MouseMap::new();
    // Later bindings shadow earlier ones; every region gets exactly one
    // click binding, so order only matters for overlaps (none here).
    map.bind(
        MousePattern::on(Region::AutoSave, MouseClass::Click),
        Command::ToggleAutoSave,
    );
    map.bind(
        MousePattern::on(Region::Telemetry, MouseClass::Click),
        Command::ToggleTelemetry,
    );
    map.bind(
        MousePattern::on(Region::Protocol, MouseClass::Click),
        Command::PickProtocol,
    );
    map.bind(
        MousePattern::on(Region::DarkMode, MouseClass::Click),
        Command::ToggleDarkMode,
    );
    map.bind(
        MousePattern::on(Region::Trigger, MouseClass::Click),
        Command::ToggleRegionList,
    );
    for index in 0..4 {
        map.bind(
            MousePattern::on(Region::Choice(index), MouseClass::Click),
            Command::PickRegion(index),
        );
    }
    map
}

/// One frame: the settings column, focus accents, hit registration.
fn draw_form(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let buffer: &mut Buffer = &mut *frame.buffer;
    buffer.set_string(
        area.x + 1,
        area.y,
        "tuvix settings — the M15 selection family",
        Style::new().bold(),
    );

    // The dropdown grows when it opens: the trigger row plus four
    // option rows, recomputed per frame the immediate-mode way.
    let region_height = if app.region.is_open() { 5 } else { 1 };
    let [_header, body, _status] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(&area);

    let [auto_save, telemetry, protocol, dark_mode, region] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(5),
        Constraint::Length(1),
        Constraint::Length(region_height),
    ])
    .flex(Flex::Start)
    .areas(&body);

    let body_x = body.x + 2;
    let accent = Style::new().fg(Color::Cyan);
    let rows = [auto_save, telemetry, protocol, dark_mode, region];
    let focused = |pane: Pane| app.focus.is_focused(&pane);
    // Shift every row into the centered column.
    let rows: Vec<Rect> = rows
        .iter()
        .map(|row| Rect::new(body_x, row.y, row.width.saturating_sub(4), row.height))
        .collect();

    let mut checkbox = Checkbox::new("auto-save every edit").checked(app.settings.auto_save);
    if focused(Pane::AutoSave) {
        checkbox = checkbox.style(accent);
    }
    Widget::render(checkbox, rows[0], buffer);

    let mut checkbox = Checkbox::new("share anonymous telemetry").checked(app.settings.telemetry);
    if focused(Pane::Telemetry) {
        checkbox = checkbox.style(accent);
    }
    Widget::render(checkbox, rows[1], buffer);

    let mut radio = RadioGroup::new(app.protocols).select(Some(app.protocol));
    if focused(Pane::Protocol) {
        radio = radio.style(accent);
    }
    Widget::render(radio, rows[2], buffer);

    let mut switch = Switch::new("dark mode").on(app.settings.dark_mode);
    if focused(Pane::DarkMode) {
        switch = switch.style(accent);
    }
    Widget::render(switch, rows[3], buffer);

    let mut select = Select::new(app.regions)
        .label("region")
        .placeholder("pick one");
    if focused(Pane::Region) {
        select = select.style(accent);
    }
    select.render(rows[4], buffer, &mut app.region);

    // The hit map mirrors the paint, top row first.
    app.hits.clear();
    app.hits.push(Region::AutoSave, rows[0]);
    app.hits.push(Region::Telemetry, rows[1]);
    app.hits.push(Region::Protocol, rows[2]);
    app.hits.push(Region::DarkMode, rows[3]);
    app.hits.push(Region::Trigger, rows[4]);
    if app.region.is_open() {
        for (index, _) in app.regions.iter().enumerate() {
            let option_row = Rect::new(
                rows[4].x,
                rows[4].y + 1 + u16::try_from(index).unwrap_or(u16::MAX),
                rows[4].width,
                1,
            );
            app.hits.push(Region::Choice(index), option_row);
        }
    }

    buffer.set_string(
        area.x + 1,
        area.y + area.height - 1,
        &app.status,
        Style::new(),
    );
}

/// The event pump: keymap first, then pane-specific keys, then the
/// mouse stack.
fn pump_events(app: &mut App, events: &mut EventStream) {
    let deadline = std::time::Instant::now() + Duration::from_millis(120);
    while std::time::Instant::now() < deadline {
        match events.try_next() {
            Some(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                match app.keymap.step(&key) {
                    KeymapOutcome::Matched(command) => apply_command(app, command, None),
                    KeymapOutcome::Miss => apply_pane_key(app, key),
                    KeymapOutcome::Pending => {}
                }
            }
            Some(Event::Mouse(mouse)) => {
                if let Some(gesture) = app.gestures.feed(mouse, Duration::from_millis(40)) {
                    let position = anchor(&gesture);
                    let hit = app.hits.hit_test(position);
                    if let Some(command) = app.mousemap.resolve(hit, &gesture) {
                        apply_command(app, command, Some(position.y));
                    }
                }
            }
            Some(_) => {}
            None => break,
        }
    }
}

/// Global commands from the keymap and the mouse map.
fn apply_command(app: &mut App, command: Command, click_row: Option<u16>) {
    match command {
        Command::Quit => app.quit = true,
        Command::FocusNext => {
            app.focus.advance();
        }
        Command::FocusPrev => {
            app.focus.retreat();
        }
        Command::ToggleAutoSave => {
            app.settings.auto_save = !app.settings.auto_save;
            app.focus.set(&Pane::AutoSave);
            app.status = format!("auto-save is now {}", on_off(app.settings.auto_save));
        }
        Command::ToggleTelemetry => {
            app.settings.telemetry = !app.settings.telemetry;
            app.focus.set(&Pane::Telemetry);
            app.status = format!("telemetry is now {}", on_off(app.settings.telemetry));
        }
        Command::ToggleDarkMode => {
            app.settings.dark_mode = !app.settings.dark_mode;
            app.focus.set(&Pane::DarkMode);
            app.status = format!("dark mode is now {}", on_off(app.settings.dark_mode));
        }
        Command::PickProtocol => {
            app.focus.set(&Pane::Protocol);
            if let Some(row) = click_row {
                // The clicked row inside the radio block maps to the
                // option index (the block border owns row 0).
                let index = row.saturating_sub(1) as usize;
                if index < app.protocols.len() {
                    app.protocol = index;
                    app.status = format!("protocol: {}", app.protocols[index]);
                }
            }
        }
        Command::ToggleRegionList => {
            app.focus.set(&Pane::Region);
            app.region.toggle();
        }
        Command::PickRegion(index) => {
            app.focus.set(&Pane::Region);
            app.region.set_selected(Some(index));
            app.region.close();
            app.status = format!("region: {}", app.regions[index]);
        }
    }
}

fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

/// Pane-specific keys: toggles, radio navigation, dropdown navigation.
fn apply_pane_key(app: &mut App, key: KeyEvent) {
    let Some(pane) = app.focus.current().copied() else {
        return;
    };
    match pane {
        Pane::AutoSave | Pane::Telemetry | Pane::DarkMode => {
            if matches!(key.code, KeyCode::Char(' ') | KeyCode::Enter) {
                let command = match pane {
                    Pane::AutoSave => Command::ToggleAutoSave,
                    Pane::Telemetry => Command::ToggleTelemetry,
                    _ => Command::ToggleDarkMode,
                };
                apply_command(app, command, None);
            }
        }
        Pane::Protocol => match key.code {
            KeyCode::Down => {
                app.protocol = (app.protocol + 1).min(app.protocols.len() - 1);
            }
            KeyCode::Up => {
                app.protocol = app.protocol.saturating_sub(1);
            }
            KeyCode::Char(' ') | KeyCode::Enter => {
                app.status = format!("protocol: {}", app.protocols[app.protocol]);
            }
            _ => {}
        },
        Pane::Region => match key.code {
            KeyCode::Char(' ') | KeyCode::Enter => {
                if app.region.is_open() {
                    if let Some(index) = app.region.choose() {
                        app.status = format!("region: {}", app.regions[index]);
                    }
                } else {
                    app.region.open();
                }
            }
            KeyCode::Esc => app.region.close(),
            KeyCode::Down => app.region.select_next(app.regions.len()),
            KeyCode::Up => app.region.select_previous(),
            _ => {}
        },
    }
}
