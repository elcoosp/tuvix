# tuvix Architecture

> **tuvix** — a high-performance terminal UI framework for Rust with
> best-in-class developer experience. This document is the definitive
> architecture for M0-M4 (the shipped foundation, runtime, widget family
> and ecosystem layer) and the roadmap beyond it.
>
> Status: M0 through M14 implemented and green. Every design decision
> below is grounded in a first-hand study of the dominant competitors
> (ratatui 0.30, crossterm 0.29), cloned and analyzed before a line of
> tuvix code was written. M1 ships the compact-cell text pipeline, the
> damage tracker and the persistent raster-state ANSI encoder. M2 ships
> the input-mode stack, the executor-agnostic async event stream and the
> panic-restoring application runner. M3 ships the widget family (Gauge,
> Sparkline, List, Table, Scrollbar, Chart, Canvas) unified behind the
> `RenderWidget` contract. M4 ships the persistent-DOM frame journal (the
> O(1) no-op frame decision), the theme system, IPC previews, Windows
> parity guarantees and benchmarks-as-CI-gate. M5 ships beyond the
> crossterm world: OSC 8 hyperlink cell attributes, the terminal-weighted
> `Constraint::Weight`, the structured JSON frame export, and two new
> platform backends — a zero-dependency ANSI renderer and a termion
> (unix) backend with its own event reader. M6 ships composition and
> interaction: the `Flex` placement pass for the solver, the popup kit
> (`Rect::centered` + `Clear`), and three new widgets — `Tabs`,
> `LineGauge` and the `Input` text field with a full editing state. M8
> ships the differentiator: the cooperative frame scheduler with
> input-priority ticks and render coalescing, cancellable frames whose
> abandoned work is discarded rather than committed, and the frame
> history sink that records every flushed screen for time-travel
> scrubbing. M9 shipped the DevTools sidecar: the session recorder, the
> `Screen` snapshot parser, the `FrameInspector` scrub view and the
> versioned session JSON document. M10 ships the application layer: the
> `TextArea` multi-line editor with exact undo/redo, `SessionReplay` —
> the golden loop that replays a recorded session through a deterministic
> app and names the diverging frame — the write-only OSC 52 clipboard,
> and the B13 memory gate that closes the architecture's budget table.
> M11 ships the interaction depth: the `Scrollable` viewport over larger
> content, the `FocusRing` focus router and the `Keymap` declarative
> binding trie. M12 ships the embedding boundary — the last crate of the
> §7 layout: the flat, versioned `TVX` mutation packet (§5.1's batching
> mechanism), the applier and the outbound frame snapshot, and the
> `tuvix-ffi` C ABI whose B15 gate measures the §8 budget table's one
> never-measured row: FFI mutation throughput.
> M13 ships the motion layer: the clock owns the value — clamped
> `Progress`, a 16-curve easing catalog pinned to an independently
> computed reference, frame-rate-independent `Tween`s with
> delay/repeat/alternate semantics, `Interpolate` over floats,
> integers and `Color`, and the keyed `Animator` whose O(1) `is_idle`
> decision stops the loop when nothing moves (B16: 1,000 concurrent
> tweens advance in ~5.7 us against a 1 ms frame budget).
> M14 ships the mouse depth: the pointer gets what the keyboard got in
> M11 — the `HitMap` click surface resolved by the painter's algorithm,
> the `GestureRecognizer` click/drag classification on an injected
> clock, and the declarative `MouseMap` router whose later bindings
> shadow earlier ones (B17: a click resolves through a 1,000-region map
> in ~765 ns).

---

## 1. Competitive analysis (what we studied, what we stole, what we fix)

| Source | Steal-worthy patterns adopted by tuvix | Weakness tuvix addresses |
| --- | --- | --- |
| **ratatui 0.30** | double-buffer + diff rendering; `diff_iter` zero-alloc lazy diff; Backend trait with associated `Error` type; `Stylize` sugar (`"x".red().bold()`); TestBackend with ASCII-grid rendering; MSRV gating; `-D warnings` CI; the `init()`/`restore()` lifecycle with a chained panic hook (M2); the Render-trait unification idea with an associated `State` (M3, see §14 for the blanket-impl rejection); scroll-as-configuration `Paragraph` (M3); the List/Table highlight-symbol gutter alignment pattern (M3) | per-cell diff dispatch (tuvix emits **runs**); benches exist but never run in CI (tuvix compiles them in CI from day one); diff soundness for wide chars fixed reactively (tuvix pins it with a randomized differential test); `"x".red()` allocates a `String` per call (fixed by M1 zero-copy spans); `restore()` re-enables nothing it never disabled — tuvix's runner tracks an input stack and unwinds it in LIFO order; ratatui's `Cell` name collides with buffer cells in user preludes (tuvix ships `TableCell`) |
| **crossterm 0.29** | event vocabulary shape (`kind`/`state`/modifiers, Paste, Resize, Focus); SGR memoization technique in the backend; kitty keyboard protocol surface; `NO_COLOR` support for free | crossterm memoizes nothing itself (every command emits full SGR) — tuvix's backend does state-diffed SGR; app code today couples directly to crossterm types — tuvix defines a backend-agnostic event model with a lossless adapter; crossterm's async `EventStream` pulls futures-core + tokio plumbing — tuvix ships a std-only, executor-agnostic stream with the same waker semantics |
| **Layout** | ratatui's constraint vocabulary (Length/Min/Max/Percentage/Ratio/Fill) | ratatui 0.30 pulls a full cassowary port (`kasuari`) + LRU cache for every split — tuvix M0 ships a deterministic O(n) three-pass solver with a precisely specified outcome (kasuari is the documented upgrade path) |
| **notcurses** | persistent raster state across frames (M1 backend); the input-mode stack idea generalized into a declarative, testable model (M2) | notcurses's egcpool indices break cross-buffer cell clones (M1 chose 16-byte inline-first symbols instead) |

**Innovations shipped in M0-M3 (not present in competitors today):**

1. **Run-batched diff** — `Buffer::diff_runs` yields contiguous `DiffRun`
   slices per row. One cursor move + one slice write per run instead of
   per-cell `(x, y, &Cell)` dispatch. Zero allocation, lazy.
2. **Contractual diff soundness** — a 500-step LCG-seeded randomized
   differential test (`tests/diff_soundness.rs`) proves visible state ==
   logical state through every mutation class (wide chars, style patches,
   row clears, resets). ratatui fixed this class of bug reactively per issue
   report; tuvix makes the regression impossible.
3. **Pair-repair as a buffer invariant** — overwriting either half of a
   wide glyph repairs the pair (`clear_with_pair_repair`), so the logical
   buffer always models what a real terminal shows. TestBackend emulates the
   same terminal behavior, keeping the simulation faithful.
4. **Benchmarks enforced from day one** — criterion targets for buffer, diff
   and layout; CI compiles them (`cargo bench --no-run`); baselines recorded
   in `docs/BENCHMARKS.md`; M2 adds the event queue and input stack to the
   measured surfaces (`benches/runtime.rs`); M3 adds the whole widget family
   (`benches/widgets.rs`), measured paint times per widget and a full
   dashboard gate.
5. **Declarative input stack (M2)** — terminal input modes are a pure
   LIFO data structure (`runtime::input::InputStack`) whose push/pop/restore
   return exact escape sequences; sequences were verified against crossterm
   0.29's own `write_ansi` implementations and are pinned by tests. The
   runner applies transitions through `Backend::write_control` and unwinds
   them in LIFO order on restore, on drop and on panic — competitors
   either hard-code mode pairs in app code or lose track of them entirely.
6. **Executor-agnostic event stream (M2)** — `event_channel()` is a
   std-only, multi-producer/single-consumer queue whose `poll_next`
   integrates with the standard waker protocol, so tokio, smol, async-std
   and hand-rolled executors all await it with zero async dependencies.
   crossterm's equivalent requires the `event-stream` feature and pulls in
   futures plumbing.
7. **Integer-math scrollbars (M3)** — the thumb geometry is pure integer
   arithmetic (`track * viewport / content`, linear position mapping),
   deterministic across platforms, with `ScrollbarState` read-only at
   render time and position clamping in the renderer, not the state.
8. **One-contract stateful widgets (M3)** — `RenderWidget` unifies the
   stateful authoring surface: one `render(self, area, buf, state)` method
   with the state type as an associated type, callable as
   `widget.render(area, buf, &mut state)` via method-call syntax (see
   §14 for why there is no `Widget` blanket impl).
9. **The O(1) no-op frame decision (M4)** — a frame journal inside every
   `Buffer` records mutations as merged row bands; `draw_persistent`
   skips the diff walk entirely when nothing painted (~4 ns at 200x60
   where the immediate-mode walk costs 103-125 us), restricts the diff to
   painted bands on partial frames, and keeps full-repaint apps
   byte-identical to immediate mode (600-step differential gate).
10. **Enforced benchmarks (M4)** — the timing-gate suite runs on a
    schedule in CI (`perf.yml`), each gate asserting its own ceiling;
    benchmarks are budgets, not decoration.
11. **The Flex placement pass (M6)** — the solver's size passes are
    untouched; a fifth pass places the budget no constraint claimed
    (Start/End/Center/SpaceBetween/SpaceAround/SpaceEvenly) with the
    module's remainder rules pinned by tests. ratatui needs its
    constraint solver reworked to add Flex; tuvix adds one pure
    placement function over the same O(n) solve.
