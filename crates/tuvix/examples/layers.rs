//! M16 layer showcase: floating surfaces — `place` decides where a
//! surface goes (flip-then-clamp geometry), `Overlay` decides how it
//! paints (erase, then paint, wide-glyph pairs repaired). The state
//! stays the app's: a `SelectState` drives the floating region filter,
//! a bool drives the modal, exactly like any other app state.
//!
//! Keys:
//! - `s` opens/closes the floating region filter below its trigger row
//!   (it flips above when the edge is near — shrink the terminal and
//!   watch it),
//! - arrows navigate the open filter, `Enter` commits, `Esc` dismisses,
//! - `o` opens a centered confirmation modal, any key closes it,
//! - `q` quits.
//!
//! Run with: cargo run -p tuvix --example layers

use std::io::Result;
use std::time::Duration;

use tuvix::prelude::*;
use tuvix::{CrosstermEvents, try_init};
use tuvix_widgets::{Block, List, ListItem, ListState, Overlay, Select, SelectState};

const REGIONS: [&str; 6] = [
    "eu-west", "eu-north", "us-east", "us-west", "ap-south", "sa-east",
];

/// Everything the app owns: the dashboard rows plus the two floating
/// surfaces' state.
struct App {
    rows: Vec<String>,
    region: SelectState,
    filter: Option<String>,
    dropdown_open: bool,
    modal_open: bool,
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
            dropdown_open: false,
            modal_open: false,
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
    match code {
        KeyCode::Char('q') => app.quit = true,
        KeyCode::Char('s') => {
            app.dropdown_open = !app.dropdown_open;
            if app.dropdown_open {
                app.region.open();
            }
        }
        KeyCode::Char('o') => app.modal_open = true,
        KeyCode::Esc => app.dropdown_open = false,
        _ => {}
    }

    if app.dropdown_open {
        let count = REGIONS.len();
        match code {
            KeyCode::Down => app.region.select_next(count),
            KeyCode::Up => app.region.select_previous(),
            KeyCode::Enter => {
                if let Some(picked) = app.region.choose() {
                    app.filter = Some(REGIONS[picked].to_owned());
                    app.dropdown_open = false;
                }
            }
            _ => {}
        }
    } else if app.modal_open {
        // Any key dismisses the modal.
        app.modal_open = false;
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
    let status = match (&app.filter, app.dropdown_open, app.modal_open) {
        (_, true, _) => "s/Esc/arrows/Enter — the region filter",
        (_, _, true) => "any key dismisses the modal",
        (Some(f), false, false) => f.as_str(),
        (None, false, false) => "s — filter, o — modal, q — quit",
    };
    frame
        .buffer
        .set_string(status_area.x, status_area.y, status, Style::new());

    // The floating surfaces paint after the base: z-order is paint
    // order, on the screen and in the hit map alike.
    if app.dropdown_open {
        let dropdown = place(
            area,
            trigger_area,
            Size::new(24, u16::try_from(REGIONS.len() + 2).unwrap_or(u16::MAX)),
            Placement::Below,
        );
        Overlay::new(Block::bordered().title("region")).render(dropdown, frame.buffer);
        List::new(REGIONS).render(
            Block::bordered().inner(dropdown),
            frame.buffer,
            &mut ListState::default(),
        );
        // The trigger re-renders open so the highlight is visible.
        Select::new(REGIONS)
            .label("region")
            .render(trigger_area, frame.buffer, &mut app.region);
    }
    if app.modal_open {
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
