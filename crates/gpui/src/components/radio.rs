//! [`RadioGroup`] supporting classic radio lists and bordered Radio Cards.

use std::rc::Rc;

use crate::{
    AnimationExt, App, ClickEvent, ElementId, FontWeight, IntoElement, KeyDownEvent, RenderOnce,
    SharedString,
    SpringAnimation, StyleRefinement, Styled, Window, div, prelude::*, px,
};
use crate::icons::{Icon, IconSize, LucideIcon};

use super::theme::{ActiveTheme, SPRING_SMOOTH, SPRING_SNAPPY};

/// Presentation style for [`RadioGroup`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RadioVariant {
    /// Traditional circular radio button list.
    #[default]
    List,
    /// Bordered clickable cards with accent ring and check indicator.
    Cards,
}

/// A single option inside a [`RadioGroup`].
#[derive(Clone, Debug)]
pub struct RadioOption<T> {
    /// Underlying value returned when selected.
    pub value: T,
    /// Primary display label.
    pub label: SharedString,
    /// Optional muted helper description.
    pub description: Option<SharedString>,
    /// Optional trailing meta text (e.g. `"$12/mo"`).
    pub meta: Option<SharedString>,
    /// Whether this individual option is disabled.
    pub disabled: bool,
}

impl<T> RadioOption<T> {
    /// Creates a new radio option with `value` and `label`.
    pub fn new(value: T, label: impl Into<SharedString>) -> Self {
        Self {
            value,
            label: label.into(),
            description: None,
            meta: None,
            disabled: false,
        }
    }

    /// Adds descriptive helper text.
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Adds trailing metadata text (especially useful with [`RadioVariant::Cards`]).
    pub fn meta(mut self, meta: impl Into<SharedString>) -> Self {
        self.meta = Some(meta.into());
        self
    }

    /// Disables selection of this option.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

/// Radio group (`List` or `Cards`) parameterized by option value type `T`.
#[derive(IntoElement)]
pub struct RadioGroup<T: Clone + PartialEq + 'static> {
    id: ElementId,
    options: Vec<RadioOption<T>>,
    selected: Option<T>,
    variant: RadioVariant,
    horizontal: bool,
    disabled: bool,
    style: StyleRefinement,
    on_change: Option<Rc<dyn Fn(&T, &mut Window, &mut App) + 'static>>,
}

/// Convenience constructor for [`RadioGroup::new`].
pub fn radio_group<T: Clone + PartialEq + 'static>(
    id: impl Into<ElementId>,
    options: Vec<RadioOption<T>>,
) -> RadioGroup<T> {
    RadioGroup::new(id, options)
}

