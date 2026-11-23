//! Compact GPU [`Sparkline`] and [`GroupedSparkline`] components plus Fritsch–Carlson
//! monotone cubic Bezier smoothing (`monotone` / `sample_monotone`).
//!
//! Designed for KPI cards, table cells, tray popovers, and always-on-top floating meters where
//! full axis margins don't fit. Supports `Area` (vertical alpha gradient), `Line`, `Bars`,
//! dashed comparison baselines, ringed live endpoint dots, adaptive tight Y autoscaling, and
//! external hover/scrub callbacks for driving detached tooltips.

use std::cell::RefCell;
use std::rc::Rc;

use crate::{
    App, BorderStyle, Bounds, Corners, CursorStyle, Edges, ElementId, Hsla, IntoElement,
    MouseMoveEvent, PathBuilder, Pixels, Point, RenderOnce, SharedString, StyleRefinement, Styled,
    Window, canvas, div, fill, linear_color_stop, linear_gradient, point, prelude::*, px, quad,
    size,
};

use super::core::ChartPalette;
use crate::components::theme::ActiveTheme;

/// One cubic Bezier segment `(p0 -> c1, c2 -> p1)` produced by [`monotone`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CubicSegment {
    /// Start point.
    pub p0: (f32, f32),
    /// First control point.
    pub c1: (f32, f32),
    /// Second control point.
    pub c2: (f32, f32),
    /// End point.
    pub p1: (f32, f32),
}

/// Computes Fritsch–Carlson monotone cubic Hermite segments passing cleanly through `pts`
/// (sorted in strictly increasing X) without overshooting local extrema or flat plateaus.
pub fn monotone(pts: &[(f32, f32)]) -> Vec<CubicSegment> {
    let n = pts.len();
    if n < 2 {
        return Vec::new();
    }
    if n == 2 {
        let (x0, y0) = pts[0];
        let (x1, y1) = pts[1];
        let dx = (x1 - x0) / 3.0;
        let dy = (y1 - y0) / 3.0;
        return vec![CubicSegment {
            p0: (x0, y0),
            c1: (x0 + dx, y0 + dy),
            c2: (x1 - dx, y1 - dy),
            p1: (x1, y1),
        }];
    }

    let mut dx = vec![0.0_f32; n - 1];
    let mut slope = vec![0.0_f32; n - 1];
    for i in 0..n - 1 {
        let d = (pts[i + 1].0 - pts[i].0).max(1e-5);
        dx[i] = d;
        slope[i] = (pts[i + 1].1 - pts[i].1) / d;
    }

    let mut tangents = vec![0.0_f32; n];
    tangents[0] = slope[0];
    tangents[n - 1] = slope[n - 2];

    for i in 1..n - 1 {
        if slope[i - 1] * slope[i] <= 0.0 {
            tangents[i] = 0.0;
        } else {
            let w1 = 2.0 * dx[i] + dx[i - 1];
            let w2 = dx[i] + 2.0 * dx[i - 1];
            tangents[i] = (w1 + w2) / (w1 / slope[i - 1] + w2 / slope[i]);
        }
    }

    // Clamp endpoint tangents so they don't overshoot first/last intervals.
    if tangents[0].abs() > 3.0 * slope[0].abs() {
        tangents[0] = 3.0 * slope[0];
    }
    if tangents[n - 1].abs() > 3.0 * slope[n - 2].abs() {
        tangents[n - 1] = 3.0 * slope[n - 2];
    }

    // Fritsch–Carlson monotonicity constraint on each interval.
    for i in 0..n - 1 {
        let s = slope[i];
        if s.abs() < 1e-6 {
            tangents[i] = 0.0;
            tangents[i + 1] = 0.0;
        } else {
            let alpha = tangents[i] / s;
            let beta = tangents[i + 1] / s;
            let hypot = (alpha * alpha + beta * beta).sqrt();
            if hypot > 3.0 {
                let tau = 3.0 / hypot;
                tangents[i] = tau * alpha * s;
                tangents[i + 1] = tau * beta * s;
            }
        }
    }

    let mut out = Vec::with_capacity(n - 1);
    for i in 0..n - 1 {
        let (x0, y0) = pts[i];
        let (x1, y1) = pts[i + 1];
        let h = dx[i] / 3.0;
        out.push(CubicSegment {
            p0: (x0, y0),
            c1: (x0 + h, y0 + tangents[i] * h),
            c2: (x1 - h, y1 - tangents[i + 1] * h),
            p1: (x1, y1),
        });
    }
    out
}

