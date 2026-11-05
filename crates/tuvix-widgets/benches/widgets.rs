//! Benchmarks for the M3 widget family: per-widget paint into plain
//! buffers at the architecture budget viewport.

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use tuvix_core::buffer::Buffer;
use tuvix_core::geometry::Rect;
use tuvix_core::layout::{Constraint, Layout};
use tuvix_core::style::{Color, Style};
use tuvix_core::widget::{RenderWidget, Widget};
use tuvix_widgets::{
    Axis, Bar, BarChart, BarGroup, Block, Chart, Dataset, Gauge, GraphType, Input, InputState,
    LineGauge, List, ListItem, ListState, Scrollbar, ScrollbarOrientation, ScrollbarState,
    Sparkline, Table, TableRow, TableState, Tabs, Tree, TreeItem, TreeState,
};

/// The architecture budget viewport: 200x60.
const WIDTH: u16 = 200;
const HEIGHT: u16 = 60;

fn bench_widgets_bars(c: &mut Criterion) {
    let mut group = c.benchmark_group("widgets");
    group.throughput(Throughput::Elements(u64::from(WIDTH) * u64::from(HEIGHT)));
    let area = Rect::new(0, 0, WIDTH, HEIGHT);

    group.bench_function("gauge_200x60", |b| {
        let gauge = Gauge::percent(73).style(Style::new().fg(Color::Green));
        b.iter(|| {
            let mut buffer = Buffer::empty(area);
            Widget::render(black_box(gauge.clone()), black_box(area), &mut buffer);
        });
    });

    group.bench_function("sparkline_512_values_200x60", |b| {
        let data: Vec<u64> = (0..512).map(|index| (index * 37 + 11) % 100).collect();
        let sparkline = Sparkline::new(data);
        b.iter(|| {
            let mut buffer = Buffer::empty(area);
            Widget::render(black_box(sparkline.clone()), black_box(area), &mut buffer);
        });
    });

    group.finish();
}

fn bench_widgets_lists(c: &mut Criterion) {
    let mut group = c.benchmark_group("widgets");
    group.throughput(Throughput::Elements(u64::from(WIDTH) * u64::from(HEIGHT)));
    let area = Rect::new(0, 0, WIDTH, HEIGHT);

    group.bench_function("list_1k_items_200x60", |b| {
        let items: Vec<ListItem> = (0..1_000)
            .map(|index| ListItem::new(format!("item {index}: the quick brown fox")))
            .collect();
        let list = List::new(items).highlight_symbol("> ");
        let mut state = ListState::default().with_selected(Some(500));
        b.iter(|| {
            let mut buffer = Buffer::empty(area);
            RenderWidget::render(
                black_box(list.clone()),
                black_box(area),
                &mut buffer,
                &mut state,
            );
        });
    });

    group.finish();
}

fn bench_widgets_data(c: &mut Criterion) {
    let mut group = c.benchmark_group("widgets");
    group.throughput(Throughput::Elements(u64::from(WIDTH) * u64::from(HEIGHT)));
    let area = Rect::new(0, 0, WIDTH, HEIGHT);

    group.bench_function("table_200_rows_200x60", |b| {
        let rows: Vec<TableRow> = (0..200)
            .map(|index| {
                TableRow::new([
                    format!("row {index}"),
                    format!("{}", index * 7),
                    String::from("ready"),
                ])
            })
            .collect();
        let table = Table::new(
            rows,
            [
                Constraint::Percentage(40),
                Constraint::Percentage(30),
                Constraint::Percentage(30),
            ],
        )
        .header(TableRow::new([
            String::from("name"),
            String::from("value"),
            String::from("status"),
        ]))
        .highlight_symbol("> ");
        let mut state = TableState::default().with_selected(Some(100));
        b.iter(|| {
            let mut buffer = Buffer::empty(area);
            RenderWidget::render(
                black_box(table.clone()),
                black_box(area),
                &mut buffer,
                &mut state,
            );
        });
    });

    group.bench_function("chart_10k_points_line_200x60", |b| {
        let data: Vec<(f64, f64)> = (0..10_000)
            .map(|index| {
                let t = f64::from(index) / 10_000.0;
                (t, (t * 12.0).sin() * 0.5 + 0.5)
            })
            .collect();
        let chart = Chart::new([Dataset::new("wave", data).graph_type(GraphType::Line)])
            .x_axis(Axis::new().bounds([0.0, 1.0]).labels(["0", "1"]))
            .y_axis(Axis::new().bounds([0.0, 1.0]).labels(["0", "1"]));
        b.iter(|| {
            let mut buffer = Buffer::empty(area);
            Widget::render(black_box(chart.clone()), black_box(area), &mut buffer);
        });
    });

    group.bench_function("canvas_braille_full_200x60", |b| {
        let canvas = tuvix_widgets::canvas::Canvas::new()
            .x_bounds([0.0, 1.0])
            .y_bounds([0.0, 1.0])
            .paint(|ctx| {
                ctx.draw(&tuvix_widgets::canvas::Line::new(
                    0.0,
                    0.0,
                    1.0,
                    1.0,
                    Color::Red,
                ));
                ctx.draw(&tuvix_widgets::canvas::Rectangle {
                    x: 0.1,
                    y: 0.1,
                    width: 0.8,
                    height: 0.8,
                    color: Color::Blue,
                });
            });
        b.iter(|| {
            let mut buffer = Buffer::empty(area);
            Widget::render(black_box(canvas.clone()), black_box(area), &mut buffer);
        });
    });

    group.bench_function("scrollbar_200x60", |b| {
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight);
        let mut state = ScrollbarState::new(1_000)
            .position(500)
            .viewport_content_length(60)
            .to_owned();
        b.iter(|| {
            let mut buffer = Buffer::empty(area);
            RenderWidget::render(
                black_box(scrollbar.clone()),
                black_box(Rect::new(WIDTH - 1, 0, 1, HEIGHT)),
                &mut buffer,
                &mut state,
            );
        });
    });

    group.finish();
}

