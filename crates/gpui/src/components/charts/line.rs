//! GPU-native multi-series [`LineChart`] supporting solid & dashed strokes via Lyon
//! [`PathBuilder`], optional area gradient fills, endpoint callout annotations, interactive
//! hover crosshairs, and optional horizontal drag-to-select brushing.

use std::borrow::Cow;
use std::cell::RefCell;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use collections::FxHasher;

use crate::{
    App, BorderStyle, Bounds, Corners, CursorStyle, Edges, ElementId, FontWeight, Hsla,
    IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Path,
    PathBuilder, Pixels, RenderOnce, SharedString, StyleRefinement, Styled, TextAlign, Window,
    canvas, div, fill, linear_color_stop, linear_gradient, point, prelude::*, px, quad, size,
};

use super::core::{
    ChartLegendItem, ChartMargins, ChartPalette, ChartTooltipRow, ChartValueFormat, LinearScale,
    PlotRect, nice_ticks, paint_brush_band, paint_chart_marker, paint_chart_text, paint_y_gridlines,
    render_brush_pill, render_chart_tooltip,
};
use crate::components::theme::ActiveTheme;

const LINE_CHART_MARGINS: ChartMargins = ChartMargins {
    left: 54.0,
    right: 16.0,
    top: 14.0,
    bottom: 26.0,
};

/// Line stroke pattern for a [`LineSeries`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LineStyle {
    /// Continuous solid stroke.
    #[default]
    Solid,
    /// Dashed comparison / baseline stroke.
    Dashed,
}

/// One data series rendered inside a [`LineChart`].
#[derive(Clone, Debug)]
pub struct LineSeries {
    /// Legend & tooltip series name (e.g. `"Today"` or `"Previous 5 workdays, average"`).
    pub label: SharedString,
    /// Ordered `(x, y)` coordinates along the shared X domain (e.g. `x = 0.0..=24.0` hours).
    pub points: Vec<(f64, f64)>,
    /// Solid or dashed line style.
    pub style: LineStyle,
    /// Optional explicit stroke color (defaults to [`ChartPalette`] for solid, `palette.baseline` for dashed).
    pub color: Option<Hsla>,
    /// Stroke thickness in pixels (default `2.25` for solid, `1.75` for dashed).
    pub stroke_width: Option<f32>,
    /// Whether to draw a soft vertical gradient fill beneath this series.
    pub fill_area: bool,
    /// Optional inline callout prefix rendered next to the last point (e.g. `Some("Today")` → `"Today: $36.21"`).
    pub endpoint_callout: Option<SharedString>,
}

impl LineSeries {
    /// Creates a new solid [`LineSeries`].
    pub fn new(label: impl Into<SharedString>, points: impl Into<Vec<(f64, f64)>>) -> Self {
        Self {
            label: label.into(),
            points: points.into(),
            style: LineStyle::Solid,
            color: None,
            stroke_width: None,
            fill_area: false,
            endpoint_callout: None,
        }
    }

    /// Configures solid or dashed stroke style.
    pub fn line_style(mut self, style: LineStyle) -> Self {
        self.style = style;
        self
    }

    /// Overrides the series stroke color.
    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    /// Overrides stroke thickness in pixels.
    pub fn stroke_width(mut self, width: f32) -> Self {
        self.stroke_width = Some(width);
        self
    }

    /// Enables a subtle vertical gradient fill under the curve.
    pub fn fill_area(mut self, enabled: bool) -> Self {
        self.fill_area = enabled;
        self
    }

    /// Renders an endpoint marker dot and inline callout annotation (`"<prefix>: <value>"`).
    pub fn endpoint_callout(mut self, prefix: impl Into<SharedString>) -> Self {
        self.endpoint_callout = Some(prefix.into());
        self
    }

    /// Linearly interpolates the series' `y` value at `target_x` in `O(log N)` time.
    ///
    /// Assumes `self.points` are sorted in non-decreasing `x` order. Returns `None` when
    /// `self.points` is empty or `target_x` falls outside the series' `[first.x, last.x]` domain.
    pub fn sample_at(&self, target_x: f64) -> Option<f64> {
        let points = &self.points;
        if points.is_empty() {
            return None;
        }
        let first = points[0];
        let last = *points.last()?;
        if target_x < first.0 - 1e-6 || target_x > last.0 + 1e-6 {
            return None;
        }
        if points.len() == 1 || target_x <= first.0 {
            return Some(first.1);
        }
        if target_x >= last.0 {
            return Some(last.1);
        }
        let idx = points.partition_point(|&(x, _)| x < target_x);
        if idx == 0 {
            return Some(first.1);
        }
        let (x0, y0) = points[idx - 1];
        let (x1, y1) = points[idx];
        let span = x1 - x0;
        if span.abs() < 1e-9 {
            return Some(y1);
        }
        let t = ((target_x - x0) / span).clamp(0.0, 1.0);
        Some(y0 + (y1 - y0) * t)
    }
}