/// Evaluates the Y coordinate of a monotone cubic spline produced by [`monotone`] at `x`.
pub fn sample_monotone(segments: &[CubicSegment], x: f32) -> Option<f32> {
    let first = segments.first()?;
    let last = segments.last()?;
    if x <= first.p0.0 {
        return Some(first.p0.1);
    }
    if x >= last.p1.0 {
        return Some(last.p1.1);
    }
    let idx = segments
        .partition_point(|seg| seg.p1.0 < x)
        .min(segments.len() - 1);
    let seg = segments[idx];
    let span = (seg.p1.0 - seg.p0.0).max(1e-5);
    let t = ((x - seg.p0.0) / span).clamp(0.0, 1.0);
    let mt = 1.0 - t;
    let y = mt * mt * mt * seg.p0.1
        + 3.0 * mt * mt * t * seg.c1.1
        + 3.0 * mt * t * t * seg.c2.1
        + t * t * t * seg.p1.1;
    Some(y)
}

/// Computes an adaptive Y domain `(min, max)` for sparklines.
///
/// - When `tight_range` is `false` (additive metrics like spend/requests), anchors at `0.0`.
/// - When `tight_range` is `true` (ratios, latencies, temperatures), pads tightly around the
///   observed `[min, max]` so subtle changes remain visually legible.
pub fn compute_sparkline_y_range(
    values: impl Iterator<Item = f64>,
    tight_range: bool,
    clamp_unit: bool,
) -> (f64, f64) {
    let mut min_v = f64::INFINITY;
    let mut max_v = f64::NEG_INFINITY;
    for v in values {
        if v.is_finite() {
            min_v = min_v.min(v);
            max_v = max_v.max(v);
        }
    }
    if !min_v.is_finite() || !max_v.is_finite() {
        return (0.0, 1.0);
    }
    if !tight_range {
        let hi = max_v.max(1e-6) * 1.08;
        let lo = min_v.min(0.0);
        return (lo, if (hi - lo).abs() < 1e-6 { lo + 1.0 } else { hi });
    }
    let span = (max_v - min_v).abs();
    let pad = if span < 1e-6 {
        (max_v.abs() * 0.1).max(0.05)
    } else {
        span * 0.16
    };
    let mut lo = min_v - pad;
    let mut hi = max_v + pad;
    if clamp_unit {
        lo = lo.clamp(0.0, 1.0);
        hi = hi.clamp(0.0, 1.0);
        if (hi - lo) < 0.02 {
            lo = (min_v - 0.02).clamp(0.0, 0.98);
            hi = (lo + 0.04).min(1.0);
        }
    }
    (lo, hi.max(lo + 1e-6))
}

/// Visual presentation mode for a [`Sparkline`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SparklineKind {
    /// Smooth curve with soft vertical alpha gradient fill underneath (default).
    #[default]
    Area,
    /// Stroke-only curve with no area fill.
    Line,
    /// Compact vertical columns with subtle rounded top corners.
    Bars,
}

/// Hover sample emitted while scrubbing across a [`Sparkline`].
#[derive(Clone, Debug, PartialEq)]
pub struct SparklineHover {
    /// Sample index in the series.
    pub index: usize,
    /// Primary value at `index`.
    pub value: f64,
    /// Optional baseline comparison value at `index`.
    pub baseline: Option<f64>,
    /// Pointer position in window logical coordinates.
    pub position: Point<Pixels>,
}

#[derive(Default)]
struct SparklineState {
    hover_idx: Option<usize>,
    bounds: Option<Bounds<Pixels>>,
}

