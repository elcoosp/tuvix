//! Wide-character corpus: incremental rendering must be byte-identical to
//! a full rebuild.
//!
//! The M1 exit gate from the architecture document: every corpus glyph
//! (ZWJ emoji, VS16, skin tones, CJK, halfwidth dakuten, combining marks)
//! goes through a sequence of mutations rendered *incrementally* through
//! [`Terminal`] + [`TestBackend`], and after every step the visible state
//! must equal a from-scratch rebuild of the same content — cell for cell,
//! including wide-glyph coverage parity.

use std::borrow::Cow;

use tuvix_core::backend::TestBackend;
use tuvix_core::buffer::{Buffer, Symbol};
use tuvix_core::geometry::{Rect, Size};
use tuvix_core::style::{Color, Modifier, Style};
use tuvix_core::terminal::Terminal;

/// The M1 wide-character corpus. Byte lengths span every storage tier of
/// `Symbol` (inline / boxed) and the width classes terminals get wrong.
const CORPUS: [&str; 10] = [
    "汉",       // CJK, 3 bytes, width 2
    "文",       // CJK, 3 bytes, width 2
    "❤️",       // VS16, 7 bytes, width 2 — inline tier boundary
    "🏳️‍🌈",       // ZWJ rainbow flag, 14 bytes — exactly at the inline cap
    "👨‍👩‍👧‍👦",       // ZWJ family, 25 bytes — boxed tier
    "🏳️‍⚧️",       // ZWJ transgender flag — boxed tier
    "👋🏽",       // skin tone modifier, 8 bytes
    "🇩🇪",       // regional indicators (flag), 8 bytes
    "ｶﾞ",        // halfwidth dakuten, 6 bytes
    "e\u{301}", // combining acute, 3 bytes, width 1
];

/// Deterministic LCG for the randomized corpus fuzz.
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

fn corpus_glyph(rng: &mut Lcg) -> &'static str {
    CORPUS[usize::try_from(rng.range(u32::try_from(CORPUS.len()).unwrap_or(u32::MAX))).unwrap_or(0)]
}

fn corpus_char(rng: &mut Lcg) -> char {
    // Single chars pulled from corpus glyphs' leading codepoints.
    match rng.range(5) {
        0 => '汉',
        1 => '文',
        2 => 'ｱ',
        3 => char::from(b'a' + u8::try_from(rng.range(26)).unwrap_or(0)),
        _ => '❤',
    }
}

fn random_style(rng: &mut Lcg) -> Style {
    let mut style = Style::new().fg(Color::Red).bg(Color::Blue);
    if rng.range(2) == 0 {
        style = style.add_modifier(Modifier::BOLD);
    }
    style
}

/// Asserts visible == truth: symbols and styles match on non-covered
/// cells; coverage (skip) parity holds on covered cells.
fn assert_visible_matches_truth(terminal: &Terminal<TestBackend>, truth: &Buffer, step: &str) {
    let visible = terminal.backend().buffer();
    assert_eq!(visible.area, truth.area, "{step}: area drifted");
    for (index, (truth_cell, visible_cell)) in
        truth.content.iter().zip(visible.content.iter()).enumerate()
    {
        let position = truth.position_of(index);
        assert_eq!(
            visible_cell.skip, truth_cell.skip,
            "{step} at {position:?}: coverage parity broken (visible={:?} truth={:?})\nvisible:\n{visible}\ntruth:\n{truth}",
            visible_cell.symbol, truth_cell.symbol
        );
        if !truth_cell.skip {
            assert_eq!(
                visible_cell.symbol, truth_cell.symbol,
                "{step} at {position:?}: symbol diverged\nvisible:\n{visible}\ntruth:\n{truth}"
            );
            assert_eq!(
                visible_cell.style, truth_cell.style,
                "{step} at {position:?}: style diverged\nvisible:\n{visible}\ntruth:\n{truth}"
            );
        }
    }
}

