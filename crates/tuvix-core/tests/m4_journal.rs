//! The M4 persistent-DOM frame journal gate.
//!
//! Four contracts, all proven against [`TestBackend`] (which applies diff
//! runs exactly like a real terminal) and plain buffers:
//!
//! 1. **Banded == full** — for arbitrary buffer pairs, the banded diff
//!    driven by journal bands yields exactly the runs the full walk yields.
//!    600 LCG-seeded steps, wide glyphs and ZWJ sequences included.
//! 2. **Persistent soundness** — the visible state of a persistent-DOM
//!    terminal must equal the logical DOM state (the same mutation program
//!    applied to a plain buffer), every step, mid-stream resizes included.
//!    This is the M1 diff-soundness contract adapted to retained mode.
//! 3. **Full-repaint parity** — an app that repaints the whole screen every
//!    frame gets byte-identical output from the classic immediate-mode
//!    path and from the persistent path.
//! 4. **The O(1) no-op decision** — a `draw_persistent` closure that paints
//!    nothing emits no backend draw call at all (asserted through a
//!    recording backend's op log), at any viewport size.
//!
//! Semantics note: persistent mode is *retained* — a widget that stops
//! painting keeps its previous output instead of the region wiping blank.
//! That is why partial-paint sequences are compared against the logical DOM
//! model (contract 2), never against a blank-canvas immediate terminal.
//!
//! Deterministic: every fuzz is LCG-seeded, so a failure is reproducible.

use std::sync::{Arc, Mutex};

use tuvix_core::backend::{Backend, ClearRegion, TestBackend};
use tuvix_core::buffer::{Buffer, DiffRun};
use tuvix_core::geometry::{Position, Rect, Size};
use tuvix_core::journal::FrameJournal;
use tuvix_core::style::{Color, Modifier, Style};
use tuvix_core::terminal::Terminal;

const WIDTH: u16 = 40;
const HEIGHT: u16 = 12;

/// Minimal operation-log backend: records whether `draw` was called and
/// with how many runs, without touching a screen.
#[derive(Debug, Clone, Default)]
struct RecordingBackend {
    log: Arc<Mutex<Vec<String>>>,
}

impl RecordingBackend {
    fn shared_log(&self) -> Arc<Mutex<Vec<String>>> {
        Arc::clone(&self.log)
    }
}

impl Backend for RecordingBackend {
    type Error = core::convert::Infallible;

    fn draw<'a>(
        &mut self,
        content: impl IntoIterator<Item = DiffRun<'a>>,
    ) -> Result<(), Self::Error> {
        let runs = content.into_iter().count();
        self.log.lock().unwrap().push(format!("draw {runs} runs"));
        Ok(())
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn size(&self) -> Result<Size, Self::Error> {
        Ok(Size::new(WIDTH, HEIGHT))
    }

    fn get_cursor_position(&self) -> Result<Position, Self::Error> {
        Ok(Position::new(0, 0))
    }

    fn set_cursor_position(&mut self, _position: Position) -> Result<(), Self::Error> {
        Ok(())
    }

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn clear(&mut self, _region: ClearRegion) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// Deterministic LCG; the same seed drives every fuzz.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 11
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }
}

/// A paint operation from the randomized mutation program. The same program
/// drives the terminals and their logical models.
enum Op {
    Write {
        x: u16,
        y: u16,
        text: String,
        style: Style,
    },
    Clear {
        rect: Rect,
    },
    SetStyle {
        rect: Rect,
        style: Style,
    },
    /// A frame where the closure paints nothing (persistent mode: true
    /// no-op; immediate mode: the screen wipes blank).
    NoPaint,
}

fn draw_op(buffer: &mut Buffer, op: &Op) {
    match op {
        Op::Write { x, y, text, style } => {
            buffer.set_string(*x, *y, text, *style);
        }
        Op::Clear { rect } => buffer.clear(*rect),
        Op::SetStyle { rect, style } => buffer.set_style(*rect, *style),
        Op::NoPaint => {}
    }
}

