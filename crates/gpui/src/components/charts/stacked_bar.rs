//! GPU-batched [`StackedBarChart`] with top-segment corner rounding, optional comparison
//! baseline line overlay ([`StackedBarBaseline`]), nice Y ticks, auto-thinned X labels,
//! interactive hover tooltip, bar click callbacks, and optional horizontal drag-to-select
//! brushing.

use std::cell::RefCell;
use std::hash::{Hash, Hasher};
use std::ops::RangeInclusive;
use std::rc::Rc;

use collections::FxHasher;

use crate::{
    App, BorderStyle, Bounds, Corners, CursorStyle, Edges, ElementId, FontWeight, Hsla,
    IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Path,
    PathBuilder, Pixels, RenderOnce, SharedString, StyleRefinement, Styled, TextAlign, Window,
    canvas, div, fill, point, prelude::*, px, quad, size, transparent_black,
};

use super::core::{
    BandScale, ChartLegendItem, ChartMargins, ChartPalette, ChartTooltipRow, ChartValueFormat,
    LegendSwatch, LinearScale, PlotRect, nice_ticks, paint_brush_band, paint_chart_marker,
    paint_chart_text, paint_y_gridlines, render_brush_pill, render_chart_tooltip,
};
use super::line::LineStyle;
use crate::components::theme::ActiveTheme;

const STACKED_BAR_MARGINS: ChartMargins = ChartMargins {
    left: 54.0,
    right: 12.0,
    top: 10.0,
    bottom: 26.0,
};

const BAND_PADDING_INNER: f32 = 0.28;

/// Metadata for one stacked series layer (e.g. `"claude-sonnet-5-5"`).
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

/// One column (e.g. a calendar day) containing one value per [`StackedBarSeries`].
#[derive(Clone, Debug)]
pub struct StackedBarPoint {
    /// Short X-axis label (e.g. `"Oct 22"`, `"Today"`).
    pub label: SharedString,
    /// Stacked values parallel to `series`.
    pub values: Vec<f64>,
}

impl StackedBarPoint {
    /// Creates a new stacked bar column.
    pub fn new(label: impl Into<SharedString>, values: impl Into<Vec<f64>>) -> Self {
        Self {
            label: label.into(),
            values: values.into(),
        }
    }

    /// Sum of all non-negative segment values in this column.
    pub fn total(&self) -> f64 {
        self.values.iter().map(|v| v.max(0.0)).sum()
    }
}

/// Optional comparison / baseline line overlaid across [`StackedBarChart`] columns
/// (for example, previous-period totals drawn as a dashed curve).
#[derive(Clone, Debug)]
pub struct StackedBarBaseline {
    /// Legend & tooltip label (e.g. `"Previous 30 days"`).
    pub label: SharedString,
    /// Per-column comparison values aligned 1-to-1 with [`StackedBarChart::points`].
    /// `None` (or non-finite) entries split the curve cleanly without bridging missing buckets.
    pub values: Vec<Option<f64>>,
    /// Stroke pattern (defaults to [`LineStyle::Dashed`]).
    pub style: LineStyle,
    /// Optional explicit stroke color (defaults to `palette.baseline`).
    pub color: Option<Hsla>,
    /// Stroke thickness in pixels (defaults to `1.75` for dashed, `2.0` for solid).
    pub stroke_width: Option<f32>,
    /// Whether to smooth the curve using Fritsch–Carlson monotone cubic interpolation
    /// (defaults to `true`, matching sparkline baselines with zero overshoot).
    pub smooth: bool,
}

impl StackedBarBaseline {
    /// Creates a new dashed, smoothed [`StackedBarBaseline`].
    pub fn new(label: impl Into<SharedString>, values: impl Into<Vec<Option<f64>>>) -> Self {
        Self {
            label: label.into(),
            values: values.into(),
            style: LineStyle::Dashed,
            color: None,
            stroke_width: None,
            smooth: true,
        }
    }

    /// Configures solid or dashed stroke style.
    pub fn line_style(mut self, style: LineStyle) -> Self {
        self.style = style;
        self
    }

