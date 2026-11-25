//! Composable GPU-native [`CartesianPlot`] combining shared axes ([`XAxis`], [`YAxis`]),
//! stackable [`PlotLayer`]s, one unified hover/drag-brush interaction controller, cached
//! tessellated geometry, and auto-aggregated / extensible tooltips.

use std::cell::RefCell;
use std::hash::{Hash, Hasher};
use std::ops::RangeInclusive;
use std::rc::Rc;

use collections::FxHasher;

use crate::{
    AnyElement, App, BorderStyle, Bounds, Corners, CursorStyle, Edges, ElementId, FontWeight, Hsla,
    IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Path,
    PathBuilder, Pixels, RenderOnce, SharedString, StyleRefinement, Styled, TextAlign, Window,
    canvas, div, fill, linear_color_stop, linear_gradient, point, prelude::*, px, quad, size,
    transparent_black,
};

use super::core::{
    BandScale, ChartMargins, ChartPalette, ChartTooltipRow, ChartValueFormat, LegendSwatch,
    LineStyle, LinearScale, PlotRect, compute_tight_y_range, decimate_monotonic_points, nice_ticks,
    nice_ticks_range, paint_brush_band, paint_chart_marker, paint_chart_text, paint_y_gridlines,
    push_curve_points, render_brush_pill, render_chart_tooltip,
};
use super::layers::{LineData, LineLayer, PlotLayer, ReferenceAxis, StackedBarLayer};
use crate::components::theme::{ActiveTheme, Theme};

const DEFAULT_PLOT_MARGINS: ChartMargins = ChartMargins {
    left: 54.0,
    right: 14.0,
    top: 12.0,
    bottom: 26.0,
};

const DEFAULT_BAND_PADDING_INNER: f32 = 0.28;

/// Horizontal axis configuration for a [`CartesianPlot`].
#[derive(Clone)]
pub enum XAxis {
    /// Discrete category / bucket columns (e.g. calendar days `"Oct 27".."Today"` or `"14:00"` slots).
    ///
    /// Columns are positioned via [`BandScale`]; [`StackedBarLayer`] renders bars across each band
    /// while [`LineLayer::from_buckets`] aligns cleanly to each band center.
    Category {
        /// Short labels drawn along the X-axis (auto-thinned to prevent overlap).
        labels: Vec<SharedString>,
        /// Optional richer labels shown in hover tooltips (defaults to `labels`).
        tooltip_labels: Option<Vec<SharedString>>,
        /// Inner spacing fraction between adjacent bar bands (`0.0..0.8`, default `0.28`).
        padding_inner: f32,
    },
    /// Continuous numeric X axis (e.g. `0.0..=24.0` hours or timestamps).
    Continuous {
        /// Optional explicit `(min, max)` domain; inferred from layers when `None`.
        domain: Option<(f64, f64)>,
        /// Explicit `(x, label)` ticks drawn along the bottom axis.
        ticks: Vec<(f64, SharedString)>,
        /// Optional custom formatter for hovered/brushed X values.
        formatter: Option<Rc<dyn Fn(f64) -> String>>,
    },
}

impl Default for XAxis {
    fn default() -> Self {
        Self::continuous()
    }
}

impl XAxis {
    /// Creates a discrete categorical X axis from ordered column labels.
    pub fn category(labels: impl IntoIterator<Item = impl Into<SharedString>>) -> Self {
        Self::Category {
            labels: labels.into_iter().map(Into::into).collect(),
            tooltip_labels: None,
            padding_inner: DEFAULT_BAND_PADDING_INNER,
        }
    }

    /// Creates a continuous numeric X axis (domain inferred from layers unless set via [`Self::domain`]).
    pub fn continuous() -> Self {
        Self::Continuous {
            domain: None,
            ticks: Vec::new(),
            formatter: None,
        }
    }

    /// Sets optional richer per-column labels displayed in hover tooltips (category axes only).
    pub fn tooltip_labels(
        mut self,
        labels: impl IntoIterator<Item = impl Into<SharedString>>,
    ) -> Self {
        if let Self::Category { tooltip_labels, .. } = &mut self {
            *tooltip_labels = Some(labels.into_iter().map(Into::into).collect());
        }
        self
    }

    /// Sets inner band padding fraction (`0.0..0.8`, category axes only).
    pub fn padding_inner(mut self, padding: f32) -> Self {
        if let Self::Category { padding_inner, .. } = &mut self {
            *padding_inner = padding.clamp(0.0, 0.8);
        }
        self
    }

    /// Sets an explicit continuous X domain `[min, max]` (continuous axes only).
    pub fn domain(mut self, min: f64, max: f64) -> Self {
        if let Self::Continuous { domain, .. } = &mut self {
            *domain = Some((min, max));
        }
        self
    }

    /// Sets explicit `(x, label)` tick marks (continuous axes only).
    pub fn ticks(mut self, ticks: impl IntoIterator<Item = (f64, SharedString)>) -> Self {
        if let Self::Continuous { ticks: dst, .. } = &mut self {
            *dst = ticks.into_iter().collect();
        }
        self
    }

    /// Sets a custom formatter for hovered/brushed continuous X values (continuous axes only).
    pub fn formatter(mut self, formatter: impl Fn(f64) -> String + 'static) -> Self {
        if let Self::Continuous {
            formatter: dst, ..
        } = &mut self
        {
            *dst = Some(Rc::new(formatter));
        }
        self
    }
}

/// Vertical axis configuration for a [`CartesianPlot`].
#[derive(Clone)]
pub struct YAxis {
    /// Explicit `(min, max)` domain override; auto-scaled from all layers when `None`.
    pub domain: Option<(f64, f64)>,
    /// Target number of horizontal ticks (default `4`).
    pub tick_count: usize,
    /// Formatter for Y-axis tick labels, callouts, and default tooltips.
    pub format: ChartValueFormat,
    /// Whether to paint horizontal hairline gridlines across the plot (default `true`).
    pub show_grid: bool,
    /// Whether auto-scaling fits tightly around observed data instead of anchoring at `0.0`.
    pub tight: bool,
    /// Optional clamp bounds applied when `tight` is `true` (e.g. `(0.0, 1.0)` for ratios).
    pub clamp: Option<(f64, f64)>,
}

impl Default for YAxis {
    fn default() -> Self {
        Self::new(ChartValueFormat::Currency)
    }
}

impl YAxis {
    /// Creates a zero-anchored Y axis using `format`.
    pub fn new(format: ChartValueFormat) -> Self {
        Self {
            domain: None,
            tick_count: 4,
            format,
            show_grid: true,
            tight: false,
            clamp: None,
        }
    }

    /// Pins the Y axis to an explicit `(min, max)` domain.
    pub fn domain(mut self, min: f64, max: f64) -> Self {
        self.domain = Some((min, max));
        self
    }

    /// Sets the target tick count (default `4`).
    pub fn ticks(mut self, count: usize) -> Self {
        self.tick_count = count.max(2);
        self
    }

    /// Toggles horizontal gridlines (default `true`).
    pub fn show_grid(mut self, show: bool) -> Self {
        self.show_grid = show;
        self
    }

    /// Fits the Y domain closely around observed data instead of anchoring at `0.0`.
    pub fn tight(mut self, tight: bool) -> Self {
        self.tight = tight;
        self
    }

    /// Clamps auto-computed tight Y bounds to `[min, max]` (e.g. `(0.0, 1.0)` for fractions).
    pub fn clamp(mut self, min: f64, max: f64) -> Self {
        self.clamp = Some((min, max));
        self
    }

