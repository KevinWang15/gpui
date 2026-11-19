//! GPU-batched [`StackedBarChart`] with top-segment corner rounding, nice Y ticks,
//! auto-thinned X labels, interactive hover tooltip, and bar click callbacks.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use crate::{
    App, Bounds, ClickEvent, Corners, Edges, ElementId, FontFeatures, FontWeight, Hsla,
    IntoElement, MouseMoveEvent, ParentElement, Pixels, RenderOnce, SharedString, StyleRefinement,
    Styled, TextAlign, Window, canvas, div, fill, point, prelude::*, px, quad, size,
    transparent_black,
};

use super::core::{
    BandScale, ChartLegendItem, ChartPalette, ChartValueFormat, LinearScale, nice_ticks,
    paint_chart_text,
};
use crate::components::theme::ActiveTheme;

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

#[derive(Default)]
struct StackedBarHoverState {
    hovered_index: Option<usize>,
    plot_bounds: Option<Bounds<Pixels>>,
}

/// GPU-native stacked vertical bar chart rendered with instanced GPUI quads and tabular axis labels.
#[derive(IntoElement)]
pub struct StackedBarChart {
    id: ElementId,
    series: Vec<StackedBarSeries>,
    points: Vec<StackedBarPoint>,
    height: Pixels,
    value_format: ChartValueFormat,
    bar_radius: Pixels,
    style: StyleRefinement,
    on_bar_click: Option<Rc<dyn Fn(&usize, &mut Window, &mut App) + 'static>>,
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
            height: px(220.),
            value_format: ChartValueFormat::Currency,
            bar_radius: px(3.),
            style: StyleRefinement::default(),
            on_bar_click: None,
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

