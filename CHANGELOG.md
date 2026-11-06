# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.15.0](https://github.com/tuvix-rs/tuvix/compare/v0.14.0...v0.15.0) - 2026-11-06
### Added
- M14-A the hit map — the click surface resolved by the painter's algorithm
- M14-B the gesture recognizer — clicks, drags and scrolls on an injected clock
- M14-C the mouse map — declarative mouse bindings, the keymap's pointer twin
- M14 prelude wiring, the mouse event types and the mouse example
### Documentation
- M14 architecture decisions, benchmark scorecard and readme
### Performance
- M14 the mouse benches — the pointer layer under measurement (B17: 765 ns per decision)

## [0.14.0](https://github.com/tuvix-rs/tuvix/compare/v0.13.0...v0.14.0) - 2026-11-05
### Added
- M13 the motion layer — the clock owns the value
- M13 prelude wiring and the animation example
### Documentation
- M13 architecture decisions, benchmark scorecard and readme
### Performance
- M13 the motion benches — the fleet under measurement

## [0.13.0](https://github.com/tuvix-rs/tuvix/compare/v0.12.0...v0.13.0) - 2026-11-05
### Added
- M12-A the mutation packet — a flat, versioned binary protocol for buffer mutations
- M12-B the applier and the frame snapshot — packets apply, buffers read back
- M12-C the C ABI — the embedding boundary as an audited unsafe surface
- M12 prelude wiring and the embed example
### Documentation
- M12 architecture decisions, benchmark scorecard and readme
### Fixed
- Repair the push branch filter and add tuvix-ffi to the publish order
### Performance
- M12 the embed benches — the boundary under measurement

## [0.12.0](https://github.com/tuvix-rs/tuvix/compare/v0.11.0...v0.12.0) - 2026-11-05
### Added
- M11-A Scrollable — a viewport over larger content
- M11-B FocusRing — focus routing over the app's own panes
- M11-C Keymap — declarative bindings with multi-key chords
- M11 interaction example — viewport, focus ring and keymap together
### Documentation
- M11 architecture decisions, benchmark scorecard and readme
### Performance
- M11 gates — B14 the viewport scroll step, the interaction benches

## [0.11.0](https://github.com/tuvix-rs/tuvix/compare/v0.10.0...v0.11.0) - 2026-11-05
### Added
- M10-A TextArea — a multi-line editor with exact undo/redo
- M10-B SessionReplay — replay recorded sessions through a deterministic app
- M10-C OSC 52 clipboard — a write-only clipboard contract
### Documentation
- M10 architecture decisions, benchmark scorecard and readme
### Fixed
- Resolve the TextArea Input intra-doc links
### Performance
- M10 gates — B12 replay golden loop, B13 the memory budget closed

## [0.10.0](https://github.com/tuvix-rs/tuvix/compare/v0.9.0...v0.10.0) - 2026-11-05
### Added
- M9-A session recorder — a time-travel timeline of frames and input events
- M9-B Screen — parse canonical ANSI snapshots back into cells
- M9-C FrameInspector — replay a recorded frame as a PiP overlay
- M9-D session JSON export/import + the golden-session diff
- M9 prelude wiring, devtools example and differential gates
### Documentation
- M9 architecture decisions, benchmark scorecard and readme
### Performance
- M9 devtools benches and the B11 scrub-latency gate

## [0.9.0](https://github.com/tuvix-rs/tuvix/compare/v0.8.0...v0.9.0) - 2026-11-05
### Added
- M8-A cancellation tokens with parent-child propagation
- M8-B cancellable frames — abandoned renders are discarded, not committed
- M8-C scheduler — input-priority ticks with render coalescing
- M8-D frame history sink — time-travel capture of every flushed frame
- M8 prelude wiring — Scheduler, CancellationToken, HistorySink
### Documentation
- M8 architecture decisions, benchmark scorecard and readme
### Fixed
- Close the scheduler doctest's closure tail
- Pedantic and rustdoc cleanups over the M8 surface
### Other
- Cargo fmt pass over the M8 surface
### Performance
- M8 scheduler benches and the B10 input-to-paint gate

## [0.8.0](https://github.com/tuvix-rs/tuvix/compare/v0.7.0...v0.8.0) - 2026-11-05
### Added
- M7-C inline markup parser with strict errors and span!/line!/text! macros
- M7-A BarChart — grouped vertical bars with values and labels
- Wire the BarChart module into the widgets crate
- M7-B Tree — collapsible hierarchy with path-based state
- M7 prelude wiring — BarChart, Tree, markup parser and macros
### Documentation
- M7 architecture decisions, benchmark scorecard and readme
- Fix the intra-doc link to ListState in the tree module
### Fixed
- Restore the bench-compile step in the build job
- Publish the ansi and termion backends before the facade
### Other
- Cargo fmt pass over the M6 surface
### Performance
- M7 markup parser and new widget paint benches
### Tests
- Correct the centered odd-remainder expectation

