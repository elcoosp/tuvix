//! Benchmarks for the layout constraint solver.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use tuvix_core::geometry::Rect;
use tuvix_core::layout::{Constraint, Layout};

fn bench_layout(c: &mut Criterion) {
    let mut group = c.benchmark_group("layout");
    let area = Rect::new(0, 0, 80, 30);

    group.bench_function("vertical_3_constraints", |b| {
        let layout = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(3),
        ]);
        b.iter(|| layout.split(black_box(area)));
    });

    group.bench_function("vertical_20_constraints", |b| {
        let constraints: Vec<Constraint> = (0..20)
            .map(|index| match index % 4 {
                0 => Constraint::Length(2),
                1 => Constraint::Min(0),
                2 => Constraint::Percentage(10),
                _ => Constraint::Fill(1),
            })
            .collect();
        let layout = Layout::vertical(constraints);
        b.iter(|| layout.split(black_box(area)));
    });

    group.bench_function("horizontal_6_mixed", |b| {
        let layout = Layout::horizontal([
            Constraint::Length(10),
            Constraint::Percentage(20),
            Constraint::Ratio(1, 3),
            Constraint::Min(10),
            Constraint::Max(20),
            Constraint::Fill(1),
        ]);
        b.iter(|| layout.split(black_box(area)));
    });

    group.bench_function("vertical_20_weighted", |b| {
        let constraints: Vec<Constraint> = (0..20)
            .map(|index| match index % 4 {
                0 => Constraint::Length(2),
                1 => Constraint::Weight(1),
                2 => Constraint::Weight(3),
                _ => Constraint::Min(0),
            })
            .collect();
        let layout = Layout::vertical(constraints);
        b.iter(|| layout.split(black_box(area)));
    });

    group.bench_function("horizontal_20_flex_center", |b| {
        // The M6 placement pass on top of a solved split: 20 fixed
        // segments leave a leftover that Flex::Center distributes.
        let constraints: Vec<Constraint> = (0..20).map(|_| Constraint::Length(2)).collect();
        let layout = Layout::horizontal(constraints).flex(tuvix_core::layout::Flex::Center);
        b.iter(|| layout.split(black_box(area)));
    });

    group.bench_function("horizontal_3_flex_space_between", |b| {
        let layout = Layout::horizontal([
            Constraint::Length(10),
            Constraint::Length(20),
            Constraint::Length(15),
        ])
        .flex(tuvix_core::layout::Flex::SpaceBetween);
        b.iter(|| layout.split(black_box(area)));
    });

    group.finish();
}

criterion_group!(benches, bench_layout);
criterion_main!(benches);