    /// Resolves `(y_min, y_max, ticks)` across `layers`.
    pub(crate) fn resolve(
        &self,
        layers: &[PlotLayer],
        category_limit: Option<usize>,
    ) -> (f64, f64, Vec<f64>) {
        if let Some((lo, hi)) = self.domain {
            return nice_ticks_range(lo, hi, self.tick_count, false);
        }
        let mut data_min = f64::INFINITY;
        let mut data_max = f64::NEG_INFINITY;
        for layer in layers {
            if let Some((lo, hi)) = layer.y_extent(category_limit) {
                data_min = data_min.min(lo);
                data_max = data_max.max(hi);
            }
        }
        if !data_min.is_finite() || !data_max.is_finite() {
            let (nice_max, ticks) = nice_ticks(0.0, self.tick_count);
            return (0.0, nice_max, ticks);
        }
        if self.tight {
            let (lo, hi) = compute_tight_y_range([data_min, data_max].into_iter(), true, self.clamp);
            let (slo, shi, ticks) = nice_ticks_range(lo, hi, self.tick_count, true);
            if let Some((c_min, c_max)) = self.clamp {
                let cl_lo = slo.max(c_min);
                let cl_hi = shi.min(c_max);
                if cl_hi > cl_lo {
                    let filtered: Vec<f64> = ticks
                        .into_iter()
                        .filter(|&t| t >= cl_lo - 1e-6 && t <= cl_hi + 1e-6)
                        .collect();
                    if filtered.len() >= 2 {
                        return (cl_lo, cl_hi, filtered);
                    }
                    return nice_ticks_range(cl_lo, cl_hi, self.tick_count, false);
                }
            }
            (slo, shi, ticks)
        } else {
            let (nice_max, ticks) = nice_ticks(data_max.max(0.0), self.tick_count);
            (0.0, nice_max, ticks)
        }
    }
}

/// Active hover target inside a [`CartesianPlot`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PlotHover {
    /// Hovered discrete column index on an [`XAxis::Category`] plot.
    Category(usize),
    /// Hovered numeric X coordinate on an [`XAxis::Continuous`] plot.
    Continuous(f64),
}

/// Structured tooltip payload aggregated from every [`PlotLayer`] at the hovered coordinate.
///
/// Passed to [`CartesianPlot::tooltip_rows`] and [`CartesianPlot::tooltip_builder`] so callers
/// can augment or replace tooltips cleanly without duplicating chart geometry.
#[derive(Clone, Debug)]
pub struct PlotTooltipData {
    /// Active hover target.
    pub hover: PlotHover,
    /// Left header text (category tooltip label or formatted X value).
    pub header: SharedString,
    /// Optional right-aligned header summary (e.g. stacked column total).
    pub summary: Option<String>,
    /// Auto-aggregated rows from all participating layers in declaration order.
    pub rows: Vec<ChartTooltipRow>,
}

/// Resolved per-layer colors computed once per render pass.
#[derive(Clone)]
enum ResolvedLayerColors {
    StackedBars(Rc<[Hsla]>),
    Line(Hsla),
    ReferenceLine(Hsla),
    ReferenceBand(Hsla),
}

enum CachedLayerPaths {
    Line {
        areas: Vec<Path>,
        casings: Vec<Path>,
        strokes: Vec<Path>,
    },
    ReferenceLine(Option<Path>),
}

struct CachedPlotGeometry {
    key: u64,
    /// Indexed parallel to `layers` (only populated for path-based layers).
    by_layer: Vec<Option<CachedLayerPaths>>,
}

#[derive(Default)]
struct PlotInteractionState {
    hover: Option<PlotHover>,
    plot_bounds: Option<Bounds<Pixels>>,
    drag_anchor_x: Option<f32>,
    drag_current_x: Option<f32>,
    cached_geometry: Option<CachedPlotGeometry>,
}

impl PlotInteractionState {
    fn is_brushing(&self) -> bool {
        match (self.drag_anchor_x, self.drag_current_x) {
            (Some(a), Some(b)) => (b - a).abs() >= 4.0,
            _ => false,
        }
    }

    fn brushed_category(&self, count: usize, padding_inner: f32) -> Option<RangeInclusive<usize>> {
        if !self.is_brushing() || count == 0 {
            return None;
        }
        let plot = PlotRect::from_bounds(self.plot_bounds?, DEFAULT_PLOT_MARGINS);
        let scale = BandScale::new(plot.left, plot.width(), count, padding_inner);
        let max_x = (plot.right - 0.01).max(plot.left);
        let a = self.drag_anchor_x?.clamp(plot.left, max_x);
        let b = self.drag_current_x?.clamp(plot.left, max_x);
        let i0 = scale.index_at_x(a.min(b)).unwrap_or(0);
        let i1 = scale
            .index_at_x(a.max(b))
            .unwrap_or_else(|| count.saturating_sub(1));
        Some(i0.min(i1)..=i0.max(i1))
    }

    fn brushed_domain(&self, x_min: f64, x_max: f64) -> Option<(f64, f64)> {
        if !self.is_brushing() {
            return None;
        }
        let plot = PlotRect::from_bounds(self.plot_bounds?, DEFAULT_PLOT_MARGINS);
        let scale = LinearScale::new(x_min, x_max, plot.left, plot.right);
        let a = self.drag_anchor_x?.clamp(plot.left, plot.right);
        let b = self.drag_current_x?.clamp(plot.left, plot.right);
        let x0 = scale.invert(a.min(b)).clamp(x_min, x_max);
        let x1 = scale.invert(a.max(b)).clamp(x_min, x_max);
        Some((x0, x1))
    }
}

#[derive(Clone, Copy)]
enum ResolvedXScale {
    Category(BandScale),
    Continuous(LinearScale),
}

impl ResolvedXScale {
    fn map_x(self, x: f64) -> f32 {
        match self {
            Self::Category(band) => band.map_continuous(x),
            Self::Continuous(lin) => lin.map(x),
        }
    }

    fn map_bucket(self, idx: usize) -> f32 {
        match self {
            Self::Category(band) => band.band_center(idx),
            Self::Continuous(lin) => lin.map(idx as f64),
        }
    }
}

#[derive(Clone, Copy)]
enum ResolvedXDomain {
    Category { count: usize, padding_inner: f32 },
    Continuous { min: f64, max: f64 },
}

impl ResolvedXDomain {
    fn scale(self, plot: PlotRect) -> ResolvedXScale {
        match self {
            Self::Category {
                count,
                padding_inner,
            } => ResolvedXScale::Category(BandScale::new(
                plot.left,
                plot.width(),
                count,
                padding_inner,
            )),
            Self::Continuous { min, max } => {
                ResolvedXScale::Continuous(LinearScale::new(min, max, plot.left, plot.right))
            }
        }
    }

    fn hash_into(self, hasher: &mut impl Hasher) {
        match self {
            Self::Category {
                count,
                padding_inner,
            } => {
                0u8.hash(hasher);
                count.hash(hasher);
                padding_inner.to_bits().hash(hasher);
            }
            Self::Continuous { min, max } => {
                1u8.hash(hasher);
                min.to_bits().hash(hasher);
                max.to_bits().hash(hasher);
            }
        }
    }

    fn category_count(self) -> Option<usize> {
        match self {
            Self::Category { count, .. } => Some(count),
            Self::Continuous { .. } => None,
        }
    }
}

fn resolve_x_domain(x_axis: &XAxis, layers: &[PlotLayer]) -> ResolvedXDomain {
    match x_axis {
        XAxis::Category {
            labels,
            padding_inner,
            ..
        } => ResolvedXDomain::Category {
            count: labels.len(),
            padding_inner: *padding_inner,
        },
        XAxis::Continuous { domain, .. } => {
            let (min, max) = domain.unwrap_or_else(|| {
                let mut min_x = f64::INFINITY;
                let mut max_x = f64::NEG_INFINITY;
                for layer in layers {
                    if let Some((lo, hi)) = layer.x_extent() {
                        min_x = min_x.min(lo);
                        max_x = max_x.max(hi);
                    }
                }
                if !min_x.is_finite() || !max_x.is_finite() || (max_x - min_x).abs() < f64::EPSILON
                {
                    (0.0, 1.0)
                } else {
                    (min_x, max_x)
                }
            });
            ResolvedXDomain::Continuous { min, max }
        }
    }
}

fn compute_plot_geometry_key(
    plot: PlotRect,
    x_domain: ResolvedXDomain,
    y_min: f64,
    y_max: f64,
    layers: &[PlotLayer],
) -> u64 {
    let mut hasher = FxHasher::default();
    plot.left.to_bits().hash(&mut hasher);
    plot.right.to_bits().hash(&mut hasher);
    plot.top.to_bits().hash(&mut hasher);
    plot.bottom.to_bits().hash(&mut hasher);
    x_domain.hash_into(&mut hasher);
    y_min.to_bits().hash(&mut hasher);
    y_max.to_bits().hash(&mut hasher);
    layers.len().hash(&mut hasher);
    let cat_limit = x_domain.category_count();
    for layer in layers {
        match layer {
            PlotLayer::StackedBars(_) | PlotLayer::ReferenceBand(_) => 0u8.hash(&mut hasher),
            PlotLayer::Line(line) => {
                1u8.hash(&mut hasher);
                line.hash_geometry(&mut hasher, cat_limit);
            }
            PlotLayer::ReferenceLine(r) => {
                2u8.hash(&mut hasher);
                r.axis.hash(&mut hasher);
                r.value.to_bits().hash(&mut hasher);
                r.style.hash(&mut hasher);
                r.stroke_width.to_bits().hash(&mut hasher);
            }
        }
    }
    hasher.finish()
}

