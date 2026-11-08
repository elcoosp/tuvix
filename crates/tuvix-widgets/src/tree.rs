//! Trees: collapsible hierarchical navigation.
//!
//! A [`Tree`] renders its [`TreeItem`]s depth-first with box-drawing
//! guide lines, one row per visible node. [`TreeState`] owns the
//! selection (a path of child indexes), the fold state (a set of
//! collapsed paths) and the scroll offset; the renderer scrolls
//! minimally so the selection stays visible.
//!
//! Navigation helpers take the item slice — like
//! [`ListState::select_last`](crate::ListState) takes a count —
//! so the state machine needs no hidden copy of the data: visibility
//! depends on the fold state, which only the caller's tree plus the
//! state's collapsed set can answer.

use std::collections::BTreeSet;

use tuvix_core::buffer::Buffer;
use tuvix_core::geometry::Rect;
use tuvix_core::style::Style;
use tuvix_core::text::Text;
use tuvix_core::widget::RenderWidget;

use crate::block::Block;

/// One node of a [`Tree`]: a text, optional children and a style.
///
/// # Examples
///
/// ```
/// use tuvix_core::style::{Color, Style};
/// use tuvix_widgets::TreeItem;
///
/// let tree = TreeItem::new("src").child(
///     TreeItem::new("main.rs").style(Style::new().fg(Color::Cyan)),
/// );
/// assert_eq!(tree.children.len(), 1);
/// ```
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TreeItem {
    /// The text of the node; only its first line is rendered (a node is
    /// one row).
    pub text: Text,
    /// The children, rendered after the node when it is expanded.
    pub children: Vec<TreeItem>,
    /// Style layered under the node's text.
    pub style: Style,
}

impl TreeItem {
    /// Creates a leaf node from anything [`Text`]-convertible.
    pub fn new<T: Into<Text>>(text: T) -> Self {
        Self {
            text: text.into(),
            children: Vec::new(),
            style: Style::new(),
        }
    }

    /// Creates a node with children.
    pub fn with_children<T, I>(text: T, children: I) -> Self
    where
        T: Into<Text>,
        I: IntoIterator<Item = TreeItem>,
    {
        Self::new(text).children(children)
    }

    /// Appends a child node (builder form).
    #[must_use]
    pub fn child(mut self, child: TreeItem) -> Self {
        self.children.push(child);
        self
    }

    /// Replaces the children with the given ones (builder form).
    #[must_use]
    pub fn children<I>(mut self, children: I) -> Self
    where
        I: IntoIterator<Item = TreeItem>,
    {
        self.children = children.into_iter().collect();
        self
    }

    /// Sets the style layered under the node's text.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }
}

impl From<&'static str> for TreeItem {
    fn from(text: &'static str) -> Self {
        Self::new(text)
    }
}

impl From<String> for TreeItem {
    fn from(text: String) -> Self {
        Self::new(text)
    }
}

/// The selection, fold and scroll state of a [`Tree`].
///
/// A *path* is the chain of child indexes from the root list to a node
/// (`[0, 1]` = the second child of the first root). A node is visible
/// when no strict prefix of its path is collapsed.
///
/// # Examples
///
/// ```
/// use tuvix_widgets::TreeState;
///
/// let mut state = TreeState::default().with_selected([1]);
/// assert_eq!(state.selected(), Some(&[1_usize][..]));
/// state.collapse([0]);
/// assert!(state.is_collapsed(&[0]));
/// state.toggle([0]);
/// assert!(!state.is_collapsed(&[0]));
/// state.expand_all();
/// assert!(state.collapsed().next().is_none());
/// ```
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TreeState {
    /// Visible nodes scrolled off the top.
    pub offset: usize,
    selected: Option<Vec<usize>>,
    collapsed: BTreeSet<Vec<usize>>,
}

impl TreeState {
    /// Creates a state with the given initial selection.
    #[must_use]
    pub fn with_selected<P>(mut self, path: P) -> Self
    where
        P: IntoIterator<Item = usize>,
    {
        self.selected = Some(path.into_iter().collect());
        self
    }

