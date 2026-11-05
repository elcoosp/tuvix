//! M7 benchmark: the markup parser and the macro sugar against the
//! cold-path expectations (markup is authored once, rendered often).

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use tuvix_core::markup::parse_line;

/// A representative dashboard header with mixed styling regions.
const HEADER: &str = "deploy [green bold]ok[/] — [cyan]api[/] v2 · [dim]region eu-west[/] · [on #101010 #ff8000]canary 12%[/] · plain tail";

fn bench_markup(c: &mut Criterion) {
    let mut group = c.benchmark_group("markup");
    group.throughput(Throughput::Bytes(HEADER.len() as u64));

    group.bench_function("parse_line_122b_dashboard_header", |b| {
        b.iter(|| {
            let line = parse_line(black_box(HEADER)).expect("bench input parses");
            black_box(line)
        });
    });

    group.bench_function("parse_line_122b_plain_text", |b| {
        let plain = "deploy ok — api v2 · region eu-west · canary 12% · plain tail";
        b.iter(|| {
            let line = parse_line(black_box(plain)).expect("plain text parses");
            black_box(line)
        });
    });

    group.finish();
}

criterion_group!(benches, bench_markup);
criterion_main!(benches);
