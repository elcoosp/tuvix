# tuvix

**A high-performance terminal UI framework for Rust with best-in-class
developer experience.**

Double-buffered cell rendering, a zero-allocation run-batched diff engine
on 16-byte compact cells, a wide-character corpus that is contractually
byte-identical between incremental and full renders, a runtime that restores
your terminal on panic, a persistent-DOM frame journal that turns no-op
frames into a ~4 ns decision, a theme system, IPC frame previews, OSC 8
hyperlinks on cells, terminal-weighted layout constraints, structured JSON
frame export, and backends beyond crossterm — a zero-dependency ANSI
renderer and a termion (unix) backend — a grouped `BarChart`, a
collapsible `Tree`, an inline markup parser with `span!`/`line!`/
`text!` macro sugar — a multi-line `TextArea` with exact coalescing
undo/redo, `SessionReplay` — the golden loop that replays a recorded
session through your app and names the frame that diverged — a
write-only OSC 52 clipboard whose base64 payload can never break its
framing — and the interaction depth of a real toolkit: a `Scrollable`
viewport that clips any widget behind a scroll window (cached child
paint, one invalidation per content change, ~166 us scroll steps), a
`FocusRing` that routes the editing keys over your own pane enum, and a
declarative `Keymap` with multi-key chords that resolves a keystroke in
~8 ns — and the differentiator no incumbent ships: a
cooperative frame scheduler where **input preempts render** (ticks
coalesce render requests, a probe yields to waiting keystrokes, and
abandoned passes are discarded, never committed), monotonic
`CancellationToken`s with parent-child propagation, and a time-travel
`HistorySink` that records every flushed screen — plus tooling that is
green from day one: pedantic clippy, TDD-tested contracts, benchmark gates with
budgets enforced in CI, latest-generation GitHub Actions CI, and
git-cliff-powered releases.

