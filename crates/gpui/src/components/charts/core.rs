//! Shared scales, nice-tick generation, Fritsch–Carlson monotone splines, color palettes,
//! swatch & tooltip rendering, GPU text/marker/grid/brush painting, and [`ChartCard`] framing
//! for `gpui::components::charts`.

use std::borrow::Cow;
use std::rc::Rc;

use crate::icons::{Icon, IconSize, LucideIcon};
use crate::{
    AnyElement, App, BorderStyle, Bounds, ClickEvent, Corners, Edges, ElementId, FontWeight, Hsla,
    IntoElement, ParentElement, PathBuilder, Pixels, Point, RenderOnce, SharedString,
    StyleRefinement, Styled, TextAlign, TextRun, Window, div, fill, point, prelude::*, px, quad,
    size,
};

use crate::components::palette;
use crate::components::theme::{ActiveTheme, Theme, ThemeMode, tabular_numerals};

/// Categorical series color palette automatically tuned for Light and Dark themes.
#[derive(Clone, Debug)]
pub struct ChartPalette {
    /// Ordered categorical colors for chart series (indexed modulo length).
    pub colors: Vec<Hsla>,
    /// Muted neutral baseline stroke (e.g. `"Previous 30 days"`).
    pub baseline: Hsla,
    /// Hairline horizontal gridline stroke.
    pub gridline: Hsla,
    /// Axis label text color.
    pub axis_label: Hsla,
    /// Vertical hover highlight / crosshair color.
    pub crosshair: Hsla,
}

impl ChartPalette {
    /// Derives a chart palette directly from [`Theme`] tokens (`chart_colors`, `chart_baseline`,
    /// `chart_gridline`, `muted_foreground`, and mode-appropriate hover crosshair).
    pub fn from_theme(theme: &Theme) -> Self {
        let colors = if theme.chart_colors.is_empty() {
            vec![theme.primary]
        } else {
            theme.chart_colors.clone()
        };
        let crosshair = match theme.mode {
            ThemeMode::Light => Hsla::from(palette::SLATE_900).opacity(0.08),
            ThemeMode::Dark => Hsla::from(palette::WHITE).opacity(0.07),
        };
        Self {
            colors,
            baseline: theme.chart_baseline,
            gridline: theme.chart_gridline,
            axis_label: theme.muted_foreground,
            crosshair,
        }
    }

    /// Returns the series color at `index` (wrapping safely).
    pub fn color(&self, index: usize) -> Hsla {
        self.colors[index % self.colors.len()]
    }
}

/// Built-in and custom formatters for axis tick labels and tooltips.
#[derive(Clone)]
pub enum ChartValueFormat {
    /// USD currency formatting (`$0.00`, `$20`, `$1.2K`).
    Currency,
    /// Compact SI suffixes (`0`, `420`, `19.7K`, `36.8M`).
    Compact,
    /// Plain integer rounding (`0`, `10`, `250`).
    Integer,
    /// Percentage formatting (`0%`, `58.0%`).
    Percent,
    /// Caller-supplied formatter closure.
    Custom(Rc<dyn Fn(f64) -> String>),
}

impl ChartValueFormat {
    /// Formats a value for an axis tick (compact, clean rounding).
    pub fn format_tick(&self, value: f64) -> String {
        match self {
            Self::Currency => {
                if value.abs() < 0.005 {
                    "$0.00".to_string()
                } else if value.abs() >= 1_000_000.0 {
                    format!("${:.1}M", value / 1_000_000.0)
                } else if value.abs() >= 10_000.0 {
                    format!("${:.0}K", value / 1_000.0)
                } else if (value.fract()).abs() < 0.01 {
                    format!("${:.0}", value)
                } else {
                    format!("${:.2}", value)
                }
            }
            Self::Compact => format_compact(value),
            Self::Integer => format!("{:.0}", value.round()),
            Self::Percent => format!("{:.0}%", value),
            Self::Custom(f) => f(value),
        }
    }

    /// Formats a value for tooltips and inline callouts (higher precision).
    pub fn format_detail(&self, value: f64) -> String {
        match self {
            Self::Currency => format!("${value:.2}"),
            Self::Compact => format_compact(value),
            Self::Integer => format!("{:.0}", value.round()),
            Self::Percent => format!("{value:.1}%"),
            Self::Custom(f) => f(value),
        }
    }
}

