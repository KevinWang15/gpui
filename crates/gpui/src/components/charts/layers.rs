//! Composable [`PlotLayer`]s for [`CartesianPlot`](super::CartesianPlot):
//!
//! - [`StackedBarLayer`]: vertical stacked bar columns with top-segment rounding
//! - [`LineLayer`]: continuous `(x, y)` or bucket-aligned `Option<f64>` line/area curves
//!   with solid/dashed strokes, Fritsch–Carlson monotone smoothing, optional surface-casing
//!   halo for contrast over bars, gap splitting, single-bucket reference ticks, and endpoint callouts
//! - [`ReferenceLine`]: horizontal or vertical threshold / target / budget line with optional label
//! - [`ReferenceBand`]: horizontal or vertical shaded target / SLA band

use std::hash::{Hash, Hasher};

use crate::{Hsla, Pixels, SharedString, px};

use super::core::{ChartLegendItem, ChartPalette, LineStyle};

/// Metadata for one series inside a [`StackedBarLayer`] (bottom-to-top stacking order).
#[derive(Clone, Debug)]
pub struct StackedBarSeries {
    /// Series display label.
    pub label: SharedString,
    /// Optional explicit color override (defaults to [`ChartPalette`]).
    pub color: Option<Hsla>,
}

impl StackedBarSeries {
    /// Creates a new [`StackedBarSeries`].
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            color: None,
        }
    }

    /// Overrides the series color.
    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }
}

/// Vertical stacked bar layer rendered across category columns.
///
/// Column X-axis labels live once on [`XAxis::Category`](super::XAxis::Category); `columns[i]`
/// holds one non-negative value per entry in `series` (bottom-to-top).
#[derive(Clone, Debug)]
pub struct StackedBarLayer {
    /// Series descriptors (bottom-to-top stacking order).
    pub series: Vec<StackedBarSeries>,
    /// Per-column stacked values parallel to `series`.
    pub columns: Vec<Vec<f64>>,
    /// Corner radius applied to the topmost non-zero segment of each stack (default `3px`).
    pub bar_radius: Pixels,
    /// Whether to contribute rows & column total to the plot hover tooltip (default `true`).
    pub include_in_tooltip: bool,
}

impl StackedBarLayer {
    /// Creates a new [`StackedBarLayer`].
    pub fn new(
        series: impl IntoIterator<Item = StackedBarSeries>,
        columns: impl IntoIterator<Item = Vec<f64>>,
    ) -> Self {
        Self {
            series: series.into_iter().collect(),
            columns: columns.into_iter().collect(),
            bar_radius: px(3.),
            include_in_tooltip: true,
        }
    }

    /// Sets the top corner radius applied to the topmost non-zero segment of each stack.
    pub fn bar_radius(mut self, radius: Pixels) -> Self {
        self.bar_radius = radius;
        self
    }

    /// Toggles whether this layer contributes rows to the hover tooltip (default `true`).
    pub fn include_in_tooltip(mut self, include: bool) -> Self {
        self.include_in_tooltip = include;
        self
    }

    /// Sum of all finite non-negative segment values in column `col_idx`.
    pub fn column_total(&self, col_idx: usize) -> f64 {
        self.columns
            .get(col_idx)
            .map(|vals| {
                vals.iter()
                    .copied()
                    .filter(|v| v.is_finite())
                    .map(|v| v.max(0.0))
                    .sum()
            })
            .unwrap_or(0.0)
    }

    /// Builds matching dot-swatch [`ChartLegendItem`]s for use with [`ChartCard::legend`](super::ChartCard::legend).
    pub fn legend_items(
        series: &[StackedBarSeries],
        palette: &ChartPalette,
    ) -> Vec<ChartLegendItem> {
        series
            .iter()
            .enumerate()
            .map(|(i, s)| {
                ChartLegendItem::dot(s.label.clone(), s.color.unwrap_or_else(|| palette.color(i)))
            })
            .collect()
    }
}

/// Underlying coordinate representation for a [`LineLayer`].
#[derive(Clone, Debug)]
pub enum LineData {
    /// Ordered continuous `(x, y)` coordinates along a shared numeric X domain.
    Points(Vec<(f64, f64)>),
    /// Bucket-aligned values (`0..N`) mapped onto category column centers or integer X positions.
    /// `None` (or non-finite) entries split the curve cleanly without bridging missing buckets.
    Buckets(Vec<Option<f64>>),
}