/// Compact, axis-less GPU sparkline for KPI cards, table cells, and floating panes.
#[derive(IntoElement)]
pub struct Sparkline {
    id: ElementId,
    values: Vec<f64>,
    baseline: Option<Vec<f64>>,
    kind: SparklineKind,
    smooth: bool,
    tight_range: bool,
    clamp_unit: bool,
    show_endpoint: bool,
    show_grid: bool,
    color: Option<Hsla>,
    baseline_color: Option<Hsla>,
    stroke_width: f32,
    height: Pixels,
    style: StyleRefinement,
    on_hover_sample: Option<Rc<dyn Fn(Option<SparklineHover>, &mut Window, &mut App) + 'static>>,
}

/// Convenience constructor for [`Sparkline::new`].
pub fn sparkline(id: impl Into<ElementId>, values: impl Into<Vec<f64>>) -> Sparkline {
    Sparkline::new(id, values)
}

impl Sparkline {
    /// Creates a new [`Sparkline`] from ordered numeric samples.
    pub fn new(id: impl Into<ElementId>, values: impl Into<Vec<f64>>) -> Self {
        Self {
            id: id.into(),
            values: values.into(),
            baseline: None,
            kind: SparklineKind::Area,
            smooth: true,
            tight_range: false,
            clamp_unit: false,
            show_endpoint: true,
            show_grid: false,
            color: None,
            baseline_color: None,
            stroke_width: 2.0,
            height: px(44.),
            style: StyleRefinement::default(),
            on_hover_sample: None,
        }
    }

    /// Sets the visual mode (`Area`, `Line`, or `Bars`).
    pub fn kind(mut self, kind: SparklineKind) -> Self {
        self.kind = kind;
        self
    }

    /// Enables or disables Fritsch–Carlson monotone cubic Bezier smoothing (default `true`).
    pub fn smooth(mut self, smooth: bool) -> Self {
        self.smooth = smooth;
        self
    }

    /// Uses tight Y-domain autoscaling instead of zero-anchored scaling (ideal for ratios/latencies).
    pub fn tight_range(mut self, tight: bool) -> Self {
        self.tight_range = tight;
        self
    }

    /// Clamps tight Y-domain bounds to `[0.0, 1.0]` (ideal for 0..1 percentages/rates).
    pub fn clamp_unit(mut self, clamp: bool) -> Self {
        self.clamp_unit = clamp;
        self
    }

    /// Adds a dashed comparison baseline series of matching length.
    pub fn baseline(mut self, baseline: impl Into<Vec<f64>>) -> Self {
        self.baseline = Some(baseline.into());
        self
    }

    /// Overrides the primary stroke/bar color.
    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    /// Overrides the dashed baseline color.
    pub fn baseline_color(mut self, color: Hsla) -> Self {
        self.baseline_color = Some(color);
        self
    }

    /// Sets stroke thickness in logical pixels (default `2.0`).
    pub fn stroke_width(mut self, width: f32) -> Self {
        self.stroke_width = width.clamp(1.0, 6.0);
        self
    }

    /// Toggles the ringed live endpoint dot on `Area`/`Line` sparklines (default `true`).
    pub fn show_endpoint(mut self, show: bool) -> Self {
        self.show_endpoint = show;
        self
    }

    /// Toggles subtle vertical guide lines (default `false`).
    pub fn show_grid(mut self, show: bool) -> Self {
        self.show_grid = show;
        self
    }

    /// Sets the chart height (default `44px`).
    pub fn height(mut self, height: Pixels) -> Self {
        self.height = height;
        self
    }

    /// Registers a hover/scrub callback invoked whenever the hovered sample changes (or `None` on leave).
    pub fn on_hover_sample(
        mut self,
        handler: impl Fn(Option<SparklineHover>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_hover_sample = Some(Rc::new(handler));
        self
    }
}

impl Styled for Sparkline {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Sparkline {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let primary = self.color.unwrap_or(theme.primary);
        let baseline_color = self.baseline_color.unwrap_or(theme.chart_baseline);
        let grid_color = theme.chart_gridline;
        let ring_bg = theme.surface;

        let state = window.use_keyed_state(
            (self.id.clone(), "sparkline-state"),
            cx,
            |_, _| Rc::new(RefCell::new(SparklineState::default())),
        );
        let state_rc = state.read(cx).clone();