fn build_line_layer_paths(
    line: &LineLayer,
    x_scale: ResolvedXScale,
    y_scale: LinearScale,
    plot: PlotRect,
    category_limit: Option<usize>,
) -> CachedLayerPaths {
    let max_buckets = (plot.width().ceil() as usize).clamp(128, 1200);
    let is_dashed = line.style == LineStyle::Dashed;
    let width = line
        .stroke_width
        .unwrap_or(if is_dashed { 1.75 } else { 2.25 });
    let raw_runs = line.contiguous_runs(category_limit);
    let mut areas = Vec::new();
    let mut casings = Vec::new();
    let mut strokes = Vec::with_capacity(raw_runs.len());

    let single_half_span = match x_scale {
        ResolvedXScale::Category(band) => (band.bandwidth() * 0.62).max(8.0),
        ResolvedXScale::Continuous(_) => 10.0,
    };

    for run in &raw_runs {
        if run.is_empty() {
            continue;
        }
        let mapped: Vec<(f32, f32)> = if run.len() == 1 {
            // Isolated single-point bucket (including 1-day ranges): paint a crisp horizontal
            // reference segment across the column so comparison baselines stay clearly legible.
            if matches!(line.data, LineData::Buckets(_))
                || matches!(x_scale, ResolvedXScale::Category(_))
            {
                let (x, y) = run[0];
                let cx = x_scale.map_x(x);
                let cy = y_scale.map(y);
                let x0 = (cx - single_half_span).max(plot.left);
                let x1 = (cx + single_half_span).min(plot.right);
                if x1 > x0 + 0.5 {
                    vec![(x0, cy), (x1, cy)]
                } else {
                    continue;
                }
            } else {
                continue;
            }
        } else {
            let decimated = decimate_monotonic_points(run, max_buckets);
            decimated
                .iter()
                .map(|&(x, y)| (x_scale.map_x(x), y_scale.map(y)))
                .collect()
        };

        if mapped.len() < 2 {
            continue;
        }

        if line.fill_area && run.len() >= 2 {
            let first = mapped[0];
            let last = mapped[mapped.len() - 1];
            let mut pb = PathBuilder::fill();
            pb.move_to(point(px(first.0), px(plot.bottom)));
            pb.line_to(point(px(first.0), px(first.1)));
            push_curve_points(&mut pb, &mapped, line.smooth, true);
            pb.line_to(point(px(last.0), px(plot.bottom)));
            pb.close();
            if let Ok(path) = pb.build() {
                areas.push(path);
            }
        }

        if line.halo {
            let mut casing_pb = PathBuilder::stroke(px(width + 2.0));
            if is_dashed {
                casing_pb = casing_pb.dash_array(&[px(5.0), px(4.0)]);
            }
            push_curve_points(&mut casing_pb, &mapped, line.smooth, false);
            if let Ok(path) = casing_pb.build() {
                casings.push(path);
            }
        }

        let mut stroke_pb = PathBuilder::stroke(px(width));
        if is_dashed {
            stroke_pb = stroke_pb.dash_array(&[px(5.0), px(4.0)]);
        }
        push_curve_points(&mut stroke_pb, &mapped, line.smooth, false);
        if let Ok(path) = stroke_pb.build() {
            strokes.push(path);
        }
    }

    CachedLayerPaths::Line {
        areas,
        casings,
        strokes,
    }
}

fn build_plot_geometry(
    key: u64,
    layers: &[PlotLayer],
    x_domain: ResolvedXDomain,
    y_scale: LinearScale,
    plot: PlotRect,
) -> CachedPlotGeometry {
    let x_scale = x_domain.scale(plot);
    let cat_limit = x_domain.category_count();
    let by_layer = layers
        .iter()
        .map(|layer| match layer {
            PlotLayer::StackedBars(_) | PlotLayer::ReferenceBand(_) => None,
            PlotLayer::Line(line) => Some(build_line_layer_paths(
                line, x_scale, y_scale, plot, cat_limit,
            )),
            PlotLayer::ReferenceLine(r) => {
                let (p0, p1) = match r.axis {
                    ReferenceAxis::Horizontal => {
                        let y = y_scale.map(r.value);
                        if y < plot.top - 1.0 || y > plot.bottom + 1.0 {
                            return Some(CachedLayerPaths::ReferenceLine(None));
                        }
                        (point(px(plot.left), px(y)), point(px(plot.right), px(y)))
                    }
                    ReferenceAxis::Vertical => {
                        let x = x_scale.map_x(r.value);
                        if x < plot.left - 1.0 || x > plot.right + 1.0 {
                            return Some(CachedLayerPaths::ReferenceLine(None));
                        }
                        (point(px(x), px(plot.top)), point(px(x), px(plot.bottom)))
                    }
                };
                let mut pb = PathBuilder::stroke(px(r.stroke_width));
                if r.style == LineStyle::Dashed {
                    pb = pb.dash_array(&[px(5.0), px(4.0)]);
                }
                pb.move_to(p0);
                pb.line_to(p1);
                Some(CachedLayerPaths::ReferenceLine(pb.build().ok()))
            }
        })
        .collect();
    CachedPlotGeometry { key, by_layer }
}

fn resolve_layer_colors(
    layers: &[PlotLayer],
    palette: &ChartPalette,
    theme: &Theme,
) -> Rc<[ResolvedLayerColors]> {
    let mut color_idx = 0usize;
    layers
        .iter()
        .map(|layer| match layer {
            PlotLayer::StackedBars(bars) => {
                let cols: Rc<[Hsla]> = bars
                    .series
                    .iter()
                    .map(|s| {
                        s.color.unwrap_or_else(|| {
                            let c = palette.color(color_idx);
                            color_idx += 1;
                            c
                        })
                    })
                    .collect();
                ResolvedLayerColors::StackedBars(cols)
            }
            PlotLayer::Line(line) => {
                let c = line.color.unwrap_or_else(|| match line.style {
                    LineStyle::Solid => {
                        let col = palette.color(color_idx);
                        color_idx += 1;
                        col
                    }
                    LineStyle::Dashed => palette.baseline,
                });
                ResolvedLayerColors::Line(c)
            }
            PlotLayer::ReferenceLine(r) => {
                ResolvedLayerColors::ReferenceLine(r.color.unwrap_or(palette.baseline))
            }
            PlotLayer::ReferenceBand(b) => {
                ResolvedLayerColors::ReferenceBand(b.color.unwrap_or(theme.primary.opacity(0.08)))
            }
        })
        .collect()
}

fn paint_stacked_bars(
    bars: &StackedBarLayer,
    series_colors: &[Hsla],
    band_scale: BandScale,
    y_scale: LinearScale,
    active_category_selection: &Option<RangeInclusive<usize>>,
    palette: &ChartPalette,
    window: &mut Window,
) {
    let bw = band_scale.bandwidth();
    let count = bars.columns.len().min(band_scale.count);
    for col_idx in 0..count {
        let col = &bars.columns[col_idx];
        let dimmed = active_category_selection
            .as_ref()
            .is_some_and(|sel| !sel.contains(&col_idx));
        let top_non_zero = col
            .iter()
            .enumerate()
            .rev()
            .find(|&(_, &v)| v.is_finite() && v > 0.0)
            .map(|(idx, _)| idx);
        let x = band_scale.band_left(col_idx);
        let mut cumulative = 0.0_f64;

        for (s_idx, &raw_val) in col.iter().enumerate() {
            let val = if raw_val.is_finite() {
                raw_val.max(0.0)
            } else {
                0.0
            };
            if val <= 0.0 {
                continue;
            }
            let y_bot = y_scale.map(cumulative);
            cumulative += val;
            let y_top = y_scale.map(cumulative);
            let seg_h = (y_bot - y_top).max(1.0);
            let is_top = top_non_zero == Some(s_idx);
            let base_color = series_colors
                .get(s_idx)
                .copied()
                .unwrap_or_else(|| palette.color(s_idx));
            let color = if dimmed {
                base_color.opacity(0.32)
            } else {
                base_color
            };

            let corner_radii = if is_top {
                let r = bars.bar_radius.min(px(bw * 0.45)).min(px(seg_h * 0.5));
                Corners {
                    top_left: r,
                    top_right: r,
                    bottom_left: px(0.),
                    bottom_right: px(0.),
                }
            } else {
                Corners::default()
            };

            let (draw_y, draw_h) = if !is_top && seg_h > 2.0 {
                (y_top + 0.75, seg_h - 0.75)
            } else {
                (y_top, seg_h)
            };

            window.paint_quad(quad(
                Bounds::new(point(px(x), px(draw_y)), size(px(bw), px(draw_h))),
                corner_radii,
                color,
                Edges::default(),
                transparent_black(),
                BorderStyle::Solid,
            ));
        }
    }
}