    /// Overrides the baseline stroke color.
    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    /// Overrides the baseline stroke thickness in pixels.
    pub fn stroke_width(mut self, width: f32) -> Self {
        self.stroke_width = Some(width);
        self
    }

    /// Enables or disables Fritsch–Carlson monotone cubic smoothing.
    pub fn smooth(mut self, smooth: bool) -> Self {
        self.smooth = smooth;
        self
    }

    /// Returns the finite non-negative comparison value at column `idx`, if present.
    fn value_at(&self, idx: usize) -> Option<f64> {
        let v = (*self.values.get(idx)?)?;
        v.is_finite().then(|| v.max(0.0))
    }

    /// Maximum finite non-negative value across `count` columns.
    fn max_value(&self, count: usize) -> f64 {
        self.values
            .iter()
            .take(count)
            .flatten()
            .copied()
            .filter(|v| v.is_finite())
            .fold(0.0_f64, f64::max)
    }
}

struct CachedBaselineGeometry {
    key: u64,
    casings: Vec<Path>,
    strokes: Vec<Path>,
}

fn compute_baseline_geometry_key(
    plot: PlotRect,
    nice_max: f64,
    point_count: usize,
    baseline: &StackedBarBaseline,
) -> u64 {
    let mut hasher = FxHasher::default();
    plot.left.to_bits().hash(&mut hasher);
    plot.right.to_bits().hash(&mut hasher);
    plot.top.to_bits().hash(&mut hasher);
    plot.bottom.to_bits().hash(&mut hasher);
    nice_max.to_bits().hash(&mut hasher);
    point_count.hash(&mut hasher);
    baseline.style.hash(&mut hasher);
    baseline.smooth.hash(&mut hasher);
    baseline.stroke_width.map(f32::to_bits).hash(&mut hasher);
    let len = point_count.min(baseline.values.len());
    len.hash(&mut hasher);
    for slot in &baseline.values[..len] {
        match slot.filter(|v| v.is_finite()) {
            Some(v) => {
                1u8.hash(&mut hasher);
                v.max(0.0).to_bits().hash(&mut hasher);
            }
            None => 0u8.hash(&mut hasher),
        }
    }
    hasher.finish()
}

