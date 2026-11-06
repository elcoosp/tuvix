# Benchmark baselines

Recorded with criterion 0.8.2.

- Host: Linux x86_64 (CI-class sandbox), Rust 1.99.1, release profile
- M0 baselines: 80x24 = 1920 cells
- M1 gate viewport: 200x60 = 12,000 cells (the architecture budget size)
- Reproduce: `cargo bench` (criterion), timing gates with
  `cargo test --release --workspace -- --ignored --nocapture`
- Policy: benchmarks compile in CI (`cargo bench --no-run`); hot-path PRs
  must quote the delta. Timing gates are `#[ignore]`d tests with generous
  ceilings; the deterministic B9 byte-ratio gate runs in normal CI. Since
  M4 the full gate suite also runs on a schedule
  (`.github/workflows/perf.yml`, weekly + on demand) — benchmarks are not
  just recorded, they are enforced.

## M13 scorecard (2026-11)

The M13 additions (`tuvix-core/benches/motion.rs`, the B16 gate in
`tuvix-core/tests/m13_motion.rs`, criterion 0.8.2, release profile,
same CI-class sandbox, Rust 1.99.1).

| Bench | What it measures | Measured | Verdict |
| --- | --- | --- | --- |
| `motion/a1_ease_out_elastic` | one eased evaluation — the priciest curve, paid once per animated element per frame | **~9.3 ns** | pass |
| `motion/a2_ease_out_bounce` | the piecewise bounce, the second-costliest shape | **~3.6 ns** | pass |
| `motion/a3_animator_advance_1000_tweens_16ms` | the B16 shape: 1,000 infinite alternating tweens advanced one 16 ms tick | **~5.7 us** (176.5 Melem/s — ~5.7 ns per tween per tick) | pass |
| `motion/a4_tween_value_read` | one value read on a warm tween — what every animated widget pays to style itself | **~17 ns** | pass — a full 16-bar easing gallery costs a rounding error next to the ~150 us dashboard paint |

Gate:

| Gate | Target | Measured | Verdict |
| --- | --- | --- | --- |
| B16 1,000 concurrent tweens advance in one frame tick (600-frame run, 10 s infinite alternating tweens, OutCubic) | worst advance <= 1 ms | **worst 18.4 us, mean 5.9 us** | pass — **54x headroom**; motion is noise next to the B2 frame it rides on, and `is_idle` makes the cost exactly zero once everything settles |

## M12 scorecard (2026-11)

The M12 additions (`tuvix-core/benches/embed.rs`, the B15 gate in
`tuvix-ffi/tests/bench_gates.rs`, criterion 0.8.2, release profile,
200x60 budget viewport = 12,000 cells, same CI-class sandbox,
Rust 1.99.1).

| Bench | What it measures | Measured | Verdict |
| --- | --- | --- | --- |
| `embed/e1_parse_10k_packet_200x60` | the strict decode of a 10,000-mutation packet (header, six ops, style encoding, control-character rejection armed) | **~525 us** | pass — **~19 ns per mutation**, ~4 Melem/s; the strict parser is not the bottleneck |
| `embed/e2_apply_10k_packet_200x60` | parse + apply through the safe core: 10,000 one-cell text mutations landing on the buffer | **~1.21 ms** | pass — the batch lands at **~826k mutations/sec** through pure safe code; the §8 budget is about the ABI boundary, measured next |
| `snapshot/e3_encode_snapshot_200x60` | the outbound half: a fully painted 200x60 viewport into a snapshot packet (empty rows one byte each) | **~171 us** | pass |
| `snapshot/e4_decode_snapshot_200x60` | a snapshot packet back into a fresh `Buffer`, cell-for-cell | **~172 us** | pass — encode + decode together cost ~0.34 ms, ~6x under the B2 frame budget |

Gate:

| Gate | Target | Measured | Verdict |
| --- | --- | --- | --- |
| B15 FFI mutation throughput (10k-mutation packet x20 through the raw C ABI, 200x60) | > 50,000 mutations/sec | **~9.3M mutations/sec** (108 ns/mutation, strict parse included) | pass — **185x headroom**; the §8 budget table's last unmeasured row closes, and batching is the point: one boundary crossing per 10,000 mutations |

