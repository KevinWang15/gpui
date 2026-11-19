//! Tailwind UI modal [`Dialog`] overlay with translucent backdrop, close button,
//! Escape & backdrop-click dismissal, title/description header, body content, and footer actions.

use std::rc::Rc;

use smallvec::SmallVec;

use crate::{
    Animation, AnimationExt, AnyElement, App, ClickEvent, ElementId, FontWeight, IntoElement,
    KeyDownEvent, MouseButton, ParentElement, Pixels, RenderOnce, SharedString, StyleRefinement,
    Styled, Window, deferred, div, ease_out_quint, prelude::*, px,
};
use crate::icons::{Icon, IconSize, LucideIcon};

use super::theme::{ANIM_NORMAL, ActiveTheme};

/// Width presets for [`Dialog`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum DialogSize {
    /// `384px` (`max-w-sm`).
    Sm,
    /// `480px` (`max-w-md`).
    #[default]
    Md,
    /// `576px` (`max-w-lg`).
    Lg,
    /// `720px` (`max-w-2xl`).
    Xl,
}

impl DialogSize {
    const fn width(self) -> Pixels {
        match self {
            Self::Sm => px(384.),
            Self::Md => px(480.),
            Self::Lg => px(576.),
            Self::Xl => px(720.),
        }
    }
}

/// Modal dialog rendered inside a top-priority deferred overlay layer.
#[derive(IntoElement)]
pub struct Dialog {
    id: ElementId,
    open: bool,
    title: Option<SharedString>,
    description: Option<SharedString>,
    icon: Option<LucideIcon>,
    destructive: bool,
    size: DialogSize,
    dismiss_on_backdrop: bool,
    show_close_button: bool,
    children: SmallVec<[AnyElement; 4]>,
    footer: SmallVec<[AnyElement; 3]>,
    on_close: Option<Rc<dyn Fn(&(), &mut Window, &mut App) + 'static>>,
    style: StyleRefinement,
}

/// Convenience constructor for [`Dialog::new`].
pub fn dialog(id: impl Into<ElementId>, open: bool) -> Dialog {
    Dialog::new(id, open)
}

impl Dialog {
    /// Creates a new modal dialog; renders nothing when `open` is `false`.
    pub fn new(id: impl Into<ElementId>, open: bool) -> Self {
        Self {
            id: id.into(),
            open,
            title: None,
            description: None,
            icon: None,
            destructive: false,
            size: DialogSize::Md,
            dismiss_on_backdrop: true,
            show_close_button: true,
            children: SmallVec::new(),
            footer: SmallVec::new(),
            on_close: None,
            style: StyleRefinement::default(),
        }
    }

    /// Sets the dialog heading.
    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Sets muted description text beneath the title.
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Adds a leading circular accent icon badge in the dialog header.
    pub fn icon(mut self, icon: LucideIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Styles the header icon badge in destructive red tones (Tailwind Alert Dialog).
    pub fn destructive(mut self, destructive: bool) -> Self {
        self.destructive = destructive;
        self
    }

    /// Sets the dialog width preset.
    pub fn size(mut self, size: DialogSize) -> Self {
        self.size = size;
        self
    }

    /// Controls whether clicking the dark backdrop invokes `on_close` (default `true`).
    pub fn dismiss_on_backdrop(mut self, dismiss: bool) -> Self {
        self.dismiss_on_backdrop = dismiss;
        self
    }

    /// Controls visibility of the top-right `X` close button (default `true`).
    pub fn show_close_button(mut self, show: bool) -> Self {
        self.show_close_button = show;
        self
    }

    /// Appends a footer action element (e.g. Cancel / Confirm buttons).
    pub fn footer_action(mut self, action: impl IntoElement) -> Self {
        self.footer.push(action.into_any_element());
        self
    }

    /// Registers the close callback invoked by backdrop clicks, close button, or Escape key.
    ///
    /// Accepts `cx.listener(|this, _, window, cx| ...)` directly.
    pub fn on_close(mut self, handler: impl Fn(&(), &mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(handler));
        self
    }
}

impl ParentElement for Dialog {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for Dialog {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Dialog {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        if !self.open {
            return div().into_any_element();
        }
        let theme = cx.theme();
        let on_close_backdrop = self.on_close.clone();
        let on_close_btn = self.on_close.clone();
        let on_close_key = self.on_close;