/// Composable line / area / comparison curve layer.
///
/// Works seamlessly on both [`XAxis::Continuous`](super::XAxis::Continuous) and
/// [`XAxis::Category`](super::XAxis::Category) plots, so the exact same layer can render
/// standalone multi-series line charts, shaded area curves, or dashed previous-period
/// comparison overlays on top of a [`StackedBarLayer`].
#[derive(Clone, Debug)]
pub struct LineLayer {
    /// Legend & tooltip display label.
    pub label: SharedString,
    /// Continuous `(x, y)` points or bucket-aligned `Option<f64>` samples.
    pub data: LineData,
    /// Solid or dashed stroke pattern (default [`LineStyle::Solid`]).
    pub style: LineStyle,
    /// Optional explicit stroke color (defaults to next [`ChartPalette`] color for solid,
    /// `palette.baseline` for dashed).
    pub color: Option<Hsla>,
    /// Stroke thickness in pixels (defaults to `2.25` for solid, `1.75` for dashed).
    pub stroke_width: Option<f32>,
    /// Whether to smooth with Fritsch–Carlson monotone cubic Bezier interpolation (default `false`).
    pub smooth: bool,
    /// Whether to paint a soft vertical gradient fill beneath this curve (default `false`).
    pub fill_area: bool,
    /// Whether to paint a subtle surface-colored casing stroke right beneath this curve so it
    /// stays crisp when overlaid directly on top of bars or dense layers (default `false`).
    pub halo: bool,
    /// Optional inline endpoint callout prefix (e.g. `Some("Today")` -> `"Today: $36.21"`).
    pub endpoint_callout: Option<SharedString>,
    /// Whether to paint a subtle divider directly above this layer's row in the hover tooltip.
    pub tooltip_separator: bool,
    /// Whether this layer contributes a row and hover marker on pointer scrub (default `true`).
    pub include_in_tooltip: bool,
}

impl LineLayer {
    /// Creates a solid [`LineLayer`] from ordered continuous `(x, y)` coordinates.
    pub fn new(label: impl Into<SharedString>, points: impl Into<Vec<(f64, f64)>>) -> Self {
        Self {
            label: label.into(),
            data: LineData::Points(points.into()),
            style: LineStyle::Solid,
            color: None,
            stroke_width: None,
            smooth: false,
            fill_area: false,
            halo: false,
            endpoint_callout: None,
            tooltip_separator: false,
            include_in_tooltip: true,
        }
    }

    /// Creates a [`LineLayer`] from bucket-aligned `Option<f64>` values (`None` creates gaps).
    pub fn from_buckets(
        label: impl Into<SharedString>,
        values: impl IntoIterator<Item = Option<f64>>,
    ) -> Self {
        Self {
            label: label.into(),
            data: LineData::Buckets(values.into_iter().collect()),
            style: LineStyle::Solid,
            color: None,
            stroke_width: None,
            smooth: false,
            fill_area: false,
            halo: false,
            endpoint_callout: None,
            tooltip_separator: false,
            include_in_tooltip: true,
        }
    }

    /// Configures solid or dashed stroke style.
    pub fn line_style(mut self, style: LineStyle) -> Self {
        self.style = style;
        self
    }

    /// Shorthand for `.line_style(LineStyle::Dashed)`.
    pub fn dashed(mut self) -> Self {
        self.style = LineStyle::Dashed;
        self
    }

    /// Overrides the stroke color.
    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    /// Overrides stroke thickness in pixels.
    pub fn stroke_width(mut self, width: f32) -> Self {
        self.stroke_width = Some(width);
        self
    }

    /// Enables or disables Fritsch–Carlson monotone cubic Bezier smoothing (zero overshoot).
    pub fn smooth(mut self, smooth: bool) -> Self {
        self.smooth = smooth;
        self
    }

    /// Enables a subtle vertical gradient fill beneath the curve.
    pub fn fill_area(mut self, enabled: bool) -> Self {
        self.fill_area = enabled;
        self
    }

    /// Enables a subtle surface-colored casing stroke beneath the line for legibility over bars.
    pub fn halo(mut self, enabled: bool) -> Self {
        self.halo = enabled;
        self
    }

