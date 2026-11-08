//! The keymap: declarative key bindings with multi-key chords.
//!
//! tuvix widget states stay key-agnostic — the app translates events into
//! state methods. For apps with more than a handful of bindings, the
//! hand-rolled `match` becomes the app's largest input handler. [`Keymap`]
//! is that translation as a type: bind sequences of [`KeyPattern`]s to
//! commands, feed it key events, and it answers [`Matched`](KeymapOutcome::Matched)
//! (run this command), [`Pending`](KeymapOutcome::Pending) (the event
//! extended a chord prefix — `g` waiting for a second `g`) or
//! [`Miss`](KeymapOutcome::Miss) (no binding, matcher reset).
//!
//! Matching semantics the tests pin down:
//!
//! - only press events advance the matcher; repeats and releases neither
//!   extend nor reset an open chord;
//! - a pattern's modifiers are a *subset* of the event's — a `Ctrl+C`
//!   binding fires for `Ctrl+Alt+C` too, and a bare `c` binding fires for
//!   `Shift+c` when the event still carries `Char('c')` (terminals report
//!   shifted letters as the uppercase char, which is a different pattern);
//! - a full binding that is a proper prefix of a longer binding is
//!   shadowed by it: with both `g` and `gg` bound, a lone `g` stays
//!   `Pending` forever. Bind the shorter form on a different key;
//! - a `Miss` or a `Matched` resets the matcher; a `Pending` keeps it open
//!   (there is no timeout — reset explicitly or feed the next key).
//!
//! # Examples
//!
//! ```
//! use tuvix_core::event::{KeyCode, KeyEvent, KeyModifiers};
//! use tuvix_core::keymap::{KeyPattern, Keymap, KeymapOutcome};
//!
//! #[derive(Debug, Clone, Copy, PartialEq, Eq)]
//! enum Command {
//!     Quit,
//!     GotoTop,
//!     DeleteWord,
//! }
//!
//! let mut keymap = Keymap::new();
//! keymap.bind(&[KeyPattern::new(KeyCode::Char('c')).ctrl()], Command::Quit);
//! keymap.bind(&[KeyPattern::new(KeyCode::Char('g')), KeyPattern::new(KeyCode::Char('g'))], Command::GotoTop);
//! keymap.bind(&[KeyPattern::new(KeyCode::Char('d')).ctrl(), KeyPattern::new(KeyCode::Backspace)], Command::DeleteWord);
//!
//! // A single key resolves immediately.
//! let quit = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL | KeyModifiers::ALT);
//! assert_eq!(keymap.step(&quit), KeymapOutcome::Matched(Command::Quit));
//!
//! // A chord opens with the first key and resolves with the last.
//! let g = KeyEvent::new(KeyCode::Char('g'), KeyModifiers::empty());
//! assert_eq!(keymap.step(&g), KeymapOutcome::Pending);
//! assert!(keymap.is_pending());
//! assert_eq!(keymap.step(&g), KeymapOutcome::Matched(Command::GotoTop));
//! assert!(!keymap.is_pending());
//! ```

use crate::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

/// One key step in a binding: a key code plus the modifiers it requires.
///
/// The pattern matches a [`KeyEvent`] when the codes are equal, the event
/// is a press, and every modifier the pattern names is held (the event may
/// hold more).
///
/// # Examples
///
/// ```
/// use tuvix_core::event::{KeyCode, KeyEvent, KeyModifiers};
/// use tuvix_core::keymap::KeyPattern;
///
/// let pattern = KeyPattern::new(KeyCode::Char('d')).ctrl();
/// let event = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL);
/// assert!(pattern.matches(&event));
///
/// let plain = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::empty());
/// assert!(!pattern.matches(&plain));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyPattern {
    /// The key code this pattern matches exactly.
    pub code: KeyCode,
    /// The modifiers that must be held (subset semantics).
    pub modifiers: KeyModifiers,
}

impl KeyPattern {
    /// A pattern for `code` with no modifier requirements.
    #[must_use]
    pub fn new(code: KeyCode) -> Self {
        Self {
            code,
            modifiers: KeyModifiers::empty(),
        }
    }

    /// Requires the control modifier.
    #[must_use]
    pub fn ctrl(mut self) -> Self {
        self.modifiers.insert(KeyModifiers::CONTROL);
        self
    }

    /// Requires the alt modifier.
    #[must_use]
    pub fn alt(mut self) -> Self {
        self.modifiers.insert(KeyModifiers::ALT);
        self
    }

    /// Requires the shift modifier.
    #[must_use]
    pub fn shift(mut self) -> Self {
        self.modifiers.insert(KeyModifiers::SHIFT);
        self
    }