/// Formats numbers compactly (`36.8M`, `19.7K`, `420`).
pub fn format_compact(value: f64) -> String {
    let abs = value.abs();
    if abs >= 1_000_000_000.0 {
        format!("{:.1}B", value / 1_000_000_000.0)
    } else if abs >= 1_000_000.0 {
        format!("{:.1}M", value / 1_000_000.0)
    } else if abs >= 1_000.0 {
        format!("{:.1}K", value / 1_000.0)
    } else if (value.fract()).abs() < 0.01 {
        format!("{:.0}", value)
    } else {
        format!("{value:.1}")
    }
}

fn nice_step(span: f64, target_ticks: usize) -> f64 {
    let target = target_ticks.max(2) as f64;
    let rough_step = span / (target - 1.0);
    let exponent = rough_step.log10().floor();
    let magnitude = 10f64.powf(exponent);
    let normalized = rough_step / magnitude;
    let nice_fraction = if normalized <= 1.0 {
        1.0
    } else if normalized <= 2.0 {
        2.0
    } else if normalized <= 2.5 {
        2.5
    } else if normalized <= 5.0 {
        5.0
    } else {
        10.0
    };
    nice_fraction * magnitude
}

/// Computes visually clean axis ticks spanning `0.0..=nice_max` (Heckbert nice-numbers algorithm).
pub fn nice_ticks(max_value: f64, target_ticks: usize) -> (f64, Vec<f64>) {
    if !max_value.is_finite() || max_value <= 0.0 {
        return (1.0, vec![0.0, 0.5, 1.0]);
    }
    let step = nice_step(max_value, target_ticks);
    let nice_max = (max_value / step).ceil() * step;

    let mut ticks = Vec::new();
    let mut v = 0.0;
    let eps = step * 1e-4;
    while v <= nice_max + eps && ticks.len() < 16 {
        ticks.push(if v.abs() < eps { 0.0 } else { v });
        v += step;
    }
    (nice_max.max(step), ticks)
}

/// Computes visually clean ticks covering `[min_value, max_value]`.
///
/// When `snap_bounds` is `true`, expands `[min_value, max_value]` outward to clean multiples of
/// the chosen nice step; when `false`, preserves the exact caller-specified domain endpoints and
/// places evenly spaced interior ticks.
pub fn nice_ticks_range(
    min_value: f64,
    max_value: f64,
    target_ticks: usize,
    snap_bounds: bool,
) -> (f64, f64, Vec<f64>) {
    if !min_value.is_finite() || !max_value.is_finite() || max_value <= min_value {
        return (0.0, 1.0, vec![0.0, 0.5, 1.0]);
    }
    let span = max_value - min_value;
    if !snap_bounds {
        let count = target_ticks.max(2);
        let step = span / (count - 1) as f64;
        let ticks = (0..count)
            .map(|i| {
                let v = if i + 1 == count {
                    max_value
                } else {
                    min_value + step * i as f64
                };
                if v.abs() < span * 1e-6 { 0.0 } else { v }
            })
            .collect();
        return (min_value, max_value, ticks);
    }
    let step = nice_step(span, target_ticks);
    let nice_min = (min_value / step).floor() * step;
    let nice_max = (max_value / step).ceil() * step;
    let mut ticks = Vec::new();
    let mut v = nice_min;
    let eps = step * 1e-4;
    while v <= nice_max + eps && ticks.len() < 16 {
        ticks.push(if v.abs() < eps { 0.0 } else { v });
        v += step;
    }
    (nice_min, nice_max.max(nice_min + step), ticks)
}

/// Computes an adaptive Y domain `(min, max)` for sparklines and tight-range plots.
///
/// - When `tight_range` is `false` (additive metrics like spend/requests), anchors at `0.0`.
/// - When `tight_range` is `true` (ratios, latencies, temperatures), pads tightly around the
///   observed `[min, max]` so subtle changes remain visually legible, optionally clamped to `clamp`.
pub fn compute_tight_y_range(
    values: impl Iterator<Item = f64>,
    tight_range: bool,
    clamp: Option<(f64, f64)>,
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
        return clamp.unwrap_or((0.0, 1.0));
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
    if let Some((c_min, c_max)) = clamp {
        lo = lo.clamp(c_min, c_max);
        hi = hi.clamp(c_min, c_max);
        let min_span = (c_max - c_min) * 0.02;
        if (hi - lo) < min_span && c_max > c_min {
            lo = (min_v - min_span).clamp(c_min, (c_max - min_span).max(c_min));
            hi = (lo + min_span * 2.0).min(c_max);
        }
    }
    (lo, hi.max(lo + 1e-6))
}

