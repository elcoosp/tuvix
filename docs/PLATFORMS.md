# Platform support

tuvix targets every terminal the std supports. The crossterm backend is
cross-platform by construction — ANSI over any `W: Write` plus crossterm
for mode control — so parity is a matter of what each platform's terminal
emulators accept, not of separate code paths.

## Support matrix (M5)

| Surface | Linux | macOS | Windows | Notes |
| --- | --- | --- | --- | --- |
| `Backend` contract (ANSI runs, SGR memoization, cursor economy) | ✅ | ✅ | ✅ | pure output over any writer |
| `tuvix-backend-ansi` (M5 simple backend) | ✅ | ✅ | ✅ | zero dependencies; the host owns the size, cursor state is virtual |
| `tuvix-backend-termion` (M5 unix backend) | ✅ | ✅ | — | termion 4 is unix-only; the crate compiles empty on Windows so the workspace stays portable; raw mode via termios, restored on drop |
| OSC 8 hyperlinks on cells | ✅ | ✅ | ✅* | emitted by every ANSI backend and preview; clickable where the terminal supports OSC 8, inert where it does not |
| Raw mode + alternate screen (`try_init` / restore-on-drop) | ✅ | ✅ | ✅ | via crossterm 0.29; conhost and Windows Terminal both supported |
| Event bridge + reader thread (`CrosstermEvents`) | ✅ | ✅ | ✅ | same lossless mapping, tested on every OS in CI |
| Input stack (kitty flags, bracketed paste, mouse, focus) | ✅ | ✅ | ✅* | the sequences are ANSI; Windows Terminal accepts them, plain conhost ignores them gracefully — the stack unwinds exactly what was pushed either way |
| Widgets, layout, diff engine, frame journal | ✅ | ✅ | ✅ | pure core, no I/O, deterministic integer math |
| Theme system | ✅ | ✅ | ✅ | ANSI 16-color slots; `NO_COLOR` honored everywhere |
| IPC previews — TCP (`PreviewTap::tcp`) | ✅ | ✅ | ✅ | std `TcpListener` on `127.0.0.1`, works under Windows Terminal and conhost |
| IPC previews — unix socket (`PreviewTap::unix_socket`) | ✅ | ✅ | — | `cfg(unix)`; use the TCP transport on Windows |
| Panic-restore lifecycle | ✅ | ✅ | ✅ | hook + drop halves are idempotent and platform-free |

✅* = works where the terminal supports the sequence; degrades gracefully
where it does not (the input-mode stack only ever disables what it pushed).

## CI verification

- `ci.yml` runs the full test suite, doctests and (on Windows) bench
  compilation natively on `ubuntu-latest`, `macos-latest` and
  `windows-latest` — the whole surface compiles and passes on all three
  OSes on every push.
- The workspace crates (`tuvix-core`, `tuvix-widgets`) additionally
  cross-check clean for `x86_64-pc-windows-gnu`; the crossterm backend's
  Windows build needs the mingw toolchain and is verified natively by the
  Windows CI job.
- `perf.yml` runs the timing gates on a schedule; gates assert generous
  ceilings so shared runners stay deterministic.

## Platform-specific behaviors worth knowing

- **Resize events**: crossterm delivers them per platform; tuvix handles
  them like any other event and `Terminal` auto-resizes its buffers at the
  next draw.
- **Kitty keyboard protocol**: pushing the flags on a terminal that does
  not support them is a no-op there; pop still unwinds (the stack is a
  data structure, not side effects).
- **`NO_COLOR`** (no-color.org convention): the crossterm backend honors it
  for live output and `Theme::from_env()` resolves to the monochrome
  preset when it is set.
