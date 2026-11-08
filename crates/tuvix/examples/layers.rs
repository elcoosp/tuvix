//! M18 layer showcase: the floating surfaces become a managed stack.
//!
//! [`SurfaceStack`] routes the routing half — which surfaces are open,
//! in what z-order they paint and which one `Esc` dismisses — while the
//! M16/M17 primitives keep answering the geometry (`place`,
//! `dropdown_rows`) and the paint (`Overlay`). The state stays the
//! app's: a `SelectState` drives the floating region filter, the modal
//! is a bool, exactly like any other app state. The frame walks
//! `surfaces.order()` once: the same sequence paints the surfaces
//! bottom-to-top and would register the hit map.
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
//! Run with: cargo run -p tuvix --example layers

use std::io::Result;
use std::time::Duration;

use tuvix::prelude::*;
use tuvix::{CrosstermEvents, try_init};

/// The app names its floating surfaces with its own enum — the stack's
/// keys, the way the focus ring's targets are the app's panes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Surface {
    Dropdown,
    Modal,
}

const REGIONS: [&str; 6] = [
    "eu-west", "eu-north", "us-east", "us-west", "ap-south", "sa-east",
];

/// Everything the app owns: the dashboard rows, the filter's select
/// state and the floating-surface stack.
struct App {
    rows: Vec<String>,
    region: SelectState,
    filter: Option<String>,
    surfaces: SurfaceStack<Surface>,
    quit: bool,
}

impl App {
    fn new() -> Self {
        Self {
            rows: (0..24)
                .map(|row| format!("server-{row:02}  healthy"))
                .collect(),
            region: SelectState::new(),
            filter: None,
            surfaces: SurfaceStack::new(),
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
}

fn main() -> Result<()> {
    let mut runner = try_init()?;
    let (mut reader, mut events) = CrosstermEvents::spawn()?;
    let mut app = App::new();

    while !app.quit {
        runner.draw(|frame| draw_dashboard(frame, &mut app))?;

        let deadline = std::time::Instant::now() + Duration::from_millis(50);
        while std::time::Instant::now() < deadline {
            match events.try_next() {
                Some(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                    handle_key(&mut app, key.code);
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
    if app.surfaces.top() == Some(&Surface::Modal) {
        app.surfaces.close_top();
        return;
    }

    match code {
        KeyCode::Char('q') => app.quit = true,
        KeyCode::Char('s') => {
            if !app.surfaces.close(&Surface::Dropdown) {
                app.surfaces.push(Surface::Dropdown);
                app.region.open();
            }
        }
        KeyCode::Char('o') => {
            app.surfaces.push(Surface::Modal);
        }
        KeyCode::Esc => {
            if let Some(Surface::Dropdown) = app.surfaces.close_top() {
                app.region.close();
            }
        }
        _ => {}
    }

    if app.surfaces.is_open(&Surface::Dropdown) {
        let count = REGIONS.len();
        match code {
            KeyCode::Down => app.region.select_next(count),
            KeyCode::Up => app.region.select_previous(),
            KeyCode::Enter => {
                if let Some(picked) = app.region.choose() {
                    app.filter = Some(REGIONS[picked].to_owned());
                    app.surfaces.close(&Surface::Dropdown);
                }
            }
            _ => {}
        }
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
    let status = if app.surfaces.is_open(&Surface::Modal) {
        "any key dismisses the modal"
    } else if app.surfaces.is_open(&Surface::Dropdown) {
        "s/Esc/arrows/Enter — the region filter"
    } else if let Some(f) = &app.filter {
        f.as_str()
    } else {
        "s — filter, o — modal, q — quit"
    };
    frame
        .buffer
        .set_string(status_area.x, status_area.y, status, Style::new());

    // The floating surfaces paint after the base, routed by the stack:
    // z-order is the stack order — the same sequence that would
    // register the hit map, so the screen and the mouse agree.
    for surface in app.surfaces.order() {
        match *surface {
            Surface::Dropdown => {
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
                Select::new(REGIONS).render_options(
                    Block::bordered().inner(dropdown),
                    frame.buffer,
                    &app.region,
                );
                // The trigger re-renders open so the indicator points up.
                Select::new(REGIONS).label("region").render(
                    trigger_area,
                    frame.buffer,
                    &mut app.region,
                );
            }
            Surface::Modal => {
                let modal = place(area, trigger_area, Size::new(34, 5), Placement::Centered);
                Overlay::new(Block::bordered().title("confirm")).render(modal, frame.buffer);
                let inner = Block::bordered().inner(modal);
                frame
                    .buffer
                    .set_string(inner.x, inner.y, "filter applied.", Style::new());
                frame.buffer.set_string(
                    inner.x,
                    inner.y + 1,
                    "press any key to continue",
                    Style::new().fg(Color::DarkGray),
                );
            }
        }
    }
}
