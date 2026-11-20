//! [`Tabs`] navigation component in three classic styles:
//! [`TabsVariant::Underline`], [`TabsVariant::Pills`], and [`TabsVariant::Segmented`].

use std::rc::Rc;

use crate::{
    AnimationExt, App, ClickEvent, ElementId, FontWeight, IntoElement, KeyDownEvent, RenderOnce,
    SharedString,
    SpringAnimation, StyleRefinement, Styled, Window, div, prelude::*, px,
};
use crate::icons::{Icon, IconSize, LucideIcon};

use super::theme::{ActiveTheme, SPRING_SMOOTH};

/// Visual presentation style for [`Tabs`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TabsVariant {
    /// Bottom-bordered bar with active indigo underline.
    #[default]
    Underline,
    /// Soft rounded pill buttons.
    Pills,
    /// iOS-style segmented control container with elevated active tab.
    Segmented,
}

/// A single tab entry inside [`Tabs`].
#[derive(Clone, Debug)]
pub struct TabItem {
    /// Tab title text.
    pub label: SharedString,
    /// Optional leading [`LucideIcon`].
    pub icon: Option<LucideIcon>,
    /// Optional trailing count pill badge.
    pub badge: Option<SharedString>,
    /// Disables clicking this tab.
    pub disabled: bool,
}

