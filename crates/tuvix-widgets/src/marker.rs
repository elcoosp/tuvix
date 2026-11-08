//! The marker glyphs shared by [`Chart`](crate::Chart) and
//! [`Canvas`](crate::Canvas).
//!
//! One enum keeps the prelude free of ambiguous `Marker` imports; the
//! renderers interpret it in their own space (character cells for the
//! chart, dot grids for the canvas).

/// The glyph painted for a plotted point.
///
/// # Examples
///
/// ```
/// use tuvix_widgets::Marker;
///
/// assert_eq!(Marker::default(), Marker::Dot);
/// assert_eq!(Marker::Block.cell_symbol(), "▒");
/// ```
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Marker {
    /// A half-height dot (`•`).
    #[default]
    Dot,
    /// A shaded full cell (`▒`).
    Block,
    /// Braille sub-cells (2x4 dots per terminal cell); only meaningful
    /// inside a [`Canvas`](crate::Canvas), where it yields an 8x
    /// resolution grid. In a [`Chart`](crate::Chart) it falls back to the
    /// dot glyph.
    Braille,
}

impl Marker {
    /// The character drawn per terminal cell in cell-space renderers.
    ///
    /// [`Marker::Braille`] degrades to the [`Marker::Dot`] glyph here;
    /// its sub-cell grid only exists in the canvas.
    #[must_use]
    pub fn cell_symbol(self) -> &'static str {
        match self {
            Self::Dot | Self::Braille => "•",
            Self::Block => "▒",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dot_is_the_default() {
        assert_eq!(Marker::default(), Marker::Dot);
    }

    #[test]
    fn cell_symbols_are_pinned() {
        assert_eq!(Marker::Dot.cell_symbol(), "•");
        assert_eq!(Marker::Block.cell_symbol(), "▒");
        assert_eq!(Marker::Braille.cell_symbol(), "•");
    }
}