fn build_tooltip_data(
    hover: PlotHover,
    x_axis: &XAxis,
    layers: &[PlotLayer],
    layer_colors: &[ResolvedLayerColors],
    value_format: &ChartValueFormat,
) -> Option<PlotTooltipData> {
    let header: SharedString = match (hover, x_axis) {
        (
            PlotHover::Category(idx),
            XAxis::Category {
                labels,
                tooltip_labels,
                ..
            },
        ) => tooltip_labels
            .as_ref()
            .and_then(|t| t.get(idx))
            .or_else(|| labels.get(idx))?
            .clone(),
        (PlotHover::Continuous(hx), XAxis::Continuous { formatter, .. }) => match formatter {
            Some(f) => f(hx).into(),
            None => format!("{hx:.1}").into(),
        },
        _ => return None,
    };

    let mut summary: Option<String> = None;
    let mut rows: Vec<ChartTooltipRow> = Vec::new();

    for (layer_idx, layer) in layers.iter().enumerate() {
        match (layer, &layer_colors[layer_idx]) {
            (PlotLayer::StackedBars(bars), ResolvedLayerColors::StackedBars(colors)) => {
                if !bars.include_in_tooltip {
                    continue;
                }
                let PlotHover::Category(h_idx) = hover else {
                    continue;
                };
                if h_idx >= bars.columns.len() {
                    continue;
                }
                summary = Some(value_format.format_detail(bars.column_total(h_idx)));
                let col = &bars.columns[h_idx];
                for (s_idx, meta) in bars.series.iter().enumerate() {
                    let val = col.get(s_idx).copied().unwrap_or(0.0);
                    if !val.is_finite() || val <= 0.0 {
                        continue;
                    }
                    rows.push(ChartTooltipRow::dot(
                        meta.label.clone(),
                        value_format.format_detail(val),
                        colors[s_idx],
                    ));
                }
            }
            (PlotLayer::Line(line), ResolvedLayerColors::Line(color)) => {
                if !line.include_in_tooltip {
                    continue;
                }
                let val = match hover {
                    PlotHover::Category(idx) => line.value_at_bucket(idx),
                    PlotHover::Continuous(hx) => line.sample_at(hx),
                };
                let Some(val) = val else {
                    continue;
                };
                let sep = line.tooltip_separator && !rows.is_empty();
                rows.push(ChartTooltipRow {
                    color: *color,
                    label: line.label.clone(),
                    value: value_format.format_detail(val),
                    swatch: line.style.swatch(),
                    separator_before: sep,
                });
            }
            (PlotLayer::ReferenceLine(_), _) | (PlotLayer::ReferenceBand(_), _) => {}
            _ => {}
        }
    }

    if rows.is_empty() && summary.is_none() {
        return None;
    }

    Some(PlotTooltipData {
        hover,
        header,
        summary,
        rows,
    })
}

/// Composable GPU-native Cartesian chart supporting any combination of [`StackedBarLayer`],
/// [`LineLayer`], [`ReferenceLine`], and [`ReferenceBand`] over shared [`XAxis`] & [`YAxis`]
/// scales, with unified hover/drag-brush interaction and customizable tooltips.
#[derive(IntoElement)]
pub struct CartesianPlot {
    id: ElementId,
    x_axis: XAxis,
    y_axis: YAxis,
    layers: Vec<PlotLayer>,
    height: Pixels,
    fill_height: bool,
    palette: Option<ChartPalette>,
    selectable: bool,
    selected_category_range: Option<RangeInclusive<usize>>,
    selected_domain_range: Option<(f64, f64)>,
    style: StyleRefinement,
    on_category_click: Option<Rc<dyn Fn(&usize, &mut Window, &mut App) + 'static>>,
    on_category_brush: Option<Rc<dyn Fn(&RangeInclusive<usize>, &mut Window, &mut App) + 'static>>,
    on_domain_brush: Option<Rc<dyn Fn(&(f64, f64), &mut Window, &mut App) + 'static>>,
    on_selection_clear: Option<Rc<dyn Fn(&(), &mut Window, &mut App) + 'static>>,
    on_hover: Option<Rc<dyn Fn(Option<PlotHover>, &mut Window, &mut App) + 'static>>,
    tooltip_rows: Option<Rc<dyn Fn(&PlotTooltipData) -> Vec<ChartTooltipRow> + 'static>>,
    tooltip_builder: Option<Rc<dyn Fn(&PlotTooltipData, &Theme) -> AnyElement + 'static>>,
}

/// Convenience constructor for [`CartesianPlot::new`].
pub fn cartesian_plot(id: impl Into<ElementId>) -> CartesianPlot {
    CartesianPlot::new(id)
}

