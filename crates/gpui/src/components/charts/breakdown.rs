//! Ranked proportional horizontal [`BreakdownList`] with tabular figures, spring-animated
//! bar widths, and `"click to narrow"` cross-filtering support.

use std::rc::Rc;
use std::sync::Arc;

use crate::{
    AnimationExt, App, ClickEvent, ElementId, FontFeatures, FontWeight, Hsla, IntoElement,
    ParentElement, RenderOnce, SharedString, SpringAnimation, StyleRefinement, Styled, Window,
    div, prelude::*, px, relative,
};

use super::core::{ChartPalette, ChartValueFormat};
use crate::components::theme::{ActiveTheme, SPRING_SMOOTH};

/// One ranked row inside a [`BreakdownList`].
#[derive(Clone, Debug)]
pub struct BreakdownItem {
    /// Stable item key forwarded to `on_select` (e.g. `"claude-sonnet-5-5"`).
    pub key: SharedString,
    /// Display label.
    pub label: SharedString,
    /// Raw numeric value (used to compute relative share & bar fill width).
    pub value: f64,
    /// Optional explicit color override; defaults to [`ChartPalette`].
    pub color: Option<Hsla>,
}

impl BreakdownItem {
    /// Creates a new [`BreakdownItem`] where `key` and `label` share the same text.
    pub fn new(label: impl Into<SharedString>, value: f64) -> Self {
        let label = label.into();
        Self {
            key: label.clone(),
            label,
            value: value.max(0.0),
            color: None,
        }
    }

    /// Overrides the programmatic `key` passed to `on_select`.
    pub fn key(mut self, key: impl Into<SharedString>) -> Self {
        self.key = key.into();
        self
    }

    /// Overrides the dot & bar color.
    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }
}

/// Ranked proportional breakdown list (e.g. `Cost by model`) with colored dot, label,
/// tabular value + share %, animated horizontal bar, and interactive click-to-narrow filtering.
#[derive(IntoElement)]
pub struct BreakdownList {
    id: ElementId,
    items: Vec<BreakdownItem>,
    format: ChartValueFormat,
    palette: Option<ChartPalette>,
    selected_key: Option<SharedString>,
    hint_text: Option<SharedString>,
    style: StyleRefinement,
    on_select: Option<Rc<dyn Fn(&SharedString, &mut Window, &mut App) + 'static>>,
}

/// Convenience constructor for [`BreakdownList::new`].
pub fn breakdown_list(id: impl Into<ElementId>) -> BreakdownList {
    BreakdownList::new(id)
}

impl BreakdownList {
    /// Creates a new [`BreakdownList`].
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            items: Vec::new(),
            format: ChartValueFormat::Currency,
            palette: None,
            selected_key: None,
            hint_text: None,
            style: StyleRefinement::default(),
            on_select: None,
        }
    }

    /// Sets the breakdown rows.
    pub fn items(mut self, items: impl IntoIterator<Item = BreakdownItem>) -> Self {
        self.items = items.into_iter().collect();
        self
    }

    /// Sets the value formatter (default [`ChartValueFormat::Currency`]).
    pub fn value_format(mut self, format: ChartValueFormat) -> Self {
        self.format = format;
        self
    }

    /// Overrides the chart color palette (defaults to [`ChartPalette::from_theme`]).
    pub fn palette(mut self, palette: ChartPalette) -> Self {
        self.palette = Some(palette);
        self
    }

    /// Marks `key` as currently narrowed/selected (dimming non-selected rows subtly).
    pub fn selected_key(mut self, key: Option<SharedString>) -> Self {
        self.selected_key = key;
        self
    }

    /// Sets a subtle footer hint (e.g. `"click a row to narrow"`).
    pub fn hint(mut self, hint: impl Into<SharedString>) -> Self {
        self.hint_text = Some(hint.into());
        self
    }

    /// Invoked when the user clicks a row (e.g. to call `FilterBarState::toggle_narrow`).
    pub fn on_select(
        mut self,
        handler: impl Fn(&SharedString, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_select = Some(Rc::new(handler));
        self
    }
}