    /// The selected path, if any.
    #[must_use]
    pub fn selected(&self) -> Option<&[usize]> {
        self.selected.as_deref()
    }

    /// Sets the selected path (raw; out-of-range paths render without a
    /// highlight).
    pub fn select<P>(&mut self, path: Option<P>)
    where
        P: IntoIterator<Item = usize>,
    {
        self.selected = path.map(|path| path.into_iter().collect());
    }

    /// The collapsed paths, in lexicographic order.
    pub fn collapsed(&self) -> impl Iterator<Item = &Vec<usize>> {
        self.collapsed.iter()
    }

    /// Whether the given path is collapsed.
    #[must_use]
    pub fn is_collapsed(&self, path: &[usize]) -> bool {
        self.collapsed.contains(path)
    }

    /// Collapses the node at `path` (hiding its descendants).
    pub fn collapse<P>(&mut self, path: P)
    where
        P: IntoIterator<Item = usize>,
    {
        self.collapsed.insert(path.into_iter().collect());
    }

    /// Expands the node at `path`.
    pub fn expand<P>(&mut self, path: P)
    where
        P: IntoIterator<Item = usize>,
    {
        self.collapsed
            .remove(path.into_iter().collect::<Vec<_>>().as_slice());
    }

    /// Expands if collapsed, collapses if expanded.
    pub fn toggle<P>(&mut self, path: P)
    where
        P: IntoIterator<Item = usize>,
    {
        let path: Vec<usize> = path.into_iter().collect();
        if !self.collapsed.remove(path.as_slice()) {
            self.collapsed.insert(path);
        }
    }

    /// Collapses every node that has children.
    pub fn collapse_all(&mut self, items: &[TreeItem]) {
        fn walk(item: &TreeItem, prefix: &mut Vec<usize>, collapsed: &mut BTreeSet<Vec<usize>>) {
            if item.children.is_empty() {
                return;
            }
            collapsed.insert(prefix.clone());
            for (index, child) in item.children.iter().enumerate() {
                prefix.push(index);
                walk(child, prefix, collapsed);
                prefix.pop();
            }
        }
        let mut prefix = Vec::new();
        for (index, item) in items.iter().enumerate() {
            prefix.push(index);
            walk(item, &mut prefix, &mut self.collapsed);
            prefix.pop();
        }
    }

    /// Expands every node.
    pub fn expand_all(&mut self) {
        self.collapsed.clear();
    }

    /// Selects the next visible node in depth-first order (or the first
    /// when nothing — or something invisible — is selected); saturates
    /// at the last visible node. Clears the selection when nothing is
    /// visible.
    pub fn select_next(&mut self, items: &[TreeItem]) {
        let visible = visible_nodes(items, &self.collapsed);
        if visible.is_empty() {
            self.selected = None;
            return;
        }
        self.selected = Some(
            match self
                .selected
                .as_ref()
                .and_then(|selected| visible.iter().position(|node| &node.path == selected))
            {
                Some(index) => visible[(index + 1).min(visible.len() - 1)].path.clone(),
                None => visible[0].path.clone(),
            },
        );
    }

    /// Selects the previous visible node in depth-first order;
    /// saturates at the first visible node. Nothing selected picks the
    /// first visible node; an invisible selection picks the last one.
    /// Clears the selection when nothing is visible.
    pub fn select_previous(&mut self, items: &[TreeItem]) {
        let visible = visible_nodes(items, &self.collapsed);
        if visible.is_empty() {
            self.selected = None;
            return;
        }
        self.selected = Some(
            match self
                .selected
                .as_ref()
                .and_then(|selected| visible.iter().position(|node| &node.path == selected))
            {
                Some(0) => visible[0].path.clone(),
                Some(index) => visible[index - 1].path.clone(),
                None if self.selected.is_none() => visible[0].path.clone(),
                None => visible[visible.len() - 1].path.clone(),
            },
        );
    }

    /// Selects the first root.
    pub fn select_first(&mut self) {
        self.selected = Some(vec![0]);
    }