12. **The in-tree text field (M6)** — `Input` + `InputState` ship the
    editing state ratatui leaves to third-party crates (tui-input,
    tui-textarea): char-indexed cursor, word motions, a display window
    that scrolls minimally in *columns* (wide glyphs included), and a
    `cursor_position` the app feeds to `Frame::set_cursor_position` —
    the terminal cursor stays under the application's control.

## 2. Principles

1. **Pure core, effectful shell.** `tuvix-core` has no I/O and no terminal
   dependency. Everything testable is tested against plain buffers — and
   since M2 that includes the runtime: the input stack, the event queue and
   the runner are exercised against `TestBackend` and scripted pollers.
2. **Zero allocation on the hot path.** Diffing, run emission and layout
   solve allocate nothing; the diff iterator borrows both buffers. Runtime
   surfaces are cold paths by nature (mode changes, event arrival) and are
   still measured (`benches/runtime.rs`).
3. **Minimal I/O.** Only changed cells reach the terminal, batched as runs,
   with SGR sequences emitted only when style state actually changes.
4. **DX is a feature, not an afterthought.** Pedantic lints deny-warned in
   CI, rustdoc deny-warned, doc examples are doctests, a `prelude`, string
   styling sugar, deterministic and documented layout semantics, TDD policy,
   a one-call `try_init()`/restore-on-drop lifecycle (M2).
5. **Semver discipline.** Conventional Commits → git-cliff changelog →
   tagged releases; MSRV 1.88 enforced by a dedicated CI job.
6. **No runtime lock-in (M2).** The event stream depends on no async
   executor; the framework never spawns untracked threads; the crossterm
   reader thread is named, stop-flagged, joinable and testable through an
   injected poller.

## 3. Workspace layout

```
tuvix/
├── crates/
│   ├── tuvix-core/               # pure: geometry, style, text, buffer, diff,
│   │   │                        # layout, events, backend contract, Terminal,
│   │   │                        # runtime (input stack, event queue, runner),
│   │   │                        # journal (M4), theme (M4), preview (M4),
│   │   │                        # markup parser + span!/line!/text! macros (M7)
│   │   └── deps: bitflags, unicode-width, unicode-segmentation
│   ├── tuvix-widgets/            # Block, Paragraph (M0/M1); M3: Gauge,
│   │   │                        # Sparkline, List, Table, Scrollbar, Chart,
│   │   │                        # Canvas, shared Marker, RenderWidget impls;
│   │   │                        # M4: .theme(&Theme) mapping on every family;
│   │   │                        # M6: Clear, Tabs, LineGauge, Input(+InputState)
│   │   │                        # M7: BarChart(+Bar/BarGroup), Tree(+TreeItem/
│   │   │                        # TreeState)
│   ├── tuvix-backend-crossterm/  # ANSI renderer + event bridge + reader
│   │   └── deps: crossterm 0.29
│   ├── tuvix-backend-ansi/       # M5: the dependency-free ANSI renderer
│   │   │                        # (the M5 "simple" backend): raster-state
│   │   │                        # machine straight to any Write target
│   │   └── deps: none beyond core
│   ├── tuvix-backend-termion/    # M5: unix platform backend over termion
│   │   │                        # 4.x: raw mode, live size queries, event
│   │   │                        # reader; compiles empty off unix
│   │   └── deps: tuvix-backend-ansi, termion (unix targets)
│   └── tuvix/                    # facade: flat re-exports + prelude +
│                                 # try_init/restore_global (M2) + preview
│                                 # re-exports (M4) + termion runtime
│                                 # (M5); examples: hello, async_events,
│                                 # widgets, preview, simple_backend (M5),
│                                 # termion_hello (M5), inputs (M6),
│                                 # data_view (M7)
├── docs/                        # ARCHITECTURE.md, BENCHMARKS.md,
│                                # PLATFORMS.md (M4), RELEASE.md
├── .github/workflows/           # ci.yml, perf.yml (M4), release.yml
│                                # (+ dependabot)
└── cliff.toml                   # conventional commits -> changelog
```

Dependency graph (one direction only, no cycles):

```
tuvix (facade)
 ├── tuvix-widgets ──────────► tuvix-core
 ├── tuvix-backend-crossterm ──► tuvix-core
 ├── tuvix-backend-ansi ─────► tuvix-core
 ├── tuvix-backend-termion ──► tuvix-backend-ansi ──► tuvix-core
 └── tuvix-core
```

All six crates share `edition = "2024"`, `rust-version = "1.88"`, the
workspace `[lints]` (`clippy::pedantic` + `unsafe_code = "forbid"` +
`missing_docs`) and are gated by CI with `-D warnings`. The runtime added
**zero new dependencies**: the event stream and runner are std-only. M2,
M3 and M4 also added none: the journal, the theme system and the IPC
previews (a broadcast thread over `std::net` / `std::os::unix::net`) are
all std-only. M5 adds exactly one, deliberately: `termion 4.0.6`, the
unix platform backend — target-gated so non-unix builds of the workspace
(Windows in CI) compile the crate as an empty library. M6 and M7 added
none: Flex, the popup kit, the M7 `BarChart`, the `Tree` and the markup
parser are std-only (the markup parser reuses the existing
`unicode-width` dependency for label math). The simple ANSI
backend is dependency-free by design: it *is* the raster machine, with no
library in between.

## 4. Rendering pipeline

```
widgets ──► Buffer (cell grid) ──► diff_runs() ──► DiffRun slices ──► Backend ──► terminal
              ▲                                        │
              └────────── Terminal double buffer ──────┘
```

1. **Paint** — widgets are pure functions of `(self, area, &mut Buffer)`.
   Every frame repaints the whole screen logically (immediate mode); nothing
   tracks widget-level damage.
2. **Diff** — `Terminal` keeps two buffers. After a frame, the *previous
   visible* buffer diffs against the *newly painted* buffer, yielding lazy,
   allocation-free `DiffRun { row, start, cells: &[Cell] }` slices.
3. **Emit** — the backend performs one cursor move per run and writes cells
   back to back. Covered (`skip`) cells advance the virtual cursor without
   writing. SGR sequences (fg/bg/underline/modifier) are memoized across
   cells and reset per batch.
4. **Cursor** — `Frame::set_cursor_position` requests; Terminal shows/hides
   the cursor only when state changes (memoized commands).

### 4.1 Diff soundness rules (wide characters)

- A width-2 grapheme writes its leading cell and marks the trailing cell
  `skip`; `skip` cells are never emitted.
- Cells whose left neighbor *was* a wide glyph are compared against a blank
  cell (the terminal auto-cleared the pair), preventing stale content.
- A changed wide leading cell forces its trailing half into the same run —
  the pair is always written atomically.
- Overwriting either half of a pair repairs the buffer
  (`clear_with_pair_repair`), including cascades through adjacent pairs.
- `TestBackend` emulates the same terminal repair, so the randomized
  differential test (`tests/diff_soundness.rs`) verifies the *whole stack*,
  not just the iterator.

## 5. Layout subsystem (M0 spec, M6 placement)

`Layout::split` is deterministic O(n) with a precisely specified outcome:

1. `avail = dimension − spacing × (segments − 1)` (saturating).
2. Pass 1: `Min` segments claim their minimum.
3. Pass 2: everything else claims its desired size (`Length`,
   `Percentage`, `Ratio` resolved against `avail`; `Max` its cap), clamped
   to the remaining budget, in declaration order.
4. Pass 3: `Weight` segments split the entire remaining budget
   proportionally to their weights (weight 0 counts as 1; remainders to
   the earliest). Weights are shares *of the terminal dimension*.
5. Pass 4: any budget pass 3 could not distribute (no `Weight` present)
   goes to `Fill` segments proportionally, else equally to `Min`
   segments (`Min(0)` = flexible filler). Remainders go to the earliest
   receivers.
6. Pass 5 (M6): the `Flex` policy places the segments along the
   axis — `Start` packs flush from the leading edge (the M0-M5
   behavior), `End`/`Center` shift the leftover to the edges,
   `SpaceBetween`/`SpaceAround`/`SpaceEvenly` distribute it as gaps
   (never below `spacing`). With `Weight`/`Fill`/`Min` present the
   earlier passes consume the budget, so every mode degenerates to
   `Start`; a single segment centers under `SpaceAround`/`SpaceEvenly`.
7. Under `Flex::Start` unclaimed budget stays a trailing gap;
   unsatisfiable claims truncate from the end.

Every rule is pinned by unit tests. M1 may move to a kasuari-based solver;
the public API is designed to survive that change.

## 6. Event model

tuvix defines its own vocabulary (`tuvix_core::event`) so applications never
import a terminal library: `KeyCode` (including `Media`/`Modifier` codes),
`KeyModifiers`, `KeyEvent` (`kind`: Press/Repeat/Release, `state`), `Mouse*`,
`Event::{FocusGained, FocusLost, Key, Mouse, Paste, Resize}`. The crossterm
backend provides the lossless `to_tuvix_event` bridge. New backends translate
their native events the same way.

M2 adds the delivery mechanism (`tuvix_core::runtime::queue`):

- `event_channel()` returns `(EventSender, EventStream)` — cloneable,
  thread-safe senders; a single-consumer stream that is `poll_next`-able
  from any executor (`next_event().await`) or drainable synchronously
  (`try_next`). Waking goes through the standard waker protocol; there is
  no async-runtime dependency.
- Close semantics: dropping the stream closes the channel (`send` returns
  `false`); dropping the last sender ends the stream with `None` after the
  queue drains.
- The crossterm backend spawns a named reader thread
  (`CrosstermEvents::spawn`, 250 ms poll deadline) that translates and
  forwards native events; the poller is injectable (`EventPoller`), so the
  loop is tested with scripted sources without a terminal.