    /// Renders an endpoint marker dot and inline callout pill (`"<prefix>: <value>"`).
    pub fn endpoint_callout(mut self, prefix: impl Into<SharedString>) -> Self {
        self.endpoint_callout = Some(prefix.into());
        self
    }

    /// Places a subtle horizontal divider directly above this layer's row in the hover tooltip.
    pub fn tooltip_separator(mut self, enabled: bool) -> Self {
        self.tooltip_separator = enabled;
        self
    }

    /// Toggles whether this layer appears in hover tooltips and markers (default `true`).
    pub fn include_in_tooltip(mut self, include: bool) -> Self {
        self.include_in_tooltip = include;
        self
    }

    /// Builds a matching [`ChartLegendItem`] (solid or dashed swatch) for this layer.
    pub fn legend_item(&self, fallback_color: Hsla) -> ChartLegendItem {
        let color = self.color.unwrap_or(fallback_color);
        match self.style {
            LineStyle::Solid => ChartLegendItem::solid_line(self.label.clone(), color),
            LineStyle::Dashed => ChartLegendItem::dashed_line(self.label.clone(), color),
        }
    }

    /// Returns the finite value at bucket index `idx` (exact lookup for `Buckets`, interpolated
    /// lookup at `idx as f64` for `Points`).
    pub fn value_at_bucket(&self, idx: usize) -> Option<f64> {
        match &self.data {
            LineData::Buckets(vals) => vals.get(idx).copied().flatten().filter(|v| v.is_finite()),
            LineData::Points(_) => self.sample_at(idx as f64),
        }
    }

    /// Linearly interpolates the layer's `y` value at `target_x` in `O(log N)` time for sorted
    /// continuous `Points`, or interpolates adjacent non-`None` buckets for `Buckets`.
    pub fn sample_at(&self, target_x: f64) -> Option<f64> {
        match &self.data {
            LineData::Points(points) => sample_points(points, target_x),
            LineData::Buckets(vals) => {
                if vals.is_empty() || target_x < -1e-6 || target_x > (vals.len() - 1) as f64 + 1e-6
                {
                    return None;
                }
                let lo = (target_x.floor().max(0.0) as usize).min(vals.len() - 1);
                let hi = (target_x.ceil().max(0.0) as usize).min(vals.len() - 1);
                if lo == hi {
                    return vals[lo].filter(|v| v.is_finite());
                }
                let y0 = vals[lo].filter(|v| v.is_finite())?;
                let y1 = vals[hi].filter(|v| v.is_finite())?;
                let t = (target_x - lo as f64).clamp(0.0, 1.0);
                Some(y0 + (y1 - y0) * t)
            }
        }
    }

    /// Returns the last finite `(x, y)` point in this layer, if any.
    pub(crate) fn last_point(&self) -> Option<(f64, f64)> {
        match &self.data {
            LineData::Points(pts) => pts
                .iter()
                .rev()
                .copied()
                .find(|&(x, y)| x.is_finite() && y.is_finite()),
            LineData::Buckets(vals) => vals
                .iter()
                .enumerate()
                .rev()
                .find_map(|(i, v)| v.filter(|y| y.is_finite()).map(|y| (i as f64, y))),
        }
    }

    /// Splits the layer's data into contiguous finite `(x, y)` runs (breaking cleanly at `None`
    /// or non-finite entries).
    pub(crate) fn contiguous_runs(&self, category_limit: Option<usize>) -> Vec<Vec<(f64, f64)>> {
        let mut runs: Vec<Vec<(f64, f64)>> = Vec::new();
        let mut current: Vec<(f64, f64)> = Vec::new();
        match &self.data {
            LineData::Points(pts) => {
                for &(x, y) in pts {
                    if x.is_finite() && y.is_finite() {
                        current.push((x, y));
                    } else if !current.is_empty() {
                        runs.push(std::mem::take(&mut current));
                    }
                }
            }
            LineData::Buckets(vals) => {
                let len = category_limit.map_or(vals.len(), |n| vals.len().min(n));
                for (i, slot) in vals.iter().take(len).enumerate() {
                    if let Some(y) = slot.filter(|y| y.is_finite()) {
                        current.push((i as f64, y));
                    } else if !current.is_empty() {
                        runs.push(std::mem::take(&mut current));
                    }
                }
            }
        }
        if !current.is_empty() {
            runs.push(current);
        }
        runs
    }

