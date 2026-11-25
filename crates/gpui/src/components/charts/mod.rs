//! Composable, GPU-native data visualization components for `gpui::components`.
//!
//! Architecture:
//! - [`core`]: shared scales (`LinearScale`, `BandScale`), nice ticks, Fritsch–Carlson monotone
//!   splines, palettes, swatches, tooltips, and [`ChartCard`] framing.
//! - [`plot`]: unified [`CartesianPlot`] engine owning axes ([`XAxis`], [`YAxis`]), hover &
//!   drag-brush interaction, geometry caching, and auto-aggregated / customizable tooltips.
//! - [`layers`]: orthogonal, stackable [`PlotLayer`]s ([`StackedBarLayer`], [`LineLayer`],
//!   [`ReferenceLine`], [`ReferenceBand`]).
//! - [`sparkline`]: compact axis-less [`Sparkline`] and [`GroupedSparkline`].
//! - [`breakdown`]: ranked proportional horizontal [`BreakdownList`].

pub mod breakdown;
pub mod core;
pub mod layers;
pub mod plot;
pub mod sparkline;

pub use breakdown::{BreakdownItem, BreakdownList, breakdown_list};
pub use core::{
    BandScale, ChartCard, ChartLegendItem, ChartPalette, ChartTooltipRow, ChartValueFormat,
    CubicSegment, LegendSwatch, LineStyle, LinearScale, chart_card, compute_tight_y_range,
    format_compact, monotone, nice_ticks, nice_ticks_range, paint_chart_text, render_chart_legend,
    sample_monotone,
};
pub use layers::{
    LineData, LineLayer, PlotLayer, ReferenceAxis, ReferenceBand, ReferenceLine, StackedBarLayer,
    StackedBarSeries,
};
pub use plot::{CartesianPlot, PlotHover, PlotTooltipData, XAxis, YAxis, cartesian_plot};
pub use sparkline::{
    GroupedSparkline, GroupedSparklineSeries, Sparkline, SparklineHover, SparklineKind,
    grouped_sparkline, sparkline,
};
