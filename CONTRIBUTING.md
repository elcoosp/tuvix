# Contributing to tuvix

Thanks for helping make tuvix fast and delightful. This project keeps its
quality bar high with a small set of non-negotiable rules.

## Development workflow

1. **TDD from day one.** Write the failing test first (red), implement until
   green, refactor. The repository history starts with a
   `test(core): ... (TDD red)` → `feat(core): ... (TDD green)` pair on
   purpose; keep that discipline. Bug fixes: write the regression test
   first.
2. **Conventional Commits.** `feat(scope): ...`, `fix(scope): ...`,
   `perf: ...`, `test: ...`, `bench: ...`, `docs: ...`, `chore: ...`,
   `ci: ...`. git-cliff groups them into the changelog and the release
   workflow depends on it. Breaking changes: `feat!:` or
   `BREAKING CHANGE:` footer.
3. **All gates must pass locally before pushing** (CI enforces the same):

   ```
   cargo fmt --all --check
   cargo clippy --workspace --all-targets --all-features -- -D warnings
   cargo test --workspace --all-features
   cargo test --workspace --doc
   RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps
   cargo bench --no-run --all-features
   ```

## Rules of the codebase

- **`#![forbid(unsafe_code)]`** workspace-wide. If perf ever requires unsafe,
  bring the decision to the decision log first. The one exception is
  `tuvix-memgate` — a dev-only, `publish = false` member housing the B13
  counting allocator, whose `unsafe impl GlobalAlloc` is a verbatim
  delegation to the system allocator with a documented safety argument;
  it deliberately does not inherit the workspace lints so the allow is
  scoped to that one impl.
- **No `unwrap`/`expect` in library code paths** (tests are fine).
- **Every public item is documented** (`missing_docs` is warn → CI denies
  warnings). Doc examples must be doctests.
- **Wide-glyph changes go through the soundness test.** Anything touching
  `buffer.rs` must keep `tests/diff_soundness.rs` green — it is the
  contractual proof that visible state == logical state.
- **Benchmarks follow behavior changes.** If you touch the diff, buffer or
  layout hot paths, run the criterion benchmarks and note the delta in the
  PR.

## MSRV

The minimum supported Rust is **1.88** (`rust-version` in `Cargo.toml`).
Do not use features newer than that; CI has an MSRV job.

## Dependencies

Only latest stable versions are accepted for new dependencies; Dependabot
keeps the tree fresh weekly. `tuvix-core` stays dependency-light
(bitflags + unicode-* only) — anything heavier belongs in backend crates.

## Releasing

See [`docs/RELEASE.md`](docs/RELEASE.md) — version bump, tag `vX.Y.Z`,
git-cliff generates the changelog, GitHub Actions creates the release and
publishes to crates.io.