impl<T: Clone + PartialEq + 'static> RadioGroup<T> {
    /// Creates a new [`RadioGroup`] with `options`.
    pub fn new(id: impl Into<ElementId>, options: Vec<RadioOption<T>>) -> Self {
        Self {
            id: id.into(),
            options,
            selected: None,
            variant: RadioVariant::List,
            horizontal: false,
            disabled: false,
            style: StyleRefinement::default(),
            on_change: None,
        }
    }

    /// Sets the currently selected value.
    pub fn selected(mut self, value: Option<T>) -> Self {
        self.selected = value;
        self
    }

    /// Sets [`RadioVariant::List`] or [`RadioVariant::Cards`].
    pub fn variant(mut self, variant: RadioVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Shorthand for [`RadioVariant::Cards`].
    pub fn cards(self) -> Self {
        self.variant(RadioVariant::Cards)
    }

    /// Arranges options horizontally.
    pub fn horizontal(mut self, horizontal: bool) -> Self {
        self.horizontal = horizontal;
        self
    }

    /// Disables all options in this group.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Registers a callback invoked whenever an option is chosen.
    pub fn on_change(mut self, handler: impl Fn(&T, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

impl<T: Clone + PartialEq + 'static> Styled for RadioGroup<T> {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl<T: Clone + PartialEq + 'static> RenderOnce for RadioGroup<T> {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let mut container = div()
            .id(self.id.clone())
            .flex()
            .w_full()
            .when(self.horizontal, |el| el.flex_row().flex_wrap().gap(px(12.)))
            .when(!self.horizontal, |el| el.flex_col().gap(px(10.)));

        // One tab stop, the selected option; the arrow keys move the choice
        // (`focus` module docs).
        let enabled: Vec<bool> = self
            .options
            .iter()
            .map(|option| !self.disabled && !option.disabled)
            .collect();
        let handles = super::focus::choice_focus(
            (self.id.clone(), "radio-focus"),
            self.options.len(),
            window,
            cx,
        );
        let selected_index = self
            .options
            .iter()
            .position(|option| self.selected.as_ref() == Some(&option.value));
        let tab_item = super::focus::choice_tab_item(selected_index, &enabled);
        if let Some(on_change) = self.on_change.clone() {
            let values: Vec<T> = self.options.iter().map(|option| option.value.clone()).collect();
            let handles = handles.clone();
            container = container.on_key_down(move |event: &KeyDownEvent, window, cx| {
                let Some(from) = handles.iter().position(|h| h.is_focused(window)) else {
                    return;
                };
                if event.keystroke.modifiers.modified() {
                    return;
                }
                let Some(to) = super::focus::choice_step(&event.keystroke.key, from, &enabled)
                else {
                    return;
                };
                cx.stop_propagation();
                on_change(&values[to], window, cx);
                window.focus(&handles[to], cx);
            });
        }

        for (index, option) in self.options.into_iter().enumerate() {
            let is_selected = self
                .selected
                .as_ref()
                .is_some_and(|sel| *sel == option.value);
            let item_disabled = self.disabled || option.disabled;
            let item_id = (self.id.clone(), format!("opt-{index}"));
            let value = option.value.clone();
            let on_change = self.on_change.clone();
            let focus = (!item_disabled)
                .then(|| super::focus::choice_item_handle(&handles, index, tab_item));
            let ring = super::focus::focus_visible_ring(&handles[index], window, &theme);

            let row = match self.variant {
                RadioVariant::List => {
                    let off_bg = theme.surface;
                    let on_bg = theme.primary;
                    let off_border = theme.input_border;
                    let on_border = theme.primary;
                    let circle_id = (self.id.clone(), format!("radio-ring-{index}"));
                    let dot_id = (self.id.clone(), format!("radio-dot-{index}"));

                    let dot = div()
                        .rounded_full()
                        .bg(theme.primary_foreground)
                        .with_spring(
                            dot_id,
                            SpringAnimation::new(SPRING_SNAPPY).to(is_selected),
                            |el, phase| {
                                let dim = phase.0.clamp(0.0, 1.15) * 6.0;
                                el.w(px(dim)).h(px(dim)).opacity(phase.0.clamp(0.0, 1.0))
                            },
                        );

                    let circle = div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .flex_shrink_0()
                        .w(px(18.))
                        .h(px(18.))
                        .mt(px(1.))
                        .rounded_full()
                        .border_1()
                        .when_some(ring, |el, ring| el.shadow(ring))
                        .child(dot)
                        .with_spring(
                            circle_id,
                            SpringAnimation::new(SPRING_SNAPPY).to(is_selected),
                            move |el, phase| {
                                el.bg(phase.interpolate_clamped(off_bg, on_bg))
                                    .border_color(phase.interpolate_clamped(off_border, on_border))
                            },
                        );

                    div()
                        .id(item_id)
                        .flex()
                        .flex_row()
                        .items_start()
                        .gap(px(10.))
                        .when(!item_disabled, |el| el.cursor_pointer())
                        .when(item_disabled, |el| el.opacity(0.55).cursor_not_allowed())
                        .child(circle)
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(theme.foreground)
                                        .child(option.label),
                                )
                                .when_some(option.description, |el, desc| {
                                    el.child(
                                        div()
                                            .text_size(px(12.5))
                                            .text_color(theme.muted_foreground)
                                            .child(desc),
                                    )
                                }),
                        )
                        .into_any_element()
                }
                RadioVariant::Cards => {
                    let off_bg = theme.surface;
                    let on_bg = theme.primary_soft.opacity(0.55);
                    let off_border = theme.border;
                    let on_border = theme.primary;
                    let card_anim_id = (self.id.clone(), format!("radio-card-{index}"));
                    let card_div = div()
                        .id(item_id)
                        .when_some(focus.clone(), |el, handle| el.track_focus(&handle))
                        .flex()
                        .flex_row()
                        .items_start()
                        .justify_between()
                        .gap(px(12.))
                        .when(self.horizontal, |el| el.flex_1().min_w(px(180.)))
                        .when(!self.horizontal, |el| el.w_full())
                        .p(px(14.))
                        .rounded(theme.radius_lg)
                        .border_1()
                        .shadow_2xs()
                        .when_some(ring, |el, ring| el.shadow(ring))
                        .when(!item_disabled, |el| {
                            el.cursor_pointer().hover(|s| {
                                if is_selected {
                                    s
                                } else {
                                    s.border_color(theme.input_border_hover)
                                }
                            })
                        })
                        .when(item_disabled, |el| el.opacity(0.55).cursor_not_allowed())
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(3.))
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(theme.foreground)
                                        .child(option.label),
                                )
                                .when_some(option.description, |el, desc| {
                                    el.child(
                                        div()
                                            .text_size(px(12.5))
                                            .text_color(theme.muted_foreground)
                                            .child(desc),
                                    )
                                })
                                .when_some(option.meta, |el, meta| {
                                    el.child(
                                        div()
                                            .pt(px(4.))
                                            .text_size(px(12.5))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(theme.foreground)
                                            .child(meta),
                                    )
                                }),
                        )
                        .child(if is_selected {
                            Icon::new(LucideIcon::CircleCheck)
                                .size(IconSize::Lg)
                                .color(theme.primary)
                                .into_any_element()
                        } else {
                            div()
                                .w(px(18.))
                                .h(px(18.))
                                .rounded_full()
                                .border_1()
                                .border_color(theme.input_border)
                                .into_any_element()
                        });

                    let card_div = if !item_disabled
                        && let Some(handler) = on_change.clone()
                    {
                        let value_c = value.clone();
                        card_div.on_click(move |_: &ClickEvent, window, cx| {
                            handler(&value_c, window, cx);
                        })
                    } else {
                        card_div
                    };

                    card_div
                        .with_spring(
                            card_anim_id,
                            SpringAnimation::new(SPRING_SMOOTH).to(is_selected),
                            move |el, phase| {
                                el.bg(phase.interpolate_clamped(off_bg, on_bg))
                                    .border_color(phase.interpolate_clamped(off_border, on_border))
                            },
                        )
                        .into_any_element()
                }
            };

            // The wrapper takes the clicks, so it's also what holds focus:
            // a keyboard click goes to the focused element.
            let row = if self.variant == RadioVariant::List
                && let Some(handle) = focus
            {
                div()
                    .id((self.id.clone(), format!("radio-wrap-{index}")))
                    .track_focus(&handle)
                    .when_some(on_change, |el, handler| {
                        el.on_click(move |_: &ClickEvent, window, cx| {
                            handler(&value, window, cx);
                        })
                    })
                    .child(row)
                    .into_any_element()
            } else {
                row
            };

            container = container.child(row);
        }

        crate::Refineable::refine(container.style(), &self.style);
        container
    }
}
