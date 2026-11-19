//! Shared scales, nice-tick generation, color palettes, legend swatches, GPU text painting,
//! and [`ChartCard`] framing for `gpui::components::charts`.

use std::rc::Rc;

use crate::{
    AnyElement, App, BorderStyle, Bounds, ClickEvent, Corners, Edges, ElementId, FontWeight, Hsla,
    IntoElement, ParentElement, Pixels, Point, RenderOnce, SharedString, StyleRefinement, Styled,
    TextAlign, TextRun, Window, div, fill, point, prelude::*, px, quad, size,
};
use crate::icons::{Icon, IconSize, LucideIcon};

use crate::components::palette as tw;
use crate::components::theme::{ActiveTheme, Theme, ThemeMode, tabular_numerals};

/// Categorical series color palette automatically tuned for Light and Dark themes.
#[derive(Clone, Debug)]
pub struct ChartPalette {
    /// Ordered categorical colors for chart series (indexed modulo length).
    pub colors: Vec<Hsla>,
    /// Muted neutral baseline stroke (e.g. `"Previous 5 workdays, average"`).
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
            ThemeMode::Light => Hsla::from(tw::SLATE_900).opacity(0.08),
            ThemeMode::Dark => Hsla::from(tw::WHITE).opacity(0.07),
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

/// Computes visually clean axis ticks spanning `0.0..=nice_max` (Heckbert nice-numbers algorithm).
pub fn nice_ticks(max_value: f64, target_ticks: usize) -> (f64, Vec<f64>) {
    let target = target_ticks.max(2) as f64;
    if !max_value.is_finite() || max_value <= 0.0 {
        return (1.0, vec![0.0, 0.5, 1.0]);
    }

    let rough_step = max_value / (target - 1.0);
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
    let step = nice_fraction * magnitude;
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

/// Paints a single tabular-numeral axis or callout label inside a GPUI `canvas` paint callback.
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
    window: &mut Window,
    cx: &mut App,
) {
    let width = plot.width();
    for &tick in ticks {
        let y = y_scale.map(tick).round();
        window.paint_quad(fill(
            Bounds::new(point(px(plot.left), px(y)), size(px(width), px(1.))),
            palette.gridline,
        ));
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
        Bounds::new(point(px(sel_left), px(plot.top)), size(px(sel_w), px(sel_h))),
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
        halo_bg,
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
        color,
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
pub(crate) struct ChartTooltipRow {
    pub color: Hsla,
    pub label: SharedString,
    pub value: String,
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
        .text_size(px(11.5))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.foreground)
        .child(header_left.into());
    if let Some(right) = header_right {
        header = header.child(right);
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
                        .child(div().w(px(7.)).h(px(7.)).rounded_full().bg(row.color))
                        .child(
                            div()
                                .text_color(theme.muted_foreground)
                                .truncate()
                                .child(row.label),
                        ),
                )
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.foreground)
                        .child(row.value),
                ),
        );
    }

    tooltip
}

/// Swatch indicator style used inside a [`ChartLegendItem`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LegendSwatch {
    /// Small circular dot (used for stacked bar and breakdown charts).
    Dot,
    /// Solid horizontal line segment (used for primary line series).
    SolidLine,
    /// Dashed horizontal line segment (used for comparison/baseline line series).
    DashedLine,
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
        let swatch = match item.swatch {
            LegendSwatch::Dot => div()
                .w(px(8.))
                .h(px(8.))
                .rounded_full()
                .bg(swatch_color)
                .into_any_element(),
            LegendSwatch::SolidLine => div()
                .w(px(16.))
                .h(px(2.5))
                .rounded_full()
                .bg(swatch_color)
                .into_any_element(),
            LegendSwatch::DashedLine => div()
                .flex()
                .items_center()
                .gap(px(2.5))
                .child(div().w(px(4.)).h(px(2.)).rounded_full().bg(swatch_color))
                .child(div().w(px(4.)).h(px(2.)).rounded_full().bg(swatch_color))
                .child(div().w(px(4.)).h(px(2.)).rounded_full().bg(swatch_color))
                .into_any_element(),
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
            .child(swatch)
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