    /// Requires the super modifier.
    #[must_use]
    pub fn super_key(mut self) -> Self {
        self.modifiers.insert(KeyModifiers::SUPER);
        self
    }

    /// Requires every modifier in `modifiers` (union with the current set).
    #[must_use]
    pub fn with(mut self, modifiers: KeyModifiers) -> Self {
        self.modifiers.insert(modifiers);
        self
    }

    /// Whether `key` satisfies this pattern: same code, a press, and the
    /// pattern's modifiers a subset of the event's.
    #[must_use]
    pub fn matches(&self, key: &KeyEvent) -> bool {
        key.kind == KeyEventKind::Press
            && key.code == self.code
            && key.modifiers.contains(self.modifiers)
    }
}

/// What one fed key event means to a [`Keymap`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeymapOutcome<C> {
    /// The event completed a binding; the matcher is reset.
    Matched(C),
    /// The event extended a chord prefix; the matcher stays open.
    Pending,
    /// The event matches nothing; the matcher is reset.
    Miss,
}

/// The binding trie: sequences of [`KeyPattern`]s mapped to commands.
///
/// The trie keeps the matcher's position between events, so chords like
/// `g g` resolve across two [`Keymap::step`] calls without the app
/// carrying any state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keymap<C> {
    root: Node<C>,
    path: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Node<C> {
    children: Vec<(KeyPattern, Node<C>)>,
    command: Option<C>,
}

impl<C> Default for Keymap<C> {
    /// An empty keymap — `C` needs no default of its own.
    fn default() -> Self {
        Self {
            root: Node {
                children: Vec::new(),
                command: None,
            },
            path: Vec::new(),
        }
    }
}

impl<C: Clone> Keymap<C> {
    /// Creates an empty keymap.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Binds `sequence` to `command`; later bindings of the same sequence
    /// overwrite earlier ones. An empty sequence cannot be bound (`false`).
    pub fn bind(&mut self, sequence: &[KeyPattern], command: C) -> bool {
        if sequence.is_empty() {
            return false;
        }
        let mut path: Vec<usize> = Vec::new();
        for pattern in sequence {
            let node = self.node_mut(&path);
            if let Some(index) = node.children.iter().position(|(known, _)| known == pattern) {
                path.push(index);
            } else {
                node.children.push((
                    *pattern,
                    Node {
                        children: Vec::new(),
                        command: None,
                    },
                ));
                path.push(node.children.len() - 1);
            }
        }
        self.node_mut(&path).command = Some(command);
        true
    }

    /// Feeds one key event to the matcher.
    ///
    /// Only press events advance matching; a repeat or release mid-chord
    /// reports [`KeymapOutcome::Pending`] (the chord stays open) and
    /// [`KeymapOutcome::Miss`] at the root.
    pub fn step(&mut self, key: &KeyEvent) -> KeymapOutcome<C> {
        // Repeats and releases carry no chord information: they neither
        // extend nor reset an open sequence.
        if key.kind != KeyEventKind::Press {
            return if self.path.is_empty() {
                KeymapOutcome::Miss
            } else {
                KeymapOutcome::Pending
            };
        }

        let decision = {
            let node = self.node_at(&self.path);
            node.children
                .iter()
                .position(|(pattern, _)| pattern.matches(key))
                .map(|index| {
                    let child = &node.children[index].1;
                    (index, child.children.is_empty())
                })
        };

        let Some((index, leaf)) = decision else {
            self.path.clear();
            return KeymapOutcome::Miss;
        };
        self.path.push(index);
        if leaf {
            let command = self.node_at(&self.path).command.clone();
            self.path.clear();
            return match command {
                Some(command) => KeymapOutcome::Matched(command),
                // Unreachable by construction (`bind` always plants a
                // command at the leaf), but the contract stays honest.
                None => KeymapOutcome::Miss,
            };
        }
        KeymapOutcome::Pending
    }

    /// Whether a chord prefix is open (the last step was
    /// [`KeymapOutcome::Pending`]).
    pub fn is_pending(&self) -> bool {
        !self.path.is_empty()
    }

    /// Closes an open chord prefix, forgetting the keys fed so far.
    pub fn reset(&mut self) {
        self.path.clear();
    }

    /// The node reached by walking `path` from the root.
    fn node_at(&self, path: &[usize]) -> &Node<C> {
        let mut node = &self.root;
        for &index in path {
            node = &node.children[index].1;
        }
        node
    }