    /// Selects the last visible node; the caller supplies the roots so
    /// no sentinel value is involved. Clears the selection when nothing
    /// is visible.
    pub fn select_last(&mut self, items: &[TreeItem]) {
        self.selected = visible_nodes(items, &self.collapsed)
            .last()
            .map(|node| node.path.clone());
    }

    /// Scrolls the view down by `rows` visible nodes.
    pub fn scroll_down_by(&mut self, rows: usize) {
        self.offset += rows;
    }

    /// Scrolls the view up by `rows` visible nodes (saturating at 0).
    pub fn scroll_up_by(&mut self, rows: usize) {
        self.offset = self.offset.saturating_sub(rows);
    }
}

/// A collapsible tree of items with selection.
///
/// # Examples
///
/// ```
/// use tuvix_core::buffer::Buffer;
/// use tuvix_core::geometry::Rect;
/// use tuvix_core::widget::RenderWidget;
/// use tuvix_widgets::{Tree, TreeItem, TreeState};
///
/// let mut buffer = Buffer::empty(Rect::new(0, 0, 8, 2));
/// let tree = Tree::new([
///     TreeItem::with_children("a", [TreeItem::new("a1")]),
///     TreeItem::new("b"),
/// ]);
/// tree.render(Rect::new(0, 0, 8, 2), &mut buffer, &mut TreeState::default());
/// assert_eq!(buffer.to_string(), "├─ a    \n│  └─ a1");
/// ```
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Tree {
    items: Vec<TreeItem>,
    block: Option<Block>,
    style: Style,
    highlight_style: Style,
}

impl Tree {
    /// Creates a tree from an iterator of [`TreeItem`]s.
    pub fn new<T, I>(items: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<TreeItem>,
    {
        Self {
            items: items.into_iter().map(Into::into).collect(),
            block: None,
            style: Style::new(),
            highlight_style: Style::new(),
        }
    }

    /// Wraps the tree in a block; nodes fill the block's inner area.
    #[must_use]
    pub fn block(mut self, block: Block) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the base style of the tree area.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the style patched over the selected node's row.
    #[must_use]
    pub fn highlight_style(mut self, style: Style) -> Self {
        self.highlight_style = style;
        self
    }

    /// Paints the tree into `buffer`, clipped to `area`, updating `state`.
    ///
    /// Called through [`RenderWidget::render`]; the method-call form is
    /// `tree.render(area, buffer, &mut state)`.
    fn paint(self, area: Rect, buffer: &mut Buffer, state: &mut TreeState) {
        if let Some(block) = &self.block {
            block.render(area, buffer);
        }
        let inner = self.block.as_ref().map_or(area, |block| block.inner(area));
        if inner.is_empty() {
            return;
        }
        buffer.set_style(inner, self.style);

        let visible = visible_nodes(&self.items, &state.collapsed);
        if visible.is_empty() {
            return;
        }
        let height = usize::from(inner.height);
        // Minimal scroll so the selection stays visible.
        if let Some(index) = state
            .selected
            .as_ref()
            .and_then(|selected| visible.iter().position(|node| &node.path == selected))
        {
            if index < state.offset {
                state.offset = index;
            } else if index >= state.offset + height {
                state.offset = index + 1 - height;
            }
        }

        for (row, node) in visible.iter().skip(state.offset).take(height).enumerate() {
            let y = inner.y + u16::try_from(row).unwrap_or(u16::MAX);
            let row_style = self.style.patch(node.item.style);
            let row_style = if state.selected.as_ref() == Some(&node.path) {
                row_style.patch(self.highlight_style)
            } else {
                row_style
            };
            let mut x = inner.x;
            for &last in &node.ancestors_last {
                if x >= inner.right() {
                    break;
                }
                let guide = if last { "   " } else { "│  " };
                let (next, _) = buffer.set_stringn(x, y, guide, 3, row_style);
                x = next;
            }
            if x < inner.right() {
                let connector = if node.is_last { "└─ " } else { "├─ " };
                let (next, _) = buffer.set_stringn(x, y, connector, 3, row_style);
                x = next;
            }

            let Some(line) = node.item.text.lines.first() else {
                continue;
            };
            for span in &line.spans {
                if x >= inner.right() {
                    break;
                }
                let remaining = inner.right() - x;
                let (next, _) = buffer.set_stringn(
                    x,
                    y,
                    span.content.as_ref(),
                    remaining,
                    row_style.patch(line.style).patch(span.style),
                );
                x = next;
            }
        }
    }
}

impl RenderWidget for Tree {
    type State = TreeState;