## M11 scorecard (2026-11)

The M11 additions (`tuvix-core/benches/interaction.rs`,
`tuvix-widgets/benches/scrollable.rs` and the B14 gate in
`tuvix-widgets/tests/m11_interaction.rs`, criterion 0.8.2, release
profile, 200x60 budget viewport = 12,000 cells, same CI-class sandbox,
Rust 1.99.1).

| Bench | What it measures | Measured | Verdict |
| --- | --- | --- | --- |
| `interaction/keymap_single_key_step` | one bound key through the trie root (a dashboard map with 4 singles and 2 chords bound) | **~8 ns** | pass — a resolved keystroke costs a rounding error next to the ~295 us B10 input-to-paint p99 |
| `interaction/keymap_chord_prefix_step` | the chord walk, averaged over the two steps of `g g` (open the prefix, complete and reset) | **~10 ns** | pass — chords are free; the matcher keeps its position between events |
| `interaction/keymap_miss_step` | an unbound key scanning the root's six siblings | **~14 ns** | pass — the worst root case is still two orders of magnitude below a frame |
| `interaction/focus_cycle_8_fields` | one wrap-around advance over an 8-field ring | **~0.7 ns** | pass — focus routing is an index increment |
| `scrollable/first_paint_400_rows_200x60` | the one-time cost: the child paints its full 400-row content into the fresh scratch surface, the top window blits | **~1.9 ms** | pass — paid once per invalidation, not per frame; the child's own paint dominates |
| `scrollable/scrolled_repaint_400_rows_200x60` | the B14 cost: a scroll step over warm cache — window move + blit only | **~166 us** | pass — **7% of the B2 steady-frame budget**; a scroll storm costs the window, never the content |
| `scrollable/invalidated_repaint_400_rows_200x60` | an app-paid content change: invalidate + child repaint + blit | **~1.45 ms** | pass — the child's 400-row paint dominates; the viewport adds only the blit |
| `scrollable/scrolled_repaint_with_scrollbar_200x60` | the B14 blit with an attached scrollbar (viewport shrunk by one column) | **~168 us** | pass — the scrollbar paint is noise |

Gate:

| Gate | Target | Measured | Verdict |
| --- | --- | --- | --- |
| B14 viewport scroll step @200x60 over 400 content rows, p99 over 200 frames | <= 1 ms | **~206 us** (median 157 us, max 306 us — every frame allocates its fresh output buffer) | pass — **4.8x headroom**; scrolling can never push a frame over the B2 line by itself |

## M10 scorecard (2026-11)

The M10 additions (`tuvix-core/benches/replay.rs`,
`tuvix-widgets/benches/widgets.rs` M10 group, the B12 gate in
`tuvix-core/tests/bench_gates.rs` and the B13 memory gate measured
through the `tuvix-memgate` counting allocator, criterion 0.8.2,
release profile, 200x60 budget viewport = 12,000 cells, same CI-class
sandbox, Rust 1.99.1).

| Bench | What it measures | Measured | Verdict |
| --- | --- | --- | --- |
| `replay/r1_replay_100_frames_200x60` | the golden loop: 100 recorded full-screen frames (50 events) replayed through a deterministic app — parse + paint + serialize + parse per step | **~47 ms** (~470 us/step) | pass — a full session replay costs a fraction of a second; the regression-test use case is instant by construction |
| `replay/r2_seed_reconstruct_200x60` | the bounded-window checkpoint: parse the first retained frame back into a live buffer | **~53 us** | pass — "resume from any checkpoint" costs one parse |
| `widgets/text_area_60_rows_200x60` | the editor paint: 60-line document, window sync per paint, fresh buffer | **~165 us** | pass — inside the B6 budget |
| `widgets/text_area_type_200_chars` | the editing hot path: 200 chars through `insert` (coalescing undo machinery per keystroke) | **~53 us** | pass — ~266 ns per keystroke including undo bookkeeping |