    /// The node reached by walking `path` from the root, mutably.
    fn node_mut(&mut self, path: &[usize]) -> &mut Node<C> {
        let mut node = &mut self.root;
        for &index in path {
            node = &mut node.children[index].1;
        }
        node
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::KeyCode::{Backspace, Char, Enter};

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    fn press(code: KeyCode) -> KeyEvent {
        key(code, KeyModifiers::empty())
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Cmd {
        Quit,
        GotoTop,
        DeleteWord,
        Save,
    }

    use Cmd::{DeleteWord, GotoTop, Quit, Save};

    #[test]
    fn single_key_binding_matches() {
        let mut keymap: Keymap<Cmd> = Keymap::new();
        assert!(keymap.bind(&[KeyPattern::new(Char('q')).ctrl()], Quit));
        let event = key(Char('q'), KeyModifiers::CONTROL);
        assert_eq!(keymap.step(&event), KeymapOutcome::Matched(Quit));
        assert!(!keymap.is_pending());
    }

    #[test]
    fn miss_resets_and_reports() {
        let mut keymap: Keymap<Cmd> = Keymap::new();
        keymap.bind(&[KeyPattern::new(Char('q')).ctrl()], Quit);
        let unbound = press(Char('x'));
        assert_eq!(keymap.step(&unbound), KeymapOutcome::Miss);
        let event = key(Char('q'), KeyModifiers::CONTROL);
        assert_eq!(keymap.step(&event), KeymapOutcome::Matched(Quit));
    }

    #[test]
    fn chord_pends_then_matches() {
        let mut keymap: Keymap<Cmd> = Keymap::new();
        keymap.bind(
            &[KeyPattern::new(Char('g')), KeyPattern::new(Char('g'))],
            GotoTop,
        );
        let g = press(Char('g'));
        assert_eq!(keymap.step(&g), KeymapOutcome::Pending);
        assert!(keymap.is_pending());
        assert_eq!(keymap.step(&g), KeymapOutcome::Matched(GotoTop));
        assert!(!keymap.is_pending());
    }

    #[test]
    fn broken_chord_misses_and_resets() {
        let mut keymap: Keymap<Cmd> = Keymap::new();
        keymap.bind(
            &[KeyPattern::new(Char('g')), KeyPattern::new(Char('g'))],
            GotoTop,
        );
        let g = press(Char('g'));
        let x = press(Char('x'));
        assert_eq!(keymap.step(&g), KeymapOutcome::Pending);
        assert_eq!(keymap.step(&x), KeymapOutcome::Miss);
        assert!(!keymap.is_pending());
        // The matcher is clean: the chord still resolves.
        assert_eq!(keymap.step(&g), KeymapOutcome::Pending);
        assert_eq!(keymap.step(&g), KeymapOutcome::Matched(GotoTop));
    }

    #[test]
    fn longer_binding_shadows_its_prefix() {
        let mut keymap: Keymap<Cmd> = Keymap::new();
        keymap.bind(&[KeyPattern::new(Char('g'))], Save);
        keymap.bind(
            &[KeyPattern::new(Char('g')), KeyPattern::new(Char('g'))],
            GotoTop,
        );
        let g = press(Char('g'));
        // `g` alone is a proper prefix of `gg`: it can never resolve.
        assert_eq!(keymap.step(&g), KeymapOutcome::Pending);
        assert_eq!(keymap.step(&g), KeymapOutcome::Matched(GotoTop));
    }

    #[test]
    fn pattern_modifiers_are_a_subset_of_event_modifiers() {
        let mut keymap: Keymap<Cmd> = Keymap::new();
        keymap.bind(&[KeyPattern::new(Char('d')).ctrl()], DeleteWord);
        let ctrl_alt = key(Char('d'), KeyModifiers::CONTROL | KeyModifiers::ALT);
        assert_eq!(keymap.step(&ctrl_alt), KeymapOutcome::Matched(DeleteWord));
        // A bare-`c` pattern matches a shift-held `c` event that still
        // carries the lowercase char.
        keymap.bind(&[KeyPattern::new(Char('c'))], Save);
        let shifted = key(Char('c'), KeyModifiers::SHIFT);
        assert_eq!(keymap.step(&shifted), KeymapOutcome::Matched(Save));
    }

    #[test]
    fn missing_required_modifier_misses() {
        let mut keymap: Keymap<Cmd> = Keymap::new();
        keymap.bind(&[KeyPattern::new(Char('d')).ctrl()], DeleteWord);
        let plain = press(Char('d'));
        assert_eq!(keymap.step(&plain), KeymapOutcome::Miss);
    }

    #[test]
    fn shift_pattern_requires_shift() {
        let mut keymap: Keymap<Cmd> = Keymap::new();
        keymap.bind(&[KeyPattern::new(Char('G')).shift()], Save);
        let shifted = key(Char('G'), KeyModifiers::SHIFT);
        assert_eq!(keymap.step(&shifted), KeymapOutcome::Matched(Save));
        let capital_without_shift = press(Char('G'));
        assert_eq!(keymap.step(&capital_without_shift), KeymapOutcome::Miss);
    }

    #[test]
    fn release_events_neither_extend_nor_reset_a_chord() {
        let mut keymap: Keymap<Cmd> = Keymap::new();
        keymap.bind(
            &[KeyPattern::new(Char('g')), KeyPattern::new(Char('g'))],
            GotoTop,
        );
        let mut release = press(Char('g'));
        release.kind = KeyEventKind::Release;
        let g = press(Char('g'));
        assert_eq!(keymap.step(&g), KeymapOutcome::Pending);
        assert_eq!(keymap.step(&release), KeymapOutcome::Pending);
        assert!(keymap.is_pending());
        assert_eq!(keymap.step(&g), KeymapOutcome::Matched(GotoTop));
        // At the root a release is simply nothing.
        let mut other = press(Char('q'));
        other.kind = KeyEventKind::Repeat;
        assert_eq!(keymap.step(&other), KeymapOutcome::Miss);
    }

    #[test]
    fn rebind_overwrites_the_command() {
        let mut keymap: Keymap<Cmd> = Keymap::new();
        keymap.bind(&[KeyPattern::new(Char('q')).ctrl()], Quit);
        keymap.bind(&[KeyPattern::new(Char('q')).ctrl()], Save);
        let event = key(Char('q'), KeyModifiers::CONTROL);
        assert_eq!(keymap.step(&event), KeymapOutcome::Matched(Save));
    }

    #[test]
    fn branches_share_a_prefix() {
        let mut keymap: Keymap<Cmd> = Keymap::new();
        keymap.bind(
            &[KeyPattern::new(Char('d')), KeyPattern::new(Char('d'))],
            DeleteWord,
        );
        keymap.bind(
            &[KeyPattern::new(Char('d')), KeyPattern::new(Backspace)],
            GotoTop,
        );
        let d = press(Char('d'));
        let backspace = press(Backspace);
        assert_eq!(keymap.step(&d), KeymapOutcome::Pending);
        assert_eq!(keymap.step(&d), KeymapOutcome::Matched(DeleteWord));
        assert_eq!(keymap.step(&d), KeymapOutcome::Pending);
        assert_eq!(keymap.step(&backspace), KeymapOutcome::Matched(GotoTop));
    }

    #[test]
    fn three_key_chord_pends_twice() {
        let mut keymap: Keymap<Cmd> = Keymap::new();
        keymap.bind(
            &[
                KeyPattern::new(Char('g')),
                KeyPattern::new(Char('g')),
                KeyPattern::new(Char('g')),
            ],
            GotoTop,
        );
        let g = press(Char('g'));
        assert_eq!(keymap.step(&g), KeymapOutcome::Pending);
        assert_eq!(keymap.step(&g), KeymapOutcome::Pending);
        assert_eq!(keymap.step(&g), KeymapOutcome::Matched(GotoTop));
    }

    #[test]
    fn empty_sequence_cannot_bind() {
        let mut keymap: Keymap<Cmd> = Keymap::new();
        assert!(!keymap.bind(&[], Quit));
        assert_eq!(keymap.step(&press(Enter)), KeymapOutcome::Miss);
    }

    #[test]
    fn reset_closes_an_open_chord() {
        let mut keymap: Keymap<Cmd> = Keymap::new();
        keymap.bind(
            &[KeyPattern::new(Char('g')), KeyPattern::new(Char('g'))],
            GotoTop,
        );
        let g = press(Char('g'));
        assert_eq!(keymap.step(&g), KeymapOutcome::Pending);
        keymap.reset();
        assert!(!keymap.is_pending());
        // Without the pending prefix, the same key is a fresh open again.
        assert_eq!(keymap.step(&g), KeymapOutcome::Pending);
    }

    #[test]
    fn default_keymap_misses_everything() {
        let mut keymap: Keymap<Cmd> = Keymap::default();
        assert_eq!(keymap.step(&press(Enter)), KeymapOutcome::Miss);
        assert!(!keymap.is_pending());
    }

    #[test]
    fn key_pattern_display_fields_are_public() {
        // The struct is data: apps can inspect and build patterns directly.
        let pattern = KeyPattern::new(Char('x')).with(KeyModifiers::ALT);
        assert_eq!(pattern.code, Char('x'));
        assert!(pattern.modifiers.contains(KeyModifiers::ALT));
        assert_eq!(pattern, KeyPattern::new(Char('x')).alt());
    }
}