struct CachedLineGeometry {
    key: u64,
    areas: Vec<(usize, Path)>,
    strokes: Vec<(usize, Path)>,
}

#[derive(Default)]
struct LineChartInteractionState {
    hover_x: Option<f64>,
    plot_bounds: Option<Bounds<Pixels>>,
    drag_anchor_x: Option<f32>,
    drag_current_x: Option<f32>,
    cached_geometry: Option<CachedLineGeometry>,
}

impl LineChartInteractionState {
    fn is_brushing(&self) -> bool {
        match (self.drag_anchor_x, self.drag_current_x) {
            (Some(a), Some(b)) => (b - a).abs() >= 4.0,
            _ => false,
        }
    }

    fn brushed_domain(&self, x_min: f64, x_max: f64) -> Option<(f64, f64)> {
        if !self.is_brushing() {
            return None;
        }
        let plot = PlotRect::from_bounds(self.plot_bounds?, LINE_CHART_MARGINS);
        let scale = LinearScale::new(x_min, x_max, plot.left, plot.right);
        let a = self.drag_anchor_x?.clamp(plot.left, plot.right);
        let b = self.drag_current_x?.clamp(plot.left, plot.right);
        let x0 = scale.invert(a.min(b)).clamp(x_min, x_max);
        let x1 = scale.invert(a.max(b)).clamp(x_min, x_max);
        Some((x0, x1))
    }
}

/// Computes an exact content + geometry cache key for tessellated line/area [`Path`]s.
fn compute_geometry_key(
    plot: PlotRect,
    x_min: f64,
    x_max: f64,
    nice_max: f64,
    smooth: bool,
    series: &[LineSeries],
) -> u64 {
    let mut hasher = FxHasher::default();
    plot.left.to_bits().hash(&mut hasher);
    plot.right.to_bits().hash(&mut hasher);
    plot.top.to_bits().hash(&mut hasher);
    plot.bottom.to_bits().hash(&mut hasher);
    x_min.to_bits().hash(&mut hasher);
    x_max.to_bits().hash(&mut hasher);
    nice_max.to_bits().hash(&mut hasher);
    smooth.hash(&mut hasher);
    series.len().hash(&mut hasher);
    for s in series {
        s.style.hash(&mut hasher);
        s.fill_area.hash(&mut hasher);
        s.stroke_width.map(f32::to_bits).hash(&mut hasher);
        s.points.len().hash(&mut hasher);
        for &(x, y) in &s.points {
            x.to_bits().hash(&mut hasher);
            y.to_bits().hash(&mut hasher);
        }
    }
    hasher.finish()
}

/// Preserves exact per-pixel-column min/max envelope for dense monotonic-X series so
/// Lyon tessellation and curve tile-binning stay `O(plot_width)` without missing spikes.
fn decimate_monotonic_points<'a>(points: &'a [(f64, f64)], max_buckets: usize) -> Cow<'a, [(f64, f64)]> {
    let buckets = max_buckets.max(64);
    if points.len() <= buckets.saturating_mul(2) || points.len() < 4 {
        return Cow::Borrowed(points);
    }
    let x_start = points[0].0;
    let x_end = points[points.len() - 1].0;
    let x_span = x_end - x_start;
    if x_span <= f64::EPSILON || !points.windows(2).all(|w| w[1].0 >= w[0].0) {
        return Cow::Borrowed(points);
    }

    let mut out = Vec::with_capacity(buckets * 2 + 2);
    let inv_bucket = (buckets as f64) / x_span;
    let mut i = 0;
    while i < points.len() {
        let bucket = (((points[i].0 - x_start) * inv_bucket).floor() as usize).min(buckets);
        let first_idx = i;
        let mut min_idx = i;
        let mut max_idx = i;
        let mut min_y = points[i].1;
        let mut max_y = points[i].1;
        let mut last_idx = i;
        i += 1;
        while i < points.len() {
            let b = (((points[i].0 - x_start) * inv_bucket).floor() as usize).min(buckets);
            if b != bucket {
                break;
            }
            let y = points[i].1;
            if y < min_y {
                min_y = y;
                min_idx = i;
            }
            if y > max_y {
                max_y = y;
                max_idx = i;
            }
            last_idx = i;
            i += 1;
        }

        // Emit up to 4 deduplicated indices in chronological order to preserve exact column envelope.
        let mut indices = [first_idx, min_idx, max_idx, last_idx];
        indices.sort_unstable();
        let mut prev = usize::MAX;
        for idx in indices {
            if idx != prev {
                out.push(points[idx]);
                prev = idx;
            }
        }
    }
    Cow::Owned(out)
}