fn bench_widgets_m6(c: &mut Criterion) {
    let mut group = c.benchmark_group("widgets");
    group.throughput(Throughput::Elements(u64::from(WIDTH) * u64::from(HEIGHT)));

    group.bench_function("tabs_8_titles_200x1", |b| {
        let tabs = Tabs::new([
            "one", "two", "three", "four", "five", "six", "seven", "eight",
        ])
        .select(3)
        .highlight_style(Style::new().fg(Color::Black).bg(Color::Cyan));
        b.iter(|| {
            let mut buffer = Buffer::empty(Rect::new(0, 0, WIDTH, 1));
            Widget::render(
                black_box(tabs.clone()),
                black_box(Rect::new(0, 0, WIDTH, 1)),
                &mut buffer,
            );
        });
    });

    group.bench_function("line_gauge_200x1", |b| {
        let gauge = LineGauge::default()
            .ratio(0.42)
            .label("load")
            .style(Style::new().fg(Color::Green));
        b.iter(|| {
            let mut buffer = Buffer::empty(Rect::new(0, 0, WIDTH, 1));
            Widget::render(
                black_box(gauge.clone()),
                black_box(Rect::new(0, 0, WIDTH, 1)),
                &mut buffer,
            );
        });
    });

    group.bench_function("input_80_chars_200x1", |b| {
        let mut state = InputState::new();
        state.set_value(
            "the quick brown fox jumps over the lazy dog and keeps typing well past the edge ",
        );
        let input = Input::new();
        b.iter(|| {
            let mut buffer = Buffer::empty(Rect::new(0, 0, WIDTH, 1));
            RenderWidget::render(
                black_box(input.clone()),
                black_box(Rect::new(0, 0, WIDTH, 1)),
                &mut buffer,
                &mut state,
            );
        });
    });

    group.finish();
}

fn bench_widget_suite(c: &mut Criterion) {
    let mut group = c.benchmark_group("gates");
    group.throughput(Throughput::Elements(u64::from(WIDTH) * u64::from(HEIGHT)));

    // M3 gate: paint the full M3 dashboard (gauge + list + table) into a
    // fresh buffer — the widget-side counterpart of the B2 frame gate.
    let items: Vec<ListItem> = (0..60)
        .map(|index| ListItem::new(format!("item {index}")))
        .collect();
    let rows: Vec<TableRow> = (0..60)
        .map(|index| TableRow::new([format!("row {index}"), String::from("ok")]))
        .collect();

    group.bench_function("m3_dashboard_paint_200x60", |b| {
        b.iter(|| {
            let mut buffer = Buffer::empty(Rect::new(0, 0, WIDTH, HEIGHT));
            let [top, middle, bottom] = Layout::vertical([
                Constraint::Length(1),
                Constraint::Length(30),
                Constraint::Min(0),
            ])
            .areas(&Rect::new(0, 0, WIDTH, HEIGHT));

            Widget::render(
                Gauge::percent(50).block(Block::bordered().title("gauge")),
                top,
                &mut buffer,
            );
            let list = List::new(items.clone())
                .highlight_symbol("> ")
                .block(Block::bordered().title("list"));
            let mut list_state = ListState::default().with_selected(Some(10));
            RenderWidget::render(list, middle, &mut buffer, &mut list_state);
            let table = Table::new(
                rows.clone(),
                [Constraint::Percentage(50), Constraint::Percentage(50)],
            )
            .header(TableRow::new([
                String::from("name"),
                String::from("status"),
            ]))
            .block(Block::bordered().title("table"));
            let mut table_state = TableState::default().with_selected(Some(20));
            RenderWidget::render(table, bottom, &mut buffer, &mut table_state);
        });
    });

    group.finish();
}