    pub(crate) fn hash_geometry(&self, hasher: &mut impl Hasher, category_limit: Option<usize>) {
        self.style.hash(hasher);
        self.smooth.hash(hasher);
        self.fill_area.hash(hasher);
        self.halo.hash(hasher);
        self.stroke_width.map(f32::to_bits).hash(hasher);
        match &self.data {
            LineData::Points(pts) => {
                0u8.hash(hasher);
                pts.len().hash(hasher);
                for &(x, y) in pts {
                    x.to_bits().hash(hasher);
                    y.to_bits().hash(hasher);
                }
            }
            LineData::Buckets(vals) => {
                1u8.hash(hasher);
                let len = category_limit.map_or(vals.len(), |n| vals.len().min(n));
                len.hash(hasher);
                for slot in &vals[..len] {
                    match slot.filter(|v| v.is_finite()) {
                        Some(v) => {
                            1u8.hash(hasher);
                            v.to_bits().hash(hasher);
                        }
                        None => 0u8.hash(hasher),
                    }
                }
            }
        }
    }
}

fn sample_points(points: &[(f64, f64)], target_x: f64) -> Option<f64> {
    if points.is_empty() {
        return None;
    }
    let first = points[0];
    let last = *points.last()?;
    if target_x < first.0 - 1e-6 || target_x > last.0 + 1e-6 {
        return None;
    }
    if points.len() == 1 || target_x <= first.0 {
        return first.1.is_finite().then_some(first.1);
    }
    if target_x >= last.0 {
        return last.1.is_finite().then_some(last.1);
    }
    let idx = points.partition_point(|&(x, _)| x < target_x);
    if idx == 0 {
        return first.1.is_finite().then_some(first.1);
    }
    let (x0, y0) = points[idx - 1];
    let (x1, y1) = points[idx];
    if !y0.is_finite() || !y1.is_finite() {
        return None;
    }
    let span = x1 - x0;
    if span.abs() < 1e-9 {
        return Some(y1);
    }
    let t = ((target_x - x0) / span).clamp(0.0, 1.0);
    Some(y0 + (y1 - y0) * t)
}

/// Orientation for [`ReferenceLine`] and [`ReferenceBand`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReferenceAxis {
    /// Constant Y value(s) spanning the plot horizontally.
    Horizontal,
    /// Constant X value(s) spanning the plot vertically.
    Vertical,
}

/// Horizontal or vertical reference / threshold line (e.g. daily budget cap or SLA target).
#[derive(Clone, Debug)]
pub struct ReferenceLine {
    /// Whether this line sits at a constant Y (`Horizontal`) or constant X (`Vertical`).
    pub axis: ReferenceAxis,
    /// Position along the reference axis in data coordinates.
    pub value: f64,
    /// Optional inline caption rendered beside the line (e.g. `"Budget $50"`).
    pub label: Option<SharedString>,
    /// Stroke pattern (defaults to [`LineStyle::Dashed`]).
    pub style: LineStyle,
    /// Optional explicit line & label color (defaults to `palette.baseline`).
    pub color: Option<Hsla>,
    /// Stroke thickness in pixels (default `1.25`).
    pub stroke_width: f32,
    /// Whether a horizontal reference line expands the auto-scaled Y domain (default `true`).
    pub include_in_y_domain: bool,
}

impl ReferenceLine {
    /// Creates a dashed horizontal reference line at Y = `value`.
    pub fn horizontal(value: f64) -> Self {
        Self {
            axis: ReferenceAxis::Horizontal,
            value,
            label: None,
            style: LineStyle::Dashed,
            color: None,
            stroke_width: 1.25,
            include_in_y_domain: true,
        }
    }

    /// Creates a dashed vertical reference line at X = `value`.
    pub fn vertical(value: f64) -> Self {
        Self {
            axis: ReferenceAxis::Vertical,
            value,
            label: None,
            style: LineStyle::Dashed,
            color: None,
            stroke_width: 1.25,
            include_in_y_domain: false,
        }
    }

    /// Sets an inline caption displayed beside the line.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Configures solid or dashed stroke style.
    pub fn line_style(mut self, style: LineStyle) -> Self {
        self.style = style;
        self
    }

    /// Shorthand for `.line_style(LineStyle::Solid)`.
    pub fn solid(mut self) -> Self {
        self.style = LineStyle::Solid;
        self
    }

