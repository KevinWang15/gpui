//! Shared scales, nice-tick generation, color palettes, legend swatches, GPU text painting,
//! and [`ChartCard`] framing for `gpui::components::charts`.

use std::rc::Rc;
use std::sync::Arc;

use crate::{
    AnyElement, App, ClickEvent, ElementId, FontFeatures, FontWeight, Hsla, IntoElement,
    ParentElement, Pixels, Point, RenderOnce, SharedString, StyleRefinement, Styled, TextAlign,
    TextRun, Window, div, prelude::*, px,
};
use crate::icons::{Icon, IconSize, LucideIcon};

use crate::components::palette as tw;
use crate::components::theme::{ActiveTheme, Theme, ThemeMode};

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
    /// Derives a harmonious light- or dark-mode chart palette from `theme`.
    pub fn from_theme(theme: &Theme) -> Self {
        match theme.mode {
            ThemeMode::Light => Self {
                colors: vec![
                    tw::INDIGO_500.into(),
                    tw::EMERALD_500.into(),
                    tw::AMBER_500.into(),
                    tw::SKY_500.into(),
                    tw::ROSE_500.into(),
                    tw::VIOLET_500.into(),
                ],
                baseline: tw::SLATE_400.into(),
                gridline: Hsla::from(tw::SLATE_200).opacity(0.85),
                axis_label: tw::SLATE_500.into(),
                crosshair: Hsla::from(tw::SLATE_900).opacity(0.08),
            },
            ThemeMode::Dark => Self {
                colors: vec![
                    tw::INDIGO_400.into(),
                    tw::EMERALD_400.into(),
                    tw::AMBER_400.into(),
                    tw::SKY_400.into(),
                    tw::ROSE_400.into(),
                    tw::VIOLET_400.into(),
                ],
                baseline: tw::SLATE_500.into(),
                gridline: Hsla::from(tw::SLATE_700).opacity(0.65),
                axis_label: tw::SLATE_400.into(),
                crosshair: Hsla::from(tw::WHITE).opacity(0.07),
            },
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
    style.font_features = FontFeatures(Arc::new(vec![("tnum".to_string(), 1)]));
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
}

impl ChartLegendItem {
    /// Creates a dot-swatch legend item.
    pub fn dot(label: impl Into<SharedString>, color: Hsla) -> Self {
        Self {
            label: label.into(),
            color,
            swatch: LegendSwatch::Dot,
        }
    }

    /// Creates a solid-line legend item.
    pub fn solid_line(label: impl Into<SharedString>, color: Hsla) -> Self {
        Self {
            label: label.into(),
            color,
            swatch: LegendSwatch::SolidLine,
        }
    }

    /// Creates a dashed-line legend item.
    pub fn dashed_line(label: impl Into<SharedString>, color: Hsla) -> Self {
        Self {
            label: label.into(),
            color,
            swatch: LegendSwatch::DashedLine,
        }
    }
}

/// Renders a horizontal wrapping legend bar from `items`.
pub fn render_chart_legend(items: &[ChartLegendItem], theme: &Theme) -> impl IntoElement {
    let mut row = div().flex().flex_wrap().items_center().gap_x(px(16.)).gap_y(px(6.));
    for item in items {
        let swatch = match item.swatch {
            LegendSwatch::Dot => div()
                .w(px(8.))
                .h(px(8.))
                .rounded_full()
                .bg(item.color)
                .into_any_element(),
            LegendSwatch::SolidLine => div()
                .w(px(16.))
                .h(px(2.5))
                .rounded_full()
                .bg(item.color)
                .into_any_element(),
            LegendSwatch::DashedLine => div()
                .flex()
                .items_center()
                .gap(px(2.5))
                .child(div().w(px(4.)).h(px(2.)).rounded_full().bg(item.color))
                .child(div().w(px(4.)).h(px(2.)).rounded_full().bg(item.color))
                .child(div().w(px(4.)).h(px(2.)).rounded_full().bg(item.color))
                .into_any_element(),
        };

        row = row.child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(swatch)
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(theme.muted_foreground)
                        .child(item.label.clone()),
                ),
        );
    }
    row
}

/// Reusable analytics card container framing any chart with title, right metadata,
/// legend, main plot body, and an optional collapsible `"Show data table"` disclosure.
#[derive(IntoElement)]
pub struct ChartCard {
    id: ElementId,
    title: SharedString,
    subtitle: Option<SharedString>,
    legend: Vec<ChartLegendItem>,
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
            legend: Vec::new(),
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

    /// Sets legend items displayed beneath the card header.
    pub fn legend(mut self, items: impl IntoIterator<Item = ChartLegendItem>) -> Self {
        self.legend = items.into_iter().collect();
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
            .shadow_xs()
            .p(px(20.))
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .justify_between()
                    .gap(px(12.))
                    .child(
                        div()
                            .text_size(px(14.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.foreground)
                            .child(self.title),
                    )
                    .when_some(self.subtitle, |el, sub| {
                        el.child(
                            div()
                                .text_size(px(12.))
                                .text_color(theme.muted_foreground)
                                .child(sub),
                        )
                    }),
            );

        if !self.legend.is_empty() {
            card = card.child(render_chart_legend(&self.legend, &theme));
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