    fn render(self, area: Rect, buffer: &mut Buffer, state: &mut Self::State) {
        Tree::paint(self, area, buffer, state);
    }
}

impl Tree {
    /// Maps the theme onto the tree: the `highlight` slot styles the
    /// selected row.
    #[must_use]
    pub fn theme(mut self, theme: &Theme) -> Self {
        self.highlight_style = theme.highlight;
        self
    }
}

use tuvix_core::theme::Theme;

/// One visible node of the depth-first flattening: its path, its
/// nesting depth, the item itself, and whether each ancestor level is
/// the last child of its own parent (for the guide lines).
struct VisibleNode<'a> {
    path: Vec<usize>,
    item: &'a TreeItem,
    ancestors_last: Vec<bool>,
    is_last: bool,
}

/// Flattens the tree depth-first over the nodes visible under `collapsed`.
fn visible_nodes<'a>(
    items: &'a [TreeItem],
    collapsed: &BTreeSet<Vec<usize>>,
) -> Vec<VisibleNode<'a>> {
    fn push_level<'a>(
        items: &'a [TreeItem],
        path: &mut Vec<usize>,
        ancestors_last: &mut Vec<bool>,
        collapsed: &BTreeSet<Vec<usize>>,
        out: &mut Vec<VisibleNode<'a>>,
    ) {
        for (index, item) in items.iter().enumerate() {
            let is_last = index + 1 == items.len();
            path.push(index);
            ancestors_last.push(is_last);
            out.push(VisibleNode {
                path: path.clone(),
                item,
                ancestors_last: ancestors_last[..ancestors_last.len() - 1].to_vec(),
                is_last,
            });
            if !item.children.is_empty() && !collapsed.contains(path.as_slice()) {
                push_level(&item.children, path, ancestors_last, collapsed, out);
            }
            ancestors_last.pop();
            path.pop();
        }
    }

    let mut out = Vec::new();
    let mut path = Vec::new();
    let mut ancestors_last = Vec::new();
    push_level(items, &mut path, &mut ancestors_last, collapsed, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tuvix_core::style::Color;

    fn sample() -> Vec<TreeItem> {
        vec![
            TreeItem::with_children("a", [TreeItem::new("a1"), TreeItem::new("a2")]),
            TreeItem::new("b"),
        ]
    }

    fn render_at(tree: Tree, area: Rect, buffer: &mut Buffer, state: &mut TreeState) {
        RenderWidget::render(tree, area, buffer, state);
    }

    mod fold_state {
        use super::*;

        #[test]
        fn collapse_expand_toggle() {
            let mut state = TreeState::default();
            assert!(!state.is_collapsed(&[0]));
            state.collapse([0]);
            assert!(state.is_collapsed(&[0]));
            assert!(!state.is_collapsed(&[1]));
            state.expand([0]);
            assert!(!state.is_collapsed(&[0]));
            state.toggle([1]);
            assert!(state.is_collapsed(&[1]));
            state.toggle([1]);
            assert!(!state.is_collapsed(&[1]));
        }

        #[test]
        fn collapse_all_marks_every_parent() {
            let mut state = TreeState::default();
            state.collapse_all(&sample());
            assert!(state.is_collapsed(&[0]));
            assert!(!state.is_collapsed(&[1]));
        }

        #[test]
        fn expand_all_clears_everything() {
            let mut state = TreeState::default();
            state.collapse([0]);
            state.expand_all();
            assert_eq!(state.collapsed().count(), 0);
        }

        #[test]
        fn with_selected_and_select() {
            let state = TreeState::default().with_selected([1, 2]);
            assert_eq!(state.selected(), Some(&[1_usize, 2][..]));
            let mut state = state;
            state.select::<Vec<usize>>(None);
            assert_eq!(state.selected(), None);
        }

        #[test]
        fn scroll_by_moves_offset() {
            let mut state = TreeState::default();
            state.scroll_down_by(3);
            assert_eq!(state.offset, 3);
            state.scroll_up_by(5);
            assert_eq!(state.offset, 0);
        }
    }

    mod navigation {
        use super::*;

        fn visible_of(items: &[TreeItem], collapsed: &BTreeSet<Vec<usize>>) -> Vec<Vec<usize>> {
            // Mirrors the renderer's flattening for test readability.
            fn walk(
                item: &TreeItem,
                path: &mut Vec<usize>,
                out: &mut Vec<Vec<usize>>,
                collapsed: &BTreeSet<Vec<usize>>,
            ) {
                out.push(path.clone());
                if item.children.is_empty() || collapsed.contains(path.as_slice()) {
                    return;
                }
                for (index, child) in item.children.iter().enumerate() {
                    path.push(index);
                    walk(child, path, out, collapsed);
                    path.pop();
                }
            }
            let mut out = Vec::new();
            let mut path = Vec::new();
            for (index, item) in items.iter().enumerate() {
                path.push(index);
                walk(item, &mut path, &mut out, collapsed);
                path.pop();
            }
            out
        }

        #[test]
        fn select_next_walks_depth_first_over_visible_nodes() {
            let items = sample();
            let mut state = TreeState::default();
            state.select_next(&items);
            assert_eq!(state.selected(), Some(&[0_usize][..]));
            state.select_next(&items);
            assert_eq!(state.selected(), Some(&[0_usize, 0][..]));
            state.select_next(&items);
            assert_eq!(state.selected(), Some(&[0_usize, 1][..]));
            state.select_next(&items);
            assert_eq!(state.selected(), Some(&[1_usize][..]));
        }

        #[test]
        fn select_next_saturates_at_the_last_visible_node() {
            let items = sample();
            let mut state = TreeState::default().with_selected([1]);
            state.select_next(&items);
            state.select_next(&items);
            assert_eq!(state.selected(), Some(&[1_usize][..]));
        }

        #[test]
        fn collapsed_children_are_skipped_by_navigation() {
            let items = sample();
            let mut state = TreeState::default();
            state.collapse([0]);
            state.select_next(&items);
            assert_eq!(state.selected(), Some(&[0_usize][..]));
            state.select_next(&items);
            assert_eq!(state.selected(), Some(&[1_usize][..]));
        }

        #[test]
        fn select_previous_saturates_at_the_first_visible_node() {
            let items = sample();
            let mut state = TreeState::default().with_selected([0]);
            state.select_previous(&items);
            assert_eq!(state.selected(), Some(&[0_usize][..]));
        }

        #[test]
        fn select_previous_walks_backwards() {
            let items = sample();
            let mut state = TreeState::default().with_selected([1]);
            state.select_previous(&items);
            assert_eq!(state.selected(), Some(&[0_usize, 1][..]));
            state.select_previous(&items);
            assert_eq!(state.selected(), Some(&[0_usize, 0][..]));
            state.select_previous(&items);
            assert_eq!(state.selected(), Some(&[0_usize][..]));
        }

        #[test]
        fn navigation_on_an_empty_tree_clears_the_selection() {
            let mut state = TreeState::default();
            state.select_next(&[]);
            assert_eq!(state.selected(), None);
            state.select(Some([1_usize]));
            state.select_previous(&[]);
            // Nothing is visible, so there is nothing to select.
            assert_eq!(state.selected(), None);
        }

        #[test]
        fn select_first_selects_the_first_root() {
            let mut state = TreeState::default().with_selected([1]);
            state.select_first();
            assert_eq!(state.selected(), Some(&[0_usize][..]));
            state.select::<Vec<usize>>(None);
            state.select_first();
            assert_eq!(state.selected(), Some(&[0_usize][..]));
        }

        #[test]
        fn select_last_selects_the_last_visible_node() {
            let items = sample();
            let mut state = TreeState::default();
            state.select_last(&items);
            assert_eq!(state.selected(), Some(&[1_usize][..]));
            state.collapse([0]);
            state.select_last(&items);
            assert_eq!(state.selected(), Some(&[1_usize][..]));
            state.expand_all();
            state.collapse([0]);
            state.select_last(&items);
            assert_eq!(state.selected(), Some(&[1_usize][..]));
        }

        #[test]
        fn navigation_matches_the_renderer_flattening() {
            let items = sample();
            let mut state = TreeState::default();
            let mut visited = Vec::new();
            for _ in 0..4 {
                state.select_next(&items);
                visited.push(state.selected().unwrap().to_vec());
            }
            assert_eq!(
                visited,
                visible_of(&items, &state.collapsed().cloned().collect())
            );
        }
    }

    mod rendering {
        use super::*;

        #[test]
        fn roots_get_connectors_and_children_indent() {
            let mut b = Buffer::empty(Rect::new(0, 0, 8, 2));
            render_at(
                Tree::new(sample()),
                Rect::new(0, 0, 8, 2),
                &mut b,
                &mut TreeState::default(),
            );
            assert_eq!(b.to_string(), "├─ a    \n│  ├─ a1");
        }

        #[test]
        fn full_tree_shows_guides_per_level() {
            let mut b = Buffer::empty(Rect::new(0, 0, 11, 4));
            render_at(
                Tree::new(sample()),
                Rect::new(0, 0, 11, 4),
                &mut b,
                &mut TreeState::default(),
            );
            assert_eq!(
                b.to_string(),
                "├─ a       \n│  ├─ a1   \n│  └─ a2   \n└─ b       "
            );
        }

        #[test]
        fn collapsed_nodes_hide_their_descendants() {
            let mut b = Buffer::empty(Rect::new(0, 0, 8, 4));
            let mut state = TreeState::default();
            state.collapse([0]);
            render_at(
                Tree::new(sample()),
                Rect::new(0, 0, 8, 4),
                &mut b,
                &mut state,
            );
            assert_eq!(b.to_string(), "├─ a    \n└─ b    \n        \n        ");
        }

        #[test]
        fn deep_nesting_extends_the_vertical_guide() {
            let items = vec![TreeItem::with_children(
                "a",
                [TreeItem::with_children("a1", [TreeItem::new("a1a")])],
            )];
            let mut b = Buffer::empty(Rect::new(0, 0, 14, 3));
            render_at(
                Tree::new(items),
                Rect::new(0, 0, 14, 3),
                &mut b,
                &mut TreeState::default(),
            );
            assert_eq!(
                b.to_string(),
                "└─ a          \n   └─ a1      \n      └─ a1a  "
            );
        }

        #[test]
        fn selection_is_highlighted() {
            let mut b = Buffer::empty(Rect::new(0, 0, 8, 2));
            let mut state = TreeState::default().with_selected([0]);
            render_at(
                Tree::new(sample()).highlight_style(Style::new().fg(Color::Green)),
                Rect::new(0, 0, 8, 2),
                &mut b,
                &mut state,
            );
            assert_eq!(b.get(0, 0).unwrap().style.fg, Some(Color::Green));
            assert_eq!(b.get(0, 1).unwrap().style.fg, None);
        }

        #[test]
        fn out_of_range_selection_renders_without_highlight() {
            let mut b = Buffer::empty(Rect::new(0, 0, 8, 2));
            let mut state = TreeState::default().with_selected([9]);
            render_at(
                Tree::new(sample()).highlight_style(Style::new().fg(Color::Green)),
                Rect::new(0, 0, 8, 2),
                &mut b,
                &mut state,
            );
            assert_eq!(b.get(0, 0).unwrap().style.fg, None);
        }

        #[test]
        fn selection_scrolls_down_minimally() {
            let mut b = Buffer::empty(Rect::new(0, 0, 11, 2));
            let mut state = TreeState::default().with_selected([1]);
            render_at(
                Tree::new(sample()),
                Rect::new(0, 0, 11, 2),
                &mut b,
                &mut state,
            );
            assert_eq!(state.offset, 2);
            assert_eq!(b.to_string(), "│  └─ a2   \n└─ b       ");
        }

        #[test]
        fn selection_scrolls_up_minimally() {
            let mut b = Buffer::empty(Rect::new(0, 0, 11, 2));
            let mut state = TreeState::default().with_selected([0]);
            state.offset = 2;
            render_at(
                Tree::new(sample()),
                Rect::new(0, 0, 11, 2),
                &mut b,
                &mut state,
            );
            assert_eq!(state.offset, 0);
            assert_eq!(b.to_string(), "├─ a       \n│  ├─ a1   ");
        }

        #[test]
        fn offset_renders_the_middle_of_the_tree() {
            let mut b = Buffer::empty(Rect::new(0, 0, 11, 2));
            let mut state = TreeState {
                offset: 1,
                ..TreeState::default()
            };
            render_at(
                Tree::new(sample()),
                Rect::new(0, 0, 11, 2),
                &mut b,
                &mut state,
            );
            assert_eq!(b.to_string(), "│  ├─ a1   \n│  └─ a2   ");
        }

        #[test]
        fn long_text_is_clipped_to_the_area() {
            let items = vec![TreeItem::new("0123456789")];
            let mut b = Buffer::empty(Rect::new(0, 0, 8, 1));
            render_at(
                Tree::new(items),
                Rect::new(0, 0, 8, 1),
                &mut b,
                &mut TreeState::default(),
            );
            assert_eq!(b.to_string(), "└─ 01234");
        }

        #[test]
        fn styled_items_layer_over_the_base_style() {
            let items = vec![TreeItem::new("x").style(Style::new().fg(Color::Red))];
            let mut b = Buffer::empty(Rect::new(0, 0, 6, 1));
            render_at(
                Tree::new(items).style(Style::new().bg(Color::Blue)),
                Rect::new(0, 0, 6, 1),
                &mut b,
                &mut TreeState::default(),
            );
            let cell = b.get(3, 0).unwrap();
            assert_eq!(cell.style.fg, Some(Color::Red));
            assert_eq!(cell.style.bg, Some(Color::Blue));
        }

        #[test]
        fn empty_tree_renders_nothing() {
            let mut b = Buffer::empty(Rect::new(0, 0, 4, 2));
            render_at(
                Tree::new(Vec::<TreeItem>::new()),
                Rect::new(0, 0, 4, 2),
                &mut b,
                &mut TreeState::default(),
            );
            assert!(b.to_string().chars().all(|c| c == ' ' || c == '\n'));
        }

        #[test]
        fn block_wraps_and_nodes_fill_inner() {
            let mut b = Buffer::empty(Rect::new(0, 0, 8, 4));
            render_at(
                Tree::new(sample()).block(Block::bordered()),
                Rect::new(0, 0, 8, 4),
                &mut b,
                &mut TreeState::default(),
            );
            assert_eq!(b.to_string(), "┌──────┐\n│├─ a  │\n││  ├─ │\n└──────┘");
        }

        #[test]
        fn theme_maps_the_highlight_slot() {
            let theme = Theme::dark();
            let tree = Tree::new(sample()).theme(&theme);
            assert_eq!(tree.highlight_style, theme.highlight);
        }

        #[test]
        fn first_line_only_for_multiline_items() {
            let mut b = Buffer::empty(Rect::new(0, 0, 10, 2));
            let items = vec![TreeItem::new("one\ntwo"), TreeItem::new("b")];
            render_at(
                Tree::new(items),
                Rect::new(0, 0, 10, 2),
                &mut b,
                &mut TreeState::default(),
            );
            assert_eq!(b.to_string(), "├─ one    \n└─ b      ");
        }

        #[test]
        fn tree_renders_through_the_render_widget_contract() {
            let mut b = Buffer::empty(Rect::new(0, 0, 8, 2));
            let mut state = TreeState::default();
            RenderWidget::render(
                Tree::new(sample()),
                Rect::new(0, 0, 8, 2),
                &mut b,
                &mut state,
            );
            assert_eq!(b.to_string(), "├─ a    \n│  ├─ a1");
        }
    }
}