/// Linear mapping from continuous domain `[domain_min, domain_max]` to pixel range `[range_start, range_end]`.
#[derive(Clone, Copy, Debug)]
pub struct LinearScale {
    /// Minimum value in data coordinates.
    pub domain_min: f64,
    /// Maximum value in data coordinates.
    pub domain_max: f64,
    /// Pixel coordinate corresponding to `domain_min`.
    pub range_start: f32,
    /// Pixel coordinate corresponding to `domain_max`.
    pub range_end: f32,
}

impl LinearScale {
    /// Creates a new [`LinearScale`].
    pub fn new(domain_min: f64, domain_max: f64, range_start: f32, range_end: f32) -> Self {
        Self {
            domain_min,
            domain_max,
            range_start,
            range_end,
        }
    }

    /// Maps a domain value to a pixel coordinate.
    pub fn map(&self, value: f64) -> f32 {
        let span = self.domain_max - self.domain_min;
        if span.abs() < f64::EPSILON {
            return self.range_start;
        }
        let t = ((value - self.domain_min) / span) as f32;
        self.range_start + t * (self.range_end - self.range_start)
    }

    /// Inverts a pixel coordinate back into domain space.
    pub fn invert(&self, pos: f32) -> f64 {
        let span = self.range_end - self.range_start;
        if span.abs() < f32::EPSILON {
            return self.domain_min;
        }
        let t = f64::from((pos - self.range_start) / span);
        self.domain_min + t * (self.domain_max - self.domain_min)
    }
}

/// Discrete band scale dividing a horizontal span into `count` equal bar columns with gap.
#[derive(Clone, Copy, Debug)]
pub struct BandScale {
    /// Left pixel edge of the band range.
    pub left: f32,
    /// Total pixel width of the band range.
    pub width: f32,
    /// Number of discrete categories.
    pub count: usize,
    /// Fraction of each slot reserved for spacing (`0.0..0.8`).
    pub padding_inner: f32,
}

impl BandScale {
    /// Creates a new [`BandScale`].
    pub fn new(left: f32, width: f32, count: usize, padding_inner: f32) -> Self {
        Self {
            left,
            width: width.max(1.0),
            count,
            padding_inner: padding_inner.clamp(0.0, 0.8),
        }
    }

    /// Width of one full slot (bar + gap).
    pub fn step(&self) -> f32 {
        if self.count == 0 {
            self.width
        } else {
            self.width / (self.count as f32)
        }
    }

    /// Width of the rendered bar inside each slot.
    pub fn bandwidth(&self) -> f32 {
        (self.step() * (1.0 - self.padding_inner)).max(2.0)
    }

    /// Left pixel X coordinate for band `index`.
    pub fn band_left(&self, index: usize) -> f32 {
        let step = self.step();
        let bw = self.bandwidth();
        self.left + (index as f32) * step + (step - bw) * 0.5
    }

    /// Center pixel X coordinate for band `index`.
    pub fn band_center(&self, index: usize) -> f32 {
        let step = self.step();
        self.left + ((index as f32) + 0.5) * step
    }

    /// Maps a fractional category coordinate `x` (`0.0` = center of column 0, `1.0` = center of column 1).
    pub fn map_continuous(&self, x: f64) -> f32 {
        self.left + ((x as f32) + 0.5) * self.step()
    }

    /// `O(1)` inverse mapping from a pointer X coordinate to the hovered category index.
    pub fn index_at_x(&self, x: f32) -> Option<usize> {
        if self.count == 0 || x < self.left || x > self.left + self.width {
            return None;
        }
        let idx = ((x - self.left) / self.step()).floor() as isize;
        if idx >= 0 && (idx as usize) < self.count {
            Some(idx as usize)
        } else {
            None
        }
    }
}

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

    if tangents[0].abs() > 3.0 * slope[0].abs() {
        tangents[0] = 3.0 * slope[0];
    }
    if tangents[n - 1].abs() > 3.0 * slope[n - 2].abs() {
        tangents[n - 1] = 3.0 * slope[n - 2];
    }

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

/// Appends either Fritsch–Carlson monotone cubic segments or straight line segments to `pb`.
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

