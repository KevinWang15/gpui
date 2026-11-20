//! Collapsible [`Disclosure`] and [`Accordion`] components.

use smallvec::SmallVec;

use crate::{
    Animation, AnimationExt, AnyElement, App, ClickEvent, ElementId, FontWeight, IntoElement,
    ParentElement, RenderOnce, SharedString, SpringAnimation, StyleRefinement, Styled, Window,
    div, ease_out_quint, prelude::*, px, radians,
};
use crate::icons::{Icon, IconSize, LucideIcon};

use super::theme::{ANIM_FAST, ActiveTheme, SPRING_SNAPPY};

/// Single collapsible disclosure section (Headless UI `<Disclosure>`).
#[derive(IntoElement)]
pub struct Disclosure {
    id: ElementId,
    title: SharedString,
    subtitle: Option<SharedString>,
    icon: Option<LucideIcon>,
    open: bool,
    bordered: bool,
    children: SmallVec<[AnyElement; 3]>,
    on_toggle: Option<Box<dyn Fn(&bool, &mut Window, &mut App) + 'static>>,
    style: StyleRefinement,
}

/// Convenience constructor for [`Disclosure::new`].
pub fn disclosure(
    id: impl Into<ElementId>,
    title: impl Into<SharedString>,
    open: bool,
) -> Disclosure {
    Disclosure::new(id, title, open)
}

impl Disclosure {
    /// Creates a new collapsible [`Disclosure`].
    pub fn new(id: impl Into<ElementId>, title: impl Into<SharedString>, open: bool) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            subtitle: None,
            icon: None,
            open,
            bordered: true,
            children: SmallVec::new(),
            on_toggle: None,
            style: StyleRefinement::default(),
        }
    }

    /// Sets optional muted subtitle next to or under the title.
    pub fn subtitle(mut self, subtitle: impl Into<SharedString>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// Sets an optional leading [`LucideIcon`].
    pub fn icon(mut self, icon: LucideIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Disables the outer card border (useful inside an [`Accordion`]).
    pub fn borderless(mut self) -> Self {
        self.bordered = false;
        self
    }

    /// Registers a toggle callback invoked with the new open boolean.
    pub fn on_toggle(
        mut self,
        handler: impl Fn(&bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_toggle = Some(Box::new(handler));
        self
    }
}

impl ParentElement for Disclosure {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for Disclosure {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Disclosure {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let next = !self.open;
        let chevron_id = (self.id.clone(), "chevron");
        let body_id = (self.id.clone(), "body");

        let chevron_el = Icon::new(LucideIcon::ChevronDown)
            .size(IconSize::Md)
            .color(theme.muted_foreground)
            .with_spring(
                chevron_id,
                SpringAnimation::new(SPRING_SNAPPY).to(self.open),
                |icon, phase| icon.rotate(radians(phase.0 * std::f32::consts::PI)),
            );

        let header = div()
            .id((self.id.clone(), "trigger"))
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap(px(12.))
            .w_full()
            .px(px(16.))
            .py(px(13.))
            .cursor_pointer()
            .hover(|s| s.bg(theme.surface_hover.opacity(0.65)))
            .when_some(self.on_toggle, |el, handler| {
                el.on_click(move |_: &ClickEvent, window, cx| handler(&next, window, cx))
            })
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(10.))
                    .min_w_0()
                    .when_some(self.icon, |el, icon| {
                        el.child(Icon::new(icon).size(IconSize::Md).color(theme.primary))
                    })
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
                                    .child(self.title),
                            )
                            .when_some(self.subtitle, |el, sub| {
                                el.child(
                                    div()
                                        .text_size(px(12.5))
                                        .text_color(theme.muted_foreground)
                                        .child(sub),
                                )
                            }),
                    ),
            )
            .child(chevron_el);

        let mut root = div()
            .id(self.id)
            .flex()
            .flex_col()
            .flex_shrink_0()
            .w_full()
            .bg(theme.surface)
            .when(self.bordered, |el| {
                el.rounded(theme.radius_lg)
                    .border_1()
                    .border_color(theme.border)
                    .overflow_hidden()
            })
            .child(header)
            .when(self.open, |el| {
                el.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .px(px(16.))
                        .pb(px(14.))
                        .pt(px(2.))
                        .text_size(px(13.5))
                        .text_color(theme.muted_foreground)
                        .children(self.children)
                        .with_animation(
                            body_id,
                            Animation::new(ANIM_FAST).with_easing(ease_out_quint()),
                            |el, delta| el.opacity(delta).mt(px(-6.0 * (1.0 - delta))),
                        ),
                )
            });

        crate::Refineable::refine(root.style(), &self.style);
        root
    }
}

/// Framed vertical stack of divided [`Disclosure`] sections (FAQ / accordion).
#[derive(IntoElement)]
pub struct Accordion {
    style: StyleRefinement,
    children: SmallVec<[AnyElement; 6]>,
}

/// Creates a new [`Accordion`].
pub fn accordion() -> Accordion {
    Accordion {
        style: StyleRefinement::default(),
        children: SmallVec::new(),
    }
}

impl ParentElement for Accordion {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for Accordion {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Accordion {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let count = self.children.len();
        let mut root = div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .w_full()
            .rounded(theme.radius_xl)
            .border_1()
            .border_color(theme.border)
            .bg(theme.surface)
            .overflow_hidden();

        for (idx, child) in self.children.into_iter().enumerate() {
            root = root.child(child);
            if idx + 1 < count {
                root = root.child(div().w_full().h(px(1.)).bg(theme.border));
            }
        }

        crate::Refineable::refine(root.style(), &self.style);
        root
    }
}