        let values = Rc::new(self.values);
        let baseline = self.baseline.map(Rc::new);
        let kind = self.kind;
        let smooth = self.smooth;
        let tight_range = self.tight_range;
        let clamp_unit = self.clamp_unit;
        let show_endpoint = self.show_endpoint;
        let show_grid = self.show_grid;
        let stroke_width = self.stroke_width;

        let state_paint = state_rc.clone();
        let values_paint = values.clone();
        let baseline_paint = baseline.clone();

        let plot_canvas = canvas(
            move |bounds, _, _| {
                state_paint.borrow_mut().bounds = Some(bounds);
                bounds
            },
            move |bounds, _, window, _cx| {
                let n = values_paint.len();
                if n == 0 || bounds.size.width <= px(2.) || bounds.size.height <= px(2.) {
                    return;
                }
                let pad_x = 4.0_f32;
                let pad_y = 4.0_f32;
                let left = f32::from(bounds.origin.x) + pad_x;
                let right = f32::from(bounds.right()) - pad_x;
                let top = f32::from(bounds.origin.y) + pad_y;
                let bottom = f32::from(bounds.bottom()) - pad_y;
                let plot_w = (right - left).max(1.0);
                let plot_h = (bottom - top).max(1.0);

                if show_grid && n >= 4 {
                    for step in 1..4 {
                        let gx = left + plot_w * (step as f32 / 4.0);
                        window.paint_quad(quad(
                            Bounds::new(point(px(gx), px(top)), size(px(1.), px(plot_h))),
                            Corners::default(),
                            grid_color.opacity(0.6),
                            Edges::default(),
                            crate::transparent_black(),
                            BorderStyle::Solid,
                        ));
                    }
                }

                let iter_all = values_paint.iter().copied().chain(
                    baseline_paint
                        .as_ref()
                        .into_iter()
                        .flat_map(|b| b.iter().copied()),
                );
                let (y_min, y_max) = compute_sparkline_y_range(iter_all, tight_range, clamp_unit);
                let map_y = |v: f64| -> f32 {
                    let frac = ((v - y_min) / (y_max - y_min)).clamp(0.0, 1.0) as f32;
                    bottom - frac * plot_h
                };
                let map_x = |i: usize| -> f32 {
                    if n <= 1 {
                        left + plot_w * 0.5
                    } else {
                        left + (i as f32 / (n - 1) as f32) * plot_w
                    }
                };

                let hover_idx = state_rc.borrow().hover_idx;

                if kind == SparklineKind::Bars {
                    let slot_w = plot_w / n as f32;
                    let gap = (slot_w * 0.22).clamp(1.0, 4.0);
                    let bar_w = (slot_w - gap).max(1.5);
                    for (i, &val) in values_paint.iter().enumerate() {
                        let bx = left + i as f32 * slot_w + gap * 0.5;
                        let by = map_y(val);
                        let bh = (bottom - by).max(1.5);
                        let is_hovered = hover_idx == Some(i);
                        let bar_color = if is_hovered {
                            primary
                        } else if hover_idx.is_some() {
                            primary.opacity(0.55)
                        } else {
                            primary.opacity(0.88)
                        };
                        let r = (bar_w * 0.25).min(3.0);
                        window.paint_quad(quad(
                            Bounds::new(point(px(bx), px(bottom - bh)), size(px(bar_w), px(bh))),
                            Corners {
                                top_left: px(r),
                                top_right: px(r),
                                bottom_right: px(0.),
                                bottom_left: px(0.),
                            },
                            bar_color,
                            Edges::default(),
                            crate::transparent_black(),
                            BorderStyle::Solid,
                        ));
                    }
                    return;
                }

                // Dashed baseline comparison series (under primary curve).
                if let Some(ref base) = baseline_paint
                    && base.len() >= 2
                {
                    let base_pts: Vec<(f32, f32)> = base
                        .iter()
                        .enumerate()
                        .map(|(i, &v)| {
                            let x = left + (i as f32 / (base.len() - 1) as f32) * plot_w;
                            (x, map_y(v))
                        })
                        .collect();
                    let mut pb = PathBuilder::stroke(px(1.25)).dash_array(&[px(4.), px(3.)]);
                    push_curve_points(&mut pb, &base_pts, smooth, false);
                    if let Ok(path) = pb.build() {
                        window.paint_path(path, baseline_color.opacity(0.75));
                    }
                }

                let pts: Vec<(f32, f32)> = values_paint
                    .iter()
                    .enumerate()
                    .map(|(i, &v)| (map_x(i), map_y(v)))
                    .collect();

                if n >= 2 {
                    if kind == SparklineKind::Area {
                        let mut pb = PathBuilder::fill();
                        pb.move_to(point(px(pts[0].0), px(bottom)));
                        pb.line_to(point(px(pts[0].0), px(pts[0].1)));
                        push_curve_points(&mut pb, &pts, smooth, true);
                        pb.line_to(point(px(pts[n - 1].0), px(bottom)));
                        pb.close();
                        if let Ok(path) = pb.build() {
                            window.paint_path(
                                path,
                                linear_gradient(
                                    180.0,
                                    linear_color_stop(primary.opacity(0.32), 0.0),
                                    linear_color_stop(primary.opacity(0.02), 1.0),
                                ),
                            );
                        }
                    }

                    let mut pb = PathBuilder::stroke(px(stroke_width));
                    push_curve_points(&mut pb, &pts, smooth, false);
                    if let Ok(path) = pb.build() {
                        window.paint_path(path, primary);
                    }
                }

                // Hover crosshair + highlighted sample dot, or resting ringed live endpoint dot.
                if let Some(h_idx) = hover_idx.filter(|&i| i < n) {
                    let (hx, hy) = pts[h_idx];
                    window.paint_quad(quad(
                        Bounds::new(point(px(hx - 0.5), px(top)), size(px(1.), px(plot_h))),
                        Corners::default(),
                        primary.opacity(0.35),
                        Edges::default(),
                        crate::transparent_black(),
                        BorderStyle::Solid,
                    ));
                    paint_ringed_dot(window, hx, hy, 4.0, primary, ring_bg);
                } else if show_endpoint && let Some(&(ex, ey)) = pts.last() {
                    paint_ringed_dot(window, ex, ey, 3.25, primary, ring_bg);
                }
            },
        )
        .size_full();

