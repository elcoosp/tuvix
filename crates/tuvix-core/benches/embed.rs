//! M12 embed benches: the embedding boundary under measurement.
//!
//! Targets (docs/BENCHMARKS.md, the M12 scorecard): parse + apply of a
//! 10,000-mutation packet against the 200x60 budget viewport inside the
//! B15 budget (more than 50,000 mutations/sec — the §8 gate 1), and the
//! snapshot round trip. The ABI-level B15 gate itself lives in
//! `tuvix-ffi/tests/bench_gates.rs`; these benches measure the safe core
//! the ABI wraps.

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use tuvix_core::buffer::Buffer;
use tuvix_core::embed::{Mutation, PacketWriter, decode_snapshot, encode_snapshot, parse_packets};
use tuvix_core::geometry::Rect;
use tuvix_core::style::{Color, Style};

/// The architecture budget viewport: 200x60.
const WIDTH: u16 = 200;
const HEIGHT: u16 = 60;
/// Mutations per packet: the batching unit of the §5.1 mechanism.
const MUTATIONS: usize = 10_000;

/// One-cell text mutations scattered across the budget viewport — the
/// realistic batch shape of a list-driven host UI.
fn batch_packet() -> Vec<u8> {
    let mut writer = PacketWriter::new();
    for index in 0..MUTATIONS {
        let x = u16::try_from((index * 37 + 11) % usize::from(WIDTH)).unwrap_or(u16::MAX);
        let y = u16::try_from((index * 53 + 7) % usize::from(HEIGHT)).unwrap_or(u16::MAX);
        writer = writer.push(&Mutation::SetText {
            x,
            y,
            text: "X".to_owned(),
            style: Style::new().fg(Color::Red),
        });
    }
    writer.finish()
}

fn bench_embed(c: &mut Criterion) {
    let mut group = c.benchmark_group("embed");
    group.throughput(Throughput::Elements(MUTATIONS as u64));

    let packet = batch_packet();

    // e1 — the parse half of the boundary: strict decode of 10k mutations.
    group.bench_function("e1_parse_10k_packet_200x60", |b| {
        b.iter(|| {
            let mutations = parse_packets(black_box(&packet)).unwrap();
            black_box(mutations.len());
        });
    });

    // e2 — parse + apply: the full host-to-buffer cost of one batch.
    group.bench_function("e2_apply_10k_packet_200x60", |b| {
        b.iter_batched(
            || Buffer::empty(Rect::new(0, 0, WIDTH, HEIGHT)),
            |mut buffer| {
                let mutations = parse_packets(black_box(&packet)).unwrap();
                for mutation in &mutations {
                    mutation.apply(&mut buffer);
                }
                black_box(());
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();

    // e3 — the outbound half: snapshot encode + decode of the painted
    // viewport (throughput reported in bytes of the encoded frame).
    let mut painted = Buffer::empty(Rect::new(0, 0, WIDTH, HEIGHT));
    for row in 0..HEIGHT {
        let line =
            format!("row {row:02}: the quick brown fox jumps over the lazy dog — 汉字 spacing");
        painted.set_string(0, row, line, Style::new());
    }
    let snapshot = encode_snapshot(&painted);

    let mut roundtrip = c.benchmark_group("snapshot");
    roundtrip.throughput(Throughput::Bytes(snapshot.len() as u64));
    roundtrip.bench_function("e3_encode_snapshot_200x60", |b| {
        b.iter(|| encode_snapshot(black_box(&painted)));
    });
    roundtrip.bench_function("e4_decode_snapshot_200x60", |b| {
        b.iter(|| decode_snapshot(black_box(&snapshot)).unwrap());
    });
    roundtrip.finish();
}

criterion_group!(benches, bench_embed);
criterion_main!(benches);
