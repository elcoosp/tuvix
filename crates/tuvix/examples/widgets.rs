//! M3 widgets showcase: every widget family on one screen, driven by
//! the M2 runtime. Keys:
//! - `q` quits, `j`/`k` (and arrow keys) move the list and table
//!   selection, `1`-`9` nudges the gauge by 10%.
//!
//! Run with: cargo run -p tuvix --example widgets

use std::io::Result;
use std::time::Duration;

use tuvix::prelude::*;
use tuvix::widget::RenderWidget;
use tuvix::{CrosstermEvents, try_init};
use tuvix_widgets::{
    Axis, Block, Borders, Chart, Dataset, Gauge, GraphType, List, ListState, Marker, Scrollbar,
    ScrollbarOrientation, ScrollbarState, Sparkline, Table, TableRow, TableState,
};

struct App {
    gauge: u16,
    list_state: ListState,
    table_state: TableState,
    scroll_state: ScrollbarState,
    quit: bool,
}

fn main() -> Result<()> {
    main_app()
}

/// The showcase loop, split out of `main` for readability.
fn main_app() -> Result<()> {
    let mut runner = try_init()?;
    runner.push_input_mode(InputMode::BracketedPaste)?;
    runner.push_input_mode(InputMode::MouseCapture)?;

    let (mut reader, mut events) = CrosstermEvents::spawn()?;
    let mut app = App {
        gauge: 60,
        list_state: ListState::default().with_selected(Some(2)),
        table_state: TableState::default().with_selected(Some(0)),
        scroll_state: {
            let mut scroll = ScrollbarState::new(120);
            scroll.position(0).viewport_content_length(9);
            scroll
        },
        quit: false,
    };

    let series: Vec<(f64, f64)> = (0..200)
        .map(|index| {
            let t = f64::from(index) / 200.0;
            (t, (t * 8.0).sin() * 0.4 + 0.5)
        })
        .collect();
    let spark: Vec<u64> = (0..120).map(|index| ((index * 13 + 7) % 64) + 4).collect();
    let items: Vec<String> = (0..120).map(|index| format!("item {index:02}")).collect();
    let rows: Vec<TableRow> = (0..40)
        .map(|index| {
            TableRow::new([
                format!("service-{index:02}"),
                String::from(if index % 3 == 0 {
                    "degraded"
                } else {
                    "healthy"
                }),
            ])
        })
        .collect();

    let data = ShowcaseData {
        series,
        spark,
        items,
        rows,
    };

    while !app.quit {
        runner.draw(|frame| draw_dashboard(frame, &data, &mut app))?;
        handle_events(&mut app, &mut events);
    }

    reader.stop();
    runner.restore()?;
    Ok(())
}

/// Static data the showcase renders from (built once).
struct ShowcaseData {
    series: Vec<(f64, f64)>,
    spark: Vec<u64>,
    items: Vec<String>,
    rows: Vec<TableRow>,
}

/// One frame of the showcase: gauge, sparkline, chart, list + scrollbar,
/// table.
fn draw_dashboard(frame: &mut Frame, data: &ShowcaseData, app: &mut App) {
    let [header, top, middle, bottom] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Length(9),
        Constraint::Min(0),
    ])
    .areas(&frame.area());

    frame.buffer.set_string(
        2,
        header.y,
        "tuvix M3 — widgets showcase (q quit, j/k move, +/- gauge)",
        Style::new().fg(Color::Cyan).bold(),
    );

    Widget::render(
        Gauge::percent(app.gauge).block(Block::bordered().title("gauge")),
        top,
        &mut *frame.buffer,
    );

    Widget::render(
        Sparkline::new(data.spark.clone()).block(Block::bordered().title("sparkline")),
        middle,
        &mut *frame.buffer,
    );

    let chart = Chart::new([Dataset::new("wave", data.series.clone())
        .graph_type(GraphType::Line)
        .marker(Marker::Dot)])
    .x_axis(Axis::new().bounds([0.0, 1.0]).labels(["0.0", "1.0"]))
    .y_axis(Axis::new().bounds([0.0, 1.0]).labels(["0", "1"]))
    .block(Block::bordered().title("chart"));
    Widget::render(chart, bottom, &mut *frame.buffer);

    let list = List::new(data.items.clone())
        .highlight_symbol("> ")
        .highlight_style(Style::new().fg(Color::Yellow).bold())
        .block(Block::new().borders(Borders::TOP).title("list (j/k)"));
    RenderWidget::render(list, middle, &mut *frame.buffer, &mut app.list_state);
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight);
    app.scroll_state.position(app.list_state.offset);
    scrollbar.render(
        Rect::new(middle.right() - 1, middle.y, 1, middle.height),
        &mut *frame.buffer,
        &mut app.scroll_state,
    );

    let table = Table::new(
        data.rows.clone(),
        [Constraint::Percentage(50), Constraint::Percentage(50)],
    )
    .header(TableRow::new([
        String::from("service"),
        String::from("status"),
    ]))
    .highlight_symbol("> ")
    .highlight_style(Style::new().fg(Color::Yellow))
    .block(Block::bordered().title("table (j/k)"));
    RenderWidget::render(table, bottom, &mut *frame.buffer, &mut app.table_state);
}

/// Event pump: quit on 'q', move on j/k, nudge the gauge on +/-.
fn handle_events(app: &mut App, events: &mut tuvix_core::runtime::queue::EventStream) {
    let deadline = std::time::Instant::now() + Duration::from_millis(120);
    while std::time::Instant::now() < deadline {
        match events.try_next() {
            Some(Event::Key(key)) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Char('q') => app.quit = true,
                KeyCode::Char('j') | KeyCode::Down => {
                    app.list_state.select_next();
                    app.table_state.select_next();
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    app.list_state.select_previous();
                    app.table_state.select_previous();
                }
                KeyCode::Char('+') => {
                    app.gauge = (app.gauge + 10).min(100);
                }
                KeyCode::Char('-') => app.gauge = app.gauge.saturating_sub(10),
                _ => {}
            },
            Some(_) => {}
            None => break,
        }
    }
}