impl TabItem {
    /// Creates a new tab item.
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            icon: None,
            badge: None,
            disabled: false,
        }
    }

    /// Adds a leading icon.
    pub fn icon(mut self, icon: LucideIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Adds a trailing count/status badge.
    pub fn badge(mut self, badge: impl Into<SharedString>) -> Self {
        self.badge = Some(badge.into());
        self
    }

    /// Disables this tab.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

/// Horizontal tab bar (`Underline`, `Pills`, or `Segmented`).
#[derive(IntoElement)]
pub struct Tabs {
    id: ElementId,
    items: Vec<TabItem>,
    selected_index: usize,
    variant: TabsVariant,
    full_width: bool,
    style: StyleRefinement,
    on_change: Option<Rc<dyn Fn(&usize, &mut Window, &mut App) + 'static>>,
}

/// Convenience constructor for [`Tabs::new`].
pub fn tabs(id: impl Into<ElementId>, items: Vec<TabItem>) -> Tabs {
    Tabs::new(id, items)
}

impl Tabs {
    /// Creates a new [`Tabs`] bar.
    pub fn new(id: impl Into<ElementId>, items: Vec<TabItem>) -> Self {
        Self {
            id: id.into(),
            items,
            selected_index: 0,
            variant: TabsVariant::Underline,
            full_width: false,
            style: StyleRefinement::default(),
            on_change: None,
        }
    }

    /// Sets the active tab index.
    pub fn selected_index(mut self, index: usize) -> Self {
        self.selected_index = index;
        self
    }

    /// Sets the visual variant.
    pub fn variant(mut self, variant: TabsVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Shorthand for [`TabsVariant::Pills`].
    pub fn pills(self) -> Self {
        self.variant(TabsVariant::Pills)
    }

    /// Shorthand for [`TabsVariant::Segmented`].
    pub fn segmented(self) -> Self {
        self.variant(TabsVariant::Segmented)
    }

    /// Distributes tabs evenly across 100% of container width.
    pub fn full_width(mut self, full_width: bool) -> Self {
        self.full_width = full_width;
        self
    }

    /// Registers a callback invoked with the newly selected tab index.
    pub fn on_change(
        mut self,
        handler: impl Fn(&usize, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

impl Styled for Tabs {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Tabs {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let mut bar = match self.variant {
            TabsVariant::Underline => div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(20.))
                .w_full()
                .border_b_1()
                .border_color(theme.border),
            TabsVariant::Pills => div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.))
                .when(self.full_width, |el| el.w_full()),
            TabsVariant::Segmented => div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(4.))
                .p(px(4.))
                .rounded(theme.radius_lg)
                .bg(theme.surface_muted)
                .when(self.full_width, |el| el.w_full()),
        };

        // One tab stop, the selected tab; the arrow keys move the choice
        // (`focus` module docs).
        let enabled: Vec<bool> = self.items.iter().map(|item| !item.disabled).collect();
        let handles = super::focus::choice_focus(
            (self.id.clone(), "tabs-focus"),
            self.items.len(),
            window,
            cx,
        );
        let tab_item = super::focus::choice_tab_item(Some(self.selected_index), &enabled);
        if let Some(on_change) = self.on_change.clone() {
            let handles = handles.clone();
            bar = bar.on_key_down(move |event: &KeyDownEvent, window, cx| {
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
                on_change(&to, window, cx);
                window.focus(&handles[to], cx);
            });
        }

        for (index, item) in self.items.into_iter().enumerate() {
            let active = index == self.selected_index;
            let tab_id = (self.id.clone(), format!("tab-{index}"));
            let on_change = self.on_change.clone();

            let fg = match self.variant {
                TabsVariant::Underline => {
                    if active {
                        theme.primary
                    } else {
                        theme.muted_foreground
                    }
                }
                TabsVariant::Pills => {
                    if active {
                        theme.primary_soft_foreground
                    } else {
                        theme.muted_foreground
                    }
                }
                TabsVariant::Segmented => {
                    if active {
                        theme.foreground
                    } else {
                        theme.muted_foreground
                    }
                }
            };

            let mut tab = div()
                .id(tab_id)
                .flex()
                .flex_row()
                .items_center()
                .justify_center()
                .gap(px(7.))
                .text_size(px(13.5))
                .font_weight(if active {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::MEDIUM
                })
                .text_color(fg)
                .whitespace_nowrap()
                .when(self.full_width, |el| el.flex_1());

            tab = match self.variant {
                TabsVariant::Underline => tab
                    .pb(px(10.))
                    .pt(px(4.))
                    .px(px(2.))
                    .border_b_2()
                    .border_color(if active {
                        theme.primary
                    } else {
                        crate::transparent_black()
                    }),
                TabsVariant::Pills => tab
                    .px(px(12.))
                    .py(px(7.))
                    .rounded(theme.radius_md)
                    .bg(if active {
                        theme.primary_soft
                    } else {
                        crate::transparent_black()
                    }),
                TabsVariant::Segmented => tab
                    .px(px(12.))
                    .py(px(6.))
                    .rounded(theme.radius_md)
                    .when(active, |el| el.bg(theme.surface).shadow_xs()),
            };

            if item.disabled {
                tab = tab.opacity(0.45).cursor_not_allowed();
            } else {
                let ring = super::focus::focus_visible_ring(&handles[index], window, &theme);
                tab = tab
                    .track_focus(&super::focus::choice_item_handle(&handles, index, tab_item))
                    .when_some(ring, |el, ring| el.shadow(ring))
                    .cursor_pointer().when(!active, |el| {
                    el.hover(|s| match self.variant {
                        TabsVariant::Underline => s.text_color(theme.foreground),
                        TabsVariant::Pills => {
                            s.bg(theme.surface_hover).text_color(theme.foreground)
                        }
                        TabsVariant::Segmented => s.text_color(theme.foreground),
                    })
                });
                if let Some(handler) = on_change {
                    tab = tab.on_click(move |_: &ClickEvent, window, cx| {
                        handler(&index, window, cx);
                    });
                }
            }

            if let Some(icon) = item.icon {
                tab = tab.child(Icon::new(icon).size(IconSize::Sm).color(fg));
            }
            tab = tab.child(item.label);
            if let Some(badge) = item.badge {
                tab = tab.child(
                    div()
                        .px(px(6.))
                        .py(px(1.))
                        .rounded_full()
                        .text_size(px(11.))
                        .bg(if active {
                            theme.primary.opacity(0.15)
                        } else {
                            theme.surface_muted
                        })
                        .text_color(if active {
                            theme.primary
                        } else {
                            theme.muted_foreground
                        })
                        .child(badge),
                );
            }

            let tab_anim_id = (self.id.clone(), format!("tab-anim-{index}"));
            let variant = self.variant;
            let primary = theme.primary;
            let primary_soft = theme.primary_soft;
            let surface = theme.surface;
            let transparent = crate::transparent_black();

            let animated_tab = tab.with_spring(
                tab_anim_id,
                SpringAnimation::new(SPRING_SMOOTH).to(active),
                move |el, phase| match variant {
                    TabsVariant::Underline => {
                        el.border_color(phase.interpolate_clamped(transparent, primary))
                    }
                    TabsVariant::Pills => {
                        el.bg(phase.interpolate_clamped(transparent, primary_soft))
                    }
                    TabsVariant::Segmented => {
                        el.bg(phase.interpolate_clamped(transparent, surface))
                    }
                },
            );

            bar = bar.child(animated_tab);
        }

        crate::Refineable::refine(bar.style(), &self.style);
        bar
    }
}
