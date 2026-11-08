//! Randomized differential soundness test for the diff engine.
//!
//! A deterministic LCG drives 500 mutation steps (random styled writes with
//! ASCII + CJK mixes, style patches over random regions, row clears, full
//! resets). Every step is applied twice: once to a plain "truth" buffer and
//! once through [`Terminal`] + [`TestBackend`]. After every step the visible
//! state must equal the truth state, modulo cells that are legitimately
//! covered by wide glyphs.
//!
//! This is the class of bug that ratatui fixed reactively (wide-char trailing
//! state edge cases); tuvix pins diff soundness contractually from day one.

use tuvix_core::backend::TestBackend;
use tuvix_core::buffer::Buffer;
use tuvix_core::geometry::{Rect, Size};
use tuvix_core::style::{Color, Modifier, Style};
use tuvix_core::terminal::Terminal;

/// Deterministic LCG (xorshift-free, reproducible across platforms).
struct Lcg(u64);

impl Lcg {
    fn next_u32(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) as u32
    }

    fn range(&mut self, n: u32) -> u32 {
        self.next_u32() % n.max(1)
    }
}

const PALETTE: [Color; 5] = [
    Color::Red,
    Color::Green,
    Color::Blue,
    Color::Indexed(42),
    Color::Rgb { r: 1, g: 2, b: 3 },
];

fn random_char(rng: &mut Lcg) -> char {
    match rng.range(5) {
        0 => '汉',
        1 => '字',
        2 => '·',
        3 => char::from(b'a' + u8::try_from(rng.range(26)).unwrap_or(0)),
        _ => char::from(b'0' + u8::try_from(rng.range(10)).unwrap_or(0)),
    }
}

fn random_style(rng: &mut Lcg) -> Style {
    let mut style = Style::new().fg(PALETTE[usize::try_from(
        rng.range(u32::try_from(PALETTE.len()).unwrap_or(u32::MAX)),
    )
    .unwrap_or(0)]);
    if rng.range(3) == 0 {
        style = style.add_modifier(Modifier::BOLD);
    }
    if rng.range(3) == 0 {
        style = style.bg(PALETTE[usize::try_from(
            rng.range(u32::try_from(PALETTE.len()).unwrap_or(u32::MAX)),
        )
        .unwrap_or(0)]);
    }
    style
}

#[test]
fn diff_soundness_under_random_mutation_sequences() {
    let size = Size::new(24, 8);
    let mut truth = Buffer::empty(Rect::from_size(size));
    let mut terminal = Terminal::new(TestBackend::new(size)).unwrap();
    let mut rng = Lcg(0x9E37_79B9_7F4A_7C15);

    for step in 0..500 {
        // Mutate the truth state (this stands in for widgets re-rendering).
        match rng.range(6) {
            // Random styled write, often containing wide glyphs.
            0..=2 => {
                let x = u16::try_from(rng.range(u32::from(size.width))).unwrap_or(u16::MAX);
                let y = u16::try_from(rng.range(u32::from(size.height))).unwrap_or(u16::MAX);
                let len = rng.range(8) as usize;
                let word: String = (0..len).map(|_| random_char(&mut rng)).collect();
                let style = random_style(&mut rng);
                truth.set_string(x, y, &word, style);
            }
            // Style patch over a random region.
            3 => {
                let width = u16::try_from(rng.range(u32::from(size.width) + 1)).unwrap_or(u16::MAX);
                let height =
                    u16::try_from(rng.range(u32::from(size.height) + 1)).unwrap_or(u16::MAX);
                let x = u16::try_from(rng.range(u32::from(size.width))).unwrap_or(u16::MAX);
                let y = u16::try_from(rng.range(u32::from(size.height))).unwrap_or(u16::MAX);
                let area = Rect::new(x, y, width, height);
                let style = random_style(&mut rng);
                truth.set_style(area, style);
            }
            // Clear a random row (pair-aware, like a widget would).
            4 => {
                let y = u16::try_from(rng.range(u32::from(size.height))).unwrap_or(u16::MAX);
                let row = Rect::new(0, y, size.width, 1);
                truth.clear(row);
            }
            // Full reset.
            5 => {
                truth.reset();
            }
            _ => unreachable!(),
        }

        // tuvix is immediate-mode: every frame repaints the whole screen from
        // state; the diff engine decides what reaches the terminal.
        terminal
            .draw(|frame| {
                frame.buffer.content.clone_from(&truth.content);
            })
            .unwrap();

        // Invariant: what the backend shows equals the truth buffer, modulo
        // covered cells.
        let visible = terminal.backend().buffer();
        let (mut mismatches, mut coverage_breaches) = (0, 0);
        for (truth_cell, visible_cell) in truth.content.iter().zip(visible.content.iter()) {
            if truth_cell.skip {
                if !visible_cell.skip {
                    coverage_breaches += 1;
                }
            } else if visible_cell.symbol != truth_cell.symbol
                || visible_cell.style != truth_cell.style
            {
                mismatches += 1;
            }
        }
        assert_eq!(
            (mismatches, coverage_breaches),
            (0, 0),
            "step {step}: visible state diverged from truth\ntruth:\n{truth}\nvisible:\n{visible}"
        );
    }
}

#[test]
fn wide_glyph_transitions_never_leak_stale_content() {
    // Focused scenario matrix over every transition kind between narrow and
    // wide content at the same cells.
    let size = Size::new(6, 1);
    let mut terminal = Terminal::new(TestBackend::new(size)).unwrap();

    let scenarios: [(&str, &str); 6] = [
        ("ab", "汉"),   // narrow -> wide
        ("汉", "ab"),   // wide -> narrow
        ("汉", "字"),   // wide -> wide (changed glyph)
        ("汉a", "汉b"), // stable wide pair, change after it
        ("a汉", "汉a"), // wide glyph moves
        ("汉汉", "a"),  // wide pair collapses to one cell
    ];

    for (before, after) in scenarios {
        terminal.clear().unwrap();
        terminal
            .draw(|frame| {
                frame.buffer.set_string(0, 0, before, Style::new());
            })
            .unwrap();
        terminal
            .draw(|frame| {
                frame.buffer.reset();
                frame.buffer.set_string(0, 0, after, Style::new());
            })
            .unwrap();

        let mut truth = Buffer::empty(Rect::from_size(size));
        truth.set_string(0, 0, after, Style::new());
        let visible = terminal.backend().buffer();
        for (truth_cell, visible_cell) in truth.content.iter().zip(visible.content.iter()) {
            if truth_cell.skip {
                assert!(visible_cell.skip);
            } else {
                assert_eq!(visible_cell.symbol, truth_cell.symbol);
                assert_eq!(visible_cell.style, truth_cell.style);
            }
        }
    }
}