The new gates:

| Gate | Target | Measured | Verdict |
| --- | --- | --- | --- |
| B12 replay of 100 recorded frames @200x60 (golden asserted) | <= 300 ms | **~178 ms** (gate app paints a heavier dashboard than the bench) | pass — ~1.8 ms/step, 1.7x headroom; a divergent replay fails the gate naming the frame |
| B13 persistent pipeline + 60 recorded frames @200x60, live bytes | <= 10 MB | **~2.3 MB** | pass — **4.4x headroom**; the §1 budget table is now fully measured |

M10 adds no cost to the render paths: the clipboard encoder runs only
when an app calls `set_clipboard`, replay is a cold-path consumer of
recordings, and the TextArea undo stacks are app state, not pipeline
state. All M0-M9 gates hold unchanged (B2 507 us, B5 21 ns, B6 125 us,
B8 5 ns, B10 p99 ~0.3 ms, B11 scrub p99 ~195 us).

## M9 scorecard (2026-11)

The M9 additions (`tuvix-core/benches/devtools.rs`,
`tuvix-widgets/benches/widgets.rs` M9 group and the B11 gate in
`tuvix-core/tests/bench_gates.rs`, criterion 0.8.2, release profile,
200x60 budget viewport = 12,000 cells, same CI-class sandbox,
Rust 1.99.1).

| Bench | What it measures | Measured | Verdict |
| --- | --- | --- | --- |
| `devtools/d1_reconstruct_screen_200x60` | the scrub path: parse a full recorded 200x60 snapshot back into a `Screen` | **~127 us** | pass — "jump to any moment" costs ~1/16 of a 2 ms frame budget; the DevTools story is instant by construction |
| `devtools/d2_screen_to_buffer_200x60` | paste the reconstructed grid into a live `Buffer` (the inspector path) | **~48 us** | pass |
| `devtools/d3_screen_diff_200x60` | frame-to-frame diff over the union grid | **~29 us** | pass — ~2.4 ns/cell |
| `devtools/d4_record_frame_200x60` | `SessionRecorder::on_frame` on top of the snapshot the preview hook already serializes | **~1.2 us** | pass — recording is 0.6% of a 200 us frame |
| `devtools/d5_session_json_export_100_frames` | the golden-session document for 100 full-screen frames | **~1.3 ms** (~13 us/frame) | pass — an export is one frame's cost per hundred frames |
| `widgets/frame_inspector_200x60` | the scrub view: bordered box + historical paste + timeline footer over the full budget viewport | **~96 us** | pass |
| `widgets/screen_reconstruct_200x60` | the same reconstruction from the widgets side | **~91 us** | pass |

The B11 gate (`§8`, DevTools costs) runs the scrub loop and the recorder
against a real captured 200x60 snapshot:

| Gate | Target | Measured | Verdict |
| --- | --- | --- | --- |
| B11 scrub p99 over 200 reconstructions @200x60 | <= 2 ms | **~195 us** (median 131 us) | pass — **10x headroom** |
| B11 recording overhead per frame @200x60 | <= 50 us | **~1.5 us** | pass — **32x headroom** |

M9 adds no cost to the render paths: recording is a preview-hook sink
off the hot path, and the M0-M8 gates hold unchanged (B2 507 us, B5
21 ns, B6 125 us, B8 5 ns, B10 p99 ~0.3 ms).

## M8 scorecard (2026-11)

The M8 additions (`tuvix-core/benches/scheduler.rs` and the B10 gate in
`tuvix-core/tests/bench_gates.rs`, criterion 0.8.2, release profile,
200x60 budget viewport = 12,000 cells, same CI-class sandbox,
Rust 1.99.1).

