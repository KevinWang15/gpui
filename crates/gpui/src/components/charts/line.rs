//! GPU-native multi-series [`LineChart`] supporting solid & dashed strokes via Lyon
//! [`PathBuilder`], optional area gradient fills, endpoint callout annotations, interactive
//! hover crosshairs, and optional horizontal drag-to-select brushing.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use crate::{
    App, Bounds, Corners, CursorStyle, Edges, ElementId, FontFeatures, FontWeight, Hsla,
    IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement,
    PathBuilder, Pixels, RenderOnce, SharedString, StyleRefinement, Styled, TextAlign, Window,
    canvas, div, fill, linear_color_stop, linear_gradient, point, prelude::*, px, quad, size,
};

use super::core::{
    ChartLegendItem, ChartPalette, ChartValueFormat, LinearScale, nice_ticks, paint_chart_text,
};
use crate::components::theme::ActiveTheme;

/// Line stroke pattern for a [`LineSeries`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
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
    /// Stroke thickness in pixels (default `2.2` for solid, `1.75` for dashed).
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
}

#[derive(Default)]
struct LineChartInteractionState {
    hover_x: Option<f64>,
    plot_bounds: Option<Bounds<Pixels>>,
    drag_anchor_x: Option<f32>,
    drag_current_x: Option<f32>,
}

impl LineChartInteractionState {
    fn is_brushing(&self) -> bool {
        match (self.drag_anchor_x, self.drag_current_x) {
            (Some(a), Some(b)) => (b - a).abs() >= 4.0,
            _ => false,
        }
    }

    fn brushed_domain(
        &self,
        margin_left: f32,
        margin_right: f32,
        x_min: f64,
        x_max: f64,
    ) -> Option<(f64, f64)> {
        if !self.is_brushing() {
            return None;
        }
        let bounds = self.plot_bounds?;
        let plot_left = bounds.origin.x.0 + margin_left;
        let plot_right =
            (bounds.origin.x.0 + bounds.size.width.0 - margin_right).max(plot_left + 1.0);
        let scale = LinearScale::new(x_min, x_max, plot_left, plot_right);
        let a = self.drag_anchor_x?.clamp(plot_left, plot_right);
        let b = self.drag_current_x?.clamp(plot_left, plot_right);
        let x0 = scale.invert(a.min(b)).clamp(x_min, x_max);
        let x1 = scale.invert(a.max(b)).clamp(x_min, x_max);
        Some((x0, x1))
    }
}

/// GPU-native multi-series line chart with solid/dashed strokes, inline endpoint callout,
/// interactive vertical crosshair scrubber, and optional horizontal drag-to-select brushing.
#[derive(IntoElement)]
pub struct LineChart {
    id: ElementId,
    series: Vec<LineSeries>,
    x_domain: Option<(f64, f64)>,
    x_ticks: Vec<(f64, SharedString)>,
    height: Pixels,
    value_format: ChartValueFormat,
    palette: Option<ChartPalette>,
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
            value_format: ChartValueFormat::Currency,
            palette: None,
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

