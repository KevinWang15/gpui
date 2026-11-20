//! [`Checkbox`] (supporting checked, unchecked, and indeterminate states)
//! and [`CheckboxGroup`].

use smallvec::SmallVec;

use crate::{
    AnimationExt, AnyElement, App, ClickEvent, ElementId, FontWeight, IntoElement, ParentElement,
    RenderOnce, SharedString, SpringAnimation, StyleRefinement, Styled, Window, div, prelude::*, px,
};
use crate::icons::{Icon, IconSize, LucideIcon};

use super::theme::{ActiveTheme, SPRING_SNAPPY};

/// Tri-state value for [`Checkbox`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CheckState {
    /// Unchecked box.
    #[default]
    Unchecked,
    /// Checked box with a checkmark icon.
    Checked,
    /// Indeterminate box with a horizontal minus icon (e.g. "select all" partial state).
    Indeterminate,
}

impl CheckState {
    /// Returns `true` if [`CheckState::Checked`].
    pub const fn is_checked(self) -> bool {
        matches!(self, Self::Checked)
    }

    /// Returns the next boolean state when clicked (`Unchecked` or `Indeterminate` -> `true`, `Checked` -> `false`).
    pub const fn toggled_bool(self) -> bool {
        !matches!(self, Self::Checked)
    }
}

impl From<bool> for CheckState {
    fn from(value: bool) -> Self {
        if value {
            Self::Checked
        } else {
            Self::Unchecked
        }
    }
}

/// Checkbox with optional label, description, indeterminate indicator,
/// disabled state, and error styling.
#[derive(IntoElement)]
pub struct Checkbox {
    id: ElementId,
    state: CheckState,
    label: Option<SharedString>,
    description: Option<SharedString>,
    disabled: bool,
    invalid: bool,
    style: StyleRefinement,
    on_change: Option<Box<dyn Fn(&bool, &mut Window, &mut App) + 'static>>,
}

/// Convenience constructor for [`Checkbox::new`].
pub fn checkbox(id: impl Into<ElementId>, state: impl Into<CheckState>) -> Checkbox {
    Checkbox::new(id, state)
}

impl Checkbox {
    /// Creates a new [`Checkbox`] from a `bool` or [`CheckState`].
    pub fn new(id: impl Into<ElementId>, state: impl Into<CheckState>) -> Self {
        Self {
            id: id.into(),
            state: state.into(),
            label: None,
            description: None,
            disabled: false,
            invalid: false,
            style: StyleRefinement::default(),
            on_change: None,
        }
    }

    /// Sets the label text next to the checkbox.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets muted description text beneath the label.
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Disables pointer interaction.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Applies destructive error border styling.
    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    /// Registers a change handler invoked with the new checked boolean.
    pub fn on_change(
        mut self,
        handler: impl Fn(&bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Some(Box::new(handler));
        self
    }
}

impl Styled for Checkbox {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Checkbox {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        // The row is the tab stop, but the ring goes on the box, as a
        // browser draws it; so the row tracks a handle the box can ask about.
        let focus = (!self.disabled).then(|| {
            window
                .use_keyed_state((self.id.clone(), "focus"), cx, |_, cx| cx.focus_handle())
                .read(cx)
                .clone()
        });
        let ring = focus
            .as_ref()
            .and_then(|handle| super::focus::focus_visible_ring(handle, window, &theme));
        let active = !matches!(self.state, CheckState::Unchecked);
        let off_bg = if self.disabled {
            theme.disabled_bg
        } else {
            theme.surface
        };
        let on_bg = theme.primary;
        let off_border = if self.invalid {
            theme.destructive
        } else {
            theme.input_border
        };
        let on_border = if self.invalid {
            theme.destructive
        } else {
            theme.primary
        };
        let box_id = (self.id.clone(), "box");
        let mark_id = (self.id.clone(), "mark");
        let fg = theme.primary_foreground;
        let icon_opt = match self.state {
            CheckState::Checked => Some(LucideIcon::Check),
            CheckState::Indeterminate => Some(LucideIcon::Minus),
            CheckState::Unchecked => None,
        };