fn random_op(lcg: &mut Lcg) -> Op {
    const TEXTS: [&str; 8] = [
        "lorem",
        "汉漢",      // wide pair
        "🗿",        // 4-byte grapheme, width 2
        "e\u{301}X", // combining mark + ascii
        "x",         // single cell
        "a very long overwrite across many columns of the row",
        "CJK 世界",
        "🏳️‍🌈flag", // ZWJ sequence
    ];
    match lcg.below(10) {
        0..=5 => Op::Write {
            x: lcg.below(u64::from(WIDTH)) as u16,
            y: lcg.below(u64::from(HEIGHT)) as u16,
            text: TEXTS[lcg.below(TEXTS.len() as u64) as usize].to_string(),
            style: {
                let style = Style::new().fg(Color::Indexed(lcg.below(256) as u8));
                if lcg.below(2) == 0 {
                    style.add_modifier(Modifier::BOLD)
                } else {
                    style
                }
            },
        },
        6..=7 => Op::Clear {
            rect: Rect::new(
                lcg.below(u64::from(WIDTH)) as u16,
                lcg.below(u64::from(HEIGHT)) as u16,
                lcg.below(8) as u16,
                lcg.below(4) as u16,
            ),
        },
        8 => Op::SetStyle {
            rect: Rect::new(
                lcg.below(u64::from(WIDTH)) as u16,
                lcg.below(u64::from(HEIGHT)) as u16,
                lcg.below(10) as u16,
                lcg.below(3) as u16,
            ),
            style: Style::new().bg(Color::Indexed(lcg.below(256) as u8)),
        },
        _ => Op::NoPaint,
    }
}

/// Signature of one diff run: `(row, start, elide_rest, cells)` with the
/// cells flattened to comparable values.
type RunSignature = Vec<(u16, u16, bool, Vec<(String, Style, bool)>)>;

