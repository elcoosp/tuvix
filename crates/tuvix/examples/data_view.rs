//! M7 data showcase: a grouped [`BarChart`], a navigable [`Tree`] and a
//! [`Paragraph`] fed by the markup parser. Keys:
//! - `Tab`/`Shift+Tab` move the focus between the tree and the bar chart,
//! - arrows move the tree selection; `Enter` toggles the focused folder,
//!   `o` expands everything, `c` collapses everything,
//! - `+`/`-` grow or shrink the sample the chart is pinned to,
//! - `Ctrl+q` quits.
//!
//! Run with: cargo run -p tuvix --example data_view

use std::io::Result;
use std::time::Duration;

use tuvix::prelude::*;
use tuvix::{CrosstermEvents, MarkupError, parse_line, try_init};
use tuvix_widgets::{Bar, BarChart, BarGroup, Tree, TreeItem, TreeState};

/// Which pane the keyboard drives.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    Chart,
    Tree,
}

struct App {
    focus: Focus,
    tree_state: TreeState,
    /// The pinned chart maximum; `+`/`-` rescale the same data.
    max: u64,
    quit: bool,
}

impl App {
    fn new() -> Self {
        Self {
            focus: Focus::Tree,
            tree_state: TreeState::default(),
            max: 100,
            quit: false,
        }
    }
}

/// The demo tree: a source directory with nested folders.
fn source_tree() -> Vec<TreeItem> {
    vec![
        TreeItem::with_children(
            "crates",
            [
                TreeItem::with_children(
                    "tuvix-core",
                    [TreeItem::new("buffer.rs"), TreeItem::new("markup.rs")],
                ),
                TreeItem::with_children(
                    "tuvix-widgets",
                    [TreeItem::new("bar_chart.rs"), TreeItem::new("tree.rs")],
                ),
                TreeItem::new("tuvix"),
            ],
        ),
        TreeItem::with_children(
            "docs",
            [
                TreeItem::new("ARCHITECTURE.md"),
                TreeItem::new("RELEASE.md"),
            ],
        ),
        TreeItem::new("Cargo.toml"),
    ]
}

/// The demo chart: two release trains, three bars each.
fn sample_chart(max: u64) -> BarChart {
    BarChart::default()
        .group(
            BarGroup::new([
                Bar::new(72).label("canary"),
                Bar::new(88).label("beta"),
                Bar::new(61).label("stable"),
            ])
            .label("1.1")
            .style(Style::new().fg(Color::Cyan)),
        )
        .group(
            BarGroup::new([
                Bar::new(45).label("canary"),
                Bar::new(97).label("beta"),
                Bar::new(79).label("stable"),
            ])
            .label("1.2")
            .style(Style::new().fg(Color::Magenta)),
        )
        .max(max)
}

fn main() -> Result<()> {
    main_app()
}

fn main_app() -> Result<()> {
    let mut runner = try_init()?;
    let (mut reader, mut events) = CrosstermEvents::spawn()?;
    let mut app = App::new();

    while !app.quit {
        runner.draw(|frame| draw(frame, &mut app))?;
        handle_events(&mut app, &mut events);
    }

    reader.stop();
    runner.restore()?;
    Ok(())
}

/// One frame: header line, chart on the left, tree on the right, footer.
fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(&area);

    let header_line =
        match parse_line("[[tuvix] data view — [cyan]chart[/] + [magenta]tree[/] + [dim]markup[/]")
        {
            Ok(line) => line,
            Err(MarkupError::UnknownToken { token, .. }) => {
                Line::from(format!("markup error: unknown token {token}"))
            }
            Err(_) => Line::from("markup error"),
        };
    Widget::render(Paragraph::new(header_line), header, &mut *frame.buffer);

    let [chart_area, tree_area] =
        Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(&body);

    let focus_ring = Style::new().fg(Color::Cyan);
    let muted_ring = Style::new().fg(Color::DarkGray);

    let chart_block =
        Block::bordered()
            .title("throughput %")
            .border_style(if app.focus == Focus::Chart {
                focus_ring
            } else {
                muted_ring
            });
    Widget::render(
        sample_chart(app.max).block(chart_block),
        chart_area,
        &mut *frame.buffer,
    );

    let tree_block = Block::bordered()
        .title("sources")
        .border_style(if app.focus == Focus::Tree {
            focus_ring
        } else {
            muted_ring
        });
    let tree = Tree::new(source_tree())
        .block(tree_block)
        .highlight_style(Style::new().bg(Color::DarkGray).bold());
    RenderWidget::render(tree, tree_area, &mut *frame.buffer, &mut app.tree_state);

    let footer_line = parse_line(
        "[dim]Tab[/] switch · [dim]arrows[/] tree · [dim]Enter[/] fold · [dim]+/-[/] scale · [dim]Ctrl+q[/] quit",
    )
    .unwrap_or_default();
    Widget::render(Paragraph::new(footer_line), footer, &mut *frame.buffer);
}

/// Event pump for the showcase.
fn handle_events(app: &mut App, events: &mut tuvix_core::runtime::queue::EventStream) {
    let deadline = std::time::Instant::now() + Duration::from_millis(120);
    while std::time::Instant::now() < deadline {
        match events.try_next() {
            Some(Event::Key(key)) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    app.quit = true;
                }
                KeyCode::Tab => app.focus = Focus::Tree,
                KeyCode::BackTab => app.focus = Focus::Chart,
                _ if app.focus != Focus::Tree => {}
                KeyCode::Char('+') => app.max = (app.max + 10).min(200),
                KeyCode::Char('-') => app.max = app.max.saturating_sub(10).max(50),
                KeyCode::Enter => {
                    if let Some(path) = app.tree_state.selected() {
                        let path = path.to_vec();
                        app.tree_state.toggle(path);
                    }
                }
                KeyCode::Char('o') => app.tree_state.expand_all(),
                KeyCode::Char('c') => app.tree_state.collapse_all(&source_tree()),
                KeyCode::Down => {
                    let tree = source_tree();
                    app.tree_state.select_next(&tree);
                }
                KeyCode::Up => {
                    let tree = source_tree();
                    app.tree_state.select_previous(&tree);
                }
                _ => {}
            },
            Some(_) => {}
            None => break,
        }
    }
}