## 7. Backend contract

```rust
pub trait Backend {
    type Error: core::error::Error;
    fn draw<'a>(&mut self, content: impl IntoIterator<Item = DiffRun<'a>>)
        -> Result<(), Self::Error>;
    fn flush, size, get_cursor_position, set_cursor_position,
        hide_cursor, show_cursor, clear(ClearRegion) ...
    fn write_control(&mut self, sequence: &str) -> Result<(), Self::Error> {
        let _ = sequence; Ok(()) // default no-op; reference impls override
    }
}
```

- `draw` consumes run slices (borrowed from the frame buffer).
- Associated `Error` keeps in-memory backends infallible
  (`TestBackend::Error = Infallible`).
- `CrosstermBackend<W: Write>` is the reference implementation: ANSI output
  over any writer, SGR memoization, virtual cursor tracking, `NO_COLOR`
  honored through crossterm.
- `write_control` (M2) is the raw-sequence escape hatch the runtime uses
  for input modes; `TestBackend` records sequences in a control log, and
  `CrosstermBackend` writes them verbatim and re-anchors its cursor before
  the next draw.

## 8. Testing strategy

| Layer | Technique |
| --- | --- |
| Primitives | unit tests inline (`geometry`, `style`, `text`, `layout`, `event`) |
| Diff engine | unit tests for run grouping + wide-char transitions **and** `tests/diff_soundness.rs` (500 randomized steps, LCG-seeded, deterministic across platforms) |
| Terminal runtime | `TestBackend` integration tests (`tests/render_integration.rs`) |
| Runtime (M2) | unit tests for the input stack, queue waker semantics (counting-waker + cross-thread park-based `block_on`), panic-hook chaining (`catch_unwind`), runner lifecycle (`RecordingBackend` op log); `tests/runtime_integration.rs` gates the whole loop: input modes through a real backend, cross-thread awaits, restore-on-drop under a real panic, hook chain carrying the spec |
| Widgets (M3) | per-widget unit tests pinning every rendering rule as buffer-string assertions; `tests/m3_widgets.rs` gates the whole family through `Terminal` + `TestBackend`: the dashboard render, the `RenderWidget` contract driving List/Table/Scrollbar uniformly, cross-frame selection navigation, and the stateless family through `Widget` |
| Journal (M4) | `tests/m4_journal.rs`: banded == full for arbitrary buffer pairs (600 LCG steps, wide glyphs, ZWJ), persistent visible == logical DOM over 600 steps with mid-stream resizes, full-repaint parity between modes, no-op frames never reaching the backend (op-log backend), wide-pair repairs journal-exact; `FrameJournal` unit tests pin band merging and the full flip |
| Themes (M4) | `Theme` preset tests pinning every slot of every preset (monochrome carries no color, by assertion); `tests/m4_theme.rs` gates every widget's `.theme()` mapping through rendered-buffer style assertions |
| Previews (M4) | `serialize_ansi` byte assertions (CUP, SGR reset+set, EL collapse, skip cells, indexed/RGB/underline colors); `tests/m4_preview.rs`: per-frame recorder streaming, DOM-faithful persistent snapshots, detach, live TCP client round trip, unix-socket round trip (unix), early tap stops |
| Backends | ANSI captured into `Vec<u8>` and asserted byte-wise; event mapping table tests; scripted `EventPoller` drives the reader loop without a tty |
| Hyperlinks (M5) | `Cell`/`Buffer` unit tests (stamping, wide-pair coverage, reset); diff emits on link change and never elides linked tails; `serialize_ansi` OSC 8 transition assertions; crossterm and ANSI backends byte-assert open/close/transition/elide/clear-invalidation |
| Weight (M5) | exact-share, remainder-to-earliest, weight-0-as-1, Min-interplay, Fill-consumption, spacing, Hash/Eq and cache tests pinning the solver pass |
| Frame export (M5) | `serialize_json` byte-exact documents (blank, styled, rgb, covered, hyperlink, escaped control characters, absolute coordinates) |
| M5 backends | `tuvix-backend-ansi`: byte-exact full-machine suite (SGR deltas/persistence, CHA/CUP economy, EL, OSC 8, clear invalidation, virtual cursor). `tuvix-backend-termion`: lossless event-bridge table tests, scripted-poller reader end-to-end, byte-queue poller semantics (lone ESC, split sequences, malformed → dropped, EOF ends the stream), non-tty raw-mode failure |
| Flex (M6) | exact placement per mode (Start/End/Center/SpaceBetween/SpaceAround/SpaceEvenly) with explicit remainder rules, spacing-as-minimum, single-segment centering, vertical symmetry, Weight/Min degeneration, split-cache interplay |
| Popup kit (M6) | `Rect::centered` exact/odd/clamped/zero tests; `Clear` blank-cells, wide-pair repair, out-of-bounds no-op, Widget-trait pass-through |
| Tabs (M6) | layout with default divider/padding, selection highlight covering padding, out-of-range selection, base style + divider, custom divider/padding, clipping, block wrapping, span styles, trait pass-through |
| LineGauge (M6) | label + gap + bar layout, full/zero/clamped ratios, integer flooring, dimmed track style, span-styled labels, block inner, first-row-only in tall areas, overlong labels |
| Input (M6) | editing ops (insert, delete, word delete), clamped motions, word-skip semantics, set/clear; display-offset sync (scroll in, pull back, clamp after deletes, wide-glyph columns, zero-width); placeholder + styles + block rendering; `cursor_position` accounting, clamping and block offsets |
| Markup (M7) | every named color, modifier, hex-3/hex-6 expansion, `on <color>` backgrounds, nesting patch semantics, escape handling, strict errors (unknown token, invalid tag, unterminated region, unopened close) with byte offsets, multi-line independence; `span!`/`line!`/`text!` macro sugar over `Style`/`Color`/`Modifier` |
| BarChart (M7) | floor-scaled heights, pinned max, clamping, zero-value/no-max no-ops, bar width/gap/group-gap geometry, value text centered above the bar top (skipped at the ceiling or when wider than the bar), custom `text_value`, bar + group label rows reserved only by rendered content, trailing bar/group dropping, style layering chart→group→bar→value, theme mapping (bars = `sparkline` slot, labels = `axis` slot), block wrap, `Widget` pass-through |
| Tree (M7) | depth-first flattening over the collapsed set, box-drawing guides per level (`│` continuation, `└─`/`├─` connectors), collapsed descendants hidden, navigation over visible nodes (next/previous saturation, first/last), empty-tree selection clearing, minimal scroll for the selection, offset windows, clipping, style layering, highlight, block wrap, `RenderWidget` pass-through, theme mapping (highlight slot) |
| Widgets | rendered into plain `Buffer`s, compared as strings |
| Docs | every doc example is a doctest (`cargo test --doc`); panic-hook tests serialize on a dedicated lock so parallel test execution stays deterministic |

## 9. Performance strategy

