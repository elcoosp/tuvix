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
