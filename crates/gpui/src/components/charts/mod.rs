//! GPU-native data visualization components for `gpui::components`.
//!
//! Includes shared scales & palettes ([`core`]), [`StackedBarChart`], [`LineChart`],
//! [`BreakdownList`], and the framing [`ChartCard`] with optional `"Show data table"` disclosure.

pub mod breakdown;
pub mod core;
pub mod line;
pub mod stacked_bar;

pub use breakdown::{BreakdownItem, BreakdownList, breakdown_list};
pub use core::{
    BandScale, ChartCard, ChartLegendItem, ChartPalette, ChartValueFormat, LegendSwatch,
    LinearScale, chart_card, format_compact, nice_ticks, paint_chart_text, render_chart_legend,
};
pub use line::{LineChart, LineSeries, LineStyle, line_chart};
pub use stacked_bar::{StackedBarChart, StackedBarPoint, StackedBarSeries, stacked_bar_chart};
