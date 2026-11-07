//! Batteries-included widgets for the tuvix terminal UI framework.
//!
//! M1 added the text pipeline: [`Paragraph`] gains word wrapping
//! ([`Wrap`], [`wrap_line`]) and scroll state ([`ScrollState`] fed through
//! [`Paragraph::scroll`]). M3 adds the widget family (Gauge, Sparkline,
//! List, Table, Scrollbar, Chart, Canvas) unified behind the
//! [`RenderWidget`](tuvix_core::widget::RenderWidget) contract — implement
//! it once, get `Widget`/`StatefulWidget` through blanket impls. Widgets
//! paint into a [`tuvix_core::buffer::Buffer`] and are pure functions of
//! `(self, area, buffer)` — trivially testable with plain buffers.

pub mod bar_chart;
pub mod block;
pub mod canvas;
pub mod chart;
pub mod checkbox;
pub mod clear;
pub mod frame_inspector;
pub mod gauge;
pub mod input;
pub mod line_gauge;
pub mod list;
pub mod marker;
pub mod paragraph;
pub mod radio;
pub mod scrollable;
pub mod scrollbar;
pub mod select;
pub mod sparkline;
pub mod switch;
pub mod table;
pub mod tabs;
pub mod text_area;
pub mod tree;

pub use bar_chart::{Bar, BarChart, BarGroup};
pub use block::{Block, BorderType, Borders};
pub use canvas::Canvas;
// Canvas shapes stay in the `canvas` module: its `Line` would otherwise
// shadow `tuvix_core::text::Line` in the facade prelude.
pub use chart::{Axis, Chart, Dataset, GraphType};
pub use checkbox::Checkbox;
pub use clear::Clear;
pub use frame_inspector::FrameInspector;
pub use gauge::Gauge;
pub use input::{Input, InputState};
pub use line_gauge::LineGauge;
pub use list::{List, ListItem, ListState};
pub use marker::Marker;
pub use paragraph::{Paragraph, ScrollState, Wrap, wrap_line};
pub use radio::RadioGroup;
pub use scrollable::{Scrollable, ViewportState};
pub use scrollbar::{Scrollbar, ScrollbarOrientation, ScrollbarState};
pub use select::{Select, SelectState};
pub use sparkline::Sparkline;
pub use switch::Switch;
pub use table::{Table, TableCell, TableRow, TableState};
pub use tabs::Tabs;
pub use text_area::{TextArea, TextAreaState};
pub use tree::{Tree, TreeItem, TreeState};
