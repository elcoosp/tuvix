//! M12 timing gate: B15, the last unmeasured row of the architecture's §8
//! budget table — FFI mutation throughput. Run with
//! `cargo test -p tuvix-ffi --release --ignored`; results are recorded in
//! docs/BENCHMARKS.md.

use std::time::Instant;

use tuvix_core::embed::{Mutation, PacketWriter};
use tuvix_core::style::{Color, Style};
use tuvix_ffi::{TVX_ERR_PACKET, TVX_OK, tvx_buffer_apply, tvx_buffer_free, tvx_buffer_new};

const WIDTH: u16 = 200;
const HEIGHT: u16 = 60;
/// Mutations per packet: the batching unit of the §5.1 mechanism.
const MUTATIONS_PER_PACKET: usize = 10_000;
/// Packets applied per measurement: amortizes the handle setup.
const PACKETS: usize = 20;

/// Builds one packet of `MUTATIONS_PER_PACKET` scattered one-cell text
/// mutations over the budget viewport.
fn batch_packet() -> Vec<u8> {
    let mut writer = PacketWriter::new();
    for index in 0..MUTATIONS_PER_PACKET {
        let x = ((index * 37 + 11) % usize::from(WIDTH)) as u16;
        let y = ((index * 53 + 7) % usize::from(HEIGHT)) as u16;
        writer = writer.push(&Mutation::SetText {
            x,
            y,
            text: "X".to_owned(),
            style: Style::new().fg(Color::Red),
        });
    }
    writer.finish()
}

#[test]
fn packet_header_and_count_are_well_formed() {
    // The gate's own fixture must be a valid packet — checked without the
    // timing loop so a malformed batch fails loudly, not slowly.
    let packet = batch_packet();
    let mutations = tuvix_core::embed::parse_packets(&packet).unwrap();
    assert_eq!(mutations.len(), MUTATIONS_PER_PACKET);
}

#[test]
#[ignore = "timing gate: run with `cargo test --release --ignored` and record in docs/BENCHMARKS.md"]
fn b15_ffi_mutation_throughput_above_fifty_thousand_per_second() {
    let packet = batch_packet();
    let packet_len = packet.len();
    let packet_bytes = u64::try_from(packet_len).unwrap();

    unsafe {
        let handle = tvx_buffer_new(WIDTH, HEIGHT);
        let mut consumed = 0usize;
        let mut err_kind = 0i32;
        let mut err_offset = 0usize;

        // Warm-up: one apply, validated.
        let result = tvx_buffer_apply(
            handle,
            packet.as_ptr(),
            packet_len,
            std::ptr::addr_of_mut!(consumed),
            std::ptr::addr_of_mut!(err_kind),
            std::ptr::addr_of_mut!(err_offset),
        );
        assert_eq!(result, TVX_OK);
        assert_eq!(consumed, packet_len);

        let total_mutations = u64::try_from(MUTATIONS_PER_PACKET * PACKETS).unwrap();
        let start = Instant::now();
        for _ in 0..PACKETS {
            let result = tvx_buffer_apply(
                handle,
                packet.as_ptr(),
                packet_len,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            assert_ne!(result, TVX_ERR_PACKET);
        }
        let elapsed = start.elapsed();

        tvx_buffer_free(handle);

        let per_second = total_mutations as f64 / elapsed.as_secs_f64();
        // The §8 gate 1 budget: more than 50,000 mutations per second.
        assert!(
            per_second >= 50_000.0,
            "B15 gate failed: {per_second:.0} mutations/sec (budget 50,000/sec); \
             {total_mutations} mutations took {elapsed:?} ({packet_bytes} bytes per packet)"
        );
        println!(
            "B15: {per_second:.0} mutations/sec ({total_mutations} mutations, {elapsed:?}, \
             {:.1} ns/mutation)",
            elapsed.as_nanos() as f64 / total_mutations as f64
        );
    }
}