| Bench | What it measures | Measured | Verdict |
| --- | --- | --- | --- |
| `scheduler/s1_idle_tick_200x60` | a scheduler tick with no render request — the O(1) static-content decision (no probe call, no diff walk) | **~0.33 ns** | pass — the architecture's gate 6 budget (< 1% static CPU) realized as a sub-nanosecond decision |
| `scheduler/s2_coalesced_10_requests_200x60` | ten `request_render` calls collapsing into one animated 20-band frame at 200x60 | **~201 us** | pass — coalescing is free; the cost is the frame itself (~11% of the 2 ms B2 budget) |
| `scheduler/s3_yielded_tick_probe_true` | the priority decision: a pending render yielding to input (probe call + yield bookkeeping) | **~2.0 ns** | pass — preemption costs nothing; the abandoned paint is never started |
| `scheduler/s4_cancelled_tick_pre_token` | a pending render abandoned by an already-cancelled token (zero terminal I/O) | **~5.1 ns** | pass |
| `scheduler/s5_cancellation_checkpoint` | ten `is_cancelled()` stage-boundary checks (~2.1 ns each) | **~21 ns** | pass — the per-frame cancellation overhead is two atomic-load chain walks |

The B10 gate (`§8` gate 5) runs the full loop: 10,000 keystrokes through
the event channel, each drained by the scheduler and coalesced into one
animated frame (a 20-row band repaints per keystroke, persistent DOM,
200x60):

| Gate | Target | Measured | Verdict |
| --- | --- | --- | --- |
| B10 input-to-paint p99 over 10,000 keystrokes @200x60 | <= 8 ms | **~295-345 us** (max 1.0 ms) | pass — **23x headroom**; the M2 queue (~29 ns/event) and the M4 journal (banded diffs) leave the entire budget to the paint itself |

M8 adds no cost to the classic draw paths: `draw()` and
`draw_persistent()` delegate through the same body with a never-cancelled
token, so all M0-M7 gates hold unchanged (B2 507 us, B5 21 ns, B6
125 us, B8 journal 5 ns after the M8 changes, re-measured).

## M7 scorecard (2026-11)

The M7 additions (`tuvix-core/benches/markup.rs` and
`tuvix-widgets/benches/widgets.rs`, criterion 0.8.2, release profile,
200-wide budget rows, same CI-class sandbox, Rust 1.99.1).