impl CartesianPlot {
    /// Creates a new [`CartesianPlot`].
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            x_axis: XAxis::default(),
            y_axis: YAxis::default(),
            layers: Vec::new(),
            height: px(220.),
            fill_height: false,
            palette: None,
            selectable: false,
            selected_category_range: None,
            selected_domain_range: None,
            style: StyleRefinement::default(),
            on_category_click: None,
            on_category_brush: None,
            on_domain_brush: None,
            on_selection_clear: None,
            on_hover: None,
            tooltip_rows: None,
            tooltip_builder: None,
        }
    }

    /// Configures the horizontal axis ([`XAxis::category`] or [`XAxis::continuous`]).
    pub fn x_axis(mut self, axis: XAxis) -> Self {
        self.x_axis = axis;
        self
    }

    /// Configures the vertical axis ([`YAxis`]).
    pub fn y_axis(mut self, axis: YAxis) -> Self {
        self.y_axis = axis;
        self
    }

    /// Appends one composable [`PlotLayer`] (painted in declaration order).
    pub fn layer(mut self, layer: impl Into<PlotLayer>) -> Self {
        self.layers.push(layer.into());
        self
    }

    /// Appends multiple [`PlotLayer`]s in declaration order.
    pub fn layers(mut self, layers: impl IntoIterator<Item = PlotLayer>) -> Self {
        self.layers.extend(layers);
        self
    }

    /// Sets a fixed plot height (default `220px`) and disables [`Self::fill_height`].
    pub fn height(mut self, height: Pixels) -> Self {
        self.height = height;
        self.fill_height = false;
        self
    }

    /// Expands vertically to fill available flex container space instead of using a fixed height.
    pub fn fill_height(mut self, fill: bool) -> Self {
        self.fill_height = fill;
        self
    }

    /// Overrides the chart color palette (defaults to [`ChartPalette::from_theme`]).
    pub fn palette(mut self, palette: ChartPalette) -> Self {
        self.palette = Some(palette);
        self
    }

    /// Enables or disables horizontal drag-brush selection.
    pub fn selectable(mut self, selectable: bool) -> Self {
        self.selectable = selectable;
        self
    }

    /// Highlights a controlled inclusive column range on [`XAxis::Category`] plots.
    pub fn selected_category_range(mut self, range: Option<RangeInclusive<usize>>) -> Self {
        self.selected_category_range = range;
        self
    }

    /// Highlights a controlled numeric span `(start_x, end_x)` on [`XAxis::Continuous`] plots.
    pub fn selected_domain_range(mut self, range: Option<(f64, f64)>) -> Self {
        self.selected_domain_range = range;
        self
    }

    /// Registers a click callback invoked with the clicked category column index.
    pub fn on_category_click(
        mut self,
        handler: impl Fn(&usize, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_category_click = Some(Rc::new(handler));
        self
    }

    /// Registers a drag-selection callback invoked with the brushed category column range.
    /// Automatically enables [`Self::selectable`] unless explicitly disabled afterward.
    pub fn on_category_brush(
        mut self,
        handler: impl Fn(&RangeInclusive<usize>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.selectable = true;
        self.on_category_brush = Some(Rc::new(handler));
        self
    }

    /// Registers a drag-selection callback invoked with the normalized continuous `(start_x, end_x)` span.
    /// Automatically enables [`Self::selectable`] unless explicitly disabled afterward.
    pub fn on_domain_brush(
        mut self,
        handler: impl Fn(&(f64, f64), &mut Window, &mut App) + 'static,
    ) -> Self {
        self.selectable = true;
        self.on_domain_brush = Some(Rc::new(handler));
        self
    }

    /// Registers a callback invoked when clicking without dragging while a controlled selection is active.
    pub fn on_selection_clear(
        mut self,
        handler: impl Fn(&(), &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_selection_clear = Some(Rc::new(handler));
        self
    }

    /// Registers a callback invoked whenever the hovered target changes (or `None` on pointer leave).
    pub fn on_hover(
        mut self,
        handler: impl Fn(Option<PlotHover>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_hover = Some(Rc::new(handler));
        self
    }

    /// Customizes or augments the tooltip rows rendered inside the default chart tooltip card.
    pub fn tooltip_rows(
        mut self,
        builder: impl Fn(&PlotTooltipData) -> Vec<ChartTooltipRow> + 'static,
    ) -> Self {
        self.tooltip_rows = Some(Rc::new(builder));
        self
    }

    /// Replaces the floating hover tooltip element completely with a custom renderer.
    pub fn tooltip_builder(
        mut self,
        builder: impl Fn(&PlotTooltipData, &Theme) -> AnyElement + 'static,
    ) -> Self {
        self.tooltip_builder = Some(Rc::new(builder));
        self
    }
}

impl Styled for CartesianPlot {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for CartesianPlot {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let palette = self
            .palette
            .unwrap_or_else(|| ChartPalette::from_theme(&theme));

        let interaction_entity = window.use_keyed_state(
            (self.id.clone(), "cartesian-plot-interaction"),
            cx,
            |_, _| Rc::new(RefCell::new(PlotInteractionState::default())),
        );
        let interaction_rc = interaction_entity.read(cx).clone();

        let x_domain = resolve_x_domain(&self.x_axis, &self.layers);
        let (y_min, y_max, y_ticks) = self
            .y_axis
            .resolve(&self.layers, x_domain.category_count());
        let layer_colors = resolve_layer_colors(&self.layers, &palette, &theme);

        let (hover, live_cat_brush, live_dom_brush) = {
            let state = interaction_rc.borrow();
            let (cb, db) = if self.selectable {
                match x_domain {
                    ResolvedXDomain::Category {
                        count,
                        padding_inner,
                    } => (state.brushed_category(count, padding_inner), None),
                    ResolvedXDomain::Continuous { min, max } => {
                        (None, state.brushed_domain(min, max))
                    }
                }
            } else {
                (None, None)
            };
            (state.hover, cb, db)
        };

        let is_live_brushing = live_cat_brush.is_some() || live_dom_brush.is_some();
        let active_cat_sel = live_cat_brush.clone().or(self.selected_category_range);
        let active_dom_sel = live_dom_brush.or(self.selected_domain_range);
        let has_controlled_selection = active_cat_sel.is_some() || active_dom_sel.is_some();

        let layers_rc: Rc<[PlotLayer]> = Rc::from(self.layers);
        let layers_paint = layers_rc.clone();
        let colors_paint = layer_colors.clone();
        let x_axis_paint = self.x_axis.clone();
        let y_ticks_paint = y_ticks;
        let show_grid = self.y_axis.show_grid;
        let format_paint = self.y_axis.format.clone();
        let palette_paint = palette;
        let surface_bg = theme.surface;
        let accent_color = theme.primary;
        let interaction_prepaint = interaction_rc.clone();
        let interaction_paint = interaction_rc.clone();
        let active_cat_paint = active_cat_sel;
        let active_dom_paint = active_dom_sel;

        let plot_canvas = canvas(
            move |bounds, _, _| {
                interaction_prepaint.borrow_mut().plot_bounds = Some(bounds);
                bounds
            },
            move |bounds, _, window, cx| {
                let plot = PlotRect::from_bounds(bounds, DEFAULT_PLOT_MARGINS);
                let x_scale = x_domain.scale(plot);
                let y_scale = LinearScale::new(y_min, y_max, plot.bottom, plot.top);

                // 1. Horizontal gridlines & Y-axis labels
                paint_y_gridlines(
                    &y_ticks_paint,
                    y_scale,
                    plot,
                    bounds.origin.x.0,
                    DEFAULT_PLOT_MARGINS.left - 10.0,
                    &format_paint,
                    &palette_paint,
                    show_grid,
                    window,
                    cx,
                );

                // 2. Hover backdrop (category slot or continuous hairline crosshair)
                if !is_live_brushing && let Some(h) = hover {
                    match (h, x_scale) {
                        (PlotHover::Category(idx), ResolvedXScale::Category(band))
                            if idx < band.count =>
                        {
                            let slot_x = plot.left + (idx as f32) * band.step();
                            window.paint_quad(fill(
                                Bounds::new(
                                    point(px(slot_x), px(plot.top)),
                                    size(px(band.step()), px(plot.height())),
                                ),
                                palette_paint.crosshair,
                            ));
                        }
                        (PlotHover::Continuous(hx), ResolvedXScale::Continuous(lin)) => {
                            let x_px = lin.map(hx).clamp(plot.left, plot.right);
                            window.paint_quad(fill(
                                Bounds::new(
                                    point(px(x_px), px(plot.top)),
                                    size(px(1.), px(plot.height())),
                                ),
                                palette_paint.gridline.opacity(1.0),
                            ));
                        }
                        _ => {}
                    }
                }

                // 3. Active drag/controlled brush band
                match x_scale {
                    ResolvedXScale::Category(band) => {
                        if let Some(sel) = &active_cat_paint
                            && band.count > 0
                        {
                            let lo = (*sel.start()).min(band.count - 1);
                            let hi = (*sel.end()).min(band.count - 1);
                            let sel_left = plot.left + (lo as f32) * band.step();
                            let sel_right = plot.left + ((hi + 1) as f32) * band.step();
                            paint_brush_band(
                                sel_left,
                                sel_right,
                                plot,
                                accent_color,
                                is_live_brushing,
                                window,
                            );
                        }
                    }
                    ResolvedXScale::Continuous(lin) => {
                        if let Some((a, b)) = active_dom_paint {
                            paint_brush_band(
                                lin.map(a),
                                lin.map(b),
                                plot,
                                accent_color,
                                is_live_brushing,
                                window,
                            );
                        }
                    }
                }

                // 4. Ensure cached tessellated paths for all path-based layers
                let geom_key =
                    compute_plot_geometry_key(plot, x_domain, y_min, y_max, &layers_paint);
                {
                    let mut state = interaction_paint.borrow_mut();
                    let hit = state
                        .cached_geometry
                        .as_ref()
                        .is_some_and(|g| g.key == geom_key);
                    if !hit {
                        state.cached_geometry = Some(build_plot_geometry(
                            geom_key,
                            &layers_paint,
                            x_domain,
                            y_scale,
                            plot,
                        ));
                    }
                }

                // 5. Paint every layer strictly in declaration order
                let state_ref = interaction_paint.borrow();
                let cached = state_ref.cached_geometry.as_ref();
                let casing_color = Hsla {
                    a: 0.78,
                    ..surface_bg
                };

                for (idx, layer) in layers_paint.iter().enumerate() {
                    match (layer, &colors_paint[idx]) {
                        (PlotLayer::ReferenceBand(band), ResolvedLayerColors::ReferenceBand(col)) => {
                            let (bx, by, bw, bh) = match band.axis {
                                ReferenceAxis::Horizontal => {
                                    let y0 = y_scale.map(band.end).clamp(plot.top, plot.bottom);
                                    let y1 = y_scale.map(band.start).clamp(plot.top, plot.bottom);
                                    (plot.left, y0, plot.width(), (y1 - y0).max(0.0))
                                }
                                ReferenceAxis::Vertical => {
                                    let x0 = x_scale.map_x(band.start).clamp(plot.left, plot.right);
                                    let x1 = x_scale.map_x(band.end).clamp(plot.left, plot.right);
                                    (x0.min(x1), plot.top, (x1 - x0).abs(), plot.height())
                                }
                            };
                            if bw > 0.5 && bh > 0.5 {
                                window.paint_quad(fill(
                                    Bounds::new(point(px(bx), px(by)), size(px(bw), px(bh))),
                                    *col,
                                ));
                                if let Some(label) = &band.label {
                                    let lw = (label.len() as f32 * 6.4 + 8.0).min(bw - 8.0).max(0.0);
                                    if lw > 12.0 {
                                        paint_chart_text(
                                            label.clone(),
                                            point(px(bx + bw - lw - 4.0), px(by + 3.0)),
                                            px(10.5),
                                            palette_paint.axis_label,
                                            FontWeight::MEDIUM,
                                            TextAlign::Right,
                                            Some(px(lw)),
                                            window,
                                            cx,
                                        );
                                    }
                                }
                            }
                        }
                        (
                            PlotLayer::StackedBars(bars),
                            ResolvedLayerColors::StackedBars(series_colors),
                        ) => {
                            if let ResolvedXScale::Category(band) = x_scale {
                                paint_stacked_bars(
                                    bars,
                                    series_colors,
                                    band,
                                    y_scale,
                                    &active_cat_paint,
                                    &palette_paint,
                                    window,
                                );
                            }
                        }
                        (PlotLayer::Line(_), ResolvedLayerColors::Line(color)) => {
                            if let Some(Some(CachedLayerPaths::Line {
                                areas,
                                casings,
                                strokes,
                            })) = cached.and_then(|g| g.by_layer.get(idx))
                            {
                                for path in areas {
                                    window.paint_path(
                                        path.clone(),
                                        linear_gradient(
                                            180.0,
                                            linear_color_stop(color.opacity(0.16), 0.0),
                                            linear_color_stop(color.opacity(0.0), 1.0),
                                        ),
                                    );
                                }
                                for path in casings {
                                    window.paint_path(path.clone(), casing_color);
                                }
                                for path in strokes {
                                    window.paint_path(path.clone(), *color);
                                }
                            }
                        }
                        (PlotLayer::ReferenceLine(r), ResolvedLayerColors::ReferenceLine(color)) => {
                            if let Some(Some(CachedLayerPaths::ReferenceLine(Some(path)))) =
                                cached.and_then(|g| g.by_layer.get(idx))
                            {
                                window.paint_path(path.clone(), *color);
                                if let Some(label) = &r.label {
                                    let lw = (label.len() as f32 * 6.4 + 10.0).clamp(40.0, 140.0);
                                    let (lx, ly, align) = match r.axis {
                                        ReferenceAxis::Horizontal => {
                                            let y = y_scale.map(r.value);
                                            let top = (y - 16.0).max(plot.top);
                                            ((plot.right - lw - 2.0).max(plot.left), top, TextAlign::Right)
                                        }
                                        ReferenceAxis::Vertical => {
                                            let x = x_scale.map_x(r.value);
                                            let left = (x + 4.0)
                                                .clamp(plot.left, (plot.right - lw).max(plot.left));
                                            (left, plot.top + 2.0, TextAlign::Left)
                                        }
                                    };
                                    paint_chart_text(
                                        label.clone(),
                                        point(px(lx), px(ly)),
                                        px(10.5),
                                        *color,
                                        FontWeight::MEDIUM,
                                        align,
                                        Some(px(lw)),
                                        window,
                                        cx,
                                    );
                                }
                            }
                        }
                        _ => {}
                    }
                }
                drop(state_ref);

                // 6. Endpoint callout badges on LineLayers
                for (idx, layer) in layers_paint.iter().enumerate() {
                    let (PlotLayer::Line(line), ResolvedLayerColors::Line(color)) =
                        (layer, &colors_paint[idx])
                    else {
                        continue;
                    };
                    let Some(prefix) = &line.endpoint_callout else {
                        continue;
                    };
                    let Some((lx, ly)) = line.last_point() else {
                        continue;
                    };
                    let ex = x_scale.map_x(lx);
                    let ey = y_scale.map(ly);
                    paint_chart_marker(ex, ey, 4.0, *color, surface_bg, window);

                    let callout_text = format!("{prefix}: {}", format_paint.format_detail(ly));
                    let callout_w = (callout_text.len() as f32 * 6.8 + 14.0).clamp(84.0, 148.0);
                    let callout_h = 18.0_f32;
                    let callout_x = (ex - callout_w * 0.5)
                        .clamp(plot.left + 4.0, (plot.right - callout_w).max(plot.left));
                    let callout_y = (ey - 26.0).max(plot.top);
                    window.paint_quad(quad(
                        Bounds::new(
                            point(px(callout_x), px(callout_y)),
                            size(px(callout_w), px(callout_h)),
                        ),
                        Corners::all(px(5.)),
                        surface_bg.opacity(0.92),
                        Edges::all(px(1.)),
                        color.opacity(0.35),
                        BorderStyle::Solid,
                    ));
                    paint_chart_text(
                        callout_text,
                        point(px(callout_x), px(callout_y + 2.0)),
                        px(11.),
                        *color,
                        FontWeight::SEMIBOLD,
                        TextAlign::Center,
                        Some(px(callout_w)),
                        window,
                        cx,
                    );
                }

                // 7. Hover marker dots on every LineLayer covering `hover`
                if !is_live_brushing && let Some(h) = hover {
                    for (idx, layer) in layers_paint.iter().enumerate() {
                        let (PlotLayer::Line(line), ResolvedLayerColors::Line(color)) =
                            (layer, &colors_paint[idx])
                        else {
                            continue;
                        };
                        if !line.include_in_tooltip {
                            continue;
                        }
                        let (x_px, y_val) = match h {
                            PlotHover::Category(c_idx) => {
                                (x_scale.map_bucket(c_idx), line.value_at_bucket(c_idx))
                            }
                            PlotHover::Continuous(hx) => (
                                x_scale.map_x(hx).clamp(plot.left, plot.right),
                                line.sample_at(hx),
                            ),
                        };
                        if let Some(hy) = y_val {
                            paint_chart_marker(
                                x_px,
                                y_scale.map(hy),
                                3.5,
                                *color,
                                surface_bg,
                                window,
                            );
                        }
                    }
                }

                // 8. X-axis labels
                match (&x_axis_paint, x_scale) {
                    (XAxis::Category { labels, .. }, ResolvedXScale::Category(band)) => {
                        let n = labels.len();
                        if n > 0 {
                            let max_labels =
                                ((plot.width() / 68.0).floor() as usize).clamp(2.min(n), n);
                            let stride = (n.saturating_sub(1))
                                .div_ceil(max_labels.saturating_sub(1).max(1))
                                .max(1);
                            for (idx, label) in labels.iter().enumerate() {
                                let is_last = idx + 1 == n;
                                if idx % stride != 0 && !is_last {
                                    continue;
                                }
                                if !is_last && n.saturating_sub(1) - idx < stride.div_ceil(2) {
                                    continue;
                                }
                                let cx_pos = band.band_center(idx);
                                let label_w = 56.0_f32;
                                let (align, label_x) = if n == 1 {
                                    (TextAlign::Center, cx_pos - label_w * 0.5)
                                } else if idx == 0 {
                                    (TextAlign::Left, plot.left)
                                } else if is_last {
                                    (TextAlign::Right, (plot.right - label_w).max(plot.left))
                                } else {
                                    (TextAlign::Center, cx_pos - label_w * 0.5)
                                };
                                paint_chart_text(
                                    label.clone(),
                                    point(px(label_x), px(plot.bottom + 7.0)),
                                    px(11.),
                                    palette_paint.axis_label,
                                    FontWeight::NORMAL,
                                    align,
                                    Some(px(label_w)),
                                    window,
                                    cx,
                                );
                            }
                        }
                    }
                    (XAxis::Continuous { ticks, .. }, ResolvedXScale::Continuous(lin)) => {
                        let tick_count = ticks.len();
                        for (t_idx, (tx, label)) in ticks.iter().enumerate() {
                            let x_px = lin.map(*tx);
                            let label_w = 50.0_f32;
                            let (align, label_x) = if t_idx == 0 {
                                (TextAlign::Left, x_px)
                            } else if t_idx + 1 == tick_count {
                                (TextAlign::Right, (x_px - label_w).max(plot.left))
                            } else {
                                (TextAlign::Center, x_px - label_w * 0.5)
                            };
                            paint_chart_text(
                                label.clone(),
                                point(px(label_x), px(plot.bottom + 7.0)),
                                px(11.),
                                palette_paint.axis_label,
                                FontWeight::NORMAL,
                                align,
                                Some(px(label_w)),
                                window,
                                cx,
                            );
                        }
                    }
                    _ => {}
                }
            },
        )
        .flex_1()
        .min_h_0()
        .w_full();

        // Unified mouse interaction handlers (written once for BOTH Category and Continuous plots!)
        let selectable = self.selectable;
        let on_cat_click = self.on_category_click;
        let has_cat_click = on_cat_click.is_some();
        let on_cat_brush_up = self.on_category_brush.clone();
        let on_cat_brush_out = self.on_category_brush;
        let on_dom_brush_up = self.on_domain_brush.clone();
        let on_dom_brush_out = self.on_domain_brush;
        let on_clear = self.on_selection_clear;
        let on_hover_move = self.on_hover.clone();
        let on_hover_leave = self.on_hover;

        let down_entity = interaction_entity.clone();
        let move_entity = interaction_entity.clone();
        let up_entity = interaction_entity.clone();
        let up_out_entity = interaction_entity.clone();
        let leave_entity = interaction_entity;

        let cursor_style = if selectable {
            Some(CursorStyle::Crosshair)
        } else if has_cat_click {
            Some(CursorStyle::PointingHand)
        } else {
            None
        };

        let mut container = div()
            .id(self.id)
            .relative()
            .w_full()
            .flex()
            .flex_col()
            .map(|el| {
                if self.fill_height {
                    el.flex_grow_1()
                        .flex_shrink_1()
                        .flex_basis(px(0.))
                        .min_h(px(160.))
                } else {
                    el.h(self.height)
                }
            })
            .when_some(cursor_style, |el, cursor| el.cursor(cursor))
            .on_mouse_down(MouseButton::Left, move |event: &MouseDownEvent, _, cx| {
                if !selectable && !has_cat_click && !has_controlled_selection {
                    return;
                }
                let mut state = down_entity.read(cx).borrow_mut();
                let Some(bounds) = state.plot_bounds else {
                    return;
                };
                let plot = PlotRect::from_bounds(bounds, DEFAULT_PLOT_MARGINS);
                let mx = event.position.x.0;
                if plot.contains_x(mx) {
                    state.drag_anchor_x = Some(mx);
                    state.drag_current_x = Some(mx);
                }
            })
            .on_mouse_move(move |event: &MouseMoveEvent, window, cx| {
                let (changed, hover_emit) = {
                    let mut state = move_entity.read(cx).borrow_mut();
                    let Some(bounds) = state.plot_bounds else {
                        return;
                    };
                    let plot = PlotRect::from_bounds(bounds, DEFAULT_PLOT_MARGINS);
                    let mx = event.position.x.0;

                    match x_domain {
                        ResolvedXDomain::Category {
                            count,
                            padding_inner,
                        } => {
                            let scale = BandScale::new(plot.left, plot.width(), count, padding_inner);
                            let next_hover = scale.index_at_x(mx).map(PlotHover::Category);
                            if selectable && state.drag_anchor_x.is_some() {
                                // Only re-render during category brushing when the snapped column range changes.
                                let prev_brush = state.brushed_category(count, padding_inner);
                                state.drag_current_x = Some(mx.clamp(plot.left, plot.right));
                                let next_brush = state.brushed_category(count, padding_inner);
                                let prev_h = state.hover;
                                state.hover = next_hover;
                                let dirty = if next_brush.is_some() {
                                    prev_brush != next_brush
                                } else {
                                    prev_h != next_hover || prev_brush != next_brush
                                };
                                (dirty, (prev_h != next_hover).then_some(next_hover))
                            } else if state.hover != next_hover {
                                state.hover = next_hover;
                                (true, Some(next_hover))
                            } else {
                                (false, None)
                            }
                        }
                        ResolvedXDomain::Continuous { min, max } => {
                            if selectable && state.drag_anchor_x.is_some() {
                                let clamped = mx.clamp(plot.left, plot.right);
                                let dirty = state
                                    .drag_current_x
                                    .is_none_or(|prev| (prev - clamped).abs() >= 1.0);
                                if dirty {
                                    state.drag_current_x = Some(clamped);
                                }
                                (dirty, None)
                            } else {
                                let next_hover = if plot.contains_x(mx) {
                                    let scale = LinearScale::new(min, max, plot.left, plot.right);
                                    Some(PlotHover::Continuous(scale.invert(mx).clamp(min, max)))
                                } else {
                                    None
                                };
                                let hover_changed = match (state.hover, next_hover) {
                                    (
                                        Some(PlotHover::Continuous(a)),
                                        Some(PlotHover::Continuous(b)),
                                    ) => (a - b).abs() > (max - min) * 0.005,
                                    (None, None) => false,
                                    _ => true,
                                };
                                if hover_changed {
                                    state.hover = next_hover;
                                    (true, Some(next_hover))
                                } else {
                                    (false, None)
                                }
                            }
                        }
                    }
                };
                if changed {
                    move_entity.update(cx, |_, cx| cx.notify());
                }
                if let Some(next_h) = hover_emit
                    && let Some(cb) = &on_hover_move
                {
                    cb(next_h, window, cx);
                }
            })
            .on_mouse_up(MouseButton::Left, move |_: &MouseUpEvent, window, cx| {
                let (cat_brush, dom_brush, clicked_cat, had_drag) = {
                    let mut state = up_entity.read(cx).borrow_mut();
                    let had = state.drag_anchor_x.is_some();
                    let (cb, db) = if selectable {
                        match x_domain {
                            ResolvedXDomain::Category {
                                count,
                                padding_inner,
                            } => (state.brushed_category(count, padding_inner), None),
                            ResolvedXDomain::Continuous { min, max } => {
                                (None, state.brushed_domain(min, max))
                            }
                        }
                    } else {
                        (None, None)
                    };
                    let clicked = match state.hover {
                        Some(PlotHover::Category(idx)) => Some(idx),
                        _ => None,
                    };
                    state.drag_anchor_x = None;
                    state.drag_current_x = None;
                    (cb, db, clicked, had)
                };
                if cat_brush.is_some() || dom_brush.is_some() {
                    up_entity.update(cx, |_, cx| cx.notify());
                }
                if let Some(range) = cat_brush {
                    if let Some(cb) = &on_cat_brush_up {
                        cb(&range, window, cx);
                    }
                } else if let Some(span) = dom_brush {
                    if let Some(cb) = &on_dom_brush_up {
                        cb(&span, window, cx);
                    }
                } else if had_drag {
                    if let Some(idx) = clicked_cat
                        && let Some(cb) = &on_cat_click
                    {
                        cb(&idx, window, cx);
                    } else if has_controlled_selection
                        && let Some(cb) = &on_clear
                    {
                        cb(&(), window, cx);
                    }
                }
            })
            .on_mouse_up_out(MouseButton::Left, move |_: &MouseUpEvent, window, cx| {
                let (cat_brush, dom_brush) = {
                    let mut state = up_out_entity.read(cx).borrow_mut();
                    let (cb, db) = if selectable {
                        match x_domain {
                            ResolvedXDomain::Category {
                                count,
                                padding_inner,
                            } => (state.brushed_category(count, padding_inner), None),
                            ResolvedXDomain::Continuous { min, max } => {
                                (None, state.brushed_domain(min, max))
                            }
                        }
                    } else {
                        (None, None)
                    };
                    state.drag_anchor_x = None;
                    state.drag_current_x = None;
                    (cb, db)
                };
                if cat_brush.is_some() || dom_brush.is_some() {
                    up_out_entity.update(cx, |_, cx| cx.notify());
                }
                if let Some(range) = cat_brush
                    && let Some(cb) = &on_cat_brush_out
                {
                    cb(&range, window, cx);
                } else if let Some(span) = dom_brush
                    && let Some(cb) = &on_dom_brush_out
                {
                    cb(&span, window, cx);
                }
            })
            .on_hover(move |hovered, window, cx| {
                if !hovered {
                    let changed = {
                        let mut state = leave_entity.read(cx).borrow_mut();
                        state.hover.take().is_some()
                    };
                    if changed {
                        leave_entity.update(cx, |_, cx| cx.notify());
                        if let Some(cb) = &on_hover_leave {
                            cb(None, window, cx);
                        }
                    }
                }
            })
            .child(plot_canvas);

        // Floating live brush pill OR hover tooltip
        if let Some(bounds) = interaction_rc.borrow().plot_bounds {
            let local = PlotRect::from_local_bounds(bounds, DEFAULT_PLOT_MARGINS);
            let local_x = x_domain.scale(local);

            if let (
                Some(brush_range),
                XAxis::Category { labels, .. },
                ResolvedXScale::Category(band),
            ) = (live_cat_brush, &self.x_axis, local_x)
                && !labels.is_empty()
            {
                let lo = (*brush_range.start()).min(labels.len() - 1);
                let hi = (*brush_range.end()).min(labels.len() - 1);
                let sel_left = local.left + (lo as f32) * band.step();
                let sel_right = local.left + ((hi + 1) as f32) * band.step();
                let mid_x = (sel_left + sel_right) * 0.5;
                let count = hi - lo + 1;
                let bar_sum: Option<f64> = layers_rc.iter().find_map(|layer| match layer {
                    PlotLayer::StackedBars(bars) => {
                        Some((lo..=hi).map(|i| bars.column_total(i)).sum())
                    }
                    _ => None,
                });
                let sum_suffix = bar_sum
                    .map(|s| format!(" · {}", self.y_axis.format.format_detail(s)))
                    .unwrap_or_default();
                let summary = if lo == hi {
                    format!("{}{sum_suffix}", labels[lo])
                } else {
                    format!("{} – {} ({count}){sum_suffix}", labels[lo], labels[hi])
                };
                container = container.child(render_brush_pill(
                    summary,
                    mid_x,
                    local.left,
                    bounds.size.width.0,
                    140.0,
                    290.0,
                    &theme,
                ));
            } else if let (
                Some((a, b)),
                XAxis::Continuous { formatter, .. },
                ResolvedXScale::Continuous(lin),
            ) = (live_dom_brush, &self.x_axis, local_x)
            {
                let fmt_x = |v: f64| match formatter {
                    Some(f) => f(v),
                    None => format!("{v:.1}"),
                };
                let mid_x = (lin.map(a) + lin.map(b)) * 0.5;
                let delta_suffix = layers_rc.iter().find_map(|layer| match layer {
                    PlotLayer::Line(line) if line.style == LineStyle::Solid => {
                        let y0 = line.sample_at(a)?;
                        let y1 = line.sample_at(b)?;
                        Some(format!(
                            " · Δ {}",
                            self.y_axis.format.format_detail((y1 - y0).max(0.0))
                        ))
                    }
                    _ => None,
                });
                let summary = format!(
                    "{} – {}{}",
                    fmt_x(a),
                    fmt_x(b),
                    delta_suffix.unwrap_or_default()
                );
                container = container.child(render_brush_pill(
                    summary,
                    mid_x,
                    local.left,
                    bounds.size.width.0,
                    124.0,
                    260.0,
                    &theme,
                ));
            } else if !is_live_brushing
                && let Some(h) = hover
                && let Some(data) =
                    build_tooltip_data(h, &self.x_axis, &layers_rc, &layer_colors, &self.y_axis.format)
            {
                if let Some(builder) = &self.tooltip_builder {
                    container = container.child(builder(&data, &theme));
                } else {
                    let rows = match &self.tooltip_rows {
                        Some(transform) => transform(&data),
                        None => data.rows.clone(),
                    };
                    let has_wide_row = rows
                        .iter()
                        .any(|r| r.swatch != LegendSwatch::Dot || r.label.len() > 14);
                    let tooltip_w = if has_wide_row { 216.0 } else { 196.0 };
                    let anchor_x = match h {
                        PlotHover::Category(idx) => local_x.map_bucket(idx),
                        PlotHover::Continuous(hx) => local_x.map_x(hx),
                    };
                    container = container.child(render_chart_tooltip(
                        data.header,
                        data.summary,
                        rows,
                        anchor_x,
                        local.left,
                        bounds.size.width.0,
                        tooltip_w,
                        px(8.),
                        &theme,
                    ));
                }
            }
        }

        crate::Refineable::refine(container.style(), &self.style);
        container
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::charts::layers::{ReferenceBand, ReferenceLine, StackedBarSeries};

    #[test]
    fn y_axis_resolves_combined_layer_extents_and_explicit_domains() {
        let layers = vec![
            PlotLayer::from(StackedBarLayer::new(
                [StackedBarSeries::new("a"), StackedBarSeries::new("b")],
                vec![vec![12.0, 8.0], vec![25.0, 5.0]],
            )),
            PlotLayer::from(
                LineLayer::from_buckets("Previous", [Some(15.0), Some(62.0)]).dashed(),
            ),
            PlotLayer::from(ReferenceLine::horizontal(75.0).label("Budget")),
            PlotLayer::from(ReferenceBand::horizontal(0.0, 200.0)),
        ];

        let (lo, hi, ticks) = YAxis::new(ChartValueFormat::Currency).resolve(&layers, Some(2));
        assert_eq!(lo, 0.0);
        assert!(hi >= 75.0 && hi <= 100.0, "expected nice_max >= 75, got {hi}");
        assert!(ticks.len() >= 3);

        // Explicit pinned domain overrides auto-scaling cleanly.
        let (elo, ehi, eticks) = YAxis::new(ChartValueFormat::Percent)
            .domain(0.0, 100.0)
            .ticks(5)
            .resolve(&layers, Some(2));
        assert_eq!((elo, ehi), (0.0, 100.0));
        assert_eq!(eticks, vec![0.0, 25.0, 50.0, 75.0, 100.0]);
    }

    #[test]
    fn tooltip_data_aggregates_stacked_bars_and_dashed_line_layers() {
        let theme = Theme::dark();
        let palette = ChartPalette::from_theme(&theme);
        let x_axis = XAxis::category(["Nov 18", "Nov 19"]).tooltip_labels(["Wed, Nov 18", "Thu, Nov 19"]);
        let layers = vec![
            PlotLayer::from(StackedBarLayer::new(
                [StackedBarSeries::new("Anthropic"), StackedBarSeries::new("OpenAI")],
                vec![vec![10.0, 5.0], vec![12.5, 0.0]],
            )),
            PlotLayer::from(
                LineLayer::from_buckets("Previous 30 days", [Some(14.0), Some(18.25)])
                    .dashed()
                    .tooltip_separator(true),
            ),
        ];
        let colors = resolve_layer_colors(&layers, &palette, &theme);
        let data = build_tooltip_data(
            PlotHover::Category(1),
            &x_axis,
            &layers,
            &colors,
            &ChartValueFormat::Currency,
        )
        .expect("tooltip data");

        assert_eq!(data.header.as_ref(), "Thu, Nov 19");
        assert_eq!(data.summary.as_deref(), Some("$12.50"));
        assert_eq!(data.rows.len(), 2);
        assert_eq!(data.rows[0].label.as_ref(), "Anthropic");
        assert_eq!(data.rows[0].swatch, LegendSwatch::Dot);
        assert!(!data.rows[0].separator_before);
        assert_eq!(data.rows[1].label.as_ref(), "Previous 30 days");
        assert_eq!(data.rows[1].swatch, LegendSwatch::DashedLine);
        assert_eq!(data.rows[1].value, "$18.25");
        assert!(data.rows[1].separator_before);
    }

    #[test]
    fn build_plot_geometry_handles_gaps_single_bucket_and_reference_lines() {
        let plot = PlotRect {
            left: 54.0,
            right: 454.0,
            top: 12.0,
            bottom: 212.0,
        };
        let x_domain = ResolvedXDomain::Category {
            count: 5,
            padding_inner: DEFAULT_BAND_PADDING_INNER,
        };
        let y_scale = LinearScale::new(0.0, 100.0, plot.bottom, plot.top);
        let layers = vec![
            PlotLayer::from(
                LineLayer::from_buckets(
                    "Previous",
                    [Some(20.0), Some(60.0), None, Some(40.0), None],
                )
                .dashed()
                .smooth(true)
                .halo(true),
            ),
            PlotLayer::from(ReferenceLine::horizontal(50.0)),
        ];
        let geom = build_plot_geometry(1, &layers, x_domain, y_scale, plot);
        assert_eq!(geom.by_layer.len(), 2);
        match &geom.by_layer[0] {
            Some(CachedLayerPaths::Line {
                casings, strokes, ..
            }) => {
                assert_eq!(casings.len(), 2);
                assert_eq!(strokes.len(), 2);
            }
            _ => panic!("expected cached line paths"),
        }
        match &geom.by_layer[1] {
            Some(CachedLayerPaths::ReferenceLine(Some(_))) => {}
            _ => panic!("expected cached reference line path"),
        }
    }
}