        let mut panel = div()
            .id((self.id.clone(), "panel"))
            .flex()
            .flex_col()
            .w(self.size.width())
            .max_w_full()
            .rounded(theme.radius_xl)
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .border_1()
            .border_color(theme.border)
            .shadow_2xl()
            .overflow_hidden()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(|_: &ClickEvent, _, cx| cx.stop_propagation());

        if self.title.is_some() || self.description.is_some() || self.show_close_button {
            let icon_bg = if self.destructive {
                theme.destructive_soft
            } else {
                theme.primary_soft
            };
            let icon_fg = if self.destructive {
                theme.destructive
            } else {
                theme.primary
            };

            panel = panel.child(
                div()
                    .flex()
                    .flex_row()
                    .items_start()
                    .justify_between()
                    .gap(px(12.))
                    .px(px(20.))
                    .pt(px(20.))
                    .pb(px(12.))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_start()
                            .gap(px(12.))
                            .flex_1()
                            .min_w_0()
                            .when_some(self.icon, |el, icon| {
                                el.child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .flex_shrink_0()
                                        .w(px(38.))
                                        .h(px(38.))
                                        .rounded_full()
                                        .bg(icon_bg)
                                        .child(Icon::new(icon).size(IconSize::Lg).color(icon_fg)),
                                )
                            })
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.))
                                    .flex_1()
                                    .min_w_0()
                                    .when_some(self.title, |el, title| {
                                        el.child(
                                            div()
                                                .text_size(px(17.))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(theme.popover_foreground)
                                                .child(title),
                                        )
                                    })
                                    .when_some(self.description, |el, desc| {
                                        el.child(
                                            div()
                                                .text_size(px(13.5))
                                                .text_color(theme.muted_foreground)
                                                .child(desc),
                                        )
                                    }),
                            ),
                    )
                    .when(self.show_close_button, |el| {
                        let id = (self.id.clone(), "close");
                        el.child(
                            div()
                                .id(id)
                                .flex()
                                .items_center()
                                .justify_center()
                                .p(px(6.))
                                .rounded(theme.radius_md)
                                .cursor_pointer()
                                .hover(|s| s.bg(theme.surface_hover))
                                .when_some(on_close_btn, |el, cb| {
                                    el.on_click(move |_, window, cx| cb(&(), window, cx))
                                })
                                .child(
                                    Icon::new(LucideIcon::X)
                                        .size(IconSize::Md)
                                        .color(theme.muted_foreground),
                                ),
                        )
                    }),
            );
        }

        if !self.children.is_empty() {
            panel = panel.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .px(px(20.))
                    .py(px(12.))
                    .children(self.children),
            );
        }

        if !self.footer.is_empty() {
            panel = panel.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_end()
                    .gap(px(8.))
                    .px(px(20.))
                    .py(px(14.))
                    .border_t_1()
                    .border_color(theme.border)
                    .bg(theme.surface_muted.opacity(0.45))
                    .children(self.footer),
            );
        }

        crate::Refineable::refine(panel.style(), &self.style);

        let panel_anim_id = (self.id.clone(), "panel-anim");
        let backdrop_anim_id = (self.id.clone(), "backdrop-anim");
        let backdrop_color = theme.backdrop;

        let animated_panel = panel.with_animation(
            panel_anim_id,
            Animation::new(ANIM_NORMAL).with_easing(ease_out_quint()),
            |el, delta| el.opacity(delta).mt(px(14.0 * (1.0 - delta))),
        );

        let overlay = div()
            .id(self.id)
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .p(px(20.))
            .when(self.dismiss_on_backdrop, |el| {
                el.when_some(on_close_backdrop, |el, cb| {
                    el.on_click(move |_, window, cx| cb(&(), window, cx))
                })
            })
            .when_some(on_close_key, |el, cb| {
                el.on_key_down(move |ev: &KeyDownEvent, window, cx| {
                    if ev.keystroke.key == "escape" {
                        cb(&(), window, cx);
                    }
                })
            })
            .child(animated_panel)
            .with_animation(
                backdrop_anim_id,
                Animation::new(ANIM_NORMAL).with_easing(ease_out_quint()),
                move |el, delta| el.bg(backdrop_color.opacity(delta)),
            );

        deferred(overlay).priority(50).into_any_element()
    }
}