fn build_line_geometry(
    key: u64,
    series: &[LineSeries],
    x_scale: LinearScale,
    y_scale: LinearScale,
    plot: PlotRect,
    smooth: bool,
) -> CachedLineGeometry {
    let max_buckets = (plot.width().ceil() as usize).clamp(128, 1200);
    let mut areas = Vec::new();
    let mut strokes = Vec::with_capacity(series.len());

    // 1. Area fill paths
    for (idx, s) in series.iter().enumerate() {
        if !s.fill_area || s.points.len() < 2 {
            continue;
        }
        let pts = decimate_monotonic_points(&s.points, max_buckets);
        let mapped: Vec<(f32, f32)> = pts
            .iter()
            .map(|&(x, y)| (x_scale.map(x), y_scale.map(y)))
            .collect();
        let first = mapped[0];
        let last = mapped[mapped.len() - 1];
        let mut pb = PathBuilder::fill();
        pb.move_to(point(px(first.0), px(plot.bottom)));
        pb.line_to(point(px(first.0), px(first.1)));
        super::sparkline::push_curve_points(&mut pb, &mapped, smooth, true);
        pb.line_to(point(px(last.0), px(plot.bottom)));
        pb.close();
        if let Ok(path) = pb.build() {
            areas.push((idx, path));
        }
    }

    // 2. Dashed series first, then solid series on top
    for pass_dashed in [true, false] {
        for (idx, s) in series.iter().enumerate() {
            let is_dashed = s.style == LineStyle::Dashed;
            if is_dashed != pass_dashed || s.points.len() < 2 {
                continue;
            }
            let pts = decimate_monotonic_points(&s.points, max_buckets);
            let mapped: Vec<(f32, f32)> = pts
                .iter()
                .map(|&(x, y)| (x_scale.map(x), y_scale.map(y)))
                .collect();
            let width = s.stroke_width.unwrap_or(if is_dashed { 1.75 } else { 2.25 });
            let mut pb = PathBuilder::stroke(px(width));
            if is_dashed {
                pb = pb.dash_array(&[px(5.0), px(4.0)]);
            }
            super::sparkline::push_curve_points(&mut pb, &mapped, smooth, false);
            if let Ok(path) = pb.build() {
                strokes.push((idx, path));
            }
        }
    }

    CachedLineGeometry {
        key,
        areas,
        strokes,
    }
}

/// GPU-native multi-series line chart with solid/dashed strokes, optional Fritsch–Carlson monotone
/// cubic Bezier smoothing (`.smooth(true)`), inline endpoint callout, interactive vertical
/// crosshair scrubber, and optional horizontal drag-to-select brushing.
#[derive(IntoElement)]
pub struct LineChart {
    id: ElementId,
    series: Vec<LineSeries>,
    x_domain: Option<(f64, f64)>,
    x_ticks: Vec<(f64, SharedString)>,
    height: Pixels,
    fill_height: bool,
    value_format: ChartValueFormat,
    palette: Option<ChartPalette>,
    smooth: bool,
    selectable: bool,
    selected_range: Option<(f64, f64)>,
    x_formatter: Option<Rc<dyn Fn(f64) -> String>>,
    style: StyleRefinement,
    on_brush_select: Option<Rc<dyn Fn(&(f64, f64), &mut Window, &mut App) + 'static>>,
    on_selection_clear: Option<Rc<dyn Fn(&(), &mut Window, &mut App) + 'static>>,
}

/// Convenience constructor for [`LineChart::new`].
pub fn line_chart(id: impl Into<ElementId>) -> LineChart {
    LineChart::new(id)
}