    /// Overrides the line & caption color.
    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    /// Sets stroke thickness in pixels.
    pub fn stroke_width(mut self, width: f32) -> Self {
        self.stroke_width = width.clamp(0.5, 6.0);
        self
    }

    /// Controls whether a horizontal reference line is included in auto Y-domain scaling.
    pub fn include_in_y_domain(mut self, include: bool) -> Self {
        self.include_in_y_domain = include;
        self
    }
}

/// Shaded horizontal or vertical target band (e.g. healthy latency corridor or weekend span).
#[derive(Clone, Debug)]
pub struct ReferenceBand {
    /// Whether this band spans `[start, end]` along Y (`Horizontal`) or X (`Vertical`).
    pub axis: ReferenceAxis,
    /// Lower bound in data coordinates.
    pub start: f64,
    /// Upper bound in data coordinates.
    pub end: f64,
    /// Optional subtle caption rendered inside the top-right corner of the band.
    pub label: Option<SharedString>,
    /// Optional fill color (defaults to `theme.primary.opacity(0.08)`).
    pub color: Option<Hsla>,
    /// Whether a horizontal band expands the auto-scaled Y domain (default `false`).
    pub include_in_y_domain: bool,
}

impl ReferenceBand {
    /// Creates a horizontal shaded band spanning `y0..=y1`.
    pub fn horizontal(y0: f64, y1: f64) -> Self {
        Self {
            axis: ReferenceAxis::Horizontal,
            start: y0.min(y1),
            end: y0.max(y1),
            label: None,
            color: None,
            include_in_y_domain: false,
        }
    }

    /// Creates a vertical shaded band spanning `x0..=x1`.
    pub fn vertical(x0: f64, x1: f64) -> Self {
        Self {
            axis: ReferenceAxis::Vertical,
            start: x0.min(x1),
            end: x0.max(x1),
            label: None,
            color: None,
            include_in_y_domain: false,
        }
    }

    /// Sets an optional subtle label inside the band.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Overrides the band fill color.
    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    /// Controls whether a horizontal band expands the auto-scaled Y domain.
    pub fn include_in_y_domain(mut self, include: bool) -> Self {
        self.include_in_y_domain = include;
        self
    }
}

/// One composable visualization layer stacked inside a [`CartesianPlot`](super::CartesianPlot).
///
/// Layers paint strictly in declaration order so callers control visual hierarchy explicitly
/// (e.g. a [`ReferenceBand`] behind a [`StackedBarLayer`], with a haloed dashed [`LineLayer`]
/// and a [`ReferenceLine`] cleanly on top).
#[derive(Clone, Debug)]
pub enum PlotLayer {
    /// Vertical stacked bar columns.
    StackedBars(StackedBarLayer),
    /// Continuous or bucket-aligned line / area / comparison curve.
    Line(LineLayer),
    /// Horizontal or vertical threshold / target line.
    ReferenceLine(ReferenceLine),
    /// Horizontal or vertical shaded band.
    ReferenceBand(ReferenceBand),
}

impl From<StackedBarLayer> for PlotLayer {
    fn from(layer: StackedBarLayer) -> Self {
        Self::StackedBars(layer)
    }
}

impl From<LineLayer> for PlotLayer {
    fn from(layer: LineLayer) -> Self {
        Self::Line(layer)
    }
}

impl From<ReferenceLine> for PlotLayer {
    fn from(layer: ReferenceLine) -> Self {
        Self::ReferenceLine(layer)
    }
}

impl From<ReferenceBand> for PlotLayer {
    fn from(layer: ReferenceBand) -> Self {
        Self::ReferenceBand(layer)
    }
}