    /// Sets the chart plot height (default `220px`).
    pub fn height(mut self, height: Pixels) -> Self {
        self.height = height;
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

    /// Registers a click callback invoked with the clicked column index.
    pub fn on_bar_click(
        mut self,
        handler: impl Fn(&usize, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_bar_click = Some(Rc::new(handler));
        self
    }

    /// Builds matching [`ChartLegendItem`]s for use with [`ChartCard::legend`](super::ChartCard::legend).
    pub fn legend_items(series: &[StackedBarSeries], palette: &ChartPalette) -> Vec<ChartLegendItem> {
        series
            .iter()
            .enumerate()
            .map(|(i, s)| ChartLegendItem::dot(s.label.clone(), s.color.unwrap_or_else(|| palette.color(i))))
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
        let palette = ChartPalette::from_theme(&theme);
        let tnum = FontFeatures(Arc::new(vec![("tnum".to_string(), 1)]));

        let hover_entity = window.use_keyed_state(
            (self.id.clone(), "stacked-bar-hover"),
            cx,
            |_, _| Rc::new(RefCell::new(StackedBarHoverState::default())),
        );
        let hover_rc = hover_entity.read(cx).clone();
        let hovered_index = hover_rc.borrow().hovered_index;

        let max_total = self
            .points
            .iter()
            .map(StackedBarPoint::total)
            .fold(0.0_f64, f64::max);
        let (nice_max, y_ticks) = nice_ticks(max_total, 4);

        let series_colors: Vec<Hsla> = self
            .series
            .iter()
            .enumerate()
            .map(|(i, s)| s.color.unwrap_or_else(|| palette.color(i)))
            .collect();

        let margin_left = 54.0_f32;
        let margin_right = 12.0_f32;
        let margin_top = 10.0_f32;
        let margin_bottom = 26.0_f32;

        let points_paint = self.points.clone();
        let colors_paint = series_colors.clone();
        let ticks_paint = y_ticks;
        let format_paint = self.value_format.clone();
        let palette_paint = palette.clone();
        let surface_bg = theme.surface;
        let bar_radius = self.bar_radius;
        let hover_paint = hover_rc.clone();

        let plot_canvas = canvas(
            move |bounds, _, _| {
                hover_paint.borrow_mut().plot_bounds = Some(bounds);
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

                let y_scale = LinearScale::new(0.0, nice_max, plot_bottom, plot_top);
                let band_scale = BandScale::new(plot_left, plot_width, points_paint.len(), 0.28);

                // 1. Horizontal gridlines & right-aligned Y-axis labels
                for &tick in &ticks_paint {
                    let y = y_scale.map(tick).round();
                    window.paint_quad(fill(
                        Bounds::new(
                            point(px(plot_left), px(y)),
                            size(px(plot_width), px(1.)),
                        ),
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

                // 2. Hover column backdrop highlight
                if let Some(h_idx) = hovered_index
                    && h_idx < points_paint.len()
                {
                    let slot_x = plot_left + (h_idx as f32) * band_scale.step();
                    window.paint_quad(fill(
                        Bounds::new(
                            point(px(slot_x), px(plot_top)),
                            size(px(band_scale.step()), px(plot_bottom - plot_top)),
                        ),
                        palette_paint.crosshair,
                    ));
                }

                // 3. Stacked bar segments (bottom-to-top; top-only corner rounding on highest segment)
                let bw = band_scale.bandwidth();
                for (col_idx, col) in points_paint.iter().enumerate() {
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
                        let color = colors_paint
                            .get(s_idx)
                            .copied()
                            .unwrap_or_else(|| palette_paint.color(s_idx));

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

                        // Add subtle 1px top divider between stacked segments.
                        let draw_y = if !is_top && seg_h > 2.0 { y_top + 0.75 } else { y_top };
                        let draw_h = if !is_top && seg_h > 2.0 { seg_h - 0.75 } else { seg_h };

                        window.paint_quad(quad(
                            Bounds::new(point(px(x), px(draw_y)), size(px(bw), px(draw_h))),
                            corner_radii,
                            color,
                            Edges::default(),
                            transparent_black(),
                            crate::BorderStyle::Solid,
                        ));

                        if !is_top && seg_h > 2.0 {
                            window.paint_quad(fill(
                                Bounds::new(point(px(x), px(y_top)), size(px(bw), px(0.75))),
                                surface_bg,
                            ));
                        }
                    }
                }

                // 4. Auto-thinned X-axis labels
                let n = points_paint.len();
                if n > 0 {
                    let max_labels = ((plot_width / 68.0).floor() as usize).clamp(2, n);
                    let stride = (n.saturating_sub(1)).div_ceil(max_labels.saturating_sub(1).max(1)).max(1);
                    for (idx, col) in points_paint.iter().enumerate() {
                        let is_last = idx + 1 == n;
                        if idx % stride != 0 && !is_last {
                            continue;
                        }
                        // Avoid colliding the second-to-last label with the last label ("Today").
                        if !is_last && n.saturating_sub(1) - idx < stride.div_ceil(2) {
                            continue;
                        }
                        let cx_pos = band_scale.band_center(idx);
                        let label_w = 56.0_f32;
                        let align = if idx == 0 {
                            TextAlign::Left
                        } else if is_last {
                            TextAlign::Right
                        } else {
                            TextAlign::Center
                        };
                        let label_x = if idx == 0 {
                            plot_left
                        } else if is_last {
                            (plot_right - label_w).max(plot_left)
                        } else {
                            cx_pos - label_w * 0.5
                        };
                        paint_chart_text(
                            col.label.clone(),
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
                }
            },
        )
        .w_full()
        .h(self.height);

        let point_count = self.points.len();
        let hover_move_entity = hover_entity.clone();
        let hover_leave_entity = hover_entity;
        let hover_click_rc = hover_rc.clone();
        let on_bar_click = self.on_bar_click.clone();

        let mut container = div()
            .id(self.id.clone())
            .relative()
            .w_full()
            .h(self.height)
            .when(on_bar_click.is_some(), |el| el.cursor_pointer())
            .on_mouse_move(move |event: &MouseMoveEvent, _, cx| {
                let next_idx = {
                    let state = hover_move_entity.read(cx).borrow();
                    state.plot_bounds.and_then(|bounds| {
                        let plot_left = bounds.origin.x.0 + margin_left;
                        let plot_width = (bounds.size.width.0 - margin_left - margin_right).max(1.0);
                        let scale = BandScale::new(plot_left, plot_width, point_count, 0.28);
                        scale.index_at_x(event.position.x.0)
                    })
                };
                let changed = {
                    let mut state = hover_move_entity.read(cx).borrow_mut();
                    if state.hovered_index != next_idx {
                        state.hovered_index = next_idx;
                        true
                    } else {
                        false
                    }
                };
                if changed {
                    hover_move_entity.update(cx, |_, cx| cx.notify());
                }
            })
            .on_hover(move |hovered, _, cx| {
                if !hovered {
                    let changed = {
                        let mut state = hover_leave_entity.read(cx).borrow_mut();
                        state.hovered_index.take().is_some()
                    };
                    if changed {
                        hover_leave_entity.update(cx, |_, cx| cx.notify());
                    }
                }
            })
            .when_some(on_bar_click, |el, cb| {
                el.on_click(move |_: &ClickEvent, window, cx| {
                    if let Some(idx) = hover_click_rc.borrow().hovered_index {
                        cb(&idx, window, cx);
                    }
                })
            })
            .child(plot_canvas);

        // 5. Floating hover tooltip pinned cleanly inside the plot bounds
        if let Some(h_idx) = hovered_index
            && let Some(col) = self.points.get(h_idx)
            && let Some(bounds) = hover_rc.borrow().plot_bounds
        {
            let plot_width = (bounds.size.width.0 - margin_left - margin_right).max(1.0);
            let band_scale = BandScale::new(margin_left, plot_width, point_count, 0.28);
            let center_x = band_scale.band_center(h_idx);
            let tooltip_w = 196.0_f32;
            let place_left = center_x + tooltip_w + 18.0 > bounds.size.width.0;
            let left_x = if place_left {
                (center_x - tooltip_w - 10.0).max(margin_left)
            } else {
                (center_x + 10.0).min((bounds.size.width.0 - tooltip_w - 4.0).max(0.0))
            };

            let mut tooltip = div()
                .absolute()
                .top(px(8.))
                .left(px(left_x))
                .w(px(tooltip_w))
                .p(px(10.))
                .rounded(theme.radius_lg)
                .bg(theme.popover)
                .border_1()
                .border_color(theme.border)
                .shadow_lg()
                .flex()
                .flex_col()
                .gap(px(5.))
                .font_features(tnum)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .text_size(px(11.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.foreground)
                        .child(col.label.clone())
                        .child(self.value_format.format_detail(col.total())),
                )
                .child(div().w_full().h(px(1.)).bg(theme.border));

            for (s_idx, series_meta) in self.series.iter().enumerate() {
                let val = col.values.get(s_idx).copied().unwrap_or(0.0);
                if val <= 0.0 {
                    continue;
                }
                let color = series_colors
                    .get(s_idx)
                    .copied()
                    .unwrap_or_else(|| palette.color(s_idx));
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
                                        .child(series_meta.label.clone()),
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

            container = container.child(tooltip);
        }

        crate::Refineable::refine(container.style(), &self.style);
        container
    }
}