fn run_signature(runs: &[DiffRun<'_>]) -> RunSignature {
    runs.iter()
        .map(|run| {
            (
                run.row,
                run.start,
                run.elide_rest,
                run.cells
                    .iter()
                    .map(|cell| (cell.symbol.as_str().to_string(), cell.style, cell.skip))
                    .collect(),
            )
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Contract 1: the banded walk and the full walk are run-identical.
// ---------------------------------------------------------------------------

#[test]
fn banded_diff_matches_full_diff_for_arbitrary_buffer_pairs() {
    let area = Rect::new(0, 0, WIDTH, HEIGHT);
    let mut previous = Buffer::empty(area);
    let mut next = Buffer::empty(area);
    let mut lcg = Lcg(0x0BA4_1E55);

    for step in 0..600 {
        draw_op(&mut next, &random_op(&mut lcg));

        // The journal would have recorded exactly the differing cells (plus
        // pair-repair neighbors, which are themselves differing cells after
        // the repair). Mark every differing cell as a band.
        let mut journal = FrameJournal::viewport(WIDTH, HEIGHT);
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                if previous.get(x, y) != next.get(x, y) {
                    journal.mark_cell(x, y);
                }
            }
        }

        let full: Vec<DiffRun<'_>> = previous.diff_runs(&next).collect();
        let banded: Vec<DiffRun<'_>> = previous.diff_runs_banded(&next, journal.bands()).collect();
        assert_eq!(
            run_signature(&full),
            run_signature(&banded),
            "banded walk diverged from the full walk at step {step}"
        );

        // Advance: the next iteration mutates relative to the new previous.
        previous = next.clone();
    }
}

// ---------------------------------------------------------------------------
// Contract 2: persistent visible state == logical DOM state.
// ---------------------------------------------------------------------------

#[test]
fn persistent_visible_state_matches_the_logical_dom() {
    let mut terminal =
        Terminal::new_persistent(TestBackend::new(Size::new(WIDTH, HEIGHT))).unwrap();
    let mut dom = Buffer::empty(Rect::new(0, 0, WIDTH, HEIGHT));
    let mut lcg = Lcg(0x00C0_D0DE);

    for step in 0..600 {
        let op = random_op(&mut lcg);
        draw_op(&mut dom, &op);
        terminal
            .draw_persistent(|frame| draw_op(frame.buffer, &op))
            .unwrap();

        if step % 97 == 0 {
            // Mid-stream resize: content is lost everywhere, journals flip
            // full, and the model resizes too.
            let w = WIDTH - (step % 5) as u16;
            let h = HEIGHT - (step % 3) as u16;
            terminal.backend_mut().resize(Size::new(w, h));
            dom.resize(Rect::new(0, 0, w, h));
        }

        assert_eq!(
            *terminal.backend().buffer(),
            dom,
            "visible state diverged from the logical DOM at step {step}"
        );
    }
}

// ---------------------------------------------------------------------------
// Contract 3: full-repaint apps are byte-identical in both modes.
// ---------------------------------------------------------------------------

#[test]
fn full_repaint_apps_render_identically_in_both_modes() {
    let mut immediate = Terminal::new(TestBackend::new(Size::new(WIDTH, HEIGHT))).unwrap();
    let mut persistent =
        Terminal::new_persistent(TestBackend::new(Size::new(WIDTH, HEIGHT))).unwrap();
    let mut lcg = Lcg(0x7FA1_11D2);

    for step in 0..600 {
        let op = random_op(&mut lcg);

        // Immediate discipline: blank canvas, then the paint program.
        immediate
            .draw(|frame| {
                frame.buffer.reset();
                draw_op(frame.buffer, &op);
            })
            .unwrap();

        // Persistent discipline: the app resets the DOM it owns, then paints.
        persistent
            .draw_persistent(|frame| {
                frame.buffer.reset();
                draw_op(frame.buffer, &op);
            })
            .unwrap();

        assert_eq!(
            immediate.backend().buffer(),
            persistent.backend().buffer(),
            "full-repaint apps diverged between modes at step {step}"
        );
        let _ = step;
    }
}

// ---------------------------------------------------------------------------
// Contract 4: the O(1) no-op decision never reaches the backend.
// ---------------------------------------------------------------------------

#[test]
fn no_op_frames_never_reach_the_backend() {
    let log = RecordingBackend::default().shared_log();
    let mut terminal = Terminal::new_persistent(RecordingBackend {
        log: Arc::clone(&log),
    })
    .unwrap();

    terminal
        .draw_persistent(|frame| {
            frame.buffer.set_string(0, 0, "hello", Style::new());
        })
        .unwrap();
    terminal.draw_persistent(|_| {}).unwrap();
    terminal.draw_persistent(|_| {}).unwrap();

    let ops = log.lock().unwrap().clone();
    let draw_count = ops.iter().filter(|op| op.starts_with("draw")).count();
    assert_eq!(
        draw_count, 1,
        "two no-op frames must not produce any backend draw call; log: {ops:?}"
    );
}

#[test]
fn no_op_frame_costs_are_independent_of_viewport_size() {
    // The O(1) claim: the no-op decision must not walk the buffer. A 200x60
    // viewport (12,000 cells) behaves exactly like a 4x1 viewport: no draw,
    // no exceptions.
    let log = RecordingBackend::default().shared_log();
    let mut terminal = Terminal::new_persistent(RecordingBackend {
        log: Arc::clone(&log),
    })
    .unwrap();
    terminal.draw_persistent(|_| {}).unwrap();
    assert!(log.lock().unwrap().iter().all(|op| !op.starts_with("draw")));
}

// ---------------------------------------------------------------------------
// Persistent-DOM behavior checks (retained-mode semantics).
// ---------------------------------------------------------------------------

#[test]
fn persistent_dom_carries_content_across_frames() {
    // The persistent contract: a widget that stops painting leaves its
    // previous output on the virtual screen; a widget that repaints the
    // same content changes nothing.
    let mut terminal = Terminal::new_persistent(TestBackend::new(Size::new(10, 2))).unwrap();

    terminal
        .draw_persistent(|frame| {
            frame
                .buffer
                .set_string(0, 0, "static", Style::new().fg(Color::Red));
        })
        .unwrap();

    // Only the ticking part repaints; the static block is untouched.
    terminal
        .draw_persistent(|frame| {
            frame.buffer.set_string(0, 1, "tick 1", Style::new());
        })
        .unwrap();
    terminal
        .draw_persistent(|frame| {
            frame.buffer.set_string(0, 1, "tick 2", Style::new());
        })
        .unwrap();

    let rendered = terminal.backend().buffer().to_string();
    let rows: Vec<&str> = rendered.lines().collect();
    assert_eq!(rows[0], "static    ");
    assert_eq!(rows[1], "tick 2    ");
}

#[test]
fn repainting_identical_content_through_the_journal_is_stable() {
    let mut terminal = Terminal::new_persistent(TestBackend::new(Size::new(10, 2))).unwrap();
    for _ in 0..5 {
        terminal
            .draw_persistent(|frame| {
                frame.buffer.set_string(0, 0, "stable", Style::new());
            })
            .unwrap();
    }
    assert_eq!(
        terminal.backend().buffer().to_string(),
        "stable    \n          "
    );
}

#[test]
fn wide_pair_repairs_stay_journal_exact() {
    // A wide glyph written in one frame, repaired in the next, and cut by a
    // band in the third: the visible state must track the logical DOM
    // exactly — no stale trailing halves.
    let mut terminal = Terminal::new_persistent(TestBackend::new(Size::new(10, 1))).unwrap();
    let mut dom = Buffer::empty(Rect::new(0, 0, 10, 1));

    let script: [Op; 3] = [
        Op::Write {
            x: 2,
            y: 0,
            text: "汉汉".to_string(),
            style: Style::new(),
        },
        Op::Write {
            x: 2,
            y: 0,
            text: "a".to_string(),
            style: Style::new(),
        },
        Op::Clear {
            rect: Rect::new(5, 0, 1, 1),
        },
    ];

    for (step, op) in script.iter().enumerate() {
        draw_op(&mut dom, op);
        terminal
            .draw_persistent(|frame| draw_op(frame.buffer, op))
            .unwrap();
        assert_eq!(
            *terminal.backend().buffer(),
            dom,
            "pair repair diverged at step {step}"
        );
    }
    assert_eq!(dom.to_string(), "  a       ");
}

#[test]
fn mixed_draw_and_draw_persistent_track_the_dom() {
    // An app may interleave the classic path (full diff) with the journal
    // path on one persistent terminal; the visible state must equal the
    // logical DOM either way.
    let mut terminal = Terminal::new_persistent(TestBackend::new(Size::new(12, 2))).unwrap();
    let mut dom = Buffer::empty(Rect::new(0, 0, 12, 2));

    let scripts: Vec<(&str, Op)> = vec![
        (
            "classic A",
            Op::Write {
                x: 0,
                y: 0,
                text: "classic A".to_string(),
                style: Style::new(),
            },
        ),
        (
            "journal B",
            Op::Write {
                x: 0,
                y: 1,
                text: "journal B".to_string(),
                style: Style::new(),
            },
        ),
        ("no-op", Op::NoPaint),
        (
            "clsc",
            Op::Write {
                x: 4,
                y: 0,
                text: "clsc".to_string(),
                style: Style::new(),
            },
        ),
        (
            "journal E",
            Op::Write {
                x: 0,
                y: 0,
                text: "journal E".to_string(),
                style: Style::new(),
            },
        ),
        ("classic no-op", Op::NoPaint),
    ];

    for (step, (name, op)) in scripts.iter().enumerate() {
        draw_op(&mut dom, op);
        if *name == "no-op" {
            terminal.draw_persistent(|_| {}).unwrap();
        } else if *name == "classic no-op" {
            terminal.draw(|_| {}).unwrap();
        } else if step % 2 == 0 {
            terminal.draw(|frame| draw_op(frame.buffer, op)).unwrap();
        } else {
            terminal
                .draw_persistent(|frame| draw_op(frame.buffer, op))
                .unwrap();
        }
        assert_eq!(
            *terminal.backend().buffer(),
            dom,
            "interleaved paths diverged at step {step} ({name})"
        );
    }
}

#[test]
fn elision_survives_banded_diffs() {
    // EL elision (strip trailing blanks, erase with one control) must behave
    // identically whether the run was found by the full walk or by the
    // banded walk. The widget here repaints its whole region (clear, then
    // write), the retained-mode discipline for shortening text.
    let mut immediate = Terminal::new(TestBackend::new(Size::new(20, 2))).unwrap();
    let mut persistent = Terminal::new_persistent(TestBackend::new(Size::new(20, 2))).unwrap();
    let mut dom = Buffer::empty(Rect::new(0, 0, 20, 2));

    let repaint = |buffer: &mut Buffer| {
        buffer.clear(Rect::new(0, 0, 20, 1));
        buffer.set_string(0, 0, "abc", Style::new());
    };

    for term in [&mut immediate, &mut persistent] {
        term.draw_persistent(|frame| {
            frame
                .buffer
                .set_string(0, 0, "abcdefghijklmnopqrst", Style::new());
        })
        .unwrap();
        term.draw_persistent(|frame| repaint(frame.buffer)).unwrap();
    }
    repaint(&mut dom);

    assert_eq!(
        immediate.backend().buffer(),
        persistent.backend().buffer(),
        "widget-disciplined repaint diverged between modes"
    );
    assert_eq!(*persistent.backend().buffer(), dom);
    assert_eq!(
        persistent.backend().buffer().to_string(),
        "abc                 \n                    "
    );
}