impl Styled for BreakdownList {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for BreakdownList {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let palette = self
            .palette
            .unwrap_or_else(|| ChartPalette::from_theme(&theme));
        let tnum = FontFeatures(Arc::new(vec![("tnum".to_string(), 1)]));
        let total: f64 = self.items.iter().map(|i| i.value).sum();
        let max_val = self
            .items
            .iter()
            .map(|i| i.value)
            .fold(0.0_f64, f64::max)
            .max(f64::EPSILON);

        let mut list = div().id(self.id.clone()).w_full().flex().flex_col().gap(px(12.));

        for (index, item) in self.items.into_iter().enumerate() {
            let color = item.color.unwrap_or_else(|| palette.color(index));
            let share_pct = if total > 0.0 {
                (item.value / total) * 100.0
            } else {
                0.0
            };
            let bar_fraction = (item.value / max_val).clamp(0.0, 1.0) as f32;
            let is_selected = self
                .selected_key
                .as_ref()
                .is_some_and(|sel| sel == &item.key);
            let is_dimmed = self.selected_key.is_some() && !is_selected;
            let clickable = self.on_select.is_some();
            let hover_bg = theme.surface_hover;
            let key = item.key.clone();
            let on_select = self.on_select.clone();

            let bar_fill = div()
                .h_full()
                .rounded_full()
                .bg(color)
                .with_spring(
                    (self.id.clone(), format!("bar-{index}")),
                    SpringAnimation::new(SPRING_SMOOTH).to(bar_fraction),
                    |el, frac| el.w(relative(frac.clamp(0.02, 1.0))),
                );

            let row = div()
                .id((self.id.clone(), format!("row-{index}")))
                .w_full()
                .px(px(8.))
                .py(px(6.))
                .mx(px(-8.))
                .rounded(theme.radius_md)
                .flex()
                .flex_col()
                .gap(px(6.))
                .when(is_dimmed, |el| el.opacity(0.5))
                .when(is_selected, |el| el.bg(theme.primary_soft.opacity(0.6)))
                .when(clickable, |el| {
                    el.cursor_pointer()
                        .hover(move |s| s.bg(hover_bg))
                        .when_some(on_select, |el, cb| {
                            el.on_click(move |_: &ClickEvent, window, cx| {
                                cb(&key, window, cx);
                            })
                        })
                })
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .justify_between()
                        .gap(px(12.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .min_w_0()
                                .child(
                                    div()
                                        .w(px(8.))
                                        .h(px(8.))
                                        .flex_shrink_0()
                                        .rounded_full()
                                        .bg(color),
                                )
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(theme.foreground)
                                        .truncate()
                                        .child(item.label),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .items_baseline()
                                .gap(px(8.))
                                .flex_shrink_0()
                                .font_features(tnum.clone())
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(theme.foreground)
                                        .child(self.format.format_detail(item.value)),
                                )
                                .child(
                                    div()
                                        .w(px(40.))
                                        .text_right()
                                        .text_size(px(12.))
                                        .text_color(theme.muted_foreground)
                                        .child(format!("{share_pct:.1}%")),
                                ),
                        ),
                )
                .child(
                    div()
                        .w_full()
                        .h(px(6.))
                        .rounded_full()
                        .bg(theme.surface_muted)
                        .overflow_hidden()
                        .child(bar_fill),
                );

            list = list.child(row);
        }

        if let Some(hint) = self.hint_text {
            list = list.child(
                div()
                    .pt(px(2.))
                    .text_size(px(11.5))
                    .text_color(theme.muted_foreground)
                    .child(hint),
            );
        }

        crate::Refineable::refine(list.style(), &self.style);
        list
    }
}