impl LineChart {
    /// Creates a new [`LineChart`].
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            series: Vec::new(),
            x_domain: None,
            x_ticks: Vec::new(),
            height: px(220.),
            fill_height: false,
            value_format: ChartValueFormat::Currency,
            palette: None,
            smooth: false,
            selectable: false,
            selected_range: None,
            x_formatter: None,
            style: StyleRefinement::default(),
            on_brush_select: None,
            on_selection_clear: None,
        }
    }

    /// Adds one [`LineSeries`].
    pub fn add_series(mut self, series: LineSeries) -> Self {
        self.series.push(series);
        self
    }

    /// Replaces the chart's series list.
    pub fn series(mut self, series: impl IntoIterator<Item = LineSeries>) -> Self {
        self.series = series.into_iter().collect();
        self
    }

    /// Sets explicit X-axis domain `[min, max]` (e.g. `(0.0, 24.0)` for a 24-hour chart).
    pub fn x_domain(mut self, min: f64, max: f64) -> Self {
        self.x_domain = Some((min, max));
        self
    }

    /// Sets explicit `(x_value, label)` X-axis ticks (e.g. `0.0 -> "00:00"`, `6.0 -> "06:00"`, ...).
    pub fn x_ticks(mut self, ticks: impl IntoIterator<Item = (f64, SharedString)>) -> Self {
        self.x_ticks = ticks.into_iter().collect();
        self
    }

    /// Sets the plot height (default `220px`) and disables [`Self::fill_height`].
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

    /// Sets the Y-axis & tooltip value formatter (default [`ChartValueFormat::Currency`]).
    pub fn value_format(mut self, format: ChartValueFormat) -> Self {
        self.value_format = format;
        self
    }

    /// Overrides the chart color palette (defaults to [`ChartPalette::from_theme`]).
    pub fn palette(mut self, palette: ChartPalette) -> Self {
        self.palette = Some(palette);
        self
    }

    /// Enables Fritsch–Carlson monotone cubic Bezier curve smoothing (zero overshoot).
    pub fn smooth(mut self, smooth: bool) -> Self {
        self.smooth = smooth;
        self
    }

    /// Enables or disables horizontal drag-brush selection across the X domain.
    pub fn selectable(mut self, selectable: bool) -> Self {
        self.selectable = selectable;
        self
    }

    /// Highlights a controlled X-domain span `(start_x, end_x)`.
    pub fn selected_range(mut self, range: Option<(f64, f64)>) -> Self {
        self.selected_range = range;
        self
    }

    /// Sets a custom formatter for the hovered/brushed X value shown in tooltips and range pills.
    pub fn x_formatter(mut self, formatter: impl Fn(f64) -> String + 'static) -> Self {
        self.x_formatter = Some(Rc::new(formatter));
        self
    }

    /// Registers a drag-selection callback invoked with the normalized `(start_x, end_x)` domain span.
    ///
    /// Automatically enables [`Self::selectable`] unless explicitly disabled afterward.
    pub fn on_brush_select(
        mut self,
        handler: impl Fn(&(f64, f64), &mut Window, &mut App) + 'static,
    ) -> Self {
        self.selectable = true;
        self.on_brush_select = Some(Rc::new(handler));
        self
    }

    /// Registers a callback invoked when the user clicks without dragging while [`Self::selected_range`] is active.
    pub fn on_selection_clear(
        mut self,
        handler: impl Fn(&(), &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_selection_clear = Some(Rc::new(handler));
        self
    }

    /// Builds matching [`ChartLegendItem`]s (solid vs dashed line swatches) for [`ChartCard::legend`](super::ChartCard::legend).
    pub fn legend_items(series: &[LineSeries], palette: &ChartPalette) -> Vec<ChartLegendItem> {
        let mut solid_idx = 0usize;
        series
            .iter()
            .map(|s| {
                let color = s.color.unwrap_or_else(|| match s.style {
                    LineStyle::Solid => {
                        let c = palette.color(solid_idx);
                        solid_idx += 1;
                        c
                    }
                    LineStyle::Dashed => palette.baseline,
                });
                match s.style {
                    LineStyle::Solid => ChartLegendItem::solid_line(s.label.clone(), color),
                    LineStyle::Dashed => ChartLegendItem::dashed_line(s.label.clone(), color),
                }
            })
            .collect()
    }
}