        let state_move = state.read(cx).clone();
        let state_leave = state.read(cx).clone();
        let values_move = values.clone();
        let baseline_move = baseline.clone();
        let on_hover_move = self.on_hover_sample.clone();
        let on_hover_leave = self.on_hover_sample;

        let mut container = div()
            .id(self.id)
            .w_full()
            .h(self.height)
            .cursor(CursorStyle::Crosshair)
            .on_mouse_move(move |ev: &MouseMoveEvent, window, cx| {
                let n = values_move.len();
                if n == 0 {
                    return;
                }
                let bounds = state_move.borrow().bounds;
                let Some(bounds) = bounds else { return };
                let left = f32::from(bounds.origin.x) + 4.0;
                let width = (f32::from(bounds.size.width) - 8.0).max(1.0);
                let rel = ((f32::from(ev.position.x) - left) / width).clamp(0.0, 1.0);
                let idx = if n == 1 {
                    0
                } else {
                    (rel * (n - 1) as f32).round() as usize
                }
                .min(n - 1);

                let changed = state_move.borrow().hover_idx != Some(idx);
                if changed {
                    state_move.borrow_mut().hover_idx = Some(idx);
                    window.refresh();
                }
                if let Some(ref cb) = on_hover_move {
                    cb(
                        Some(SparklineHover {
                            index: idx,
                            value: values_move[idx],
                            baseline: baseline_move.as_ref().and_then(|b| b.get(idx).copied()),
                            position: ev.position,
                        }),
                        window,
                        cx,
                    );
                }
            })
            .on_hover(move |hovered, window, cx| {
                if !*hovered {
                    if state_leave.borrow_mut().hover_idx.take().is_some() {
                        window.refresh();
                    }
                    if let Some(ref cb) = on_hover_leave {
                        cb(None, window, cx);
                    }
                }
            })
            .child(plot_canvas);

