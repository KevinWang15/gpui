//! Tailwind-inspired [`Toggle`] / [`Switch`], [`ToggleButton`], and [`ToggleGroup`] components.

use smallvec::SmallVec;

use crate::{
    AnyElement, App, ClickEvent, ElementId, FontWeight, IntoElement, KeyDownEvent, ParentElement,
    RenderOnce, SharedString, StyleRefinement, Styled, Window, div, prelude::*, px,
};
use crate::icons::{Icon, IconSize, LucideIcon};

use super::palette as tw;
use super::theme::{ActiveTheme, ComponentSize};

/// Alias for [`Toggle`].
pub type Switch = Toggle;

/// Tailwind UI Toggle / Headless UI Switch with pill track, sliding thumb, optional
/// on/off icons inside the thumb, and optional label & description.
#[derive(IntoElement)]
pub struct Toggle {
    id: ElementId,
    checked: bool,
    disabled: bool,
    size: ComponentSize,
    with_icons: bool,
    label: Option<SharedString>,
    description: Option<SharedString>,
    label_left: bool,
    style: StyleRefinement,
    on_change: Option<Box<dyn Fn(&bool, &mut Window, &mut App) + 'static>>,
}

/// Convenience constructor for [`Toggle::new`].
pub fn toggle(id: impl Into<ElementId>, checked: bool) -> Toggle {
    Toggle::new(id, checked)
}

/// Convenience constructor alias for [`Toggle::new`].
pub fn switch(id: impl Into<ElementId>, checked: bool) -> Switch {
    Toggle::new(id, checked)
}

impl Toggle {
    /// Creates a new [`Toggle`] in the given `checked` state.
    pub fn new(id: impl Into<ElementId>, checked: bool) -> Self {
        Self {
            id: id.into(),
            checked,
            disabled: false,
            size: ComponentSize::Md,
            with_icons: false,
            label: None,
            description: None,
            label_left: false,
            style: StyleRefinement::default(),
            on_change: None,
        }
    }

    /// Sets the toggle size (`Sm`, `Md`, or `Lg`).
    pub fn size(mut self, size: ComponentSize) -> Self {
        self.size = size;
        self
    }

    /// Renders subtle check/x Lucide icons inside the circular thumb.
    pub fn with_icons(mut self, with_icons: bool) -> Self {
        self.with_icons = with_icons;
        self
    }

    /// Attaches a label next to the toggle switch.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Attaches a muted description under the label.
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Places label/description on the left with the switch aligned to the right.
    pub fn label_left(mut self, label_left: bool) -> Self {
        self.label_left = label_left;
        self
    }

    /// Disables user interaction.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Registers a callback invoked with the new boolean state whenever toggled.
    pub fn on_change(
        mut self,
        handler: impl Fn(&bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Some(Box::new(handler));
        self
    }
}

impl Styled for Toggle {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Toggle {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (track_w, track_h, thumb_size, pad) = match self.size {
            ComponentSize::Xs | ComponentSize::Sm => (px(34.), px(20.), px(16.), px(2.)),
            ComponentSize::Md => (px(44.), px(24.), px(20.), px(2.)),
            ComponentSize::Lg | ComponentSize::Xl => (px(52.), px(28.), px(24.), px(2.)),
        };

        let track_bg = if self.checked {
            theme.primary
        } else {
            theme.input_border
        };

        let thumb = div()
            .flex()
            .items_center()
            .justify_center()
            .w(thumb_size)
            .h(thumb_size)
            .rounded_full()
            .bg(tw::WHITE)
            .shadow_xs()
            .when(self.with_icons, |el| {
                let icon = if self.checked {
                    LucideIcon::Check
                } else {
                    LucideIcon::X
                };
                let color = if self.checked {
                    theme.primary
                } else {
                    theme.muted_foreground
                };
                el.child(Icon::new(icon).size(IconSize::Xs).color(color))
            });

        let track = div()
            .flex()
            .flex_row()
            .items_center()
            .flex_shrink_0()
            .w(track_w)
            .h(track_h)
            .p(pad)
            .rounded_full()
            .bg(track_bg)
            .when(self.checked, |el| el.justify_end())
            .when(!self.checked, |el| el.justify_start())
            .child(thumb);