impl Styled for LineChart {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for LineChart {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let palette = self
            .palette
            .unwrap_or_else(|| ChartPalette::from_theme(&theme));

        let interaction_entity = window.use_keyed_state(
            (self.id.clone(), "line-chart-interaction"),
            cx,
            |_, _| Rc::new(RefCell::new(LineChartInteractionState::default())),
        );
        let interaction_rc = interaction_entity.read(cx).clone();

        let (x_min, x_max) = self.x_domain.unwrap_or_else(|| {
            let mut min_x = f64::INFINITY;
            let mut max_x = f64::NEG_INFINITY;
            for s in &self.series {
                for &(x, _) in &s.points {
                    min_x = min_x.min(x);
                    max_x = max_x.max(x);
                }
            }
            if !min_x.is_finite() || !max_x.is_finite() || (max_x - min_x).abs() < f64::EPSILON {
                (0.0, 1.0)
            } else {
                (min_x, max_x)
            }
        });

        let (hover_x, live_brush) = {
            let state = interaction_rc.borrow();
            (
                state.hover_x,
                if self.selectable {
                    state.brushed_domain(x_min, x_max)
                } else {
                    None
                },
            )
        };
        let active_selection = live_brush.or(self.selected_range);
        let has_controlled_selection = self.selected_range.is_some();

        let max_y = self
            .series
            .iter()
            .flat_map(|s| s.points.iter().map(|&(_, y)| y))
            .fold(0.0_f64, f64::max);
        let (nice_max, y_ticks) = nice_ticks(max_y, 5);

        let mut solid_idx = 0usize;
        let resolved_colors: Rc<[Hsla]> = self
            .series
            .iter()
            .map(|s| {
                s.color.unwrap_or_else(|| match s.style {
                    LineStyle::Solid => {
                        let c = palette.color(solid_idx);
                        solid_idx += 1;
                        c
                    }
                    LineStyle::Dashed => palette.baseline,
                })
            })
            .collect();

        // Share series & colors by Rc slice to avoid cloning thousands of points per hover frame.
        let series_rc: Rc<[LineSeries]> = Rc::from(self.series);
        let series_paint = series_rc.clone();
        let colors_paint = resolved_colors.clone();
        let y_ticks_paint = y_ticks;
        let x_ticks_paint = self.x_ticks;
        let format_paint = self.value_format.clone();
        let palette_paint = palette;
        let surface_bg = theme.surface;
        let accent_color = theme.primary;
        let interaction_prepaint = interaction_rc.clone();
        let interaction_paint = interaction_rc.clone();
        let is_live_brushing = live_brush.is_some();
        let smooth_paint = self.smooth;