    /// Sets the plot height (default `220px`).
    pub fn height(mut self, height: Pixels) -> Self {
        self.height = height;
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

fn interpolate_series_y(points: &[(f64, f64)], target_x: f64) -> Option<f64> {
    if points.is_empty() {
        return None;
    }
    let first = points[0];
    let last = *points.last()?;
    if target_x < first.0 - 1e-6 || target_x > last.0 + 1e-6 {
        return None;
    }
    for pair in points.windows(2) {
        let (x0, y0) = pair[0];
        let (x1, y1) = pair[1];
        if target_x >= x0 - 1e-6 && target_x <= x1 + 1e-6 {
            let span = x1 - x0;
            if span.abs() < 1e-9 {
                return Some(y1);
            }
            let t = ((target_x - x0) / span).clamp(0.0, 1.0);
            return Some(y0 + (y1 - y0) * t);
        }
    }
    Some(last.1)
}

impl RenderOnce for LineChart {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let palette = self
            .palette
            .unwrap_or_else(|| ChartPalette::from_theme(&theme));
        let tnum = FontFeatures(Arc::new(vec![("tnum".to_string(), 1)]));

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

        let margin_left = 54.0_f32;
        let margin_right = 16.0_f32;
        let margin_top = 14.0_f32;
        let margin_bottom = 26.0_f32;

        let (hover_x, live_brush) = {
            let state = interaction_rc.borrow();
            (
                state.hover_x,
                if self.selectable {
                    state.brushed_domain(margin_left, margin_right, x_min, x_max)
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
        let resolved_colors: Vec<Hsla> = self
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

        let series_paint = self.series.clone();
        let colors_paint = resolved_colors.clone();
        let y_ticks_paint = y_ticks;
        let x_ticks_paint = self.x_ticks.clone();
        let format_paint = self.value_format.clone();
        let palette_paint = palette;
        let surface_bg = theme.surface;
        let accent_color = theme.primary;
        let interaction_paint = interaction_rc.clone();
        let is_live_brushing = live_brush.is_some();

        let plot_canvas = canvas(
            move |bounds, _, _| {
                interaction_paint.borrow_mut().plot_bounds = Some(bounds);
                bounds
            },
            move |bounds, _, window, cx| {
                let plot_left = bounds.origin.x.0 + margin_left;
                let plot_right = (bounds.origin.x.0 + bounds.size.width.0 - margin_right)
                    .max(plot_left + 10.0);
                let plot_top = bounds.origin.y.0 + margin_top;
                let plot_bottom = (bounds.origin.y.0 + bounds.size.height.0 - margin_bottom)
                    .max(plot_top + 10.0);
                let plot_width = plot_right - plot_left;

                let x_scale = LinearScale::new(x_min, x_max, plot_left, plot_right);
                let y_scale = LinearScale::new(0.0, nice_max, plot_bottom, plot_top);

                // 1. Horizontal gridlines & Y-axis labels
                for &tick in &y_ticks_paint {
                    let y = y_scale.map(tick).round();
                    window.paint_quad(fill(
                        Bounds::new(point(px(plot_left), px(y)), size(px(plot_width), px(1.))),
                        palette_paint.gridline,
                    ));
                    paint_chart_text(
                        format_paint.format_tick(tick),
                        point(px(bounds.origin.x.0), px(y - 7.0)),
                        px(11.),
                        palette_paint.axis_label,
                        FontWeight::NORMAL,
                        TextAlign::Right,
                        Some(px(margin_left - 10.0)),
                        window,
                        cx,
                    );
                }

                // 2. Highlighted brush/selected X-domain band
                if let Some((a, b)) = active_selection {
                    let sx0 = x_scale.map(a.min(b)).clamp(plot_left, plot_right);
                    let sx1 = x_scale.map(a.max(b)).clamp(plot_left, plot_right);
                    let sel_w = (sx1 - sx0).max(2.0);
                    let sel_h = plot_bottom - plot_top;
                    window.paint_quad(fill(
                        Bounds::new(point(px(sx0), px(plot_top)), size(px(sel_w), px(sel_h))),
                        accent_color.opacity(if is_live_brushing { 0.16 } else { 0.11 }),
                    ));
                    window.paint_quad(fill(
                        Bounds::new(point(px(sx0), px(plot_top)), size(px(1.5), px(sel_h))),
                        accent_color.opacity(0.85),
                    ));
                    window.paint_quad(fill(
                        Bounds::new(
                            point(px((sx1 - 1.5).max(sx0)), px(plot_top)),
                            size(px(1.5), px(sel_h)),
                        ),
                        accent_color.opacity(0.85),
                    ));
                }

                // 3. Vertical crosshair line if hovered (hidden while actively brushing)
                if !is_live_brushing
                    && let Some(hx) = hover_x
                {
                    let x_px = x_scale.map(hx).clamp(plot_left, plot_right);
                    window.paint_quad(fill(
                        Bounds::new(
                            point(px(x_px), px(plot_top)),
                            size(px(1.), px(plot_bottom - plot_top)),
                        ),
                        palette_paint.gridline.opacity(1.0),
                    ));
                }

                // 4. Area fills first, then dashed series, then solid series on top
                for (idx, s) in series_paint.iter().enumerate() {
                    if !s.fill_area || s.points.len() < 2 {
                        continue;
                    }
                    let color = colors_paint[idx];
                    let mut pb = PathBuilder::fill();
                    let first = s.points[0];
                    let last = *s.points.last().unwrap();
                    pb.move_to(point(px(x_scale.map(first.0)), px(plot_bottom)));
                    for &(x, y) in &s.points {
                        pb.line_to(point(px(x_scale.map(x)), px(y_scale.map(y))));
                    }
                    pb.line_to(point(px(x_scale.map(last.0)), px(plot_bottom)));
                    pb.close();
                    if let Ok(path) = pb.build() {
                        window.paint_path(
                            path,
                            linear_gradient(
                                180.0,
                                linear_color_stop(color.opacity(0.16), 0.0),
                                linear_color_stop(color.opacity(0.0), 1.0),
                            ),
                        );
                    }
                }

                for pass_dashed in [true, false] {
                    for (idx, s) in series_paint.iter().enumerate() {
                        let is_dashed = s.style == LineStyle::Dashed;
                        if is_dashed != pass_dashed || s.points.len() < 2 {
                            continue;
                        }
                        let color = colors_paint[idx];
                        let width = s.stroke_width.unwrap_or(if is_dashed { 1.75 } else { 2.25 });
                        let mut pb = PathBuilder::stroke(px(width));
                        if is_dashed {
                            pb = pb.dash_array(&[px(5.0), px(4.0)]);
                        }
                        for (p_idx, &(x, y)) in s.points.iter().enumerate() {
                            let pt = point(px(x_scale.map(x)), px(y_scale.map(y)));
                            if p_idx == 0 {
                                pb.move_to(pt);
                            } else {
                                pb.line_to(pt);
                            }
                        }
                        if let Ok(path) = pb.build() {
                            window.paint_path(path, color);
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
                    let dot_r = 4.0_f32;
                    window.paint_quad(quad(
                        Bounds::new(
                            point(px(ex - dot_r - 1.5), px(ey - dot_r - 1.5)),
                            size(px((dot_r + 1.5) * 2.0), px((dot_r + 1.5) * 2.0)),
                        ),
                        Corners::all(px(dot_r + 1.5)),
                        surface_bg,
                        Edges::default(),
                        surface_bg,
                        crate::BorderStyle::Solid,
                    ));
                    window.paint_quad(quad(
                        Bounds::new(
                            point(px(ex - dot_r), px(ey - dot_r)),
                            size(px(dot_r * 2.0), px(dot_r * 2.0)),
                        ),
                        Corners::all(px(dot_r)),
                        color,
                        Edges::default(),
                        color,
                        crate::BorderStyle::Solid,
                    ));

                    let callout_text = format!("{prefix}: {}", format_paint.format_detail(ly));
                    let callout_w = (callout_text.len() as f32 * 6.8 + 14.0).clamp(84.0, 148.0);
                    let callout_h = 18.0_f32;
                    let callout_x = (ex - callout_w * 0.5)
                        .clamp(plot_left + 4.0, (plot_right - callout_w).max(plot_left));
                    let callout_y = (ey - 26.0).max(plot_top);
                    window.paint_quad(quad(
                        Bounds::new(
                            point(px(callout_x), px(callout_y)),
                            size(px(callout_w), px(callout_h)),
                        ),
                        Corners::all(px(5.)),
                        surface_bg.opacity(0.92),
                        Edges::all(px(1.)),
                        color.opacity(0.35),
                        crate::BorderStyle::Solid,
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
                    let x_px = x_scale.map(hx).clamp(plot_left, plot_right);
                    for (idx, s) in series_paint.iter().enumerate() {
                        if let Some(hy) = interpolate_series_y(&s.points, hx) {
                            let y_px = y_scale.map(hy);
                            let color = colors_paint[idx];
                            let r = 3.5_f32;
                            window.paint_quad(quad(
                                Bounds::new(
                                    point(px(x_px - r - 1.5), px(y_px - r - 1.5)),
                                    size(px((r + 1.5) * 2.0), px((r + 1.5) * 2.0)),
                                ),
                                Corners::all(px(r + 1.5)),
                                surface_bg,
                                Edges::default(),
                                surface_bg,
                                crate::BorderStyle::Solid,
                            ));
                            window.paint_quad(quad(
                                Bounds::new(
                                    point(px(x_px - r), px(y_px - r)),
                                    size(px(r * 2.0), px(r * 2.0)),
                                ),
                                Corners::all(px(r)),
                                color,
                                Edges::default(),
                                color,
                                crate::BorderStyle::Solid,
                            ));
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
                        (TextAlign::Right, (x_px - label_w).max(plot_left))
                    } else {
                        (TextAlign::Center, x_px - label_w * 0.5)
                    };
                    paint_chart_text(
                        label.clone(),
                        point(px(label_x), px(plot_bottom + 7.0)),
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
        .w_full()
        .h(self.height);

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
            .id(self.id.clone())
            .relative()
            .w_full()
            .h(self.height)
            .when(selectable, |el| el.cursor(CursorStyle::Crosshair))
            .on_mouse_down(MouseButton::Left, move |event: &MouseDownEvent, _, cx| {
                if !selectable {
                    return;
                }
                let mut state = down_entity.read(cx).borrow_mut();
                let Some(bounds) = state.plot_bounds else {
                    return;
                };
                let plot_left = bounds.origin.x.0 + margin_left;
                let plot_right =
                    (bounds.origin.x.0 + bounds.size.width.0 - margin_right).max(plot_left + 1.0);
                let mx = event.position.x.0;
                if mx >= plot_left && mx <= plot_right {
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
                    let plot_left = bounds.origin.x.0 + margin_left;
                    let plot_right = (bounds.origin.x.0 + bounds.size.width.0 - margin_right)
                        .max(plot_left + 1.0);
                    let mx = event.position.x.0;
                    let next_x = if mx >= plot_left && mx <= plot_right {
                        let scale = LinearScale::new(x_min, x_max, plot_left, plot_right);
                        Some(scale.invert(mx).clamp(x_min, x_max))
                    } else {
                        None
                    };
                    let mut dirty = match (state.hover_x, next_x) {
                        (Some(a), Some(b)) => (a - b).abs() > (x_max - x_min) * 0.005,
                        (None, None) => false,
                        _ => true,
                    };
                    if dirty {
                        state.hover_x = next_x;
                    }
                    if selectable && state.drag_anchor_x.is_some() {
                        let clamped = mx.clamp(plot_left, plot_right);
                        if state.drag_current_x != Some(clamped) {
                            state.drag_current_x = Some(clamped);
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
                        state.brushed_domain(margin_left, margin_right, x_min, x_max)
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
                        state.brushed_domain(margin_left, margin_right, x_min, x_max)
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

        // Live drag-selection pill
        if let Some((a, b)) = live_brush
            && let Some(bounds) = interaction_rc.borrow().plot_bounds
        {
            let plot_left = margin_left;
            let plot_right = (bounds.size.width.0 - margin_right).max(plot_left + 1.0);
            let x_scale = LinearScale::new(x_min, x_max, plot_left, plot_right);
            let mid_x = (x_scale.map(a) + x_scale.map(b)) * 0.5;
            let fmt_x = |v: f64| match &self.x_formatter {
                Some(f) => f(v),
                None => format!("{v:.1}"),
            };
            let delta_suffix = self.series.first().and_then(|primary| {
                let y0 = interpolate_series_y(&primary.points, a)?;
                let y1 = interpolate_series_y(&primary.points, b)?;
                Some(format!(" · Δ {}", self.value_format.format_detail((y1 - y0).max(0.0))))
            });
            let summary = format!(
                "{} – {}{}",
                fmt_x(a),
                fmt_x(b),
                delta_suffix.unwrap_or_default()
            );
            let pill_w = ((summary.len() as f32) * 6.8 + 20.0).clamp(124.0, 260.0);
            let left_x = (mid_x - pill_w * 0.5)
                .clamp(plot_left, (bounds.size.width.0 - pill_w - 4.0).max(plot_left));

            container = container.child(
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
                    .font_features(tnum)
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(theme.elevation_shadows, |el| el.shadow_md())
                    .child(summary),
            );
        } else if let Some(hx) = hover_x
            && let Some(bounds) = interaction_rc.borrow().plot_bounds
        {
            // Floating crosshair tooltip
            let plot_left = margin_left;
            let plot_right = (bounds.size.width.0 - margin_right).max(plot_left + 1.0);
            let x_scale = LinearScale::new(x_min, x_max, plot_left, plot_right);
            let cursor_x = x_scale.map(hx);
            let tooltip_w = 210.0_f32;
            let place_left = cursor_x + tooltip_w + 18.0 > bounds.size.width.0;
            let left_x = if place_left {
                (cursor_x - tooltip_w - 10.0).max(plot_left)
            } else {
                (cursor_x + 10.0).min((bounds.size.width.0 - tooltip_w - 4.0).max(0.0))
            };
            let header_label = match &self.x_formatter {
                Some(fmt) => fmt(hx),
                None => format!("{hx:.1}"),
            };

            let mut tooltip = div()
                .absolute()
                .top(px(10.))
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
                .font_features(tnum)
                .child(
                    div()
                        .text_size(px(11.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.foreground)
                        .child(header_label),
                )
                .child(div().w_full().h(px(1.)).bg(theme.border));

            for (idx, s) in self.series.iter().enumerate() {
                if let Some(val) = interpolate_series_y(&s.points, hx) {
                    let color = resolved_colors[idx];
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
                                    .child(div().w(px(7.)).h(px(7.)).rounded_full().bg(color))
                                    .child(
                                        div()
                                            .text_color(theme.muted_foreground)
                                            .truncate()
                                            .child(s.label.clone()),
                                    ),
                            )
                            .child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.foreground)
                                    .child(self.value_format.format_detail(val)),
                            ),
                    );
                }
            }

            container = container.child(tooltip);
        }

        crate::Refineable::refine(container.style(), &self.style);
        container
    }
}