        crate::Refineable::refine(container.style(), &self.style);
        container
    }
}

/// One series inside a [`GroupedSparkline`].
#[derive(Clone, Debug)]
pub struct GroupedSparklineSeries {
    /// Series label.
    pub label: SharedString,
    /// Ordered values across shared buckets.
    pub values: Vec<f64>,
    /// Optional explicit color override.
    pub color: Option<Hsla>,
}

impl GroupedSparklineSeries {
    /// Creates a new [`GroupedSparklineSeries`].
    pub fn new(label: impl Into<SharedString>, values: impl Into<Vec<f64>>) -> Self {
        Self {
            label: label.into(),
            values: values.into(),
            color: None,
        }
    }

    /// Overrides the series color.
    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }
}

/// Compact multi-series GPU sparkline supporting stacked bars or overlaid monotone smooth lines.
#[derive(IntoElement)]
pub struct GroupedSparkline {
    id: ElementId,
    series: Vec<GroupedSparklineSeries>,
    stacked_bars: bool,
    smooth: bool,
    height: Pixels,
    palette: Option<ChartPalette>,
    style: StyleRefinement,
}

/// Convenience constructor for [`GroupedSparkline::new`].
pub fn grouped_sparkline(
    id: impl Into<ElementId>,
    series: impl Into<Vec<GroupedSparklineSeries>>,
) -> GroupedSparkline {
    GroupedSparkline::new(id, series)
}

impl GroupedSparkline {
    /// Creates a new [`GroupedSparkline`] (defaults to stacked bars).
    pub fn new(
        id: impl Into<ElementId>,
        series: impl Into<Vec<GroupedSparklineSeries>>,
    ) -> Self {
        Self {
            id: id.into(),
            series: series.into(),
            stacked_bars: true,
            smooth: true,
            height: px(48.),
            palette: None,
            style: StyleRefinement::default(),
        }
    }

    /// Switches between stacked bars (`true`) and overlaid lines (`false`).
    pub fn stacked_bars(mut self, stacked: bool) -> Self {
        self.stacked_bars = stacked;
        self
    }

    /// Toggles Fritsch–Carlson monotone smoothing when rendering lines.
    pub fn smooth(mut self, smooth: bool) -> Self {
        self.smooth = smooth;
        self
    }

    /// Sets height (default `48px`).
    pub fn height(mut self, height: Pixels) -> Self {
        self.height = height;
        self
    }

    /// Overrides the categorical color palette.
    pub fn palette(mut self, palette: ChartPalette) -> Self {
        self.palette = Some(palette);
        self
    }
}

impl Styled for GroupedSparkline {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for GroupedSparkline {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let palette = self
            .palette
            .unwrap_or_else(|| ChartPalette::from_theme(&theme));
        let series = self.series;
        let stacked_bars = self.stacked_bars;
        let smooth = self.smooth;

        let plot = canvas(
            |bounds, _, _| bounds,
            move |bounds, _, window, _| {
                let bucket_count = series.iter().map(|s| s.values.len()).max().unwrap_or(0);
                if bucket_count == 0
                    || series.is_empty()
                    || bounds.size.width <= px(2.)
                    || bounds.size.height <= px(2.)
                {
                    return;
                }
                let pad = 3.0_f32;
                let left = f32::from(bounds.origin.x) + pad;
                let right = f32::from(bounds.right()) - pad;
                let top = f32::from(bounds.origin.y) + pad;
                let bottom = f32::from(bounds.bottom()) - pad;
                let plot_w = (right - left).max(1.0);
                let plot_h = (bottom - top).max(1.0);

                if stacked_bars {
                    let mut totals = vec![0.0_f64; bucket_count];
                    for s in &series {
                        for (i, &v) in s.values.iter().enumerate() {
                            if v.is_finite() && v > 0.0 {
                                totals[i] += v;
                            }
                        }
                    }
                    let max_total = totals.iter().copied().fold(0.0_f64, f64::max).max(1e-6);
                    let slot_w = plot_w / bucket_count as f32;
                    let gap = (slot_w * 0.2).clamp(1.0, 3.5);
                    let bar_w = (slot_w - gap).max(1.5);

                    for bucket in 0..bucket_count {
                        let bx = left + bucket as f32 * slot_w + gap * 0.5;
                        let mut acc_y = bottom;
                        for (s_idx, s) in series.iter().enumerate() {
                            let val = s.values.get(bucket).copied().unwrap_or(0.0).max(0.0);
                            if val <= 0.0 {
                                continue;
                            }
                            let seg_h = ((val / max_total) as f32 * plot_h).max(1.0);
                            let seg_top = (acc_y - seg_h).max(top);
                            let actual_h = (acc_y - seg_top).max(1.0);
                            let color = s.color.unwrap_or_else(|| palette.color(s_idx));
                            window.paint_quad(fill(
                                Bounds::new(
                                    point(px(bx), px(seg_top)),
                                    size(px(bar_w), px(actual_h)),
                                ),
                                color,
                            ));
                            acc_y = seg_top;
                        }
                    }
                } else {
                    let all_vals = series.iter().flat_map(|s| s.values.iter().copied());
                    let (y_min, y_max) = compute_sparkline_y_range(all_vals, true, false);
                    for (s_idx, s) in series.iter().enumerate() {
                        if s.values.len() < 2 {
                            continue;
                        }
                        let color = s.color.unwrap_or_else(|| palette.color(s_idx));
                        let pts: Vec<(f32, f32)> = s
                            .values
                            .iter()
                            .enumerate()
                            .map(|(i, &v)| {
                                let x = left
                                    + (i as f32 / (s.values.len() - 1) as f32) * plot_w;
                                let frac = ((v - y_min) / (y_max - y_min)).clamp(0.0, 1.0) as f32;
                                (x, bottom - frac * plot_h)
                            })
                            .collect();
                        let mut pb = PathBuilder::stroke(px(1.75));
                        push_curve_points(&mut pb, &pts, smooth, false);
                        if let Ok(path) = pb.build() {
                            window.paint_path(path, color);
                        }
                    }
                }
            },
        )
        .size_full();

