//! Tailwind UI floating [`Toast`] notifications and [`ToastHost`] stack overlay.

use std::rc::Rc;

use crate::{
    Animation, AnimationExt, App, ClickEvent, ElementId, FontWeight, Hsla, IntoElement,
    RenderOnce, SharedString, Window, deferred, div, ease_out_quint, prelude::*, px,
};
use crate::icons::{Icon, IconSize, LucideIcon};

use super::theme::{ANIM_NORMAL, ActiveTheme, Theme};

/// Semantic variant for a [`Toast`] notification.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToastVariant {
    /// Neutral informational toast.
    #[default]
    Info,
    /// Emerald success toast.
    Success,
    /// Amber warning toast.
    Warning,
    /// Red error toast.
    Destructive,
}

/// Data model for a single toast notification.
#[derive(Clone, Debug)]
pub struct ToastItem {
    /// Unique identifier for dismissing this toast.
    pub id: u64,
    /// Semantic variant.
    pub variant: ToastVariant,
    /// Short notification title.
    pub title: SharedString,
    /// Optional detail body text.
    pub description: Option<SharedString>,
}

impl ToastItem {
    /// Creates a new toast item.
    pub fn new(id: u64, title: impl Into<SharedString>) -> Self {
        Self {
            id,
            variant: ToastVariant::Info,
            title: title.into(),
            description: None,
        }
    }

    /// Sets the semantic variant.
    pub fn variant(mut self, variant: ToastVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Sets optional description text.
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// Single elevated Tailwind notification card.
#[derive(IntoElement)]
pub struct Toast {
    id: ElementId,
    item: ToastItem,
    on_dismiss: Option<Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>,
}

impl Toast {
    /// Creates a renderable [`Toast`] from `item`.
    pub fn new(id: impl Into<ElementId>, item: ToastItem) -> Self {
        Self {
            id: id.into(),
            item,
            on_dismiss: None,
        }
    }

    /// Registers a dismiss click handler.
    pub fn on_dismiss(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_dismiss = Some(Box::new(handler));
        self
    }

    fn accent(variant: ToastVariant, theme: &Theme) -> (Hsla, LucideIcon) {
        match variant {
            ToastVariant::Info => (theme.info, LucideIcon::Info),
            ToastVariant::Success => (theme.success, LucideIcon::CircleCheck),
            ToastVariant::Warning => (theme.warning, LucideIcon::TriangleExclamationPoint),
            ToastVariant::Destructive => (theme.destructive, LucideIcon::CircleX),
        }
    }
}

impl RenderOnce for Toast {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (accent, icon) = Self::accent(self.item.variant, &theme);

        let anim_id = (self.id.clone(), "toast-enter");
        div()
            .id(self.id.clone())
            .flex()
            .flex_row()
            .items_start()
            .gap(px(12.))
            .w(px(340.))
            .p(px(14.))
            .rounded(theme.radius_lg)
            .bg(theme.popover)
            .border_1()
            .border_color(theme.border)
            .shadow_lg()
            .child(
                div()
                    .pt(px(1.))
                    .child(Icon::new(icon).size(IconSize::Lg).color(accent)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(px(13.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.popover_foreground)
                            .child(self.item.title),
                    )
                    .when_some(self.item.description, |el, desc| {
                        el.child(
                            div()
                                .text_size(px(12.5))
                                .text_color(theme.muted_foreground)
                                .child(desc),
                        )
                    }),
            )
            .when_some(self.on_dismiss, |el, handler| {
                el.child(
                    div()
                        .id((self.id, "close"))
                        .flex()
                        .items_center()
                        .justify_center()
                        .p(px(4.))
                        .rounded(theme.radius_sm)
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.surface_hover))
                        .on_click(move |ev, window, cx| handler(ev, window, cx))
                        .child(
                            Icon::new(LucideIcon::X)
                                .size(IconSize::Sm)
                                .color(theme.muted_foreground),
                        ),
                )
            })
            .with_animation(
                anim_id,
                Animation::new(ANIM_NORMAL).with_easing(ease_out_quint()),
                |el, delta| el.opacity(delta).mr(px(-24.0 * (1.0 - delta))),
            )
    }
}

/// Floating bottom-right notification stack rendered in a deferred overlay layer.
#[derive(IntoElement)]
pub struct ToastHost {
    toasts: Vec<ToastItem>,
    on_dismiss: Option<Rc<dyn Fn(&u64, &mut Window, &mut App) + 'static>>,
}

/// Convenience constructor for [`ToastHost::new`].
pub fn toast_host(toasts: Vec<ToastItem>) -> ToastHost {
    ToastHost::new(toasts)
}

impl ToastHost {
    /// Creates a floating stack for `toasts`.
    pub fn new(toasts: Vec<ToastItem>) -> Self {
        Self {
            toasts,
            on_dismiss: None,
        }
    }

    /// Registers a callback invoked with the dismissed toast's `id`.
    ///
    /// Accepts `cx.listener(|this, id, window, cx| ...)` directly.
    pub fn on_dismiss(mut self, handler: impl Fn(&u64, &mut Window, &mut App) + 'static) -> Self {
        self.on_dismiss = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for ToastHost {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        if self.toasts.is_empty() {
            return div().into_any_element();
        }
        let mut stack = div()
            .absolute()
            .bottom(px(20.))
            .right(px(20.))
            .flex()
            .flex_col()
            .gap(px(10.));

        for item in self.toasts {
            let toast_id = item.id;
            let mut toast = Toast::new(("gpui-toast", toast_id as usize), item);
            if let Some(ref handler) = self.on_dismiss {
                let handler = handler.clone();
                toast = toast.on_dismiss(move |_, window, cx| handler(&toast_id, window, cx));
            }
            stack = stack.child(toast);
        }

        deferred(stack).priority(60).into_any_element()
    }
}