/// Drives one mutation on both the truth buffer and the terminal frame.
fn apply_mutation(
    truth: &mut Buffer,
    terminal: &mut Terminal<TestBackend>,
    rng: &mut Lcg,
    size: Size,
) {
    match rng.range(7) {
        // Write a corpus glyph at a random position.
        0 | 1 => {
            let x = u16::try_from(rng.range(u32::from(size.width))).unwrap_or(u16::MAX);
            let y = u16::try_from(rng.range(u32::from(size.height))).unwrap_or(u16::MAX);
            let glyph = corpus_glyph(rng);
            let style = random_style(rng);
            truth.set_string(x, y, glyph, style);
        }
        // Write a narrow run that overlaps wide pairs.
        2 => {
            let x = u16::try_from(rng.range(u32::from(size.width))).unwrap_or(u16::MAX);
            let y = u16::try_from(rng.range(u32::from(size.height))).unwrap_or(u16::MAX);
            let len = 1 + rng.range(6) as usize;
            let word: String = (0..len).map(|_| corpus_char(rng)).collect();
            let style = random_style(rng);
            truth.set_string(x, y, &word, style);
        }
        // Overwrite with a corpus glyph (wide over narrow and back).
        3 => {
            let x = u16::try_from(rng.range(u32::from(size.width.saturating_sub(1))))
                .unwrap_or(u16::MAX);
            let y = u16::try_from(rng.range(u32::from(size.height))).unwrap_or(u16::MAX);
            let glyph = corpus_glyph(rng);
            let style = random_style(rng);
            truth.set_string(x, y, glyph, style);
        }
        // Clear a random row through the pair-repairing path.
        4 => {
            let y = u16::try_from(rng.range(u32::from(size.height))).unwrap_or(u16::MAX);
            let row = Rect::new(0, y, size.width, 1);
            truth.clear(row);
        }
        // Clear a random cell region.
        5 => {
            let x = u16::try_from(rng.range(u32::from(size.width))).unwrap_or(u16::MAX);
            let y = u16::try_from(rng.range(u32::from(size.height))).unwrap_or(u16::MAX);
            let w = u16::try_from(rng.range(u32::from(size.width) + 1)).unwrap_or(u16::MAX);
            let h = u16::try_from(rng.range(u32::from(size.height) + 1)).unwrap_or(u16::MAX);
            let area = Rect::new(x, y, w, h);
            truth.clear(area);
        }
        // Full reset.
        _ => {
            truth.reset();
        }
    }
    // tuvix is immediate-mode: each frame repaints the whole screen from
    // state; the diff engine decides what reaches the terminal.
    let snapshot = truth.content.clone();
    terminal
        .draw(|frame| frame.buffer.content.clone_from(&snapshot))
        .unwrap();
}

#[test]
fn every_corpus_glyph_transitions_are_byte_identical_to_full_rebuild() {
    let size = Size::new(20, 4);
    for glyph in CORPUS {
        // The transition matrix: glyph arrives, is overwritten by every
        // other corpus glyph, is overwritten by narrow text, and is cleared.
        let mut transitions: Vec<Cow<'static, str>> = Vec::new();
        transitions.push(Cow::Borrowed(glyph));
        for other in CORPUS {
            transitions.push(Cow::Borrowed(other));
        }
        transitions.push(Cow::Borrowed("ab"));
        transitions.push(Cow::Borrowed(""));
        transitions.push(Cow::Borrowed(glyph));

        let mut terminal = Terminal::new(TestBackend::new(size)).unwrap();
        let mut truth = Buffer::empty(Rect::from_size(size));
        let mut cursor_x = 0_u16;

        for (step, text) in transitions.iter().enumerate() {
            // Advance the write position so glyphs interact with neighbors.
            cursor_x = (cursor_x + 3) % (size.width.saturating_sub(4));
            truth.set_string(cursor_x, 1, text, Style::new().fg(Color::Cyan));
            // Immediate mode: the frame repaints the accumulated truth.
            let snapshot = truth.content.clone();
            terminal
                .draw(|frame| frame.buffer.content.clone_from(&snapshot))
                .unwrap();
            assert_visible_matches_truth(
                &terminal,
                &truth,
                &format!("glyph {glyph:?} transition {step} to {text:?}"),
            );
        }
    }
}

#[test]
fn randomized_corpus_fuzz_matches_full_rebuild() {
    let size = Size::new(24, 8);
    let mut truth = Buffer::empty(Rect::from_size(size));
    let mut terminal = Terminal::new(TestBackend::new(size)).unwrap();
    let mut rng = Lcg(0x5EED_C0DE_5EED_C0DE);

    for step in 0..600 {
        apply_mutation(&mut truth, &mut terminal, &mut rng, size);
        assert_visible_matches_truth(&terminal, &truth, &format!("corpus fuzz step {step}"));
    }
}

#[test]
fn corpus_symbols_keep_width_contracts() {
    // The width table the wrapper and the diff engine both rely on.
    let expected: [(&str, usize, bool); 10] = [
        ("汉", 2, true),
        ("文", 2, true),
        ("❤️", 2, true),
        ("🏳️‍🌈", 2, true),
        ("👨‍👩‍👧‍👦", 2, true),
        ("🏳️‍⚧️", 2, true),
        ("👋🏽", 2, true),
        ("🇩🇪", 2, true),
        ("ｶﾞ", 2, true),
        ("e\u{301}", 1, false),
    ];
    for (glyph, width, wide) in expected {
        let symbol = Symbol::new(glyph);
        assert_eq!(symbol.width(), width, "width of {glyph:?}");
        assert_eq!(symbol.is_inline(), symbol.as_str().len() <= 14);
        let mut cell = tuvix_core::buffer::Cell::empty();
        cell.set_symbol(glyph);
        assert_eq!(cell.is_wide(), wide, "wide flag of {glyph:?}");
    }
}