        let mut container = div().id(self.id).w_full().h(self.height).child(plot);
        crate::Refineable::refine(container.style(), &self.style);
        container
    }
}

pub(crate) fn push_curve_points(
    pb: &mut PathBuilder,
    pts: &[(f32, f32)],
    smooth: bool,
    already_moved: bool,
) {
    if pts.is_empty() {
        return;
    }
    if !already_moved {
        pb.move_to(point(px(pts[0].0), px(pts[0].1)));
    }
    if smooth && pts.len() >= 2 {
        for seg in monotone(pts) {
            pb.cubic_bezier_to(
                point(px(seg.p1.0), px(seg.p1.1)),
                point(px(seg.c1.0), px(seg.c1.1)),
                point(px(seg.c2.0), px(seg.c2.1)),
            );
        }
    } else {
        for &(x, y) in &pts[1..] {
            pb.line_to(point(px(x), px(y)));
        }
    }
}

fn paint_ringed_dot(window: &mut Window, x: f32, y: f32, radius: f32, fill_color: Hsla, ring_bg: Hsla) {
    let outer = radius + 1.5;
    window.paint_quad(quad(
        Bounds::new(
            point(px(x - outer), px(y - outer)),
            size(px(outer * 2.0), px(outer * 2.0)),
        ),
        Corners::all(px(outer)),
        ring_bg,
        Edges::default(),
        crate::transparent_black(),
        BorderStyle::Solid,
    ));
    window.paint_quad(quad(
        Bounds::new(
            point(px(x - radius), px(y - radius)),
            size(px(radius * 2.0), px(radius * 2.0)),
        ),
        Corners::all(px(radius)),
        fill_color,
        Edges::default(),
        crate::transparent_black(),
        BorderStyle::Solid,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monotone_cubic_never_overshoots_step_plateau() {
        let pts = [(0.0, 0.0), (10.0, 0.0), (20.0, 100.0), (30.0, 100.0)];
        let segs = monotone(&pts);
        assert_eq!(segs.len(), 3);
        for x_step in 0..=300 {
            let x = x_step as f32 * 0.1;
            let y = sample_monotone(&segs, x).unwrap();
            assert!(
                (-0.01..=100.01).contains(&y),
                "overshoot at x={x}: y={y}"
            );
        }
    }

    #[test]
    fn tight_unit_range_keeps_high_ratios_legible() {
        let (lo, hi) = compute_sparkline_y_range([0.97, 0.985, 0.992, 0.98].into_iter(), true, true);
        assert!(lo >= 0.95 && hi <= 1.0);
        assert!(hi > lo);
    }
}