/// Preserves exact per-pixel-column min/max envelope for dense monotonic-X series so
/// Lyon tessellation and curve tile-binning stay `O(plot_width)` without missing spikes.
pub(crate) fn decimate_monotonic_points<'a>(
    points: &'a [(f64, f64)],
    max_buckets: usize,
) -> Cow<'a, [(f64, f64)]> {
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

/// Line stroke pattern for line layers and reference lines.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LineStyle {
    /// Continuous solid stroke.
    #[default]
    Solid,
    /// Dashed comparison / baseline stroke.
    Dashed,
}

impl LineStyle {
    /// Returns the matching [`LegendSwatch`] for this line style.
    pub const fn swatch(self) -> LegendSwatch {
        match self {
            Self::Solid => LegendSwatch::SolidLine,
            Self::Dashed => LegendSwatch::DashedLine,
        }
    }
}

/// Swatch indicator style used inside [`ChartLegendItem`] and [`ChartTooltipRow`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LegendSwatch {
    /// Small circular dot (used for stacked bar and breakdown charts).
    Dot,
    /// Solid horizontal line segment (used for primary line series).
    SolidLine,
    /// Dashed horizontal line segment (used for comparison/baseline line series).
    DashedLine,
}

/// Renders a single legend or tooltip swatch indicator (`Dot`, `SolidLine`, or `DashedLine`).
pub(crate) fn render_swatch(swatch: LegendSwatch, color: Hsla, compact: bool) -> AnyElement {
    match swatch {
        LegendSwatch::Dot => {
            let d = if compact { 7. } else { 8. };
            div()
                .w(px(d))
                .h(px(d))
                .flex_shrink_0()
                .rounded_full()
                .bg(color)
                .into_any_element()
        }
        LegendSwatch::SolidLine => {
            let (w, h) = if compact { (11., 2.) } else { (16., 2.5) };
            div()
                .w(px(w))
                .h(px(h))
                .flex_shrink_0()
                .rounded_full()
                .bg(color)
                .into_any_element()
        }
        LegendSwatch::DashedLine => {
            let (dash_w, gap) = if compact { (3., 1.5) } else { (4., 2.5) };
            div()
                .flex()
                .flex_shrink_0()
                .items_center()
                .gap(px(gap))
                .child(div().w(px(dash_w)).h(px(2.)).rounded_full().bg(color))
                .child(div().w(px(dash_w)).h(px(2.)).rounded_full().bg(color))
                .child(div().w(px(dash_w)).h(px(2.)).rounded_full().bg(color))
                .into_any_element()
        }
    }
}

/// Paints a single tabular-numeral axis or callout label inside a GPUI `canvas` paint callback.
#[allow(clippy::too_many_arguments)]
pub fn paint_chart_text(
    text: impl Into<SharedString>,
    origin: Point<Pixels>,
    font_size: Pixels,
    color: Hsla,
    weight: FontWeight,
    align: TextAlign,
    align_width: Option<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let text: SharedString = text.into();
    if text.is_empty() {
        return;
    }
    let mut style = window.text_style();
    style.font_weight = weight;
    style.font_features = tabular_numerals();
    let run = TextRun {
        len: text.len(),
        font: style.font(),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line = window
        .text_system()
        .shape_line(text, font_size, &[run], None);
    let _ = line.paint(origin, font_size * 1.25, align, align_width, window, cx);
}

/// Inner plot margins (in logical pixels) shared by Cartesian chart renderers.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ChartMargins {
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
}

/// Resolved Cartesian plot rectangle computed from element bounds and [`ChartMargins`].
#[derive(Clone, Copy, Debug)]
pub(crate) struct PlotRect {
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
}

impl PlotRect {
    /// Computes window-space plot coordinates from painted `bounds` and `margins`.
    #[inline]
    pub fn from_bounds(bounds: Bounds<Pixels>, margins: ChartMargins) -> Self {
        let left = bounds.origin.x.0 + margins.left;
        let right = (bounds.origin.x.0 + bounds.size.width.0 - margins.right).max(left + 10.0);
        let top = bounds.origin.y.0 + margins.top;
        let bottom = (bounds.origin.y.0 + bounds.size.height.0 - margins.bottom).max(top + 10.0);
        Self {
            left,
            right,
            top,
            bottom,
        }
    }