[![CI](https://github.com/tuvix-rs/tuvix/actions/workflows/ci.yml/badge.svg)](https://github.com/tuvix-rs/tuvix/actions/workflows/ci.yml)
[![MSRV](https://img.shields.io/badge/MSRV-1.88-blue)](./Cargo.toml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](#license)

## Quick start

Add to `Cargo.toml`:

```toml
[dependencies]
tuvix = "0.12"
```

```rust
use tuvix::prelude::*;
use tuvix::CrosstermEvents;
use tuvix::runtime::InputMode;

fn main() -> std::io::Result<()> {
    // One call: panic hook + raw mode + alternate screen. The runner
    // restores the terminal on drop, on error and on panic.
    let mut runner = tuvix::try_init()?;
    runner.push_input_mode(InputMode::BracketedPaste)?;

    let (_reader, mut events) = CrosstermEvents::spawn()?;

    loop {
        runner.draw(|frame| {
            frame
                .buffer
                .set_string(0, 0, "hello".red().bold(), Style::new());
        })?;

        if let Some(_event) = events.try_next() {
            // handle it — or await it with any async executor:
            // while let Some(event) = events.next_event().await { ... }
        }
        # break;
    }
    runner.restore()?;
    Ok(())
}
```

The event stream is std-only and executor-agnostic — tokio, smol,
async-std or the 30-line park-based `block_on` from
[`examples/async_events.rs`](crates/tuvix/examples/async_events.rs) all
await it with zero extra dependencies.

Runnable apps live in [`crates/tuvix/examples/`](crates/tuvix/examples):

```
cargo run -p tuvix --example hello          # the runtime loop, input stack included
cargo run -p tuvix --example async_events   # the async stream, no executor needed
cargo run -p tuvix --example widgets        # the M3 widget family, live
cargo run -p tuvix --example preview        # M4: watch the app from another terminal
                                            # (telnet 127.0.0.1 4223)
cargo run -p tuvix --example simple_backend # M5: the zero-dependency ANSI renderer
cargo run -p tuvix --example termion_hello  # M5: the termion backend (unix)
cargo run -p tuvix --example inputs         # M6: tabs, text fields and a popup
cargo run -p tuvix --example data_view      # M7: bar chart, tree and markup
cargo run -p tuvix --example devtools       # M9: record, scrub, inspect, export
cargo run -p tuvix --example interaction    # M11: scrollable log, focus ring, keymap chords
```

## Persistent DOM in three lines

Widgets keep their painted output across frames; the frame journal turns
"nothing changed" into the O(1) frame decision:

```rust
let mut terminal = Terminal::new_persistent(backend)?;
terminal.draw_persistent(|frame| {
    // repaint only what changed — untouched regions keep their content
})?;
terminal.draw_persistent(|_| {})?;  // no-op: no diff walk, no backend draw
```

## Why tuvix

| | what you get |
| --- | --- |
| **Perf** | 16-byte inline-first cell glyphs (the diff walk is ~8.6 ns/cell, 4.9x faster than M0's `String` cells), run-batched diff, persistent raster state with CHA cursor economy and EL elision — a 1-row change serializes to **1.65%** of a full frame; the M2 event queue moves an event in **~29 ns** and a waker round trip in **~57 ns**; M3 paints a full 200x60 dashboard of gauge + list + table in **~150 us**; the **M4 frame journal** decides a no-op frame in **~4 ns** and repaints one ticker band for **~1.3 us** (the full diff walk costs 105 us there), the **M8 scheduler** decides an idle tick in **~0.3 ns**, coalesces render storms into one frame, and yields a pending render to waiting input in **~2 ns** — input-to-paint p99 is **~0.3 ms** against the 8 ms budget over 10,000 keystrokes @200x60; the **M9 DevTools** scrub any recorded 200x60 frame back into a renderable grid in **~130 us** (p99 195 us against a 2 ms gate) at a recording overhead of **~1.5 us/frame**, `#![forbid(unsafe_code)]` |
| **DX** | `tuvix::try_init()` → runner with restore-on-drop, `"hello".red().bold()` styling sugar over zero-copy literal spans, `Layout::vertical([...]).areas(&area)` with a memoized split cache and the **M6 `Flex` placement policies** (Start/End/Center/SpaceBetween/SpaceAround/SpaceEvenly), `Paragraph` word wrapping + scroll state, the **M3 widget family** (Gauge, Sparkline, List, Table, Scrollbar, Chart, Canvas) behind one unified `RenderWidget` contract for stateful widgets, the **M4 theme system** (`Theme::dark()/light()/monochrome()`, `NO_COLOR` honored, one `.theme(&theme)` builder per widget), **M4 IPC previews** (watch a live app from `telnet 127.0.0.1 4223`), the **M6 interaction kit**: `Tabs`, `LineGauge`, the `Input` text field (char cursor, word motions, column-exact scroll window, `cursor_position` for the terminal cursor), and the popup pattern (`Rect::centered` + `Clear`), the **M7 data kit**: `BarChart` (grouped bars, values + labels, theme-aware), `Tree` (path-based selection + fold state) and the inline markup parser (`parse_line` with strict, byte-offset errors) plus `span!`/`line!`/`text!` macro sugar — the **M8 responsiveness kit**: the `Scheduler` loop (input-priority ticks, render coalescing, `tick_cancellable`), monotonic `CancellationToken`s, and the `HistorySink` time-travel ring — and the **M9 time-travel DevTools**: the `SessionRecorder` timeline (frames + input events), `Screen` snapshot reconstruction, the `FrameInspector` scrub overlay, and golden-session JSON export/import with a one-line `diff` regression gate — and the **M11 interaction kit**: the `Scrollable` viewport (any widget behind a scroll window with an optional attached scrollbar and mouse-wheel mapping), the `FocusRing` focus router, and the `Keymap` binding trie (`g g` chords included), a `prelude`, every doc example is a doctest |
| **Correctness** | 1,100+ tests: the **M11 interaction gates** (a scroll session renders byte-identically to a direct jump, the child paint is a cache — invalidated frames repaint, scroll frames blit, a resize repaints — wide-glyph pairs are never split by a window edge, the keymap's prefix-shadowing and subset-modifier semantics are pinned, and chords survive release events), the **M9 DevTools gates** (recorded frames reconstruct byte-identically through the real pipeline, the round trip `from_ansi(serialize_ansi(buf))` is a differential gate over styled/hyperlinked/wide content, session documents round-trip exactly across the full event vocabulary, golden sessions diff clean and regressions name the frame), the **M8 differential gates** (interrupted sequences end byte-identical to uninterrupted ones, immediate and persistent; persistent histories byte-identical to immediate ones frame-for-frame), a 500-step randomized diff-soundness property test, a wide-character corpus (ZWJ emoji, VS16, skin tones, flags, CJK, halfwidth dakuten) where incremental renders must be byte-identical to full rebuilds, an input-mode stack whose escape sequences are pinned byte-wise against crossterm 0.29's own encoder, a restore-on-drop gate proven under a real panic unwind, per-widget rendering contracts pinned by buffer-level assertions, and the **M4 gates**: banded diff == full diff, persistent visible == logical DOM over 600 randomized steps with mid-stream resizes, full-repaint parity between modes, no-op frames proven absent from the backend op log, and every theme slot mapping pinned |
| **Tooling** | clippy::pedantic deny-warned, rustdoc deny-warned, MSRV 1.88 job, 3-OS test matrix (benches compiled natively on Windows too), bench compilation + a deterministic byte-ratio gate in CI, the **M4 scheduled perf-gate workflow** (budgets enforced weekly), Dependabot, git-cliff changelogs, a documented platform support matrix |

## Workspace

| Crate | Contents |
| --- | --- |
| [`tuvix`](crates/tuvix) | facade — flat re-exports + `prelude` + `try_init`/`restore_global` + preview re-exports |
| [`tuvix-core`](crates/tuvix-core) | geometry, style + stylize, zero-copy text, compact `Symbol` cells + diff engine with EL elision, cached layout, row-band damage, events, backend contract, Terminal, TestBackend, **runtime**: input stack, async event queue, panic restore, app runner; **M4**: persistent-DOM frame journal, theme system, IPC previews; **M7**: markup parser + macros; **M8**: the scheduler (input-priority ticks, render coalescing), cancellation tokens, the frame history ring; **M9**: the `SessionRecorder` timeline, `Screen` snapshot reconstruction, the session JSON document + golden diff; **M11**: the `Keymap` binding trie and the `FocusRing` focus router |
| [`tuvix-widgets`](crates/tuvix-widgets) | Block, Paragraph with word wrapping + scroll state, **M3 family**: Gauge, Sparkline, List, Table, Scrollbar, Chart, Canvas (braille grid), **M6**: Clear, Tabs, LineGauge, Input + InputState, **M7**: BarChart + Bar/BarGroup, Tree + TreeItem/TreeState, **M9**: `FrameInspector` (the picture-in-picture scrub view with a timeline footer), **M11**: `Scrollable` + `ViewportState` (the clip-any-widget scroll viewport) — unified behind the `RenderWidget` contract, every family themable via `.theme(&Theme)` |
| [`tuvix-backend-crossterm`](crates/tuvix-backend-crossterm) | ANSI renderer with persistent raster state, cursor economy, EL elision, lossless event bridge + background event reader |
| [`tuvix-backend-ansi`](crates/tuvix-backend-ansi) | **M5** — the dependency-free ANSI renderer: the full persistent raster-state machine straight to any `Write` target |
| [`tuvix-backend-termion`](crates/tuvix-backend-termion) | **M5** — unix platform backend over termion 4: raw mode, live size queries, event reader |

Full design and the competitor-analysis grounding live in
[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md); benchmark baselines in
[`docs/BENCHMARKS.md`](docs/BENCHMARKS.md); the platform support matrix in
[`docs/PLATFORMS.md`](docs/PLATFORMS.md).

## Versions (verified 2026-11)

Latest stable of everything, as required by the project's dependency policy:

| Dependency | Version |
| --- | --- |
| Rust (toolchain channel `stable`) | 1.99.1 |
| crossterm | 0.29.0 |
| termion (unix targets) | 4.0.6 |
| bitflags | 2.13.2 |
| unicode-width | 0.2.3 |
| unicode-segmentation | 1.13.3 |
| criterion | 0.8.2 |
| pretty_assertions | 1.4.1 |
| actions/checkout | v7 |
| Swatinem/rust-cache | v2.9.3 |
| orhun/git-cliff-action | v4 (git-cliff 2.14.2) |
| softprops/action-gh-release | v3 |

M2 through M9 added **zero new dependencies** — the runtime,
every M3 widget, the M4 frame journal, the theme system, the IPC
previews, the M6 Flex pass, the M6 widgets, the M7 data kit, the M8
scheduler (cancellation, ticks, history) and the M9 DevTools (timeline,
snapshot parser, session JSON, inspector) are std-only (M3 reuses
`unicode-width` for label layout and the core layout solver for table
columns; M4's preview tap is a broadcast thread over `std::net` /
`std::os::unix::net`; M6's `Input` reuses `unicode-width` for cursor
column math).

## MSRV

Rust 1.88 (enforced by `rust-version` and a dedicated CI job). Edition 2024.

## Contributing

See [`CONTRIBUTING.md`](CONTRIBUTING.md) — TDD policy, conventional commits,
lint gates. Releases: [`docs/RELEASE.md`](docs/RELEASE.md).

## License

Dual-licensed under [MIT](LICENSE-MIT) or
[Apache-2.0](LICENSE-APACHE), at your option (the Rust ecosystem standard).