## [0.7.0](https://github.com/tuvix-rs/tuvix/compare/v0.6.0...v0.7.0) - 2026-11-05
### Added
- M6 Flex layout — placement pass for the leftover budget
- M6 Rect::centered — clamped popup geometry helper
- M6 Clear — the popup paint primitive
- M6 Tabs — highlighted horizontal tab row
- M6 LineGauge — integer-precise single-row gauge
- M6 Input — single-line text field with an editing state
- M6 prelude wiring — Flex, Clear, Tabs, LineGauge, Input
- M6 inputs example — form with tabs, fields and a popup
### Documentation
- M6 architecture decisions, benchmark scorecard and readme
### Fixed
- Repair the push branch filter
### Performance
- M6 flex placement and new widget paint benches

## [0.6.0](https://github.com/tuvix-rs/tuvix/compare/v0.5.0...v0.6.0) - 2026-11-05
### Added
- M5 hyperlinks — OSC 8 cell attributes through diff, renderers and previews
- M5 terminal-weighted layout — Constraint::Weight
- M5 structured frame export — serialize_json
- M5 platform backends — dependency-free ANSI + termion (unix)
- M5 facade wiring — backends in the prelude, termion runtime, examples
### Documentation
- M5 architecture decisions, benchmark scorecard and readme
### Other
- Satisfy pedantic clippy on the hyperlink pass

## [0.5.0](https://github.com/tuvix-rs/tuvix/compare/v0.4.0...v0.5.0) - 2026-11-04
### Added
- M4 persistent-DOM frame journal with the O(1) no-op decision
- M4 theme system — named slots, presets, NO_COLOR and widget mapping
- M4 IPC previews — full-frame ANSI snapshots broadcast to clients
- M4 Windows backend parity and benchmarks-as-CI-gate
### Documentation
- M4 architecture decisions, benchmark scorecard and readme
### Fixed
- Resolve journal and terminal intra-doc links for rustdoc -D warnings
### Miscellaneous
- Drop the stale v0.4.0 copy embedded in the delivered archive

## [0.4.0](https://github.com/tuvix-rs/tuvix/compare/v0.3.0...v0.4.0) - 2026-11-04
### Added
- **breaking:** Unify stateful authoring behind a RenderWidget contract
- **breaking:** Scroll is configuration for text, state for lists
- M3 family - Gauge, Sparkline, List, Table, Scrollbar, Chart, Canvas
- Facade re-exports, prelude additions and the widgets example
### Documentation
- M3 architecture decisions, benchmark scorecard and readme
### Performance
- M3 widget and dashboard benches
### Tests
- M3 integration gate - dashboard, contract, navigation

## [0.3.0](https://github.com/tuvix-rs/tuvix/compare/v0.2.0...v0.3.0) - 2026-11-04
### Added
- M2 runtime — input stack, async event stream, panic restore, app runner
- Raw control sequences and background event reader
- One-call runtime init/restore with default runner and async example
### Documentation
- M2 architecture decisions, benchmarks and readme
### Fixed
- Redundant explicit links and a latent private-intra-doc-link failure
### Other
- Rustfmt
### Performance
- M2 event-queue and input-stack benchmarks
### Tests
- M2 runtime integration gate

## [0.2.0](https://github.com/tuvix-rs/tuvix/compare/v0.1.0...v0.2.0) - 2026-11-04
### Added
- **breaking:** M1 batch A — compact Symbol, zero-copy spans, damage tracker, widget traits, layout cache
- **breaking:** M1 batch B — word wrap, scroll state, widget traits, raster-state ANSI with EL elision
### Changed
- **breaking:** Rename the project to tuvix (crates.io name collision)
### Documentation
- Correct test count (215)
- Rephrase the rename entry without the old name
### Tests
- M1 batch C — corpus gate, B-gates, measured baselines, version 0.2.0

## v0.1.0 - 2026-11-04
### Added
- Geometry and style primitives (TDD green)
- Text model and stylize sugar
- Buffer, cell grid and zero-alloc run-batched diff engine
- Deterministic layout constraint solver
- Event model, backend contract and double-buffered terminal
- Pair-repair invariants, Buffer::clear, and soundness test suite
- Block and Paragraph
- ANSI renderer with SGR memoization + lossless event bridge
- Facade crate with prelude and runnable hello example
### CI/CD
- Latest GitHub Actions pipeline, git-cliff releases, dependabot
### Miscellaneous
- Scaffold cargo workspace with pedantic lint gates
- Satisfy clippy::pedantic and rustdoc gates workspace-wide
### Performance
- Criterion benchmarks for buffer, diff and layout
### Tests
- Geometry and style specs (TDD red)

<!-- generated by git-cliff -->