    /// Computes element-local plot coordinates (origin at `0, 0`) for floating overlays.
    #[inline]
    pub fn from_local_bounds(bounds: Bounds<Pixels>, margins: ChartMargins) -> Self {
        let left = margins.left;
        let right = (bounds.size.width.0 - margins.right).max(left + 1.0);
        let top = margins.top;
        let bottom = (bounds.size.height.0 - margins.bottom).max(top + 1.0);
        Self {
            left,
            right,
            top,
            bottom,
        }
    }

    #[inline]
    pub fn width(self) -> f32 {
        self.right - self.left
    }

    #[inline]
    pub fn height(self) -> f32 {
        self.bottom - self.top
    }

    #[inline]
    pub fn contains_x(self, x: f32) -> bool {
        x >= self.left && x <= self.right
    }
}

/// Paints horizontal 1px gridlines and right-aligned Y-axis tick labels.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_y_gridlines(
    ticks: &[f64],
    y_scale: LinearScale,
    plot: PlotRect,
    label_origin_x: f32,
    label_width: f32,
    format: &ChartValueFormat,
    palette: &ChartPalette,
    show_grid: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let width = plot.width();
    for &tick in ticks {
        let y = y_scale.map(tick).round();
        if show_grid {
            window.paint_quad(fill(
                Bounds::new(point(px(plot.left), px(y)), size(px(width), px(1.))),
                palette.gridline,
            ));
        }
        paint_chart_text(
            format.format_tick(tick),
            point(px(label_origin_x), px(y - 7.0)),
            px(11.),
            palette.axis_label,
            FontWeight::NORMAL,
            TextAlign::Right,
            Some(px(label_width)),
            window,
            cx,
        );
    }
}

/// Paints a translucent horizontal brush/selection band with crisp 1.5px accent vertical edge guides.
pub(crate) fn paint_brush_band(
    x0: f32,
    x1: f32,
    plot: PlotRect,
    accent: Hsla,
    is_live_brushing: bool,
    window: &mut Window,
) {
    let sel_left = x0.min(x1).clamp(plot.left, plot.right);
    let sel_right = x0.max(x1).clamp(plot.left, plot.right);
    let sel_w = (sel_right - sel_left).max(2.0);
    let sel_h = plot.height();
    window.paint_quad(fill(
        Bounds::new(
            point(px(sel_left), px(plot.top)),
            size(px(sel_w), px(sel_h)),
        ),
        accent.opacity(if is_live_brushing { 0.16 } else { 0.11 }),
    ));
    window.paint_quad(fill(
        Bounds::new(point(px(sel_left), px(plot.top)), size(px(1.5), px(sel_h))),
        accent.opacity(0.85),
    ));
    window.paint_quad(fill(
        Bounds::new(
            point(px((sel_right - 1.5).max(sel_left)), px(plot.top)),
            size(px(1.5), px(sel_h)),
        ),
        accent.opacity(0.85),
    ));
}

/// Paints a circular series marker dot surrounded by a subtle surface-colored halo ring.
pub(crate) fn paint_chart_marker(
    cx_px: f32,
    cy_px: f32,
    radius: f32,
    color: Hsla,
    halo_bg: Hsla,
    window: &mut Window,
) {
    let halo_r = radius + 1.5;
    window.paint_quad(quad(
        Bounds::new(
            point(px(cx_px - halo_r), px(cy_px - halo_r)),
            size(px(halo_r * 2.0), px(halo_r * 2.0)),
        ),
        Corners::all(px(halo_r)),
        halo_bg,
        Edges::default(),
        crate::transparent_black(),
        BorderStyle::Solid,
    ));
    window.paint_quad(quad(
        Bounds::new(
            point(px(cx_px - radius), px(cy_px - radius)),
            size(px(radius * 2.0), px(radius * 2.0)),
        ),
        Corners::all(px(radius)),
        color,
        Edges::default(),
        crate::transparent_black(),
        BorderStyle::Solid,
    ));
}

