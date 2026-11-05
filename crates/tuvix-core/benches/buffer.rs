//! Benchmarks for buffer operations: painting and styling.

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use tuvix_core::buffer::Buffer;
use tuvix_core::geometry::Rect;
use tuvix_core::style::{Color, Modifier, Style};

const WIDTH: u16 = 80;
const HEIGHT: u16 = 24;

fn bench_buffer(c: &mut Criterion) {
    let mut group = c.benchmark_group("buffer");

    group.bench_function("set_string_ascii_80", |b| {
        let mut buffer = Buffer::empty(Rect::new(0, 0, WIDTH, HEIGHT));
        b.iter(|| {
            buffer.set_string(
                0,
                0,
                black_box("the quick brown fox jumps over the lazy dog 0123456789"),
                Style::new(),
            );
        });
    });

    group.bench_function("set_string_cjk_40", |b| {
        let mut buffer = Buffer::empty(Rect::new(0, 0, WIDTH, HEIGHT));
        b.iter(|| {
            buffer.set_string(
                0,
                0,
                black_box("终端界面框架性能基准测试汉字宽度处理"),
                Style::new(),
            );
        });
    });

    group.bench_function("set_string_styled_80", |b| {
        let style = Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD);
        let mut buffer = Buffer::empty(Rect::new(0, 0, WIDTH, HEIGHT));
        b.iter(|| {
            buffer.set_string(
                0,
                0,
                black_box("the quick brown fox jumps over the lazy dog 0123456789"),
                style,
            );
        });
    });

    group.throughput(Throughput::Elements(u64::from(WIDTH) * u64::from(HEIGHT)));
    group.bench_function("set_style_full_screen", |b| {
        let mut buffer = Buffer::empty(Rect::new(0, 0, WIDTH, HEIGHT));
        let style = Style::new().add_modifier(Modifier::BOLD);
        b.iter(|| {
            buffer.set_style(black_box(buffer.area), style);
        });
    });

    group.bench_function("reset_full_screen", |b| {
        let mut buffer = Buffer::filled(Rect::new(0, 0, WIDTH, HEIGHT), "x", Style::new());
        b.iter(|| {
            buffer.reset();
        });
    });

    group.bench_function("empty_80x24_alloc", |b| {
        b.iter(|| Buffer::empty(black_box(Rect::new(0, 0, WIDTH, HEIGHT))));
    });

    group.finish();
}

criterion_group!(benches, bench_buffer);
criterion_main!(benches);