        let plot_canvas = canvas(
            move |bounds, _, _| {
                interaction_prepaint.borrow_mut().plot_bounds = Some(bounds);
                bounds
            },
            move |bounds, _, window, cx| {
                let plot = PlotRect::from_bounds(bounds, LINE_CHART_MARGINS);
                let x_scale = LinearScale::new(x_min, x_max, plot.left, plot.right);
                let y_scale = LinearScale::new(0.0, nice_max, plot.bottom, plot.top);

                // 1. Horizontal gridlines & Y-axis labels
                paint_y_gridlines(
                    &y_ticks_paint,
                    y_scale,
                    plot,
                    bounds.origin.x.0,
                    LINE_CHART_MARGINS.left - 10.0,
                    &format_paint,
                    &palette_paint,
                    window,
                    cx,
                );

                // 2. Highlighted brush/selected X-domain band
                if let Some((a, b)) = active_selection {
                    paint_brush_band(
                        x_scale.map(a),
                        x_scale.map(b),
                        plot,
                        accent_color,
                        is_live_brushing,
                        window,
                    );
                }

                // 3. Vertical crosshair line if hovered (hidden while actively brushing)
                if !is_live_brushing
                    && let Some(hx) = hover_x
                {
                    let x_px = x_scale.map(hx).clamp(plot.left, plot.right);
                    window.paint_quad(fill(
                        Bounds::new(
                            point(px(x_px), px(plot.top)),
                            size(px(1.), px(plot.height())),
                        ),
                        palette_paint.gridline.opacity(1.0),
                    ));
                }

                // 4. Cached tessellated area & stroke paths (O(1) Arc clone on hover/brush frames)
                let geom_key =
                    compute_geometry_key(plot, x_min, x_max, nice_max, smooth_paint, &series_paint);
                {
                    let mut state = interaction_paint.borrow_mut();
                    let cache_hit = state
                        .cached_geometry
                        .as_ref()
                        .is_some_and(|g| g.key == geom_key);
                    if !cache_hit {
                        state.cached_geometry = Some(build_line_geometry(
                            geom_key,
                            &series_paint,
                            x_scale,
                            y_scale,
                            plot,
                            smooth_paint,
                        ));
                    }
                    if let Some(geom) = &state.cached_geometry {
                        for (idx, path) in &geom.areas {
                            let color = colors_paint[*idx];
                            window.paint_path(
                                path.clone(),
                                linear_gradient(
                                    180.0,
                                    linear_color_stop(color.opacity(0.16), 0.0),
                                    linear_color_stop(color.opacity(0.0), 1.0),
                                ),
                            );
                        }
                        for (idx, path) in &geom.strokes {
                            window.paint_path(path.clone(), colors_paint[*idx]);
                        }
                    }
                }

                // 5. Endpoint dot & clamped inline callout badge (e.g. "Today: $36.21")
                for (idx, s) in series_paint.iter().enumerate() {
                    let Some(prefix) = &s.endpoint_callout else {
                        continue;
                    };
                    let Some(&(lx, ly)) = s.points.last() else {
                        continue;
                    };
                    let color = colors_paint[idx];
                    let ex = x_scale.map(lx);
                    let ey = y_scale.map(ly);
                    paint_chart_marker(ex, ey, 4.0, color, surface_bg, window);

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
                        color,
                        FontWeight::SEMIBOLD,
                        TextAlign::Center,
                        Some(px(callout_w)),
                        window,
                        cx,
                    );
                }

                // 6. Hover marker dots on every series covering hover_x
                if !is_live_brushing
                    && let Some(hx) = hover_x
                {
                    let x_px = x_scale.map(hx).clamp(plot.left, plot.right);
                    for (idx, s) in series_paint.iter().enumerate() {
                        if let Some(hy) = s.sample_at(hx) {
                            paint_chart_marker(
                                x_px,
                                y_scale.map(hy),
                                3.5,
                                colors_paint[idx],
                                surface_bg,
                                window,
                            );
                        }
                    }
                }

                // 7. X-axis ticks
                let tick_count = x_ticks_paint.len();
                for (t_idx, (tx, label)) in x_ticks_paint.iter().enumerate() {
                    let x_px = x_scale.map(*tx);
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
            },
        )
        .flex_1()
        .min_h_0()
        .w_full();

        let selectable = self.selectable;
        let on_brush_up = self.on_brush_select.clone();
        let on_brush_up_out = self.on_brush_select;
        let on_clear = self.on_selection_clear;