| Bench | What it measures | Measured | Verdict |
| --- | --- | --- | --- |
| `markup/parse_line_122b_dashboard_header` | 122 B dashboard header: 8 regions, named colors, modifiers, an `on #rrggbb` background, one escape | **~0.85 us** | pass — markup is authored once per string, parsed per render; a full-frame header costs a rounding error next to the ~8.6 ns/cell diff walk |
| `markup/parse_line_122b_plain_text` | the same 122 B with no tags (the parser's floor) | **~0.28 us** | pass |
| `widgets/bar_chart_2_groups_200x60` | two labeled groups of three bars, values + labels, into a fresh 200x60 buffer | **~47 us** | pass — ~3.9 ns/cell including the fresh-buffer allocation, inside the B6 budget |
| `widgets/tree_256_nodes_200x60` | 256-node tree (16 roots x 15 children), one selection, minimal-scroll sync, fresh 200x60 buffer | **~166 us** | pass — flattening 256 nodes dominates; the visible-window paint is a fraction of it |

## M6 scorecard (2026-11)

The M6 additions (`tuvix-core/benches/layout.rs` and
`tuvix-widgets/benches/widgets.rs`, criterion 0.8.2, release profile,
200-wide budget rows, same CI-class sandbox, Rust 1.99.1).

| Bench | What it measures | Measured | Verdict |
| --- | --- | --- | --- |
| `layout/horizontal_20_flex_center` | 20 fixed segments + the Flex::Center placement pass over the leftover | **~30 ns** | pass — placement costs nothing over the cached solve path (the M5 weighted pass measured ~29 ns) |
| `layout/horizontal_3_flex_space_between` | 3 fixed segments, leftover distributed as gaps | **~29 ns** | pass |
| `widgets/tabs_8_titles_200x1` | 8-title tab row with one highlighted selection into a fresh 200-wide buffer | **~3.8 µs** | pass — span-by-span paint, cold buffer allocation included |
| `widgets/line_gauge_200x1` | labeled one-row gauge, 200 columns | **~3.1 µs** | pass |
| `widgets/input_80_chars_200x1` | 80-char value scrolled to the tail, fresh 200-wide buffer | **~3.9 µs** | pass — includes the display-offset sync and the borrowed tail write |

## M5 scorecard (2026-11)

The M5 additions (`tuvix-core/benches/layout.rs` and
`tuvix-backend-ansi/benches/gates.rs`, criterion 0.8.2, release profile,
200x60 budget viewport = 12,000 cells, same CI-class sandbox, Rust 1.99.1).

| Bench | What it measures | Measured | Verdict |
| --- | --- | --- | --- |
| `layout/vertical_20_weighted` | 20-segment split mixing Length/Weight/Min — the new Weight distribution pass | **~29 ns** | pass — the terminal-weighted pass costs nothing over the cached solve path |
| `gates/m5_ansi_backend_b6_incremental_frame` | paint + diff + ANSI serialization, 5 dirty rows @200x60, through the dependency-free backend | **~121 µs** | pass — the B6 budget (≤ 500 µs) holds with 4x headroom, no terminal library in between |
| `gates/m5_ansi_backend_full_frame` | the same backend writing every cell of the 200x60 viewport (blank → full) | **~214 µs** | pass — full-frame throughput for the scorecard; the simple backend is byte-economical (SGR persistence + EL elision carry over unchanged) |

The M5 hyperlinks add OSC 8 emission only when a frame actually carries
hyperlinks (memoized transitions, like SGR), so frames without links are
byte-identical to M4 output and all M0–M4 gates hold unchanged.

## M4 scorecard (2026-11)

The M4 persistent-DOM frame journal (`tuvix-core/benches/journal.rs`,
criterion 0.8.2, release profile, 200x60 budget viewport = 12,000 cells,
same CI-class sandbox).

| Bench | What it measures | Measured | Verdict |
| --- | --- | --- | --- |
| J1 `journal/b8_journal_noop_frame_200x60` | persistent-DOM frame whose closure paints nothing | **~3.8 ns** | pass — the B8 budget (50 us) is finally met at the full budget viewport: **~13,000x headroom**, and ~30,000x cheaper than the immediate-mode identical-frame walk (103-125 us) |
| J2 `journal/journal_partial_tick_band_200x60` | one 20-column ticker band repaints; 59 of 60 rows untouched DOM | **~1.3 us** | pass — a partial frame costs ~1% of the full diff walk over the same screen |
| J3 `journal/journal_full_repaint_200x60` | full repaint through the journal path (DOM reset + full diff + whole-buffer reference sync) | **~234 us** | pass — the retained-mode sync adds ~2x over the classic 103-125 us walk only on the full-repaint path; partial frames pay band-cell copies instead |
| J4 `journal/journal_empty_check_200x60` | the O(1) decision itself (empty-journal check) | **~282 ps** | pass — constant time, viewport-independent |

Gate suite after M4 (release profile, `cargo test --release --workspace --
--ignored --nocapture`, all passing):

| Gate | Target | Measured | Verdict |
| --- | --- | --- | --- |
| B8 **journal** no-op frame @200x60 | <= 50 us | **4 ns** | pass (12,500x headroom) — the original B8 budget, finally met |
| B2 steady frame (1k cells changed, full stack, 200x60) | <= 2 ms | **0.35-0.38 ms** | pass (5-6x headroom) |
| B5 incremental layout (cached 20-constraint split) | <= 0.5 ms | **22 ns** | pass |
| B6 paint + diff + ANSI, 5 dirty rows @200x60 | <= 0.5 ms | **109 us** | pass |
| B8 immediate no-op frame @200x60 (classic path, unchanged) | (see note) | 105 us | documented deviation — superseded by the journal path above |
| B9 1-row-change bytes vs full frame | <= 10% | **1.65%** | pass (unchanged) |

B8 note, **closed in M4**: the 50 us budget was written for the v3
architecture's O(1) "journal empty" frame decision. The immediate-mode
M1 equivalent (an identical-frame diff walk) met the budget only at the
80x24 reference screen; at 200x60 it cost 103-125 us. M4 ships exactly
the mechanism the note asked for — the persistent paint buffer plus a
frame journal — and `draw_persistent` makes the no-op decision O(1):
~4 ns at the full budget viewport. The classic `draw` path keeps its
measured cost and its documented deviation; apps that repaint everything
every frame pay nothing for it.

M4's theme system and IPC previews add no frame-path code (styles are
classified once at widget build time; previews serialize off the render
critical path after flush), so all M0-M3 gates hold unchanged.