/// Renders the floating accent summary pill centered above an active horizontal brush selection.
pub(crate) fn render_brush_pill(
    summary: String,
    mid_x: f32,
    min_x: f32,
    container_width: f32,
    min_w: f32,
    max_w: f32,
    theme: &Theme,
) -> impl IntoElement {
    let pill_w = ((summary.len() as f32) * 6.8 + 20.0).clamp(min_w, max_w);
    let left_x = (mid_x - pill_w * 0.5).clamp(min_x, (container_width - pill_w - 4.0).max(min_x));
    div()
        .absolute()
        .top(px(6.))
        .left(px(left_x))
        .w(px(pill_w))
        .px(px(10.))
        .py(px(5.))
        .rounded(theme.radius_md)
        .bg(theme.primary)
        .text_color(theme.primary_foreground)
        .text_size(px(11.5))
        .font_weight(FontWeight::SEMIBOLD)
        .font_features(tabular_numerals())
        .flex()
        .items_center()
        .justify_center()
        .when(theme.elevation_shadows, |el| el.shadow_md())
        .child(summary)
}

/// One swatch + label + value row displayed inside a floating chart hover tooltip.
#[derive(Clone, Debug)]
pub struct ChartTooltipRow {
    /// Swatch indicator color.
    pub color: Hsla,
    /// Series or metric display label.
    pub label: SharedString,
    /// Formatted right-aligned value text.
    pub value: String,
    /// Swatch shape (`Dot`, `SolidLine`, or `DashedLine`).
    pub swatch: LegendSwatch,
    /// When `true`, renders a subtle horizontal divider directly above this row.
    pub separator_before: bool,
}

impl ChartTooltipRow {
    /// Creates a tooltip row with a circular dot swatch.
    pub fn dot(label: impl Into<SharedString>, value: impl Into<String>, color: Hsla) -> Self {
        Self {
            color,
            label: label.into(),
            value: value.into(),
            swatch: LegendSwatch::Dot,
            separator_before: false,
        }
    }

    /// Creates a tooltip row with a solid-line swatch.
    pub fn solid_line(
        label: impl Into<SharedString>,
        value: impl Into<String>,
        color: Hsla,
    ) -> Self {
        Self {
            color,
            label: label.into(),
            value: value.into(),
            swatch: LegendSwatch::SolidLine,
            separator_before: false,
        }
    }

    /// Creates a tooltip row with a dashed-line swatch.
    pub fn dashed_line(
        label: impl Into<SharedString>,
        value: impl Into<String>,
        color: Hsla,
    ) -> Self {
        Self {
            color,
            label: label.into(),
            value: value.into(),
            swatch: LegendSwatch::DashedLine,
            separator_before: false,
        }
    }

    /// Marks whether to draw a subtle horizontal divider directly above this row.
    pub fn separator_before(mut self, enabled: bool) -> Self {
        self.separator_before = enabled;
        self
    }
}

/// Renders a floating edge-flipping hover tooltip card anchored near `anchor_x`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_chart_tooltip(
    header_left: impl Into<SharedString>,
    header_right: Option<String>,
    rows: impl IntoIterator<Item = ChartTooltipRow>,
    anchor_x: f32,
    min_left_x: f32,
    container_width: f32,
    tooltip_w: f32,
    top_offset: Pixels,
    theme: &Theme,
) -> impl IntoElement {
    let place_left = anchor_x + tooltip_w + 18.0 > container_width;
    let left_x = if place_left {
        (anchor_x - tooltip_w - 10.0).max(min_left_x)
    } else {
        (anchor_x + 10.0).min((container_width - tooltip_w - 4.0).max(0.0))
    };

    let mut header = div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(8.))
        .text_size(px(11.5))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.foreground)
        .child(header_left.into());
    if let Some(right) = header_right {
        header = header.child(div().flex_shrink_0().child(right));
    }

    let mut tooltip = div()
        .absolute()
        .top(top_offset)
        .left(px(left_x))
        .w(px(tooltip_w))
        .p(px(10.))
        .rounded(theme.radius_lg)
        .bg(theme.popover)
        .border_1()
        .border_color(theme.border)
        .when(theme.elevation_shadows, |el| el.shadow_lg())
        .flex()
        .flex_col()
        .gap(px(5.))
        .font_features(tabular_numerals())
        .child(header)
        .child(div().w_full().h(px(1.)).bg(theme.border));

    for row in rows {
        if row.separator_before {
            tooltip = tooltip.child(div().w_full().h(px(1.)).bg(theme.border.opacity(0.65)));
        }
        tooltip = tooltip.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(8.))
                .text_size(px(11.5))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .min_w_0()
                        .child(render_swatch(row.swatch, row.color, true))
                        .child(
                            div()
                                .text_color(theme.muted_foreground)
                                .truncate()
                                .child(row.label),
                        ),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.foreground)
                        .child(row.value),
                ),
        );
    }

    tooltip
}

