//! Tailwind-inspired [`RadioGroup`] supporting classic radio lists and bordered Tailwind Radio Cards.

use std::rc::Rc;

use crate::{
    App, ClickEvent, ElementId, FontWeight, IntoElement, RenderOnce, SharedString, StyleRefinement,
    Styled, Window, div, prelude::*, px,
};
use crate::icons::{Icon, IconSize, LucideIcon};

use super::palette as tw;
use super::theme::ActiveTheme;

/// Presentation style for [`RadioGroup`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RadioVariant {
    /// Traditional circular radio button list.
    #[default]
    List,
    /// Tailwind UI bordered clickable cards with accent ring and check indicator.
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

/// Tailwind-style radio group (`List` or `Cards`) parameterized by option value type `T`.
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
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let mut container = div()
            .id(self.id.clone())
            .flex()
            .w_full()
            .when(self.horizontal, |el| el.flex_row().flex_wrap().gap(px(12.)))
            .when(!self.horizontal, |el| el.flex_col().gap(px(10.)));

        for (index, option) in self.options.into_iter().enumerate() {
            let is_selected = self
                .selected
                .as_ref()
                .is_some_and(|sel| *sel == option.value);
            let item_disabled = self.disabled || option.disabled;
            let item_id = (self.id.clone(), format!("opt-{index}"));
            let value = option.value.clone();
            let on_change = self.on_change.clone();

            let row = match self.variant {
                RadioVariant::List => {
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
                        .border_color(if is_selected {
                            theme.primary
                        } else {
                            theme.input_border
                        })
                        .bg(if is_selected {
                            theme.primary
                        } else {
                            theme.surface
                        })
                        .when(is_selected, |el| {
                            el.child(div().w(px(6.)).h(px(6.)).rounded_full().bg(tw::WHITE))
                        });

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
                }
                RadioVariant::Cards => div()
                    .id(item_id)
                    .flex()
                    .flex_row()
                    .items_start()
                    .justify_between()
                    .gap(px(12.))
                    .when(self.horizontal, |el| el.flex_1().min_w(px(180.)))
                    .when(!self.horizontal, |el| el.w_full())
                    .p(px(14.))
                    .rounded(theme.radius_lg)
                    .bg(if is_selected {
                        theme.primary_soft.opacity(0.55)
                    } else {
                        theme.surface
                    })
                    .border_1()
                    .border_color(if is_selected {
                        theme.primary
                    } else {
                        theme.border
                    })
                    .shadow_2xs()
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
                    }),
            };

            let row = if !item_disabled
                && let Some(handler) = on_change
            {
                row.on_click(move |_: &ClickEvent, window, cx| {
                    handler(&value, window, cx);
                })
            } else {
                row
            };

            container = container.child(row);
        }

        crate::Refineable::refine(container.style(), &self.style);
        container
    }
}