## M3 scorecard (2026-11)

The M3 widget family (`tuvix-widgets/benches/widgets.rs`, criterion 0.8.2,
release profile, 200x60 budget viewport = 12,000 cells, same CI-class
sandbox). Each bench paints into a fresh `Buffer`; clone cost of the
widget value is included (widgets are cheap value types).

| Bench | What it measures | Measured | Verdict |
| --- | --- | --- | --- |
| W1 `widgets/gauge_200x60` | full-height 75% gauge with style, fresh buffer | **~126 us** (95 Melem/s) | pass — a gauge repaint is cheaper than the diff walk over the same area |
| W2 `widgets/sparkline_512_values_200x60` | 512-value series, 200 columns | **~100 us** (120 Melem/s) | pass |
| W3 `widgets/list_1k_items_200x60` | 1,000 items, selection 500, highlight gutter | **~245 us** (49 Melem/s) | pass — paint touches only the ~60 visible rows |
| W4 `widgets/table_200_rows_200x60` | 3 solved columns + header + selection gutter | **~170 us** (71 Melem/s) | pass — the Layout-cached column solve is amortized |
| W5 `widgets/chart_10k_points_line_200x60` | 10,000-point line dataset, both axes labeled | **~539 us** (22 Melem/s) | pass — dominated by the 10k-point map + Bresenham walks |
| W6 `widgets/canvas_braille_full_200x60` | braille grid, full-diagonal line + rectangle outline | **~105 us** (114 Melem/s) | pass — the 8x dot resolution costs one `Vec<u8>` fill |
| W7 `widgets/scrollbar_200x60` | vertical right track, integer thumb math | **~32 us** (375 Melem/s) | pass |
| W8 `gates/m3_dashboard_paint_200x60` | blocked gauge + 60-item list + 60-row table on one screen | **~150 us** (80 Melem/s) | pass — the full widget paint of a dashboard is ~12% of the 1.2 ms frame budget |

M3 adds no new frame-path code (widgets paint into buffers; the diff and
ANSI encoder are unchanged), so the B2/B5/B6/B8/B9 gates from M1/M2 hold.

## M2 scorecard (2026-11)

The M2 runtime surfaces (`benches/runtime.rs`, criterion 0.8.2, release
profile, same CI-class sandbox):

| Bench | What it measures | Measured | Verdict |
| --- | --- | --- | --- |
| R1 `runtime/r1_send_drain_1k` | 1,000 events through one channel (send + mutex queue + drain), per-event cost | **~29.4 ns/event** (33.99 Melem/s) | pass — the queue is not a bottleneck at any human input rate |
| R2 `runtime/r2_waker_roundtrip` | one async iteration: poll (registers waker) → send (wakes) → re-poll (ready) | **~57 ns** (17.57 Melem/s) | pass — waker integration adds no measurable overhead over the queue itself |
| R3 `runtime/r3_input_stack_push_restore` | 4 mode pushes (kitty, paste, mouse, focus) + LIFO restore | **~319 ns** per full cycle | pass — cold-path mode changes cost nothing |
| R4 `runtime/r4_mouse_enable_sequences` | one mouse-capture enable transition (5 sequences) | **~15.5 ns** | pass |

The draw-path gates are unchanged by M2 (B2/B5/B6/B8/B9 below); the
runtime added no code to the frame path.