impl PlotLayer {
    /// Contributes this layer's finite `(min_y, max_y)` extent to Y-domain resolution.
    pub(crate) fn y_extent(&self, category_limit: Option<usize>) -> Option<(f64, f64)> {
        match self {
            Self::StackedBars(bars) => {
                let len = category_limit.map_or(bars.columns.len(), |n| bars.columns.len().min(n));
                if len == 0 {
                    return None;
                }
                let mut max_y = 0.0_f64;
                for i in 0..len {
                    max_y = max_y.max(bars.column_total(i));
                }
                Some((0.0, max_y))
            }
            Self::Line(line) => {
                let mut min_y = f64::INFINITY;
                let mut max_y = f64::NEG_INFINITY;
                match &line.data {
                    LineData::Points(pts) => {
                        for &(_, y) in pts {
                            if y.is_finite() {
                                min_y = min_y.min(y);
                                max_y = max_y.max(y);
                            }
                        }
                    }
                    LineData::Buckets(vals) => {
                        let len = category_limit.map_or(vals.len(), |n| vals.len().min(n));
                        for slot in &vals[..len] {
                            if let Some(y) = slot.filter(|v| v.is_finite()) {
                                min_y = min_y.min(y);
                                max_y = max_y.max(y);
                            }
                        }
                    }
                }
                (min_y.is_finite() && max_y.is_finite()).then_some((min_y, max_y))
            }
            Self::ReferenceLine(r) => {
                (r.axis == ReferenceAxis::Horizontal && r.include_in_y_domain && r.value.is_finite())
                    .then_some((r.value, r.value))
            }
            Self::ReferenceBand(b) => (b.axis == ReferenceAxis::Horizontal
                && b.include_in_y_domain
                && b.start.is_finite()
                && b.end.is_finite())
            .then_some((b.start, b.end)),
        }
    }

    /// Contributes this layer's finite `(min_x, max_x)` extent for auto continuous X domains.
    pub(crate) fn x_extent(&self) -> Option<(f64, f64)> {
        match self {
            Self::Line(line) => match &line.data {
                LineData::Points(pts) => {
                    let mut min_x = f64::INFINITY;
                    let mut max_x = f64::NEG_INFINITY;
                    for &(x, _) in pts {
                        if x.is_finite() {
                            min_x = min_x.min(x);
                            max_x = max_x.max(x);
                        }
                    }
                    (min_x.is_finite() && max_x.is_finite()).then_some((min_x, max_x))
                }
                LineData::Buckets(vals) => {
                    (!vals.is_empty()).then_some((0.0, (vals.len() - 1) as f64))
                }
            },
            Self::StackedBars(bars) => {
                (!bars.columns.is_empty()).then_some((0.0, (bars.columns.len() - 1) as f64))
            }
            Self::ReferenceLine(r) => {
                (r.axis == ReferenceAxis::Vertical && r.value.is_finite())
                    .then_some((r.value, r.value))
            }
            Self::ReferenceBand(b) => (b.axis == ReferenceAxis::Vertical
                && b.start.is_finite()
                && b.end.is_finite())
            .then_some((b.start, b.end)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_layer_buckets_split_gaps_and_sample_cleanly() {
        let layer = LineLayer::from_buckets(
            "Previous 7 days",
            [Some(10.0), Some(30.0), None, Some(25.0), Some(f64::NAN)],
        )
        .dashed()
        .halo(true);

        assert_eq!(layer.style, LineStyle::Dashed);
        assert!(layer.halo);
        assert_eq!(layer.value_at_bucket(0), Some(10.0));
        assert_eq!(layer.value_at_bucket(2), None);
        assert_eq!(layer.value_at_bucket(4), None);
        assert_eq!(layer.sample_at(0.5), Some(20.0));
        assert_eq!(layer.sample_at(1.5), None);

        let runs = layer.contiguous_runs(Some(5));
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0], vec![(0.0, 10.0), (1.0, 30.0)]);
        assert_eq!(runs[1], vec![(3.0, 25.0)]);
    }

    #[test]
    fn plot_layer_y_extent_unions_bars_lines_and_reference_lines() {
        let bars = PlotLayer::from(StackedBarLayer::new(
            [StackedBarSeries::new("a"), StackedBarSeries::new("b")],
            vec![vec![10.0, 5.0], vec![20.0, 10.0]],
        ));
        let prev = PlotLayer::from(
            LineLayer::from_buckets("Previous", [Some(12.0), Some(55.0)]).dashed(),
        );
        let budget = PlotLayer::from(ReferenceLine::horizontal(80.0).label("Budget"));
        let band = PlotLayer::from(ReferenceBand::horizontal(0.0, 100.0));

        assert_eq!(bars.y_extent(Some(2)), Some((0.0, 30.0)));
        assert_eq!(prev.y_extent(Some(2)), Some((12.0, 55.0)));
        assert_eq!(budget.y_extent(Some(2)), Some((80.0, 80.0)));
        // ReferenceBand opts out of Y-domain distortion by default.
        assert_eq!(band.y_extent(Some(2)), None);
    }
}
