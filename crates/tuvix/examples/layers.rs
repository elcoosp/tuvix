//! M18/M19 layer showcase: the floating surfaces become a managed
//! stack, and the pointer joins it.
//!
//! [`SurfaceStack`] routes the routing half — which surfaces are open,
//! in what z-order they paint and which one `Esc` dismisses — while the
//! M16/M17 primitives keep answering the geometry (`place`,
//! `dropdown_rows`) and the paint (`Overlay`). The state stays the
//! app's: a `SelectState` drives the floating region filter, the modal
//! is a bool, exactly like any other app state. The frame walks
//! `surfaces.order()` once: the same sequence paints the surfaces
//! bottom-to-top *and* registers the hit map, so the screen and the
//! mouse agree on z-order by construction — M18's one-sequence promise,
//! kept. Since M19 the click resolves through the M14 pointer layer
//! (`HitMap` + `GestureRecognizer` + `MouseMap`) and
//! [`SelectState::option_at`] — the inverse of the windowed paint — so
//! clicking an option commits it through the same window the keyboard
//! navigates.
//!
//! Keys:
//! - `s` opens/closes the floating region filter below its trigger row
//!   (it flips above when the edge is near — shrink the terminal and
//!   watch it),
//! - arrows navigate the open filter, `Enter` commits,
//! - `Esc` dismisses the *top* surface — a modal over the dropdown goes
//!   first, the dropdown underneath stays open,
//! - `o` opens a centered confirmation modal, any key dismisses it,
//! - `q` quits.
//!
//! Mouse:
//! - click the trigger to toggle the region filter,
//! - click an option to commit it (the pointer twin of `Enter`),
//! - click anywhere else to dismiss the top surface.
//!
//! Run with: cargo run -p tuvix --example layers

use std::io::Result;
use std::time::{Duration, Instant};

use tuvix::prelude::*;
use tuvix::{CrosstermEvents, try_init};

/// The app names its regions with its own enum — the stack's keys and
/// the hit map's ids, the way the focus ring's targets are the app's
/// panes. `Base` and `Trigger` are paint-order regions only; the
/// floating surfaces are the stack's keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Region {
    Base,
    Trigger,
    Dropdown,
    Modal,
}

const REGIONS: [&str; 6] = [
    "eu-west", "eu-north", "us-east", "us-west", "ap-south", "sa-east",
];

/// What a resolved click means — the `MouseMap`'s commands, the app's
/// own vocabulary. The modal opens from the keyboard (`o`); the pointer
/// toggles the dropdown, commits options and dismisses the top surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    ToggleDropdown,
    CommitOption,
    DismissTop,
}

/// Everything the app owns: the dashboard rows, the filter's select
/// state, the floating-surface stack and the pointer layer.
struct App {
    rows: Vec<String>,
    region: SelectState,
    filter: Option<String>,
    surfaces: SurfaceStack<Region>,
    hits: HitMap<Region>,
    gestures: GestureRecognizer,
    mousemap: MouseMap<Region, Command>,
    quit: bool,
}

impl App {
    fn new() -> Self {
        let mut mousemap = MouseMap::new();
        mousemap.bind(
            MousePattern::on(Region::Trigger, MouseClass::Click),
            Command::ToggleDropdown,
        );
        mousemap.bind(
            MousePattern::on(Region::Dropdown, MouseClass::Click),
            Command::CommitOption,
        );
        mousemap.bind(
            MousePattern::on(Region::Modal, MouseClass::AnyClick),
            Command::DismissTop,
        );
        mousemap.bind(
            MousePattern::on(Region::Base, MouseClass::Click),
            Command::DismissTop,
        );

        Self {
            rows: (0..24)
                .map(|row| format!("server-{row:02}  healthy"))
                .collect(),
            region: SelectState::new(),
            filter: None,
            surfaces: SurfaceStack::new(),
            hits: HitMap::new(),
            gestures: GestureRecognizer::new(GestureConfig::default()),
            mousemap,
            quit: false,
        }
    }

    /// The visible rows: the region filter narrows the dashboard.
    fn visible_rows(&self) -> Vec<ListItem> {
        self.rows
            .iter()
            .filter(|row| self.filter.as_deref().is_none_or(|f| row.contains(f)))
            .map(|row| ListItem::new(row.clone()))
            .collect()
    }

    /// Opens the dropdown, or closes it when it is the one open surface
    /// — the toggle both the `s` key and the trigger click mean.
    fn toggle_dropdown(&mut self) {
        if self.surfaces.close(&Region::Dropdown) {
            self.region.close();
        } else {
            self.surfaces.push(Region::Dropdown);
            self.region.open();
        }
    }

    /// Dismisses the top surface, syncing the widget state it owned —
    /// the dismissal precedence the `Esc` key and an outside click
    /// share.
    fn dismiss_top(&mut self) {
        if let Some(Region::Dropdown) = self.surfaces.close_top() {
            self.region.close();
        }
    }

    /// Commits the clicked option: the inverse query answers which
    /// option the row paints, the commit path stays `choose`.
    fn commit_clicked(&mut self, area: Rect, position: Position) {
        if let Some(index) = self.region.option_at(REGIONS.len(), area, position) {
            self.region.set_highlight(index);
            if let Some(picked) = self.region.choose() {
                self.filter = Some(REGIONS[picked].to_owned());
                self.surfaces.close(&Region::Dropdown);
            }
        }
    }
}