        let down_entity = interaction_entity.clone();
        let move_entity = interaction_entity.clone();
        let up_entity = interaction_entity.clone();
        let up_out_entity = interaction_entity.clone();
        let leave_entity = interaction_entity;

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
            .when(selectable, |el| el.cursor(CursorStyle::Crosshair))
            .on_mouse_down(MouseButton::Left, move |event: &MouseDownEvent, _, cx| {
                if !selectable {
                    return;
                }
                let mut state = down_entity.read(cx).borrow_mut();
                let Some(bounds) = state.plot_bounds else {
                    return;
                };
                let plot = PlotRect::from_bounds(bounds, LINE_CHART_MARGINS);
                let mx = event.position.x.0;
                if plot.contains_x(mx) {
                    state.drag_anchor_x = Some(mx);
                    state.drag_current_x = Some(mx);
                    drop(state);
                    down_entity.update(cx, |_, cx| cx.notify());
                }
            })
            .on_mouse_move(move |event: &MouseMoveEvent, _, cx| {
                let changed = {
                    let mut state = move_entity.read(cx).borrow_mut();
                    let Some(bounds) = state.plot_bounds else {
                        return;
                    };
                    let plot = PlotRect::from_bounds(bounds, LINE_CHART_MARGINS);
                    let mx = event.position.x.0;
                    let mut dirty = false;

                    if selectable && state.drag_anchor_x.is_some() {
                        let clamped = mx.clamp(plot.left, plot.right);
                        if state
                            .drag_current_x
                            .is_none_or(|prev| (prev - clamped).abs() >= 1.0)
                        {
                            state.drag_current_x = Some(clamped);
                            dirty = true;
                        }
                    } else {
                        let next_x = if plot.contains_x(mx) {
                            let scale = LinearScale::new(x_min, x_max, plot.left, plot.right);
                            Some(scale.invert(mx).clamp(x_min, x_max))
                        } else {
                            None
                        };
                        let hover_changed = match (state.hover_x, next_x) {
                            (Some(a), Some(b)) => (a - b).abs() > (x_max - x_min) * 0.005,
                            (None, None) => false,
                            _ => true,
                        };
                        if hover_changed {
                            state.hover_x = next_x;
                            dirty = true;
                        }
                    }
                    dirty
                };
                if changed {
                    move_entity.update(cx, |_, cx| cx.notify());
                }
            })
            .on_mouse_up(MouseButton::Left, move |_: &MouseUpEvent, window, cx| {
                let (brushed, had_drag) = {
                    let mut state = up_entity.read(cx).borrow_mut();
                    let had = state.drag_anchor_x.is_some();
                    let brushed = if selectable {
                        state.brushed_domain(x_min, x_max)
                    } else {
                        None
                    };
                    state.drag_anchor_x = None;
                    state.drag_current_x = None;
                    (brushed, had)
                };
                if had_drag {
                    up_entity.update(cx, |_, cx| cx.notify());
                }
                if let Some(span) = brushed {
                    if let Some(cb) = &on_brush_up {
                        cb(&span, window, cx);
                    }
                } else if had_drag
                    && has_controlled_selection
                    && let Some(cb) = &on_clear
                {
                    cb(&(), window, cx);
                }
            })
            .on_mouse_up_out(MouseButton::Left, move |_: &MouseUpEvent, window, cx| {
                let (brushed, had_drag) = {
                    let mut state = up_out_entity.read(cx).borrow_mut();
                    let had = state.drag_anchor_x.is_some();
                    let brushed = if selectable {
                        state.brushed_domain(x_min, x_max)
                    } else {
                        None
                    };
                    state.drag_anchor_x = None;
                    state.drag_current_x = None;
                    (brushed, had)
                };
                if had_drag {
                    up_out_entity.update(cx, |_, cx| cx.notify());
                }
                if let Some(span) = brushed
                    && let Some(cb) = &on_brush_up_out
                {
                    cb(&span, window, cx);
                }
            })
            .on_hover(move |hovered, _, cx| {
                if !hovered {
                    let changed = {
                        let mut state = leave_entity.read(cx).borrow_mut();
                        state.hover_x.take().is_some()
                    };
                    if changed {
                        leave_entity.update(cx, |_, cx| cx.notify());
                    }
                }
            })
            .child(plot_canvas);

        let fmt_x = |v: f64| match &self.x_formatter {
            Some(f) => f(v),
            None => format!("{v:.1}"),
        };

        // Live drag-selection pill or floating crosshair tooltip
        if let Some((a, b)) = live_brush
            && let Some(bounds) = interaction_rc.borrow().plot_bounds
        {
            let local = PlotRect::from_local_bounds(bounds, LINE_CHART_MARGINS);
            let x_scale = LinearScale::new(x_min, x_max, local.left, local.right);
            let mid_x = (x_scale.map(a) + x_scale.map(b)) * 0.5;
            let delta_suffix = series_rc.first().and_then(|primary| {
                let y0 = primary.sample_at(a)?;
                let y1 = primary.sample_at(b)?;
                Some(format!(
                    " · Δ {}",
                    self.value_format.format_detail((y1 - y0).max(0.0))
                ))
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
        } else if let Some(hx) = hover_x
            && let Some(bounds) = interaction_rc.borrow().plot_bounds
        {
            let local = PlotRect::from_local_bounds(bounds, LINE_CHART_MARGINS);
            let x_scale = LinearScale::new(x_min, x_max, local.left, local.right);
            let rows = series_rc.iter().enumerate().filter_map(|(idx, s)| {
                let val = s.sample_at(hx)?;
                Some(ChartTooltipRow {
                    color: resolved_colors[idx],
                    label: s.label.clone(),
                    value: self.value_format.format_detail(val),
                })
            });
            container = container.child(render_chart_tooltip(
                fmt_x(hx),
                None,
                rows,
                x_scale.map(hx),
                local.left,
                bounds.size.width.0,
                210.0,
                px(10.),
                &theme,
            ));
        }

        crate::Refineable::refine(container.style(), &self.style);
        container
    }
}