- Benchmarks (criterion 0.8) exist for the three hot paths from day one:
  `benches/buffer.rs`, `benches/diff.rs`, `benches/layout.rs`. CI compiles
  them; `docs/BENCHMARKS.md` records baselines. M3 adds
  `tuvix-widgets/benches/widgets.rs`: one bench per widget family plus the
  `gates/m3_dashboard_paint` gate (gauge + list + table at 200x60). M4 adds
  `benches/journal.rs` (no-op, partial, full, decision-in-isolation) and
  promotes the whole gate suite to a scheduled CI workflow (`perf.yml`).
  M5 adds a `vertical_20_weighted` layout bench and the ANSI backend gate
  (`m5_ansi_backend_b6_incremental_frame` + full frame), proving the B6
  budget holds without a terminal library in between. M6 adds the flex
  placement benches (`horizontal_20_flex_center`,
  `horizontal_3_flex_space_between`) and paint benches for the new
  widgets (`tabs_8_titles_200x1`, `line_gauge_200x1`,
  `input_80_chars_200x1`). M7 adds the markup parser bench
  (`markup/parse_line_122b_dashboard_header` + a plain-text twin in
  `tuvix-core/benches/markup.rs`) and paint benches for the new widgets
  (`bar_chart_2_groups_200x60`, `tree_256_nodes_200x60`). M10 adds the
  replay benches (`replay/r1_replay_100_frames_200x60`,
  `r2_seed_reconstruct_200x60`), the editor benches
  (`text_area_60_rows_200x60`, `text_area_type_200_chars`), the B12
  replay gate (100 recorded frames, golden + 300 ms) and the B13 memory
  gate (§1's last unmeasured row) measured through the `tuvix-memgate`
  counting allocator.
- M1 measured the fix: the 16-byte `Symbol` (14-byte inline tier, thin-box
  heap tier, double-box fallback) cut the walk from ~42 to ~8.6 ns/cell
  (80 us -> 15.8 us for the 1920-cell screen). Cell shrank 48 -> 40 bytes.
  Gate measurements and targets live in `docs/BENCHMARKS.md`.
- `unsafe_code = "forbid"` workspace-wide; any future unsafe (SIMD text
  measurement, packed cells) must land behind a documented safety argument.
  The one exception is `tuvix-memgate`, the dev-only B13 counting
  allocator: a verbatim delegation to the system allocator, the
  workspace's single audited `unsafe`, documented in that crate.

## 10. Release engineering

- **Commits**: Conventional Commits (enforced by convention, parsed by
  git-cliff).
- **Changelog**: `cliff.toml` maps commit types to Keep-a-Changelog groups;
  `release.yml` generates release notes with `orhun/git-cliff-action@v4` on
  tag push and publishes to crates.io in dependency order.
- **CI gates** (`.github/workflows/ci.yml`, actions pinned to current
  majors): rustfmt, clippy pedantic `-D warnings` (all targets/features),
  tests + doctests on 3 OSes, MSRV 1.88 check, rustdoc `-D warnings`,
  release build + bench compilation. Dependabot keeps actions and cargo
  dependencies fresh weekly.

## 11. Roadmap

| Milestone | Scope | Status |
| --- | --- | --- |
| **M0 — Foundation** | workspace, diff engine, layout, events, backend contract, crossterm backend, Block/Paragraph, benches, CI/CD, git-cliff releases | **done** |
| **M1 — Text & cells** | compact inline `Symbol` (16 B, 14 B inline tier), zero-copy `Cow<'static, str>` spans, word wrapping + scroll state, row-band damage tracker, split cache, Widget/StatefulWidget traits, persistent raster-state ANSI with CHA economy + EL elision | **done** |
| **M2 — Runtime** | async event stream (std-only, executor-agnostic), input stack (kitty protocol push/pop, bracketed paste, mouse capture, focus reporting), app/runner with panic hooks and terminal restore-on-drop, one-call `try_init`/`restore_global` lifecycle, runtime benches | **done** |
| **M3 — Widgets** | Scrollbar, List, Table, Chart, Gauge, Sparkline, Canvas (braille grid); `RenderWidget` contract with associated `State`; per-widget benches | **done** |
| **M4 — Ecosystem** | persistent-DOM frame journal (O(1) no-op decision), theme system, IPC previews, Windows backend parity, benchmarks-as-CI-gate | **done** |
| **M5 — Beyond** | OSC 8 hyperlink cell attributes (diff, all renderers, previews), `Constraint::Weight` terminal-weighted layout, structured JSON frame export, platform backends beyond crossterm (`tuvix-backend-ansi` simple, `tuvix-backend-termion` unix) | **done** |
| **M6 — Composition & interaction** | `Flex` placement pass (Start/End/Center/SpaceBetween/SpaceAround/SpaceEvenly), popup kit (`Rect::centered` + `Clear`), `Tabs`, `LineGauge`, `Input` text field with full editing state; inputs example, flex + widget benches | **done** |
| **M7 — Data & declarative text** | grouped vertical `BarChart` (bar/value/group labels, style layering, trailing-edge dropping), collapsible `Tree` with path-based selection + fold state, inline markup parser with strict errors + `span!`/`line!`/`text!` macros; data_view example, markup + widget benches, ci.yml push-filter repair, release publish-order repair | **done** |
| **M8 — Responsiveness & time-travel** | cooperative `CancellationToken` (monotonic, parent-child), `Terminal::draw_cancellable` / `draw_persistent_cancellable` with stage-boundary checkpoints and discard-not-commit semantics, the `Scheduler` (input-priority ticks, render coalescing, O(1) idle decision), the `HistorySink` time-travel ring over the M4 preview hook; scheduler benches + the B10 input-to-paint gate | **done** |
| **M9 — Time-travel DevTools** | the `SessionRecorder` timeline (frames + input events on one clock, bounded ring that evicts adjacent input with the oldest frame so replays start from a frame), `Screen` — a strict parser for the canonical ANSI snapshot with byte-offset errors, reconstructing live `Buffer`s that diff and render like the real thing, `FrameInspector` (picture-in-picture scrub view with the dot-bar timeline footer), the session JSON document (zero-dep writer, strict minimal parser, full `Event` vocabulary) and `SessionRecorder::diff` as the golden-session gate; devtools + inspector benches, the B11 scrub-latency gate; snapshot-encoder repairs the parser exposed (mid-row cursor re-addressing for interior blanks, correct `58;5;n` named underline colors in both ANSI encoders) | **done** |
| **M10 — Applications: editing, replay & clipboard** | `TextArea` — the multi-line editor (char-indexed cursor, goal column, column-exact window sync, bounded op-based undo/redo with coalescing groups), `SessionReplay` — the golden loop replaying a recorded session through a deterministic app with the seed checkpoint for bounded rings and frame-precise divergence reports, the write-only OSC 52 clipboard (`Backend::set_clipboard`, in-house RFC 4648, 64 KiB cap, raster-state neutral), the B12 replay gate and the B13 memory gate closing the §1 budget table | **done** |
| **M11 — Interaction depth** | `Scrollable` — the viewport widget (any `RenderWidget` behind a scroll window: cached child paint in the state, explicit invalidation, offset clamping, wide-glyph clip/snap honesty, optional attached `Scrollbar`, mouse-wheel mapping), `FocusRing` — focus routing as a type over the app's own pane enum (wrap-around advance/retreat, lookup by value or index, explicit unfocus), `Keymap` — the declarative binding trie (single keys and multi-key chords, subset modifier semantics, press-only matching, `Pending`/`Matched`/`Miss` outcomes), the B14 viewport-scroll gate; the `interaction` benches and the `interaction` example | **done** |
| **M12 — The embedding boundary** | the flat, versioned `TVX` mutation packet (`Mutation`, `PacketWriter`, strict streaming `MutationReader` with byte-offset errors), `Mutation::apply` with the `Buffer`'s exact clip semantics, the outbound frame snapshot (`encode_snapshot`/`decode_snapshot`, `decode(encode(buf)) == buf` differential gate), `tuvix-ffi` — the C ABI (audited unsafe, `tuvix.h` shipped, capacity negotiation, pinned result codes) closing the §7 crate layout, the B15 FFI-throughput gate closing the §8 budget table; the `embed` benches and the `embed` example | **done** |
| **M13 — Motion** | the animation layer: clamped `Progress` on the clock, the 16-curve `Easing` catalog pinned to an independent reference (endpoints exact, `OutBack`/`OutElastic` overshoot preserved), frame-rate-independent `Tween` with delay/repeat/alternate semantics and the one boundary rule, `Interpolate` for floats, integers and `Color` (RGB blends per channel, discrete colors snap), the keyed `Animator` with sorted finish reports, pinned rest values and the O(1) `is_idle` static-CPU decision; the `animation` example, the motion benches and the B16 1,000-tween gate | **done** |
| **M14 — Mouse depth** | the pointer layer: the `HitMap` click surface (paint-order z-order, local coordinates, `push_clipped` with the buffer's clip discipline), the `GestureRecognizer` (clicks counted release-to-release on an injected clock, drags anchored at their origin with same-cell wobble ignored, scroll/move pass-through), the declarative `MouseMap` (`MousePattern`/`MouseClass`/`ButtonPattern`, later bindings shadow earlier ones, drags resolve against their origin, stateless `Option<C>` resolution), the `anchor` helper; the `mouse` example, the mouse benches and the B17 mouse-decision gate | **done** |

## 12. Decision log

- **Own event model instead of re-exporting crossterm** — backends are
  pluggable; adapters are lossless (tests pin it).
- **Run-batched diff instead of per-cell** — fewer backend dispatches and
  cursor moves; measured in benches; flat per-cell `diff()` remains for
  simple consumers.
- **Deterministic solver instead of cassowary in M0** — zero extra
  dependencies, exact specified behavior, easy to reason about; upgrade path
  documented.
- **Immediate-mode rendering with whole-frame repaint** — matches the
  ratatui-proven model; all smartness lives in the diff, which is fully
  tested.
- **Lints live in `Cargo.toml` `[workspace.lints]`** — the Rust 2024 way;
  `clippy.toml` unnecessary.
- **Crate split follows ratatui 0.30** — proven scaling path; facade keeps
  the user-facing API flat.
- **Runtime lives in core, effects in the backend (M2)** — the input stack,
  event queue and runner are pure and backend-generic (tested against
  `TestBackend`); only the crossterm crate touches real I/O. A new backend
  gets the whole runtime for free by implementing `Backend` + translating
  events.
- **No new dependencies for M2** — crossterm's async `EventStream` pulls
  futures plumbing; tuvix's stream is ~250 lines of std (mutex + waker +
  VecDeque), measured at ~29 ns/event throughput and ~57 ns per waker
  round trip. Locking apps to an executor was never on the table.
- **Panic restoration splits in two (M2)** — global terminal modes (raw
  mode, alternate screen, input modes) are process-global state and are
  restored from the chained panic hook without a `Terminal` handle (the
  ratatui `restore()` pattern); object-level state (cursor, memoized
  buffers) is restored by `AppRunner::drop` during unwind. Both halves are
  idempotent and mutually safe.
- **Hyperlinks are cell attributes, not style (M5)** — a URI cannot live
  in `Style` without breaking `Copy` on every cell; `Cell.hyperlink: Option<Hyperlink>`
  (Arc-backed) keeps cells cheap, participates in diff equality (a link
  change is content), and renders as OSC 8 from every ANSI emitter with
  the same memoized-state discipline as SGR. Control characters are
  sanitized at construction: a URI can never break the OSC 8 framing.
- **`Weight` consumes the remaining budget (M5)** — the solver already
  had a leftover-distribution pass (`Fill`); `Weight` is a *distribution*
  pass: after fixed segments claim theirs, weights split everything left
  exactly (remainder cells to the earliest segment, the module's one
  remainder rule). `Fill` remains for the no-weight case, so both stay
  deterministic and the spec stays a list of passes.
- **The simple backend owns no TTY (M5)** — `AnsiBackend` writes the full
  raster machine to any `Write` target and queries nothing: the host
  supplies the size (and updates it on resize), cursor positions are the
  backend's own virtual state. Platform backends compose it: termion adds
  raw mode (termios, restored on drop), live `size()` and the input
  reader — one renderer implementation, two shipping backends.
- **termion event corners are documented, not hidden (M5)** — the bridge
  maps `\r`/`\n` both to Enter (termion's parser), button-less mouse
  releases to `Up(Left)` (the protocols report releases button-less), and
  drops `Unsupported` byte runs; each corner has a name and a test. The
  stdin pump thread blocks on `read(2)` and exits at EOF (termion's own
  contract); the reader thread above it stays stop-flagged and joinable.
- **`write_control` has a no-op default (M2)** — extending the `Backend`
  trait without breaking third-party implementations; correctness holds
  because both reference implementations override it (`TestBackend`
  records, `CrosstermBackend` writes).
- **Default init enables raw mode + alternate screen only (M2)** —
  ratatui-parity minimalism; apps push the input modes they want and the
  stack unwinds exactly what they pushed.
- **Tables solve columns through the core Layout solver (M3)** — one
  constraint engine, one documented pass semantics; column spacing maps
  onto the solver's spacing, and the highlight gutter shifts the whole
  column grid so header and rows stay aligned (ratatui semantics).
- **Flex is a placement pass, not a size pass (M6)** — sizes come out of
  the same four passes regardless of the policy, so Flex composes with
  every constraint and the M0 semantics stay byte-identical under the
  default `Start`. The module keeps exactly one remainder rule per mode
  (earliest gaps first for SpaceBetween; the trailing edge absorbs the
  extra cell under Center/SpaceAround/SpaceEvenly), and spacing is the
  *minimum* gap. Modes that cannot act (a single segment under
  SpaceBetween, any mode once Weight/Fill/Min consumed the budget)
  degenerate to Start instead of inventing behavior.
- **Markup errors are strict, not silent (M7)** — an unknown token,
  an unterminated region, a `[/]` with no open region or a `[` that
  forms no tag all return a `MarkupError` with a byte offset: markup is
  authored once and reviewed once, so typos must fail loudly instead of
  rendering literally. `[[` stays the one escape; a lone `]` is literal.
- **Tree navigation takes the items (M7)** — visibility depends on the
  fold state, which only the caller's tree plus the state's collapsed
  set can answer, so `select_next`/`select_previous`/`select_last`
  accept the item slice instead of hiding a copy of the data inside the
  state (the same honesty as `ListState::select_last(count)`).
- **BarChart is vertical-only (M7)** — grouped vertical bars with
  per-bar values, per-bar labels and per-group labels cover the
  dashboard use case; a horizontal renderer would duplicate the label
  layout rather than extend it. Vertical-only keeps the layout rules
  (floor scaling, trailing-edge dropping, label-row reservation) pinned
  and one-directional.
- **Label rows are reserved by rendered content only (M7)** — a bar
  label or group label that was dropped for width reserves no row, so
  charts never show an empty label strip above bars that fill the
  whole height.
- **Input's cursor is char-indexed, its window is column-exact (M6)** —
  the editing model counts characters (a cursor can never split a
  grapheme), while the display offset syncs on display *columns* so a
  2-wide glyph near the window edge scrolls like two cells. The sync is
  a public, pure method (`sync_display_offset`) that the renderer calls
  and tests pin directly — the same state stays truthful whether the
  app renders now or later. The terminal cursor itself is not managed
  by the widget: `cursor_position` reports where it belongs and the app
  decides when to show it, mirroring the framework's cursor policy.

### The responsiveness contract (M8)

- **Cancellation is cooperative, at stage boundaries (M8)** — render
  passes are never interrupted mid-stage: a `CancellationToken` is checked
  *before* the pass (zero terminal I/O) and *after the paint closure*
  (before anything is emitted). The diff/draw/flush tail is never
  abortable: it is the cheap, allocation-free part of the pipeline, and
  abandoning it after bytes reached the backend would corrupt the raster
  state. Frame carries the token so widget loops check between widgets —
  the same place the architecture's §5.7 model puts the boundaries.
- **Abandoned work is discarded, not committed (M8)** — the whole point
  of preemption is that the terminal shows exactly what it showed before.
  Immediate mode resets the paint target to the blank canvas; persistent
  mode restores the DOM's journaled bands from the reference (the whole
  buffer when the journal flipped full), so no partial paint leaks into
  the next frame. The differential tests prove interrupted sequences end
  byte-identical to uninterrupted ones — wide pairs included.
- **The scheduler makes input priority a type (M8)** — request_render
  marks the frame dirty; ticks coalesce any number of requests into at
  most one frame; and when a render is pending while the probe reports
  waiting input, the tick yields. The probe is only consulted when there
  is work to preempt — an idle tick never touches it, which is the
  static-content budget (§8 gate 6) realized as an O(1) decision
  (measured at ~0.3 ns). Draining the event queue stays the app's job:
  the probe reports, it never consumes.
- **A render once requested is scheduled exactly once (M8)** — a
  preempted tick leaves the request pending (the input handler will
  re-render); a cancelled tick consumes it (the frame was abandoned by
  contract, so re-rendering is the caller's decision). Counters
  (frames/yields/cancellations) make the loop observable without
  instrumentation.
- **The history is a preview sink, not a pipeline stage (M8)** —
  `HistorySink` rides the M4 preview hook that already serializes every
  flushed frame; time travel records the *screen the user saw*,
  byte-identical between the immediate and persistent paths (differential
  test). It is a bounded ring with drop-oldest semantics and a dropped
  counter, cloned handles share one ring, and no-op frames are recorded
  too — quiet frames are part of the story. It stays a cold path: opt-in,
  off the render critical path, like every preview surface.

## 13. M1 decisions (2026-11)

- **`Symbol` is a 16-byte enum, not a pool index.** The notcurses-style
  egcpool (cell stores a pool index) breaks cross-buffer cell clones, which
  the diff and `TestBackend` rely on. Instead: 14-byte inline tier, 62-byte
  thin-box tier (`Box<[u8; 62]>` is a sized pointee — the enum stays 16
  bytes), and a double-box fallback for pathological graphemes (verified
  with a 200-combining-accent glyph). Equality is a flat 15-byte compare on
  the inline fast path.
- **Spans are `Cow<'static, str>`, not `Arc<str>`.** String literals — the
  dominant UI text — become `Cow::Borrowed` with zero allocation and free
  clones; owned strings move in; dynamic `&str` copies once through
  `Span::shared`. `Stylize` on `&str` goes through `Span::shared` (one
  copy, same as M0's `String`).
- **kasuari was evaluated and not adopted.** The deterministic O(n) solver
  solves 20-segment splits in ~28 ns; a cassowary pass costs microseconds
  for split-shaped problems and would have rewritten the pinned split
  semantics. M1 ships the *cache* half of "kasuari layout + cache":
  `Layout::split` memoizes per-area results (4 slots, clones start fresh),
  measured at ~29 ns per cached split. kasuari remains available if
  constraint-shaped features ever demand it.
- **EL elision lives in the diff, not the backend.** `DiffRuns` strips
  trailing blank cells (min 2) when the entire row tail past the run is
  default-blank and marks `DiffRun::elide_rest`; backends choose the erase.
  Covered (skip) cells are never stripped — a wide pair is always written
  atomically — and a non-blank tail blocks elision. The 500-step soundness
  fuzz passes through the elided path.
- **Raster state persists across frames.** The M0 backend reset SGR at the
  end of every draw; M1 keeps the memoized fg/bg/underline/modifier state
  (the notcurses model), re-anchoring after `clear()` invalidation. Same-row
  jumps emit CHA column addressing; the deterministic B9 gate measures the
  result: a 1-row change serializes to 1.65% of a full-frame redraw.
- **Damage tracking is a standalone module in M1.** `core::damage::Damage`
  implements the row-band model (same-row merge at gap <= 2, row
  independence, >50% viewport -> mark_full) as a public API and gate
  harness. Restricting the *paint* to bands requires the persistent DOM
  (M4); the tracker is its foundation and the B6 harness measures through
  it.
- **The corpus is contractual.** `tests/wide_char_corpus.rs` renders every
  corpus glyph (ZWJ emoji, VS16, skin tones, flags, CJK, halfwidth dakuten,
  combining marks) through a transition matrix plus a 600-step randomized
  fuzz; after every step the incremental render must equal a from-scratch
  rebuild byte-for-byte, coverage parity included.

## 14. M2 decisions (2026-11)

- **The input stack is a data structure, not side effects.** `InputMode`
  variants (kitty flags, bracketed paste, mouse capture, focus reporting)
  and `InputStack` (LIFO) produce `InputTransition`s — exact sequences —
  without touching a terminal. Every sequence was transcribed from
  crossterm 0.29's `write_ansi` implementations read first-hand (kitty
  push `CSI > flags u`, pop `CSI < 1 u`; mouse enable
  `1000/1002/1003/1015/1006 h` and its exact reverse; paste `?2004`;
  focus `?1004`) and is pinned byte-wise by tests. This is what makes the
  runner, the panic spec and `TestBackend` assertions possible: the same
  pure transition is what a real terminal receives and what tests assert
  on.
- **The event queue is 250 lines of std, not a dependency.** Multi-producer
  (cloneable `EventSender`), single-consumer (`EventStream`),
  waker-integrated (`poll_next` + `next_event().await`), synchronous
  (`try_next`). Close semantics match `std::sync::mpsc`: stream dropped →
  `send` lies; senders gone → stream drains then yields `None`. Measured
  at ~29.4 ns/event (send+drain 1k batch), ~57 ns per poll→wake→ready
  round trip — and tokio/smol/async-std users just `await` it like any
  stream. A park-based `block_on` ships in the examples and the
  integration gate to prove executor-freedom.
- **The reader thread is a testable component, not a fact of nature.**
  crossterm's `poll`/`read` are hidden behind `EventPoller`; the production
  `CrosstermPoller` delegates to the real global source, tests script
  events/timing/errors through a queue. The loop checks the stop flag and
  the closed stream each iteration (250 ms deadline), translates through
  the same lossless bridge, and `stop()` joins before returning. Dropping
  the stream stops the thread within one iteration — no leaked spinners.
- **Panic restoration is two idempotent halves.** The hook half (global
  modes: raw mode off, registered input modes popped, alternate screen
  left, cursor shown) runs from any thread and needs no `Terminal` handle —
  ratatui's `restore()` insight. The drop half (`AppRunner::drop` during
  unwind) restores object state and flushes the backend. The hook chains to
  the previous hook (panic messages still print); the registry spec is
  unregistered only by the runner that installed it, and normal `restore()`
  clears it.
- **Failure rollbacks keep the stack truthful.** `push_input_mode` that
  errors mid-write pops the mode back off (restore-on-drop never disables
  what never applied); `pop_input_mode` that errors pushes the mode back
  (restore-on-drop still disables what might be applied). The LIFO restore
  order was verified under a real `panic!` unwinding through the runner in
  `tests/runtime_integration.rs`.
- **Defaults are ratatui-minimal; modes are opt-in.** `try_init()` enables
  raw mode + alternate screen and installs the panic hook; apps push
  mouse/paste/kitty flags explicitly (the examples do). Kitty-capable
  terminals get the full CSI-u behavior when the app asks for it; plain
  terminals simply ignore the unsupported push, and the stack still unwinds
  exactly what was pushed.
- **The O(1) no-op frame decision moved out of M2 — and shipped in M4.**
  The B8 note hoped for a journal-based frame scheduler here, but the
  honest prerequisite is the persistent paint buffer (widgets mutating one
  buffer across frames), which changes the double-buffer contract. M4
  shipped exactly that (see section 16); the B8 budget note in
  BENCHMARKS.md is closed.

## 15. M3 decisions (2026-11)

- **`RenderWidget` has an associated `State` and no blanket impls.** The
  ratified plan was a ratatui-0.30-style unification: implement one trait,
  get `Widget` for `State = ()` via a blanket. Empirically rejected: a
  generic blanket impl stays a method-resolution candidate at every probe
  step and makes `widget.render(area, buf)` a two-candidate call (E0034)
  for every widget — a minimal repro confirmed it. The shipped design:
  `RenderWidget` is the stateful authoring contract (List, Table,
  Scrollbar) and stays the widget's ONLY `render`-named impl, so
  `widget.render(area, buf, &mut state)` resolves by method-call syntax;
  stateless widgets keep the concrete `Widget` impls they shipped with
  (`.render(area, buf)` keeps working). Concrete impls are filtered by
  arity; generic blankets are not.
- **Scroll is configuration for text, state for lists (ratatui-0.30
  parity).** `Paragraph` lost its M1 `StatefulWidget` impl and gained a
  `scroll(ScrollState)` builder: the app owns the offsets and applies a
  copy each frame. List/Table selection is genuinely stateful and stays
  that way.
- **Selection-scroll rules are pinned, not emergent.** The offset moves
  minimally so the selected item stays visible, offset counts display rows
  (multi-line items occupy several), navigation helpers move `selected`
  without knowing the item count (the app clamps; `select_last` takes the
  count so no `usize::MAX` sentinel exists), and an out-of-range selection
  renders without highlight or gutter.
- **Table columns are solved by the core Layout solver**, not a private
  formula: `Table::new(rows, widths)` takes the same
  `Constraint` vocabulary, with `column_spacing` mapping onto the solver
  spacing. The header sits above the scrolling rows and never scrolls.
- **The highlight gutter shifts the whole column grid** — when a table row
  is selected, every row (header included) renders into columns offset by
  the symbol width; non-selected rows get spaces. This is ratatui's
  alignment semantics.
- **Scrollbar geometry is integer math.** thumb length =
  `track * viewport / content` clamped to `1..=track`, position maps
  linearly across the free track, content = 0 renders a full thumb, and
  `viewport_content_length = 0` means "unknown" (track length used). No
  floats, so the same state renders identically everywhere. The renderer
  takes the state by shared reference and clamps the position itself —
  the state object stays truthful.
- **Chart axes reserve space only when labeled.** Y labels add a
  right-aligned gutter plus one gap column carrying the `│` axis line; x
  labels add one bottom row carrying the `─` line and the labels. Points
  outside the bounds and NaN points are skipped; degenerate bounds plot
  nothing. `GraphType::Line` connects consecutive points in data order
  with integer Bresenham walks (the same walk the canvas uses).
- **Canvas is a braille dot grid with layered labels.** Each cell is a
  2x4 dot matrix (0x2800 bit layout, pinned by tests); `Marker::Dot` /
  `Marker::Block` downgrade the grid to one glyph per cell. Shapes are
  `Points`/`Line`/`Rectangle` over a `Painter`; labels anchor at float
  coordinates and layer above the grid. A shared `Marker` enum serves
  Chart and Canvas — one type, no ambiguous prelude imports.
- **`TableCell`, not `Cell`.** The facade prelude exports
  `buffer::Cell`; a table cell of the same name would be an instant
  ambiguity bug for every `use tuvix::prelude::*` app.
- **M3 added zero dependencies** — every widget is a pure paint function
  over the existing core (`unicode-width` measures labels, the core
  solver lays out columns, the core `Buffer` is the paint target).

## 16. M4 decisions (2026-11)

### The persistent-DOM frame journal

- **The O(1) no-op decision is a data structure, not a heuristic.**
  `Buffer` carries an optional `FrameJournal`; every mutator (`set_string`,
  `clear`, `set_style`, `reset`, `get_mut`) records the touched region as
  merged row bands (the M1 damage model reused as a bookkeeping device).
  At frame end the journal answers exactly one question — did anything
  change? — in O(1): empty journal → no diff walk, no backend draw,
  ~4 ns at 200x60 (the B8 budget of 50 us was written for precisely this
  and is finally met; the immediate-mode walk cost 103-125 us there).
- **Persistent DOM is retained mode; immediate mode stays the default.**
  `Terminal::new_persistent` keeps the paint buffer across frames —
  widgets see their previous output and repaint only what changed —
  while `Terminal::new` keeps the classic blank-canvas contract unchanged
  (every M0-M3 test passes untouched). `draw_persistent` works on both;
  on immediate terminals it degenerates gracefully (empty closure → true
  no-op, any paint → full diff), because a blank canvas differs from the
  displayed buffer outside the painted bands.
- **The reference is synced, never swapped.** The first implementation
  swapped buffer roles after emit; the m4 differential gate caught it
  immediately — the swapped-in paint target held the pre-frame canvas, so
  the DOM never actually carried over. The shipped design keeps a fixed
  DOM buffer (`buffers[0]`) and syncs the reference from it after every
  emit: band-cell copies on partial frames (proportional to what was
  painted), a whole-buffer copy on full frames. The visible state stays
  byte-identical to immediate mode for full-repaint apps — the 600-step
  parity gate proves it.
- **Banded diff = full diff, restricted.** `diff_runs_banded` reuses the
  exact run-extension, elision and wide-pair rules of the full walk; only
  run *starts* are constrained to journal bands. A buffer-level property
  test (600 LCG steps, wide glyphs, ZWJ sequences, pair repairs) proves
  banded output is run-identical to full output for the same buffer pair.
- **Direct content writes bypass the journal — and say so.** `content` is
  a public field (tests and benches clone snapshots into it); the
  documented contract is `Buffer::mark_fully_painted()` after such writes,
  which is exactly what the immediate-mode fallback does internally.
- **J4: the decision itself is 282 ps.** The empty check is a bool +
  band-slice probe — constant time, viewport-independent, measured.

### The theme system

- **Thirteen named slots, no magic.** `Theme` is a record of `Style`
  slots (text, muted, border, title, highlight, gauge, sparkline, axis,
  scrollbar, success, warning, error, accent) — a plain data type with
  dark/light/monochrome presets and consuming builder overrides. Widgets
  map slots through one `.theme(&theme)` method each; the m4_theme gate
  pins every mapping through rendered-buffer style assertions.
- **`NO_COLOR` is a preset, not a special case.** `Theme::resolve` (pure,
  env-free, pinned by doctests) maps a present, non-empty `NO_COLOR` to
  the monochrome preset — default colors, emphasis through modifiers
  only — matching the no-color.org convention the backend already honors.
- **Zero new dependencies** — the whole system is styles over the
  existing core.

### IPC previews

- **A preview is a snapshot, not a second renderer.** `serialize_ansi`
  writes the visible buffer as a full-frame ANSI string (cursor addressing
  per row, memoized reset+set SGR, covered cells never written, one EL
  per blank tail). Cold path by construction: one serialization per
  flushed frame, opt-in through `Terminal::set_preview`.
- **The tap is a well-behaved thread, not a hidden task.** `PreviewTap`
  follows the M2 thread contract: named (`tuvix-preview`), stop-flagged,
  joinable; dropping the tap stops and joins. TCP on `127.0.0.1` works on
  every platform (Windows included); unix sockets are `cfg(unix)`.
- **Connect-then-send races resolve in the worker.** Pending accepts drain
  before every broadcast — a client that connected one instruction before
  the frame still receives it (the race was caught by the m4_preview
  gate's parallel run). Slow or dead clients drop on write failure without
  disturbing the others; a dead tap never kills a draw.
- **Previews emit even for no-op frames** — the screen is unchanged, and
  the snapshot shows exactly that, which is what makes remote watching
  trustworthy.

### Windows parity and benchmarks-as-CI-gate

- **Parity is verified, not promised.** The windows-latest CI job compiles
  and tests the whole workspace natively (plus benches since M4); core and
  widgets additionally cross-check clean for `x86_64-pc-windows-gnu`. The
  honest matrix lives in `docs/PLATFORMS.md`: everything works everywhere
  except unix-socket previews (`cfg(unix)`) and kitty flags on terminals
  that ignore them gracefully (the input stack only unwinds what it
  pushed).
- **Benchmarks graduate from recorded to enforced.** `perf.yml` runs the
  full timing-gate suite (each gate asserts its own generous ceiling) on a
  weekly schedule and on demand, and uploads the report as an artifact.
  Deterministic gates (B9 byte ratio, journal soundness, preview
  behavior) keep running on every push in the normal CI job.

### What M4 deliberately did not do

- **No widget-level dirty tracking.** The journal observes *buffer*
  mutations; widgets stay pure paint functions. A widget tree that skips
  painting unchanged widgets gets the O(1)/banded wins for free without
  the framework tracking widget identity, lifetime or equality.
- **No automatic theme propagation.** Widgets accept `.theme(&Theme)`
  explicitly; there is no global mutable theme. Apps that want one wrap
  it in their own state.
- **No preview compression or frame coalescing.** Frames are small, the
  transport is loopback, and the snapshot encoder is intentionally simple.
  If a use case outgrows it, the diff encoder already exists as the
  upgrade path.

- **Hyperlink is content in the snapshot encoder (M5), and now so is the
  cursor (M9).** The M9 parser exposed that interior blank cells wrote no
  bytes and no cursor motion: playing the snapshot (or parsing it) shifted
  every later glyph left. `serialize_ansi` now tracks the terminal cursor
  column and re-addresses with CUP when the grid position drifts — the
  same discipline the live diff encoders always had. Round-trip identity
  (`from_ansi(serialize_ansi(buf))` reconstructs the buffer byte-for-byte)
  is a differential gate, not a claim.
- **Named underline colors use the extended selector in both encoders
  (M9).** The snapshot encoder emitted them as bare 50-57/110-117 codes
  (invalid; they are bg-style codes) and the ansi backend did the same.
  Both now emit `58;5;0-15`, matching crossterm's `SetUnderlineColor`; the
  256-palette ambiguity (Indexed(n<16) underline reads back as the named
  color) is documented and confined to the underline role.
- **The session timeline pairs frames with input events (M9).** Frames
  alone (M8's HistorySink) show *what* the screen showed; replay needs
  *why*. Input recording is explicit — the app calls `record_event` when
  it handles an event — because only the app knows what it acted on. The
  bounded ring evicts the oldest frame together with the input events
  adjacent to it, so a retained timeline always starts with a frame and a
  replay is well-defined.
- **Screens are parsed, not re-exported (M9).** Reconstructing a past
  frame means parsing the canonical snapshot the pipeline already emits —
  one format, no second encoder to keep in sync, and it works on captures
  from the preview tap as well as recordings. The parser is strict
  (byte-offset errors) to pin the format, the same philosophy as the M7
  markup parser.
- **Session documents are versioned and strict (M9).** `to_json` is the
  zero-dependency writer; `from_json` is a minimal strict JSON parser
  (objects, arrays, standard escapes incl. surrogate pairs, u64 numbers)
  that rejects unknown event types, key codes and entry kinds with byte
  offsets. Hand-edited logs fail loudly instead of replaying silently
  wrong; timestamps truncate to milliseconds, making exports
  re-serialization-identical.
- **The golden session is a diff, not a snapshot assertion (M9).**
  `SessionRecorder::diff` compares frames by absolute number and events by
  position, so a regression names the frame that changed — CI prints the
  culprit instead of a wall of bytes.

## 17. M10 decisions (2026-11)

- **The editing model is the M6 `Input` contract, generalized, not
  reinvented.** The cursor stays char-indexed (it can never split a
  grapheme) while the horizontal window syncs on display columns — the
  M6 decisions hold per row, with one addition: vertical motion keeps a
  *goal column* (the display column is restored on the next longer row
  until a horizontal motion or an edit resets it), because without it
  moving through short lines destroys the column the user aimed at.
- **Undo is operation-based with coalescing groups, not snapshots.** A
  snapshot stack is O(document) per keystroke and dishonest about the
  10 MB budget; ops (`Insert`/`Delete`/`Join`/`Split`/`EraseLine`) are
  exact inverses. Typing runs coalesce into one step (a word undoes as
  a word) by adjacency checks, and undo/redo *open a new group* so a
  fresh edit never merges into older history. The stacks are bounded
  (128 default, configurable, 0 disables); bulk replaces (`set_text`,
  `clear`) reset the history and say so — a destructive replace is not
  an edit step. Two randomized gates prove undo-all is the identity,
  over the plain alphabet and over the wide corpus (multi-line pastes,
  wide glyphs, flags).
- **Replay compares screen content, not canvas bytes.** A canonical
  snapshot does not encode the viewport (a blank tail is one `ESC[K`),
  so the step buffer is sized to the *discovered* grid and golden is
  decided on the union grid — a replay painting identical content onto
  a larger canvas replays golden, because replay verifies what the user
  saw. The alternative (threading the terminal size through the session
  format) would version the M9 document for no testing value.
- **Bounded rings replay from a seed checkpoint.** The first retained
  frame's events were evicted with the frames they produced, so a
  window cannot replay from events alone; `ReplayStep::seed` marks the
  checkpoint step and `SessionReplay::seed_buffer` hands over the
  parsed content — the FrameInspector "resume from here" flow, made a
  type. Events recorded after the last frame produced no screen and are
  reported (`unverified_events`), never delivered: a replay cannot
  verify what no one saw.
- **The clipboard is write-only, by decision.** OSC 52 readback is the
  variant terminals gate behind prompts and the reason many disable the
  escape; tuvix builds no API whose honest behavior is "silently
  returns nothing". The payload is base64 with an in-house RFC 4648
  encoder (pinned against the RFC vectors) — escape-free by
  construction, the M5 hyperlink framing argument again — and caps at
  64 KiB so tuvix never builds a sequence no terminal accepts. Empty
  contents are the clear convention. The write is raster-state neutral
  (no terminal moves the cursor for OSC 52), which the differential
  test pins.
- **The memory budget is measured, not asserted.** The §1 table's last
  row (< 10 MB overhead) was the only one without a gate; B13 measures
  the live bytes of a full persistent-pipeline build (buffers, journal,
  60 recorded snapshots, preview sink) through a counting allocator:
  ~2.3 MB, 4.4x headroom. The allocator is a dev-only `publish = false`
  member (`tuvix-memgate`) whose whole surface is one `unsafe impl
  GlobalAlloc` delegating verbatim to the system allocator — the
  workspace `forbid` stands everywhere else, and the safety argument
  lives with the code.

## 18. M11 decisions (2026-11)

- **The viewport's scratch surface lives in the state and is invalidated
  explicitly, not re-rendered every frame.** The first implementation
  re-painted the child on every frame and the B14 gate failed it honestly:
  a 400-row content repaint pushed the scroll step to ~1.7 ms, over the
  1 ms budget, before any diff ran. The shipped design paints the child
  once per invalidation and blits the window the rest of the time —
  ~166 us per scroll step, the same explicit-dirtiness contract the
  persistent-DOM journal uses one level down. A frame that only scrolls
  costs the window; a content change costs the child's own paint on top,
  which is the child's cost, not the viewport's.
- **The scroll state remembers the world it clamps against.** Offsets are
  clamped to `content − viewport` at render, and the state keeps both
  numbers so the scroll operations are honest between frames too
  (`scroll_down` returns `false` at the bottom instead of lying until the
  next render). Before the first frame the app can prime the content
  extent with `set_content`; the zero viewport then means the clamp is the
  bare content, which is what "nothing is visible yet" means.
- **Wide glyphs are never split by the window edge — in either direction.**
  A pair crossing the *right* edge is clipped (blanked), never emitted as
  a lone leading half mid-row, and a window starting on a covered half
  snaps back one column to the leading edge. The same terminal-pair
  honesty the buffer's write path enforces, applied at the viewport
  boundary; the unit tests pin both edges over CJK content.
- **Focus is routing, not a widget trait.** Widget states stay
  key-agnostic (the M6 contract), so focus is the app's mapping question:
  "which pane receives the editing keys?" `FocusRing<T>` answers it over
  the app's own pane enum — wrap-around `advance`/`retreat`, lookup by
  value or index, an explicit unfocused state, and no trait for widgets to
  implement. The methods are `advance`/`retreat`, not `next`/`previous`:
  a ring is not an `Iterator` (no termination, returns a reference), and
  confusing the two names buys nothing.
- **The keymap shadows prefixes, on purpose.** With both `g` and `gg`
  bound, a lone `g` stays `Pending` forever — the trie cannot know a chord
  is over without a timeout, and a timeout is a policy the framework
  should not own (`reset` exists for apps that want one). Modifiers match
  as a subset (a `Ctrl+C` binding fires for `Ctrl+Alt+C`), only press
  events advance matching, and commands are cloned out so `step` needs no
  lifetime on the outcome — the app owns a plain `Matched(Command)`.
  `Miss` and `Matched` reset the matcher; `Pending` keeps it open.
- **Zero new dependencies, again.** The trie is a `Vec`-backed tree, the
  ring a `Vec` and an index, the viewport a second `Buffer`. The M5
  framing argument still holds: the fewer bytes that cross the trust
  boundary, the fewer the escape hatches to audit.

## 19. M12 decisions (2026-11)

- **The packet is the batching unit, not the mutation.** §5.1's mechanism
  is a *packet*: one boundary crossing carries a batch, one strict parse
  validates all of it, and apply is all-or-nothing — a rejected packet
  leaves the buffer untouched (the differential tests pin it), which is
  what makes error reporting useful: the host learns the offending byte
  offset without half a batch having landed. `MutationReader::next`
  streams for hosts that want incremental consumption; `parse_packets`
  is the collect-everything convenience; both enforce the same grammar.
- **The parser is strict because the boundary is a trust boundary.** The
  M9 `Screen` philosophy applied to bytes: bad magic, unknown version,
  unknown op, unknown color byte, invalid UTF-8, C0/DEL in text and URIs,
  non-ASCII fill symbols, reserved style bits set and trailing bytes all
  fail with a byte offset — a protocol consumer gets an error, never
  silently different content. The M5 framing argument carries over: no
  byte that crosses the boundary can later break an escape sequence, so
  text, URIs and fill symbols are constrained to what the renderers can
  echo verbatim.
- **Ops clip like the `Buffer` methods they name.** `Mutation::apply` is
  a thin mapping onto `set_string`/`set_style`/`clear`/`resize` and the
  hyperlink writer, sharing their exact edge semantics (an op that lands
  partly outside paints the part that lands inside; wide glyphs are never
  split). An empty hyperlink URI clears its region — the M10 clipboard's
  "empty means clear" convention, kept consistent across the boundary.
- **The snapshot is the outbound half, not a second format family.** The
  same `TVX` envelope at version 2: empty rows cost one byte, content
  cells list symbol, style, hyperlink and the covered-half flag. The
  differential gate is the strongest form of documentation:
  `decode(encode(buffer)) == buffer`, wide pairs and links included, and
  the decoded buffer diffs and renders through the real pipeline (the M9
  Screen discipline, one layer down).
- **The C ABI is the audited-unsafe exception, scoped like the
  allocator.** tuvix-ffi does not inherit the workspace lints so
  `unsafe_code` is allowed exactly there: thin `extern "C"` wrappers with
  null-checked inputs, safe `tuvix-core` calls inside, one `Box` per
  handle, capacity negotiation on every output function (write at most
  `cap` bytes, or return `-needed`), pinned result and error-kind codes
  mirrored in `tuvix.h`, and the ownership/threading contract documented
  at the boundary the way a C library documents it. Panics never cross:
  the strict parser reports errors, and allocation failure aborts rather
  than unwinding into C.
- **B15 closes the budget table.** §8 gate 1 — more than 50,000 FFI
  mutations per second — was the one row with no measurement. The gate
  applies a 10,000-mutation packet twenty times through the raw C ABI and
  asserts the budget; the measured 9.3M mutations/sec (108 ns per
  mutation, parse included) leaves 185x headroom, and the batching is
  the point: the per-call overhead amortizes to noise at batch size.


---

## 20. M13 decisions (2026-11)

- **The clock owns the value, not the tick count.** A `Tween`'s value is
  a pure function of the accumulated `Duration`: advancing one 400 ms
  jump and 400 one-ms slices land bit-identical, and the differential
  test pins it. A render loop's cadence is none of the tween's business —
  that is what makes an animation correct at 5 fps and at 144.
- **`Progress` is the clamped time domain; the eased coefficient may
  overshoot.** `Progress::new` clamps into [0, 1] and `NaN` becomes
  zero — a broken clock reads as "not started", never as undefined
  behavior downstream. `Easing::ease`, though, returns `f64`:
  `OutBack` overshoots to 1.10 and `OutElastic` to ~1.37 on purpose,
  and `Interpolate` implementations must stay well-defined beyond the
  unit interval (integer and channel casts saturate; RGB channels clamp
  at the rails). Clamping at the curve's output would have silently
  deleted the two overshooting curves' entire personality.
- **Endpoints are contracts; closed forms are not.** The catalog is
  pinned at p = 0.25 / 0.5 / 0.75 against a reference computed
  independently (the easings.net formulas, evaluated in Python before
  the Rust existed), and both endpoints are exact for every curve —
  which costs guarded early-returns where float arithmetic evaluates
  the closed form to 2.2e-16 (`OutBack` at zero) and buys the animator
  rest positions that need no easing at all.
- **One boundary rule for every schedule.** Exactly on a cycle boundary
  a tween reads as the *end of the previous cycle*: play-once tweens
  rest at `to`, alternate tweens stay continuous at every bounce apex,
  exhausted `Count(n)` rests where its last cycle ended, and infinite
  loops sample the apex before wrapping. Alternate's reverse cycle plays
  the same curve backwards (`ease(1 - raw)`), so the motion is literally
  the forward motion reversed, not a different curve.
- **`Interpolate` is honest about what has no in-between.** RGB colors
  blend per channel; the ANSI palette and indexed entries snap at the
  midpoint — a fade never invents a color the terminal cannot render.
  Integers round half-away-from-zero (direction-agnostic: both
  directions of the midpoint land on the same integer) and saturate at
  the type's bounds, which is also what makes overshooting coefficients
  safe.
- **The idle decision is the static-CPU budget as a type.**
  `Animator::is_idle` is a counter, not a scan — the O(1) decision shape
  of the M4 journal's no-op frame and the M8 scheduler's idle tick.
  Finished tweens stay pinned: readable at their rest value, never
  re-reported by `advance`, until replaced or cancelled. An app that
  ticks only while `!is_idle()` burns nothing on settled UIs; the
  `animation` example's loop literally swaps its 16 ms frame poll for a
  120 ms input poll the moment the engine goes idle, and B16 measures
  the working end: 1,000 concurrent tweens advance in ~5.7 us — 54x
  headroom under the 1 ms worst-tick budget — so motion is a rounding
  error next to the B2 frame it rides on.

---

## 21. M14 decisions (2026-11)

- **The mouse layer is three pure types, not a widget system.** The
  `HitMap` is a `Vec` of `(id, Rect)` pairs, the `GestureRecognizer` is
  a four-field state machine, the `MouseMap` is a flat binding list —
  none of them touch the terminal, the event loop or the widget tree.
  The app registers regions while it paints (a `push` next to every
  render call), feeds raw mouse events through the recognizer on its own
  clock, and resolves the result. Widgets stay key-agnostic and now
  mouse-agnostic: no trait gained a mouse hook, no widget state learned
  about buttons, and the M11 honesty (`FocusRing` is a type over the
  app's own panes) repeats — the `HitMap` is a type over the app's own
  region ids.
- **One rule for the whole mouse layer: the painter's algorithm.** The
  `HitMap` resolves a click to the *last registered* region containing
  it — the one that painted on top — and the `MouseMap` resolves a
  gesture to the *latest bound* pattern matching it. One mental model,
  pinned by the same-shaped tests: rebinding is reprioritizing, painting
  later is clicking first. The app registers bottom-to-top exactly as it
  renders bottom-to-top, so hit-testing cannot disagree with what the
  user sees.
- **The clock is injected; clicks count release-to-release.** The
  recognizer never reads a clock (the M13 discipline repeated): the app
  feeds `Instant::now()` deltas, tests feed scripted durations, and a
  double click is a pure function of the stream — same button, same
  cell, released within the interval of the previous release. Cell
  granularity is honest at the terminal: positions are cells, so a
  "double click" is two releases on one cell, and a held button that
  moves to a *different* cell has started a drag — same-cell wobble
  never does.
- **Drags belong to their origin; drops are the app's business.** A
  drag reports its press origin in every gesture, and the `MouseMap`
  resolves drag classes against the origin's region — the pointer
  wandering off the panel must not unbind the drag. Where the drag
  *lands* is a different question about a different position: the app
  resolves the `DragEnd` landing cell itself (the `anchor` helper names
  which position each gesture is resolved against — origin for drags,
  pointer for everything else).
- **Unmatched input is ignored, not invented.** A release with no press,
  a drag with no button held, an `Up` for the wrong button — the
  recognizer answers `None` instead of guessing (terminals start
  reporting mid-press; SGR mouse modes can drop a nibble). A second
  `Down` replaces the active press and abandons any drag: out-of-order
  terminals degrade to the newest truth instead of wedging the
  classifier.
- **Stateless resolution is the point.** The `Keymap` needs a matcher
  because keys accumulate into chords; a gesture is atomic, so the
  `MouseMap` resolves in one call and returns `Option<C>` — no
  `Pending`, no reset discipline, the same answer every time. The
  click-family classes are exact on purpose: `Click` is `clicks == 1`,
  `DoubleClick` is `clicks == 2`, and `AnyClick` is the catch-all — no
  implicit triple-click class, because the honest 90% needs three names,
  not five.
- **B17 closes the mouse budget before there is a mouse budget.** A
  click resolves through a 1,000-region hit map plus a 32-binding mouse
  map in ~765 ns (a reverse rect scan with an early exit, measured
  against a 20 us ceiling — 26x headroom), and one event classifies in
  ~8 ns (125x under the 1 us event ceiling). A dense dashboard can
  register every tile it paints and still decide a click in under a
  microsecond; the per-frame registration of that same map costs ~2.7 us
  (M2), so the whole pointer layer stays a rounding error next to the
  B2 frame it rides on.