/// Splits baseline values into contiguous finite runs in pixel coordinates.
fn baseline_pixel_runs(
    baseline: &StackedBarBaseline,
    point_count: usize,
    band_scale: BandScale,
    y_scale: LinearScale,
    plot: PlotRect,
) -> Vec<Vec<(f32, f32)>> {
    let len = point_count.min(baseline.values.len());
    let mut runs: Vec<Vec<(f32, f32)>> = Vec::new();
    let mut current: Vec<(f32, f32)> = Vec::new();

    for idx in 0..len {
        if let Some(val) = baseline.value_at(idx) {
            current.push((band_scale.band_center(idx), y_scale.map(val)));
        } else if !current.is_empty() {
            runs.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        runs.push(current);
    }

    // For isolated 1-point runs (including 1-day ranges), draw a crisp horizontal reference
    // segment across the bar column so the comparison level remains clearly visible.
    let half_span = (band_scale.bandwidth() * 0.62).max(8.0);
    for run in &mut runs {
        if run.len() == 1 {
            let (cx, cy) = run[0];
            let x0 = (cx - half_span).max(plot.left);
            let x1 = (cx + half_span).min(plot.right);
            if x1 > x0 + 0.5 {
                *run = vec![(x0, cy), (x1, cy)];
            }
        }
    }
    runs
}

fn build_baseline_geometry(
    key: u64,
    baseline: &StackedBarBaseline,
    point_count: usize,
    band_scale: BandScale,
    y_scale: LinearScale,
    plot: PlotRect,
) -> CachedBaselineGeometry {
    let is_dashed = baseline.style == LineStyle::Dashed;
    let width = baseline
        .stroke_width
        .unwrap_or(if is_dashed { 1.75 } else { 2.0 });
    let runs = baseline_pixel_runs(baseline, point_count, band_scale, y_scale, plot);
    let mut casings = Vec::with_capacity(runs.len());
    let mut strokes = Vec::with_capacity(runs.len());

    for run in &runs {
        if run.len() < 2 {
            continue;
        }
        let mut casing_pb = PathBuilder::stroke(px(width + 2.0));
        let mut stroke_pb = PathBuilder::stroke(px(width));
        if is_dashed {
            casing_pb = casing_pb.dash_array(&[px(5.0), px(4.0)]);
            stroke_pb = stroke_pb.dash_array(&[px(5.0), px(4.0)]);
        }
        super::sparkline::push_curve_points(&mut casing_pb, run, baseline.smooth, false);
        super::sparkline::push_curve_points(&mut stroke_pb, run, baseline.smooth, false);
        if let Ok(path) = casing_pb.build() {
            casings.push(path);
        }
        if let Ok(path) = stroke_pb.build() {
            strokes.push(path);
        }
    }

    CachedBaselineGeometry {
        key,
        casings,
        strokes,
    }
}

#[derive(Default)]
struct StackedBarInteractionState {
    hovered_index: Option<usize>,
    plot_bounds: Option<Bounds<Pixels>>,
    drag_anchor_x: Option<f32>,
    drag_current_x: Option<f32>,
    cached_baseline: Option<CachedBaselineGeometry>,
}

impl StackedBarInteractionState {
    fn is_brushing(&self) -> bool {
        match (self.drag_anchor_x, self.drag_current_x) {
            (Some(a), Some(b)) => (b - a).abs() >= 4.0,
            _ => false,
        }
    }

    fn brushed_indices(&self, point_count: usize) -> Option<RangeInclusive<usize>> {
        if !self.is_brushing() || point_count == 0 {
            return None;
        }
        let plot = PlotRect::from_bounds(self.plot_bounds?, STACKED_BAR_MARGINS);
        let scale = BandScale::new(plot.left, plot.width(), point_count, BAND_PADDING_INNER);
        let max_x = (plot.right - 0.01).max(plot.left);
        let a = self.drag_anchor_x?.clamp(plot.left, max_x);
        let b = self.drag_current_x?.clamp(plot.left, max_x);
        let i0 = scale.index_at_x(a.min(b)).unwrap_or(0);
        let i1 = scale
            .index_at_x(a.max(b))
            .unwrap_or_else(|| point_count.saturating_sub(1));
        Some(i0.min(i1)..=i0.max(i1))
    }
}

/// GPU-native stacked vertical bar chart rendered with instanced GPUI quads, tabular axis labels,
/// hover tooltips, and optional horizontal drag-to-select brushing.
#[derive(IntoElement)]
pub struct StackedBarChart {
    id: ElementId,
    series: Vec<StackedBarSeries>,
    points: Vec<StackedBarPoint>,
    baseline: Option<StackedBarBaseline>,
    height: Pixels,
    fill_height: bool,
    value_format: ChartValueFormat,
    bar_radius: Pixels,
    palette: Option<ChartPalette>,
    selectable: bool,
    selected_range: Option<RangeInclusive<usize>>,
    style: StyleRefinement,
    on_bar_click: Option<Rc<dyn Fn(&usize, &mut Window, &mut App) + 'static>>,
    on_brush_select: Option<Rc<dyn Fn(&RangeInclusive<usize>, &mut Window, &mut App) + 'static>>,
}

/// Convenience constructor for [`StackedBarChart::new`].
pub fn stacked_bar_chart(id: impl Into<ElementId>) -> StackedBarChart {
    StackedBarChart::new(id)
}

impl StackedBarChart {
    /// Creates a new [`StackedBarChart`].
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            series: Vec::new(),
            points: Vec::new(),
            baseline: None,
            height: px(220.),
            fill_height: false,
            value_format: ChartValueFormat::Currency,
            bar_radius: px(3.),
            palette: None,
            selectable: false,
            selected_range: None,
            style: StyleRefinement::default(),
            on_bar_click: None,
            on_brush_select: None,
        }
    }

    /// Sets the series descriptors (bottom-to-top stacking order).
    pub fn series(mut self, series: impl IntoIterator<Item = StackedBarSeries>) -> Self {
        self.series = series.into_iter().collect();
        self
    }

    /// Sets the bar columns.
    pub fn points(mut self, points: impl IntoIterator<Item = StackedBarPoint>) -> Self {
        self.points = points.into_iter().collect();
        self
    }

    /// Overlays a comparison / baseline line across the bar columns (e.g. previous period totals).
    pub fn baseline(mut self, baseline: StackedBarBaseline) -> Self {
        self.baseline = Some(baseline);
        self
    }

    /// Sets the chart plot height (default `220px`) and disables [`Self::fill_height`].
    pub fn height(mut self, height: Pixels) -> Self {
        self.height = height;
        self.fill_height = false;
        self
    }

    /// Expands vertically to fill available flex container space instead of using a fixed
    /// [`Self::height`]. Defaults to a `160px` minimum height (customize via [`Styled::min_h`]).
    pub fn fill_height(mut self, fill: bool) -> Self {
        self.fill_height = fill;
        self
    }

    /// Sets the Y-axis and tooltip value formatter (default [`ChartValueFormat::Currency`]).
    pub fn value_format(mut self, format: ChartValueFormat) -> Self {
        self.value_format = format;
        self
    }

    /// Sets the top corner radius applied to the topmost non-zero segment of each stack.
    pub fn bar_radius(mut self, radius: Pixels) -> Self {
        self.bar_radius = radius;
        self
    }

    /// Overrides the chart color palette (defaults to [`ChartPalette::from_theme`]).
    pub fn palette(mut self, palette: ChartPalette) -> Self {
        self.palette = Some(palette);
        self
    }

    /// Enables or disables horizontal drag-brush selection across bar columns.
    pub fn selectable(mut self, selectable: bool) -> Self {
        self.selectable = selectable;
        self
    }

    /// Highlights a controlled inclusive column index range (dimming columns outside it).
    pub fn selected_range(mut self, range: Option<RangeInclusive<usize>>) -> Self {
        self.selected_range = range;
        self
    }

    /// Registers a click callback invoked with the clicked column index.
    pub fn on_bar_click(
        mut self,
        handler: impl Fn(&usize, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_bar_click = Some(Rc::new(handler));
        self
    }

    /// Registers a drag-selection callback invoked with the brushed inclusive column index range.
    ///
    /// Automatically enables [`Self::selectable`] unless explicitly disabled afterward.
    pub fn on_brush_select(
        mut self,
        handler: impl Fn(&RangeInclusive<usize>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.selectable = true;
        self.on_brush_select = Some(Rc::new(handler));
        self
    }

    /// Builds matching [`ChartLegendItem`]s for use with [`ChartCard::legend`](super::ChartCard::legend).
    pub fn legend_items(series: &[StackedBarSeries], palette: &ChartPalette) -> Vec<ChartLegendItem> {
        series
            .iter()
            .enumerate()
            .map(|(i, s)| {
                ChartLegendItem::dot(s.label.clone(), s.color.unwrap_or_else(|| palette.color(i)))
            })
            .collect()
    }
}

impl Styled for StackedBarChart {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for StackedBarChart {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let palette = self
            .palette
            .unwrap_or_else(|| ChartPalette::from_theme(&theme));

        let interaction_entity = window.use_keyed_state(
            (self.id.clone(), "stacked-bar-interaction"),
            cx,
            |_, _| Rc::new(RefCell::new(StackedBarInteractionState::default())),
        );
        let interaction_rc = interaction_entity.read(cx).clone();
        let point_count = self.points.len();

        let (hovered_index, live_brush) = {
            let state = interaction_rc.borrow();
            (
                state.hovered_index,
                if self.selectable {
                    state.brushed_indices(point_count)
                } else {
                    None
                },
            )
        };
        let active_selection = live_brush.clone().or(self.selected_range);

        let max_bars = self
            .points
            .iter()
            .map(StackedBarPoint::total)
            .fold(0.0_f64, f64::max);
        let max_baseline = self
            .baseline
            .as_ref()
            .map_or(0.0, |b| b.max_value(point_count));
        let max_total = max_bars.max(max_baseline);
        let (nice_max, y_ticks) = nice_ticks(max_total, 4);

        let series_colors: Rc<[Hsla]> = self
            .series
            .iter()
            .enumerate()
            .map(|(i, s)| s.color.unwrap_or_else(|| palette.color(i)))
            .collect();

        // Share points, baseline & colors via Rc to eliminate deep Vec cloning per hover frame.
        let points_rc: Rc<[StackedBarPoint]> = Rc::from(self.points);
        let baseline_rc: Option<Rc<StackedBarBaseline>> = self.baseline.map(Rc::new);
        let baseline_color = baseline_rc
            .as_ref()
            .map(|b| b.color.unwrap_or(palette.baseline));
        let points_paint = points_rc.clone();
        let baseline_paint = baseline_rc.clone();
        let colors_paint = series_colors.clone();
        let ticks_paint = y_ticks;
        let format_paint = self.value_format.clone();
        let palette_paint = palette.clone();
        let surface_bg = theme.surface;
        let accent_color = theme.primary;
        let bar_radius = self.bar_radius;
        let interaction_prepaint = interaction_rc.clone();
        let interaction_paint = interaction_rc.clone();
        let active_selection_paint = active_selection;
        let is_live_brushing = live_brush.is_some();

        let plot_canvas = canvas(
            move |bounds, _, _| {
                interaction_prepaint.borrow_mut().plot_bounds = Some(bounds);
                bounds
            },
            move |bounds, _, window, cx| {
                let plot = PlotRect::from_bounds(bounds, STACKED_BAR_MARGINS);
                let y_scale = LinearScale::new(0.0, nice_max, plot.bottom, plot.top);
                let band_scale =
                    BandScale::new(plot.left, plot.width(), points_paint.len(), BAND_PADDING_INNER);

                // 1. Horizontal gridlines & right-aligned Y-axis labels
                paint_y_gridlines(
                    &ticks_paint,
                    y_scale,
                    plot,
                    bounds.origin.x.0,
                    STACKED_BAR_MARGINS.left - 10.0,
                    &format_paint,
                    &palette_paint,
                    window,
                    cx,
                );

                // 2. Hover column backdrop highlight (suppressed while dragging a brush)
                if !is_live_brushing
                    && let Some(h_idx) = hovered_index
                    && h_idx < points_paint.len()
                {
                    let slot_x = plot.left + (h_idx as f32) * band_scale.step();
                    window.paint_quad(fill(
                        Bounds::new(
                            point(px(slot_x), px(plot.top)),
                            size(px(band_scale.step()), px(plot.height())),
                        ),
                        palette_paint.crosshair,
                    ));
                }

                // 3. Active drag/selected brush band behind bars
                if let Some(sel) = &active_selection_paint
                    && !points_paint.is_empty()
                {
                    let lo = (*sel.start()).min(points_paint.len() - 1);
                    let hi = (*sel.end()).min(points_paint.len() - 1);
                    let sel_left = plot.left + (lo as f32) * band_scale.step();
                    let sel_right = plot.left + ((hi + 1) as f32) * band_scale.step();
                    paint_brush_band(
                        sel_left,
                        sel_right,
                        plot,
                        accent_color,
                        is_live_brushing,
                        window,
                    );
                }

                // 4. Stacked bar segments (bottom-to-top; top-only corner rounding on highest segment).
                // A 0.75px hairline gap between stacked layers exposes the backdrop cleanly without
                // painting an extra opaque separator quad per segment.
                let bw = band_scale.bandwidth();
                for (col_idx, col) in points_paint.iter().enumerate() {
                    let dimmed = active_selection_paint
                        .as_ref()
                        .is_some_and(|sel| !sel.contains(&col_idx));
                    let top_non_zero = col
                        .values
                        .iter()
                        .enumerate()
                        .rev()
                        .find(|&(_, &v)| v > 0.0)
                        .map(|(idx, _)| idx);
                    let x = band_scale.band_left(col_idx);
                    let mut cumulative = 0.0_f64;

                    for (s_idx, &raw_val) in col.values.iter().enumerate() {
                        let val = raw_val.max(0.0);
                        if val <= 0.0 {
                            continue;
                        }
                        let y_bot = y_scale.map(cumulative);
                        cumulative += val;
                        let y_top = y_scale.map(cumulative);
                        let seg_h = (y_bot - y_top).max(1.0);
                        let is_top = top_non_zero == Some(s_idx);
                        let base_color = colors_paint
                            .get(s_idx)
                            .copied()
                            .unwrap_or_else(|| palette_paint.color(s_idx));
                        let color = if dimmed {
                            base_color.opacity(0.32)
                        } else {
                            base_color
                        };

                        let corner_radii = if is_top {
                            let r = bar_radius.min(px(bw * 0.45)).min(px(seg_h * 0.5));
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

                // 5. Optional comparison / baseline curve overlaid cleanly on top of bars
                if let Some(baseline) = &baseline_paint
                    && !points_paint.is_empty()
                {
                    let b_color = baseline_color.unwrap_or(palette_paint.baseline);
                    let casing_color = Hsla {
                        a: 0.78,
                        ..surface_bg
                    };
                    let geom_key = compute_baseline_geometry_key(
                        plot,
                        nice_max,
                        points_paint.len(),
                        baseline,
                    );
                    {
                        let mut state = interaction_paint.borrow_mut();
                        let cache_hit = state
                            .cached_baseline
                            .as_ref()
                            .is_some_and(|g| g.key == geom_key);
                        if !cache_hit {
                            state.cached_baseline = Some(build_baseline_geometry(
                                geom_key,
                                baseline,
                                points_paint.len(),
                                band_scale,
                                y_scale,
                                plot,
                            ));
                        }
                        if let Some(geom) = &state.cached_baseline {
                            for path in &geom.casings {
                                window.paint_path(path.clone(), casing_color);
                            }
                            for path in &geom.strokes {
                                window.paint_path(path.clone(), b_color);
                            }
                        }
                    }

                    if !is_live_brushing
                        && let Some(h_idx) = hovered_index
                        && let Some(b_val) = baseline.value_at(h_idx)
                    {
                        paint_chart_marker(
                            band_scale.band_center(h_idx),
                            y_scale.map(b_val),
                            3.5,
                            b_color,
                            surface_bg,
                            window,
                        );
                    }
                }

                // 6. Auto-thinned X-axis labels
                let n = points_paint.len();
                if n > 0 {
                    let max_labels = ((plot.width() / 68.0).floor() as usize).clamp(2.min(n), n);
                    let stride = (n.saturating_sub(1))
                        .div_ceil(max_labels.saturating_sub(1).max(1))
                        .max(1);
                    for (idx, col) in points_paint.iter().enumerate() {
                        let is_last = idx + 1 == n;
                        if idx % stride != 0 && !is_last {
                            continue;
                        }
                        if !is_last && n.saturating_sub(1) - idx < stride.div_ceil(2) {
                            continue;
                        }
                        let cx_pos = band_scale.band_center(idx);
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
                            col.label.clone(),
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
            },
        )
        .flex_1()
        .min_h_0()
        .w_full();

        let selectable = self.selectable;
        let on_bar_click = self.on_bar_click;
        let on_brush_select = self.on_brush_select;

        let down_entity = interaction_entity.clone();
        let move_entity = interaction_entity.clone();
        let up_entity = interaction_entity.clone();
        let up_out_entity = interaction_entity.clone();
        let leave_entity = interaction_entity;

        let on_brush_up = on_brush_select.clone();
        let on_brush_up_out = on_brush_select;
        let on_click_up = on_bar_click.clone();

        let cursor_style = if selectable {
            Some(CursorStyle::Crosshair)
        } else if on_bar_click.is_some() {
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
                let mut state = down_entity.read(cx).borrow_mut();
                let Some(bounds) = state.plot_bounds else {
                    return;
                };
                let plot = PlotRect::from_bounds(bounds, STACKED_BAR_MARGINS);
                let mx = event.position.x.0;
                if plot.contains_x(mx) {
                    state.drag_anchor_x = Some(mx);
                    state.drag_current_x = Some(mx);
                }
            })
            .on_mouse_move(move |event: &MouseMoveEvent, _, cx| {
                let changed = {
                    let mut state = move_entity.read(cx).borrow_mut();
                    let Some(bounds) = state.plot_bounds else {
                        return;
                    };
                    let plot = PlotRect::from_bounds(bounds, STACKED_BAR_MARGINS);
                    let scale =
                        BandScale::new(plot.left, plot.width(), point_count, BAND_PADDING_INNER);
                    let next_idx = scale.index_at_x(event.position.x.0);

                    if selectable && state.drag_anchor_x.is_some() {
                        // Only re-render during bar brushing when the snapped column range changes.
                        let prev_brush = state.brushed_indices(point_count);
                        let clamped = event.position.x.0.clamp(plot.left, plot.right);
                        state.drag_current_x = Some(clamped);
                        let next_brush = state.brushed_indices(point_count);
                        if next_brush.is_some() {
                            state.hovered_index = next_idx;
                            prev_brush != next_brush
                        } else if state.hovered_index != next_idx {
                            state.hovered_index = next_idx;
                            true
                        } else {
                            prev_brush != next_brush
                        }
                    } else if state.hovered_index != next_idx {
                        state.hovered_index = next_idx;
                        true
                    } else {
                        false
                    }
                };
                if changed {
                    move_entity.update(cx, |_, cx| cx.notify());
                }
            })
            .on_mouse_up(MouseButton::Left, move |_: &MouseUpEvent, window, cx| {
                let (brushed, clicked_idx, had_drag) = {
                    let mut state = up_entity.read(cx).borrow_mut();
                    let had = state.drag_anchor_x.is_some();
                    let brushed = if selectable {
                        state.brushed_indices(point_count)
                    } else {
                        None
                    };
                    let clicked = state.hovered_index;
                    state.drag_anchor_x = None;
                    state.drag_current_x = None;
                    (brushed, clicked, had)
                };
                if brushed.is_some() {
                    up_entity.update(cx, |_, cx| cx.notify());
                }
                if let Some(range) = brushed {
                    if let Some(cb) = &on_brush_up {
                        cb(&range, window, cx);
                    }
                } else if had_drag
                    && let Some(idx) = clicked_idx
                    && let Some(cb) = &on_click_up
                {
                    cb(&idx, window, cx);
                }
            })
            .on_mouse_up_out(MouseButton::Left, move |_: &MouseUpEvent, window, cx| {
                let brushed = {
                    let mut state = up_out_entity.read(cx).borrow_mut();
                    let brushed = if selectable {
                        state.brushed_indices(point_count)
                    } else {
                        None
                    };
                    state.drag_anchor_x = None;
                    state.drag_current_x = None;
                    brushed
                };
                if let Some(range) = brushed {
                    up_out_entity.update(cx, |_, cx| cx.notify());
                    if let Some(cb) = &on_brush_up_out {
                        cb(&range, window, cx);
                    }
                }
            })
            .on_hover(move |hovered, _, cx| {
                if !hovered {
                    let changed = {
                        let mut state = leave_entity.read(cx).borrow_mut();
                        state.hovered_index.take().is_some()
                    };
                    if changed {
                        leave_entity.update(cx, |_, cx| cx.notify());
                    }
                }
            })
            .child(plot_canvas);

        // 6. Floating live brush pill or hover tooltip
        if let Some(brush_range) = live_brush
            && let Some(bounds) = interaction_rc.borrow().plot_bounds
            && !points_rc.is_empty()
        {
            let lo = (*brush_range.start()).min(points_rc.len() - 1);
            let hi = (*brush_range.end()).min(points_rc.len() - 1);
            let local = PlotRect::from_local_bounds(bounds, STACKED_BAR_MARGINS);
            let band_scale =
                BandScale::new(local.left, local.width(), point_count, BAND_PADDING_INNER);
            let sel_left = local.left + (lo as f32) * band_scale.step();
            let sel_right = local.left + ((hi + 1) as f32) * band_scale.step();
            let mid_x = (sel_left + sel_right) * 0.5;
            let count = hi - lo + 1;
            let span_sum: f64 = points_rc[lo..=hi].iter().map(StackedBarPoint::total).sum();
            let summary = if lo == hi {
                format!(
                    "{} · {}",
                    points_rc[lo].label,
                    self.value_format.format_detail(span_sum)
                )
            } else {
                format!(
                    "{} – {} ({}d) · {}",
                    points_rc[lo].label,
                    points_rc[hi].label,
                    count,
                    self.value_format.format_detail(span_sum)
                )
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
        } else if let Some(h_idx) = hovered_index
            && let Some(col) = points_rc.get(h_idx)
            && let Some(bounds) = interaction_rc.borrow().plot_bounds
        {
            let local = PlotRect::from_local_bounds(bounds, STACKED_BAR_MARGINS);
            let band_scale =
                BandScale::new(local.left, local.width(), point_count, BAND_PADDING_INNER);
            let mut rows: Vec<ChartTooltipRow> = self
                .series
                .iter()
                .enumerate()
                .filter_map(|(s_idx, meta)| {
                    let val = col.values.get(s_idx).copied().unwrap_or(0.0);
                    if val <= 0.0 {
                        return None;
                    }
                    let color = series_colors
                        .get(s_idx)
                        .copied()
                        .unwrap_or_else(|| palette.color(s_idx));
                    Some(ChartTooltipRow {
                        color,
                        label: meta.label.clone(),
                        value: self.value_format.format_detail(val),
                        swatch: LegendSwatch::Dot,
                        separator_before: false,
                    })
                })
                .collect();
            let mut has_baseline_row = false;
            if let Some(baseline) = &baseline_rc
                && let Some(b_val) = baseline.value_at(h_idx)
            {
                has_baseline_row = true;
                rows.push(ChartTooltipRow {
                    color: baseline_color.unwrap_or(palette.baseline),
                    label: baseline.label.clone(),
                    value: self.value_format.format_detail(b_val),
                    swatch: match baseline.style {
                        LineStyle::Solid => LegendSwatch::SolidLine,
                        LineStyle::Dashed => LegendSwatch::DashedLine,
                    },
                    separator_before: !rows.is_empty(),
                });
            }
            let tooltip_w = if has_baseline_row { 214.0 } else { 196.0 };
            container = container.child(render_chart_tooltip(
                col.label.clone(),
                Some(self.value_format.format_detail(col.total())),
                rows,
                band_scale.band_center(h_idx),
                local.left,
                bounds.size.width.0,
                tooltip_w,
                px(8.),
                &theme,
            ));
        }

        crate::Refineable::refine(container.style(), &self.style);
        container
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stacked_bar_baseline_defaults_and_max_value() {
        let b = StackedBarBaseline::new(
            "Previous 7 days",
            vec![Some(10.0), None, Some(-5.0), Some(f64::NAN), Some(42.5)],
        );
        assert_eq!(b.style, LineStyle::Dashed);
        assert!(b.smooth);
        assert_eq!(b.value_at(0), Some(10.0));
        assert_eq!(b.value_at(1), None);
        assert_eq!(b.value_at(2), Some(0.0));
        assert_eq!(b.value_at(3), None);
        assert_eq!(b.value_at(4), Some(42.5));
        assert_eq!(b.max_value(5), 42.5);
        assert_eq!(b.max_value(2), 10.0);
    }

    #[test]
    fn stacked_bar_baseline_splits_gaps_and_handles_single_point() {
        let plot = PlotRect {
            left: 54.0,
            right: 454.0,
            top: 10.0,
            bottom: 210.0,
        };
        let band_scale = BandScale::new(plot.left, plot.width(), 5, BAND_PADDING_INNER);
        let y_scale = LinearScale::new(0.0, 100.0, plot.bottom, plot.top);

        let b = StackedBarBaseline::new(
            "Previous period",
            vec![Some(20.0), Some(60.0), None, Some(40.0), None],
        );
        let runs = baseline_pixel_runs(&b, 5, band_scale, y_scale, plot);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].len(), 2);
        // Isolated single-point bucket expands to a crisp horizontal tick across the column.
        assert_eq!(runs[1].len(), 2);
        assert!((runs[1][0].1 - runs[1][1].1).abs() < 1e-4);
        assert!(runs[1][1].0 > runs[1][0].0);

        let geom = build_baseline_geometry(1, &b, 5, band_scale, y_scale, plot);
        assert_eq!(geom.casings.len(), 2);
        assert_eq!(geom.strokes.len(), 2);
    }
}