/// One entry rendered in a chart legend bar.
#[derive(Clone, Debug)]
pub struct ChartLegendItem {
    /// Series display name.
    pub label: SharedString,
    /// Swatch color.
    pub color: Hsla,
    /// Swatch indicator shape.
    pub swatch: LegendSwatch,
    /// Whether this series is currently muted/hidden via interactive legend toggle.
    pub muted: bool,
}

impl ChartLegendItem {
    /// Creates a dot-swatch legend item.
    pub fn dot(label: impl Into<SharedString>, color: Hsla) -> Self {
        Self {
            label: label.into(),
            color,
            swatch: LegendSwatch::Dot,
            muted: false,
        }
    }

    /// Creates a solid-line legend item.
    pub fn solid_line(label: impl Into<SharedString>, color: Hsla) -> Self {
        Self {
            label: label.into(),
            color,
            swatch: LegendSwatch::SolidLine,
            muted: false,
        }
    }

    /// Creates a dashed-line legend item.
    pub fn dashed_line(label: impl Into<SharedString>, color: Hsla) -> Self {
        Self {
            label: label.into(),
            color,
            swatch: LegendSwatch::DashedLine,
            muted: false,
        }
    }

    /// Marks this legend item as muted/toggled off.
    pub fn muted(mut self, muted: bool) -> Self {
        self.muted = muted;
        self
    }
}

/// Renders a horizontal wrapping legend bar from `items`.
pub fn render_chart_legend(items: &[ChartLegendItem], theme: &Theme) -> impl IntoElement {
    render_interactive_chart_legend("chart-legend", items, theme, None)
}

fn render_interactive_chart_legend(
    id: impl Into<ElementId>,
    items: &[ChartLegendItem],
    theme: &Theme,
    on_click: Option<Rc<dyn Fn(&usize, &mut Window, &mut App) + 'static>>,
) -> impl IntoElement {
    let base_id = id.into();
    let hover_bg = theme.surface_hover;
    let mut row = div()
        .id(base_id)
        .flex_shrink_0()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_x(px(12.))
        .gap_y(px(6.));
    for (idx, item) in items.iter().enumerate() {
        let swatch_color = if item.muted {
            item.color.opacity(0.32)
        } else {
            item.color
        };

        let mut pill = div()
            .id(("leg", idx))
            .flex()
            .items_center()
            .gap(px(6.))
            .px(px(6.))
            .py(px(2.))
            .mx(px(-6.))
            .rounded(theme.radius_sm)
            .when(item.muted, |el| el.opacity(0.5))
            .child(render_swatch(item.swatch, swatch_color, false))
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(theme.muted_foreground)
                    .when(item.muted, |el| el.line_through())
                    .child(item.label.clone()),
            );

        if let Some(cb) = &on_click {
            let cb = cb.clone();
            pill = pill
                .cursor_pointer()
                .hover(move |s| s.bg(hover_bg))
                .on_click(move |_: &ClickEvent, window, cx| {
                    cb(&idx, window, cx);
                });
        }

        row = row.child(pill);
    }
    row
}

/// Reusable analytics card container framing any chart with title, right metadata,
/// interactive legend, main plot body, and an optional collapsible `"Show data table"` disclosure.
#[derive(IntoElement)]
pub struct ChartCard {
    id: ElementId,
    title: SharedString,
    subtitle: Option<SharedString>,
    header_action: Option<AnyElement>,
    legend: Vec<ChartLegendItem>,
    on_legend_click: Option<Rc<dyn Fn(&usize, &mut Window, &mut App) + 'static>>,
    body: Option<AnyElement>,
    data_table: Option<AnyElement>,
    style: StyleRefinement,
}

/// Convenience constructor for [`ChartCard::new`].
pub fn chart_card(id: impl Into<ElementId>, title: impl Into<SharedString>) -> ChartCard {
    ChartCard::new(id, title)
}