        let has_text = self.label.is_some() || self.description.is_some();
        let text_block = if has_text {
            Some(
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
        } else {
            None
        };

        let next_checked = !self.checked;
        let on_change = self.on_change;
        let mut root = div()
            .id(self.id)
            .flex()
            .flex_row()
            .items_center()
            .gap(px(12.))
            .when(self.label_left && has_text, |el| el.justify_between().w_full())
            .when(!self.disabled, |el| el.cursor_pointer())
            .when(self.disabled, |el| el.opacity(0.55).cursor_not_allowed());

        if self.label_left {
            if let Some(text) = text_block {
                root = root.child(text);
            }
            root = root.child(track);
        } else {
            root = root.child(track);
            if let Some(text) = text_block {
                root = root.child(text);
            }
        }

        if !self.disabled
            && let Some(handler) = on_change
        {
            let handler = std::rc::Rc::new(handler);
            let click_handler = handler.clone();
            root = root
                .on_click(move |_: &ClickEvent, window, cx| {
                    click_handler(&next_checked, window, cx);
                })
                .on_key_down(move |event: &KeyDownEvent, window, cx| {
                    if event.keystroke.key == "space" || event.keystroke.key == "enter" {
                        handler(&next_checked, window, cx);
                    }
                });
        }

        crate::Refineable::refine(root.style(), &self.style);
        root
    }
}

/// Pressable toolbar toggle button (e.g. Bold / Italic / Grid view switcher).
#[derive(IntoElement)]
pub struct ToggleButton {
    id: ElementId,
    label: Option<SharedString>,
    icon: Option<LucideIcon>,
    pressed: bool,
    disabled: bool,
    size: ComponentSize,
    on_toggle: Option<Box<dyn Fn(&bool, &mut Window, &mut App) + 'static>>,
}

impl ToggleButton {
    /// Creates a new [`ToggleButton`].
    pub fn new(id: impl Into<ElementId>, pressed: bool) -> Self {
        Self {
            id: id.into(),
            label: None,
            icon: None,
            pressed,
            disabled: false,
            size: ComponentSize::Sm,
            on_toggle: None,
        }
    }

    /// Sets the button label.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets a [`LucideIcon`].
    pub fn icon(mut self, icon: LucideIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Sets button size.
    pub fn size(mut self, size: ComponentSize) -> Self {
        self.size = size;
        self
    }

    /// Disables the button.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Registers a toggle callback invoked with the toggled boolean state.
    pub fn on_toggle(
        mut self,
        handler: impl Fn(&bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_toggle = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for ToggleButton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let next = !self.pressed;
        let (bg, fg) = if self.pressed {
            (theme.primary_soft, theme.primary_soft_foreground)
        } else {
            (crate::transparent_black(), theme.muted_foreground)
        };

        div()
            .id(self.id)
            .flex()
            .flex_row()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .h(self.size.height())
            .px(self.size.px_x())
            .rounded(theme.radius_sm)
            .bg(bg)
            .text_color(fg)
            .text_size(self.size.font_size())
            .font_weight(FontWeight::MEDIUM)
            .when(!self.disabled, |el| {
                el.cursor_pointer().hover(|s| {
                    if self.pressed {
                        s.bg(theme.primary_soft)
                    } else {
                        s.bg(theme.surface_hover).text_color(theme.foreground)
                    }
                })
            })
            .when(self.disabled, |el| el.opacity(0.5).cursor_not_allowed())
            .when_some(self.icon, |el, icon| {
                el.child(Icon::new(icon).size(self.size.icon_size()).color(fg))
            })
            .when_some(self.label, |el, label| el.child(label))
            .when(!self.disabled, |el| {
                el.when_some(self.on_toggle, |el, handler| {
                    el.on_click(move |_, window, cx| handler(&next, window, cx))
                })
            })
    }
}

/// Framed horizontal group of [`ToggleButton`] controls.
#[derive(IntoElement)]
pub struct ToggleGroup {
    style: StyleRefinement,
    children: SmallVec<[AnyElement; 4]>,
}

/// Creates a new [`ToggleGroup`].
pub fn toggle_group() -> ToggleGroup {
    ToggleGroup {
        style: StyleRefinement::default(),
        children: SmallVec::new(),
    }
}

impl ParentElement for ToggleGroup {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for ToggleGroup {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for ToggleGroup {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let mut el = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(2.))
            .p(px(3.))
            .rounded(theme.radius_md)
            .bg(theme.surface)
            .border_1()
            .border_color(theme.border)
            .children(self.children);
        crate::Refineable::refine(el.style(), &self.style);
        el
    }
}