        let box_el = div()
            .flex()
            .items_center()
            .justify_center()
            .flex_shrink_0()
            .w(px(18.))
            .h(px(18.))
            .mt(px(1.))
            .rounded(theme.radius_sm)
            .border_1()
            .when(!active && !self.disabled, |el| el.shadow_2xs())
            .when_some(ring, |el, ring| el.shadow(ring))
            .when_some(icon_opt, |el, icon| {
                el.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            Icon::new(icon)
                                .size(IconSize::Xs)
                                .stroke_width(2.75)
                                .color(fg),
                        )
                        .with_spring(
                            mark_id,
                            SpringAnimation::new(SPRING_SNAPPY).to(true).from(false),
                            |el, phase| {
                                let p = phase.0.clamp(0.0, 1.0);
                                el.opacity(p).mt(px((1.0 - p) * 2.5))
                            },
                        ),
                )
            })
            .with_spring(
                box_id,
                SpringAnimation::new(SPRING_SNAPPY).to(active),
                move |el, phase| {
                    el.bg(phase.interpolate_clamped(off_bg, on_bg))
                        .border_color(phase.interpolate_clamped(off_border, on_border))
                },
            );

        let next_value = self.state.toggled_bool();
        let has_text = self.label.is_some() || self.description.is_some();

        let mut root = div()
            .id(self.id)
            .when_some(focus, |el, handle| el.track_focus(&handle).tab_stop(true))
            .flex()
            .flex_row()
            .items_start()
            .gap(px(10.))
            .when(!self.disabled, |el| el.cursor_pointer())
            .when(self.disabled, |el| el.opacity(0.55).cursor_not_allowed())
            .child(box_el)
            .when(has_text, |el| {
                el.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .when_some(self.label, |el, label| {
                            el.child(
                                div()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.foreground)
                                    .child(label),
                            )
                        })
                        .when_some(self.description, |el, desc| {
                            el.child(
                                div()
                                    .text_size(px(12.5))
                                    .text_color(theme.muted_foreground)
                                    .child(desc),
                            )
                        }),
                )
            });

        if !self.disabled
            && let Some(handler) = self.on_change
        {
            root = root.on_click(move |_: &ClickEvent, window, cx| {
                handler(&next_value, window, cx);
            });
        }

        crate::Refineable::refine(root.style(), &self.style);
        root
    }
}

/// Vertical or horizontal container grouping multiple [`Checkbox`] controls with an optional legend.
#[derive(IntoElement)]
pub struct CheckboxGroup {
    legend: Option<SharedString>,
    horizontal: bool,
    style: StyleRefinement,
    children: SmallVec<[AnyElement; 4]>,
}

/// Creates a new [`CheckboxGroup`].
pub fn checkbox_group() -> CheckboxGroup {
    CheckboxGroup {
        legend: None,
        horizontal: false,
        style: StyleRefinement::default(),
        children: SmallVec::new(),
    }
}

impl CheckboxGroup {
    /// Sets an optional group legend heading.
    pub fn legend(mut self, legend: impl Into<SharedString>) -> Self {
        self.legend = Some(legend.into());
        self
    }

    /// Arranges checkboxes horizontally with wrapping.
    pub fn horizontal(mut self, horizontal: bool) -> Self {
        self.horizontal = horizontal;
        self
    }
}

impl ParentElement for CheckboxGroup {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for CheckboxGroup {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for CheckboxGroup {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let mut el = div()
            .arrow_navigation(crate::ArrowNavigation::Both)
            .flex()
            .flex_col()
            .gap(px(8.))
            .when_some(self.legend, |el, legend| {
                el.child(
                    div()
                        .text_size(px(13.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.foreground)
                        .child(legend),
                )
            })
            .child(
                div()
                    .flex()
                    .when(self.horizontal, |el| el.flex_row().flex_wrap().gap(px(16.)))
                    .when(!self.horizontal, |el| el.flex_col().gap(px(10.)))
                    .children(self.children),
            );
        crate::Refineable::refine(el.style(), &self.style);
        el
    }
}