impl ChartCard {
    /// Creates a new [`ChartCard`].
    pub fn new(id: impl Into<ElementId>, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            subtitle: None,
            header_action: None,
            legend: Vec::new(),
            on_legend_click: None,
            body: None,
            data_table: None,
            style: StyleRefinement::default(),
        }
    }

    /// Sets right-aligned muted header metadata (e.g. `"30 days · local time"`).
    pub fn subtitle(mut self, subtitle: impl Into<SharedString>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// Sets an optional inline action element in the top-right header (e.g. a `"Reset zoom"` button).
    pub fn header_action(mut self, action: impl IntoElement) -> Self {
        self.header_action = Some(action.into_any_element());
        self
    }

    /// Sets legend items displayed beneath the card header.
    pub fn legend(mut self, items: impl IntoIterator<Item = ChartLegendItem>) -> Self {
        self.legend = items.into_iter().collect();
        self
    }

    /// Makes legend items interactive, invoking `handler` with the clicked legend index.
    pub fn on_legend_click(
        mut self,
        handler: impl Fn(&usize, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_legend_click = Some(Rc::new(handler));
        self
    }

    /// Sets the primary chart visualization element.
    pub fn content(mut self, body: impl IntoElement) -> Self {
        self.body = Some(body.into_any_element());
        self
    }

    /// Attaches an optional collapsible `"Show data table"` element rendered at the card footer.
    pub fn data_table(mut self, table: impl IntoElement) -> Self {
        self.data_table = Some(table.into_any_element());
        self
    }
}

impl Styled for ChartCard {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for ChartCard {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let show_table_state =
            window.use_keyed_state((self.id.clone(), "show-data-table"), cx, |_, _| false);
        let table_open = *show_table_state.read(cx);

        let mut card = div()
            .id(self.id.clone())
            .w_full()
            .flex_shrink_0()
            .rounded(theme.radius_xl)
            .bg(theme.surface)
            .border_1()
            .border_color(theme.border)
            .when(theme.elevation_shadows, |el| el.shadow_xs())
            .p(px(20.))
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .child(
                        div()
                            .text_size(px(14.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.foreground)
                            .child(self.title),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .when_some(self.subtitle, |el, sub| {
                                el.child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(theme.muted_foreground)
                                        .child(sub),
                                )
                            })
                            .when_some(self.header_action, |el, act| el.child(act)),
                    ),
            );

        if !self.legend.is_empty() {
            card = card.child(render_interactive_chart_legend(
                (self.id.clone(), "legend"),
                &self.legend,
                &theme,
                self.on_legend_click,
            ));
        }

        if let Some(body) = self.body {
            card = card.child(body);
        }

        if let Some(table_el) = self.data_table {
            let toggle_state = show_table_state;
            let fg_hover = theme.foreground;
            let toggle_row = div()
                .flex_shrink_0()
                .pt(px(4.))
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(
                    div()
                        .id((self.id.clone(), "toggle-table-btn"))
                        .flex()
                        .items_center()
                        .gap(px(5.))
                        .cursor_pointer()
                        .text_size(px(12.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.muted_foreground)
                        .hover(move |s| s.text_color(fg_hover))
                        .child(
                            Icon::new(if table_open {
                                LucideIcon::ChevronDown
                            } else {
                                LucideIcon::ChevronRight
                            })
                            .size(IconSize::Xs)
                            .color(theme.muted_foreground),
                        )
                        .child(if table_open {
                            "Hide data table"
                        } else {
                            "Show data table"
                        })
                        .on_click(move |_: &ClickEvent, _, cx| {
                            toggle_state.update(cx, |open, cx| {
                                *open = !*open;
                                cx.notify();
                            });
                        }),
                )
                .when(table_open, |el| el.child(table_el));

            card = card.child(toggle_row);
        }

        crate::Refineable::refine(card.style(), &self.style);
        card
    }
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
        let (lo, hi) = compute_tight_y_range(
            [0.97, 0.985, 0.992, 0.98].into_iter(),
            true,
            Some((0.0, 1.0)),
        );
        assert!(lo >= 0.95 && hi <= 1.0);
        assert!(hi > lo);
    }

    #[test]
    fn nice_ticks_range_supports_exact_and_snapped_bounds() {
        let (lo, hi, ticks) = nice_ticks_range(0.0, 100.0, 5, false);
        assert_eq!((lo, hi), (0.0, 100.0));
        assert_eq!(ticks, vec![0.0, 25.0, 50.0, 75.0, 100.0]);

        let (slo, shi, sticks) = nice_ticks_range(94.2, 99.8, 4, true);
        assert!(slo <= 94.2 && shi >= 99.8);
        assert!(sticks.len() >= 3);
    }
}