## M1 gate scorecard (2026-11)

| Gate | Target | Measured | Verdict |
| --- | --- | --- | --- |
| B2 steady frame (1k cells changed, full stack, 200x60) | <= 2 ms | **0.22-0.31 ms** | pass (6.5-9x headroom) |
| B5 incremental layout (cached 20-constraint split) | <= 0.5 ms | **29-40 ns** | pass (12,500x headroom) |
| B6 paint + diff + ANSI, 5 dirty rows @200x60 | <= 0.5 ms | **111 us** | pass (4.5x headroom) |
| B8 no-op frame decision @80x24 | <= 50 us | **~21 us** (8.6 ns/cell) | pass |
| B8 no-op frame decision @200x60 | (see note) | 103-125 us | documented deviation |
| B9 1-row-change bytes vs full frame | <= 10% | **1.65%** (76 vs 4,615 bytes) | pass (6x better) |

B8 note: the 50 us budget was written for the v3 architecture's O(1)
"journal empty" frame decision. The immediate-mode M1 equivalent is the
identical-frame diff walk. At the M0 reference screen (80x24) it meets the
budget; at the full 200x60 budget viewport it costs 103-125 us at 8.6 ns/cell
— a 4.9x per-cell improvement over M0. The O(1) decision needs the
persistent paint buffer (widgets mutating one buffer across frames), which
re-frames the double-buffer contract; M2 scoped itself to the runtime and
moved the journal to M4 (see ARCHITECTURE.md §14).

## Diff walk: M0 -> M1 (the Symbol win)

The documented M1 optimization target was the `String`-comparison cost of
the cell walk. The 16-byte compact `Symbol` (14-byte inline tier) delivered
the order-of-magnitude-per-cell win:

| Scenario (80x24, 1920 cells) | M0 median | M1 median | Delta |
| --- | --- | --- | --- |
| `80x24_10_cells_changed` | ~80.3 us | **15.8 us** | **5.1x faster** |
| `80x24_half_cells_changed` | ~81.9 us | **19.6 us** | **4.2x faster** |
| `80x24_no_changes` | ~80.6 us | **15.5 us** | **5.2x faster** |
| `80x24_full_repaint` | ~81.8 us | **16.8 us** | **4.9x faster** |

Per-cell walk cost: ~42 ns -> **~8.1-8.6 ns**. The walk is now dominated by
cache misses on the cell arrays, not by symbol comparison.

## Buffer (unchanged API, new storage)

| Scenario | M0 median | M1 median |
| --- | --- | --- |
| `set_string_ascii_80` | ~1.92 us | ~1.94 us |
| `set_string_cjk_40` | ~1.38 us | ~1.49 us |
| `set_string_styled_80` | ~1.93 us | ~1.93 us |
| `set_style_full_screen` | ~5.72 us | ~5.48 us |
| `reset_full_screen` | ~2.20 us | ~2.21 us |
| `empty_80x24_alloc` | ~16.7 us | ~5.49 us |

`Cell` shrank from 48 to 40 bytes (`Symbol` 16 + `Style` 16 + flag), which
is where the allocation-time win comes from; paint throughput was already
allocation-free for short glyphs.

## Layout

| Scenario | M0 median | M1 median |
| --- | --- | --- |
| `vertical_3_constraints` | ~83 ns | ~27 ns |
| `vertical_20_constraints` | ~239 ns | ~28 ns |
| `horizontal_6_mixed` | ~79 ns | ~27 ns |
| `gates/b5_layout_cached_20_200x60` | — | **~29 ns** |

The un-cached solve also got faster (u32 arithmetic tightening); the cache
turns repeated same-area splits into a Vec clone.

## ANSI output path (new in M1)

`gates/b6_paint_damage_ansi_5rows_200x60`: **~111 us** for paint-diff
walk (12,000 cells) + 5 dirty rows serialized through the persistent
raster-state encoder (SGR deltas, CHA cursor economy, EL elision). The
elision and persistent state are what take B9 from "every cell written"
to **76 bytes per 1-row change**.