fn main() -> Result<()> {
    let mut runner = try_init()?;
    let (mut reader, mut events) = CrosstermEvents::spawn()?;
    let mut app = App::new();
    let started = Instant::now();

    while !app.quit {
        runner.draw(|frame| draw_dashboard(frame, &mut app))?;

        let deadline = Instant::now() + Duration::from_millis(50);
        while Instant::now() < deadline {
            match events.try_next() {
                Some(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                    handle_key(&mut app, key.code);
                }
                Some(Event::Mouse(mouse)) => {
                    if let Some(gesture) = app.gestures.feed(mouse, started.elapsed()) {
                        let hit = app.hits.hit_test(anchor(&gesture));
                        if let Some(command) = app.mousemap.resolve(hit, &gesture) {
                            dispatch(&mut app, command, hit, &gesture);
                        }
                    }
                }
                Some(_) => {}
                None => std::thread::sleep(Duration::from_millis(5)),
            }
        }
    }

    reader.stop();
    runner.restore()?;
    Ok(())
}

fn handle_key(app: &mut App, code: KeyCode) {
    // The top surface eats the keys first: a modal over the dropdown
    // dismisses on any key, and nothing below routes while it is open.
    if app.surfaces.top() == Some(&Region::Modal) {
        app.surfaces.close_top();
        return;
    }

    match code {
        KeyCode::Char('q') => app.quit = true,
        KeyCode::Char('s') => app.toggle_dropdown(),
        KeyCode::Char('o') => {
            app.surfaces.push(Region::Modal);
        }
        KeyCode::Esc => app.dismiss_top(),
        _ => {}
    }

    if app.surfaces.is_open(&Region::Dropdown) {
        let count = REGIONS.len();
        match code {
            KeyCode::Down => app.region.select_next(count),
            KeyCode::Up => app.region.select_previous(),
            KeyCode::Enter => {
                if let Some(picked) = app.region.choose() {
                    app.filter = Some(REGIONS[picked].to_owned());
                    app.surfaces.close(&Region::Dropdown);
                }
            }
            _ => {}
        }
    }
}

/// The pointer's half of the pump: the gesture resolves to a region
/// through the hit map and a command through the mouse map — the same
/// z-order the frame painted.
fn dispatch(app: &mut App, command: Command, hit: Option<Hit<Region>>, gesture: &Gesture) {
    match command {
        Command::ToggleDropdown => app.toggle_dropdown(),
        Command::CommitOption => {
            if let Some(hit) = hit
                && let Gesture::Click { position, .. } = *gesture
            {
                app.commit_clicked(hit.area, position);
            }
        }
        Command::DismissTop => app.dismiss_top(),
    }
}

fn draw_dashboard(frame: &mut Frame<'_>, app: &mut App) {
    let area = frame.area();
    let block = Block::bordered().title("dashboard");
    block.clone().render(area, frame.buffer);
    let inner = block.inner(area);

    let [list_area, trigger_area, status_area] = Layout::vertical([
        Constraint::Min(5),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(&inner);

    List::new(app.visible_rows()).render(list_area, frame.buffer, &mut ListState::default());
    Select::new(REGIONS)
        .label("region")
        .placeholder("all")
        .render(trigger_area, frame.buffer, &mut app.region);
    let status = if app.surfaces.is_open(&Region::Modal) {
        "any key dismisses the modal"
    } else if app.surfaces.is_open(&Region::Dropdown) {
        "s/Esc/arrows/Enter — the region filter"
    } else if let Some(f) = &app.filter {
        f.as_str()
    } else {
        "s — filter, o — modal, q — quit"
    };
    frame
        .buffer
        .set_string(status_area.x, status_area.y, status, Style::new());

    // The click surface rebuilds every frame, in paint order: the base
    // and the trigger register first, then the surfaces in `order()` —
    // the painter's algorithm, one sequence for the paint and the hit
    // map alike.
    app.hits.clear();
    app.hits.push(Region::Base, area);
    app.hits.push(Region::Trigger, trigger_area);

    // The floating surfaces paint after the base, routed by the stack:
    // z-order is the stack order.
    for surface in app.surfaces.order() {
        match *surface {
            Region::Dropdown => {
                // The M17 recipe, one surface of the stack: size the
                // surface from the host's room (the block's borders ride
                // on top), place it, erase it, paint the visible option
                // window into the clean rect.
                let rows = dropdown_rows(area, trigger_area, REGIONS.len());
                let dropdown = place(
                    area,
                    trigger_area,
                    Size::new(24, rows.saturating_add(2)),
                    Placement::Below,
                );
                Overlay::new(Block::bordered().title("region")).render(dropdown, frame.buffer);
                let options_area = Block::bordered().inner(dropdown);
                Select::new(REGIONS).render_options(options_area, frame.buffer, &app.region);
                // The pointer reads the same rect back: the options area
                // is what the click resolves against.
                app.hits.push(Region::Dropdown, options_area);
                // The trigger re-renders open so the indicator points up.
                Select::new(REGIONS).label("region").render(
                    trigger_area,
                    frame.buffer,
                    &mut app.region,
                );
            }
            Region::Modal => {
                let modal = place(area, trigger_area, Size::new(34, 5), Placement::Centered);
                Overlay::new(Block::bordered().title("confirm")).render(modal, frame.buffer);
                let modal_area = Block::bordered().inner(modal);
                frame.buffer.set_string(
                    modal_area.x,
                    modal_area.y,
                    "filter applied.",
                    Style::new(),
                );
                frame.buffer.set_string(
                    modal_area.x,
                    modal_area.y + 1,
                    "press any key to continue",
                    Style::new().fg(Color::DarkGray),
                );
                app.hits.push(Region::Modal, modal_area);
            }
            Region::Base | Region::Trigger => {}
        }
    }
}