/// M7: the bar chart and the tree at the budget viewport.
fn bench_widgets_m7(c: &mut Criterion) {
    let mut group = c.benchmark_group("widgets");
    group.throughput(Throughput::Elements(u64::from(WIDTH) * u64::from(HEIGHT)));
    let area = Rect::new(0, 0, WIDTH, HEIGHT);

    group.bench_function("bar_chart_2_groups_200x60", |b| {
        let chart = BarChart::default()
            .group(
                BarGroup::new([
                    Bar::new(72).label("canary"),
                    Bar::new(88).label("beta"),
                    Bar::new(61).label("stable"),
                ])
                .label("1.1"),
            )
            .group(
                BarGroup::new([
                    Bar::new(45).label("canary"),
                    Bar::new(97).label("beta"),
                    Bar::new(79).label("stable"),
                ])
                .label("1.2"),
            )
            .max(100);
        b.iter(|| {
            let mut buffer = Buffer::empty(area);
            Widget::render(black_box(chart.clone()), black_box(area), &mut buffer);
        });
    });

    group.bench_function("tree_256_nodes_200x60", |b| {
        // A 256-node balanced-ish tree: 16 roots with 15 children each.
        let items: Vec<TreeItem> = (0..16)
            .map(|root| {
                let children: Vec<TreeItem> = (0..15)
                    .map(|index| TreeItem::new(format!("n{root}-{index}: the quick brown fox")))
                    .collect();
                TreeItem::with_children(format!("root {root}"), children)
            })
            .collect();
        let tree = Tree::new(items).highlight_style(Style::new().bg(Color::DarkGray));
        let mut state = TreeState::default().with_selected([8, 7]);
        b.iter(|| {
            let mut buffer = Buffer::empty(area);
            RenderWidget::render(
                black_box(tree.clone()),
                black_box(area),
                &mut buffer,
                &mut state,
            );
        });
    });

    group.finish();
}

/// M9: the DevTools surfaces.
fn bench_widgets_m9(c: &mut Criterion) {
    use tuvix_core::preview::HistorySink;
    use tuvix_core::screen::Screen;
    use tuvix_core::terminal::Terminal;
    use tuvix_widgets::FrameInspector;

    let mut group = c.benchmark_group("widgets");
    group.throughput(Throughput::Elements(u64::from(WIDTH) * u64::from(HEIGHT)));
    let area = Rect::new(0, 0, WIDTH, HEIGHT);

    // A real recorded snapshot through the preview hook.
    let mut terminal = Terminal::new(tuvix_core::backend::TestBackend::new(
        tuvix_core::geometry::Size::new(WIDTH, HEIGHT),
    ))
    .unwrap();
    let history = HistorySink::unbounded();
    terminal.set_preview(history.clone());
    terminal
        .draw(|frame| {
            frame.buffer.set_string(
                0,
                0,
                "M9 inspector bench — the quick brown fox",
                Style::new(),
            );
            for row in 1..HEIGHT {
                let band = format!("row {row:02}: dashboard band {row}");
                frame.buffer.set_string(0, row, band.as_str(), Style::new());
            }
        })
        .unwrap();
    let snapshot = history.latest().unwrap();
    let screen = Screen::from_ansi(&snapshot).unwrap();

    // The scrub view: paste the historical frame over a bordered box
    // with the timeline footer.
    group.bench_function("frame_inspector_200x60", |b| {
        let inspector = FrameInspector::new(screen.clone())
            .title("scrub")
            .timeline(1, 3)
            .border_style(Style::new().fg(Color::Cyan));
        b.iter(|| {
            let mut buffer = Buffer::empty(area);
            Widget::render(black_box(inspector.clone()), black_box(area), &mut buffer);
        });
    });

    // The screen reconstruction feeds it: parse cost of a full frame.
    group.bench_function("screen_reconstruct_200x60", |b| {
        b.iter(|| Screen::from_ansi(black_box(snapshot.as_str())).unwrap());
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_widgets_bars,
    bench_widgets_lists,
    bench_widgets_data,
    bench_widgets_m6,
    bench_widgets_m7,
    bench_widgets_m9,
    bench_widget_suite
);
criterion_main!(benches);
