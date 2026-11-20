//! Inline anchored [`Popconfirm`] confirmation popover attached directly to a trigger element.
//!
//! Ideal for quick destructive or irreversible actions (delete, reset, revoke) where a full-window
//! modal [`super::Dialog`] would be unnecessarily disruptive.

use std::cell::Cell;
use std::rc::Rc;

use crate::icons::{Icon, IconSize, LucideIcon};
use crate::{
    Anchor, Animation, AnimationExt, AnyElement, App, Bounds, ClickEvent, ElementId, FocusHandle,
    FontWeight, IntoElement, KeyDownEvent, MouseDownEvent, Pixels, RenderOnce, SharedString,
    StyleRefinement, Styled, Window, anchored, canvas, deferred, div, ease_out_quint, point,
    prelude::*, px,
};

use super::button::{ButtonVariant, button};
use super::theme::{ANIM_FAST, ActiveTheme, ComponentSize};

struct PopconfirmOpenState {
    open: bool,
    focus_handle: FocusHandle,
    previous_focus: Option<FocusHandle>,
    trigger_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
}

impl PopconfirmOpenState {
    fn new(cx: &mut App) -> Self {
        Self {
            open: false,
            focus_handle: cx.focus_handle(),
            previous_focus: None,
            trigger_bounds: Rc::new(Cell::new(None)),
        }
    }

    fn close(&mut self, window: &mut Window, cx: &mut crate::Context<Self>) {
        if self.open {
            self.open = false;
            if let Some(prev) = self.previous_focus.take() {
                window.focus(&prev, cx);
            } else if self.focus_handle.is_focused(window) {
                window.blur(cx);
            }
            cx.notify();
        }
    }
}

/// Anchored inline confirmation popover wrapping a trigger element.
#[derive(IntoElement)]
pub struct Popconfirm {
    id: ElementId,
    trigger: AnyElement,
    title: SharedString,
    description: Option<SharedString>,
    icon: LucideIcon,
    destructive: bool,
    disabled: bool,
    confirm_label: SharedString,
    cancel_label: SharedString,
    anchor: Anchor,
    offset_y: Pixels,
    on_confirm: Option<Rc<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>,
    on_cancel: Option<Rc<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>,
    style: StyleRefinement,
}

/// Convenience constructor for [`Popconfirm::new`].
pub fn popconfirm(
    id: impl Into<ElementId>,
    title: impl Into<SharedString>,
    trigger: impl IntoElement,
) -> Popconfirm {
    Popconfirm::new(id, title, trigger)
}

impl Popconfirm {
    /// Creates a new [`Popconfirm`] wrapping `trigger` with confirmation `title`.
    pub fn new(
        id: impl Into<ElementId>,
        title: impl Into<SharedString>,
        trigger: impl IntoElement,
    ) -> Self {
        Self {
            id: id.into(),
            trigger: trigger.into_any_element(),
            title: title.into(),
            description: None,
            icon: LucideIcon::TriangleExclamationPoint,
            destructive: true,
            disabled: false,
            confirm_label: "Confirm".into(),
            cancel_label: "Cancel".into(),
            anchor: Anchor::TopLeft,
            offset_y: px(6.),
            on_confirm: None,
            on_cancel: None,
            style: StyleRefinement::default(),
        }
    }

    /// Sets muted explanatory text below the confirmation heading.
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Overrides the leading status icon (defaults to warning triangle).
    pub fn icon(mut self, icon: LucideIcon) -> Self {
        self.icon = icon;
        self
    }

    /// Uses destructive red styling (default `true`) or primary brand styling (`false`).
    pub fn destructive(mut self, destructive: bool) -> Self {
        self.destructive = destructive;
        self
    }

    /// Disables opening the confirmation popover when the trigger is clicked.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Customizes the Confirm action button label (default `"Confirm"`).
    pub fn confirm_label(mut self, label: impl Into<SharedString>) -> Self {
        self.confirm_label = label.into();
        self
    }

    /// Customizes the Cancel button label (default `"Cancel"`).
    pub fn cancel_label(mut self, label: impl Into<SharedString>) -> Self {
        self.cancel_label = label.into();
        self
    }

    /// Sets the anchored placement corner.
    pub fn anchor(mut self, anchor: Anchor) -> Self {
        self.anchor = anchor;
        self
    }

    /// Sets vertical spacing offset between trigger and confirmation popover (default `6px`).
    pub fn offset_y(mut self, offset: Pixels) -> Self {
        self.offset_y = if offset.0.is_finite() { offset } else { px(6.) };
        self
    }

    /// Registers a callback executed when the user clicks Confirm.
    pub fn on_confirm(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_confirm = Some(Rc::new(handler));
        self
    }

    /// Registers an optional callback executed when the user clicks Cancel.
    pub fn on_cancel(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_cancel = Some(Rc::new(handler));
        self
    }
}

impl Styled for Popconfirm {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Popconfirm {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let reduce_motion = cx.reduce_motion();
        let state = window.use_keyed_state(
            (self.id.clone(), "popconfirm-open"),
            cx,
            |_, cx| PopconfirmOpenState::new(cx),
        );

        // If disabled while open, close immediately so stale popups never linger.
        if self.disabled && state.read(cx).open {
            state.update(cx, |s, cx| s.close(window, cx));
        }

        let (is_open, focus_handle, bounds_cell) = {
            let s = state.read(cx);
            (s.open && !self.disabled, s.focus_handle.clone(), s.trigger_bounds.clone())
        };
        let captured_bounds = bounds_cell.get();

        let st_toggle = state.clone();
        let st_key_open = state.clone();
        let st_cancel = state.clone();
        let st_confirm = state.clone();
        let st_outside = state.clone();
        let st_escape = state;
        let bounds_for_outside = bounds_cell.clone();

        let icon_color = if self.destructive {
            theme.warning_soft_foreground
        } else {
            theme.primary
        };
        let on_confirm = self.on_confirm;
        let on_cancel = self.on_cancel;
        let on_confirm_key = on_confirm.clone();

        let popup = if is_open {
            let mut card = div()
                .id((self.id.clone(), "popconfirm-card"))
                .track_focus(&focus_handle)
                // Modal while open: Tab cycles its buttons.
                .focus_trap()
                .flex()
                .flex_col()
                .gap(px(12.))
                .w(px(280.))
                .p(px(14.))
                .rounded(theme.radius_lg)
                .bg(theme.popover)
                .text_color(theme.popover_foreground)
                .border_1()
                .border_color(theme.border)
                .when(theme.elevation_shadows, |el| el.shadow_lg())
                .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
                .on_mouse_down_out(move |ev: &MouseDownEvent, window, cx| {
                    // Clicking the trigger itself is handled by trigger's toggle click — avoid double-toggle!
                    if bounds_for_outside
                        .get()
                        .is_some_and(|b| b.contains(&ev.position))
                    {
                        return;
                    }
                    st_outside.update(cx, |s, cx| s.close(window, cx));
                })
                .on_key_down({
                    let focus_handle = focus_handle.clone();
                    move |ev: &KeyDownEvent, window, cx| {
                        if ev.keystroke.key == "escape" {
                            st_escape.update(cx, |s, cx| s.close(window, cx));
                            cx.stop_propagation();
                        } else if ev.keystroke.key == "enter"
                            // On a focused button, Enter presses that button.
                            && focus_handle.is_focused(window)
                            && !ev.keystroke.modifiers.control
                            && !ev.keystroke.modifiers.platform
                            && !ev.keystroke.modifiers.alt
                        {
                            st_escape.update(cx, |s, cx| s.close(window, cx));
                            if let Some(ref cb) = on_confirm_key {
                                cb(&ClickEvent::default(), window, cx);
                            }
                            cx.stop_propagation();
                        }
                    }
                })
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_start()
                        .gap(px(10.))
                        .child(
                            div()
                                .pt(px(2.))
                                .flex_shrink_0()
                                .child(Icon::new(self.icon).size(IconSize::Md).color(icon_color)),
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
                                        .child(self.title),
                                )
                                .when_some(self.description, |el, desc| {
                                    el.child(
                                        div()
                                            .text_size(px(12.5))
                                            .text_color(theme.muted_foreground)
                                            .child(desc),
                                    )
                                }),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_end()
                        .gap(px(6.))
                        .child(
                            button((self.id.clone(), "pc-cancel"))
                                .secondary()
                                .size(ComponentSize::Xs)
                                .label(self.cancel_label)
                                .on_click(move |ev, window, cx| {
                                    st_cancel.update(cx, |s, cx| s.close(window, cx));
                                    if let Some(ref cb) = on_cancel {
                                        cb(ev, window, cx);
                                    }
                                }),
                        )
                        .child(
                            button((self.id.clone(), "pc-ok"))
                                .variant(if self.destructive {
                                    ButtonVariant::Destructive
                                } else {
                                    ButtonVariant::Primary
                                })
                                .size(ComponentSize::Xs)
                                .label(self.confirm_label)
                                .on_click(move |ev, window, cx| {
                                    st_confirm.update(cx, |s, cx| s.close(window, cx));
                                    if let Some(ref cb) = on_confirm {
                                        cb(ev, window, cx);
                                    }
                                }),
                        ),
                );
            crate::Refineable::refine(card.style(), &self.style);

            let card_el = if reduce_motion {
                card.into_any_element()
            } else {
                card.with_animation(
                    (self.id.clone(), "pc-anim"),
                    Animation::new(ANIM_FAST).with_easing(ease_out_quint()),
                    |el, delta| el.opacity(delta),
                )
                .into_any_element()
            };

            let (target_corner, signed_offset_y) = match self.anchor {
                Anchor::TopLeft => (Anchor::BottomLeft, self.offset_y),
                Anchor::TopCenter => (Anchor::BottomCenter, self.offset_y),
                Anchor::TopRight => (Anchor::BottomRight, self.offset_y),
                Anchor::BottomLeft => (Anchor::TopLeft, -self.offset_y),
                Anchor::BottomCenter => (Anchor::TopCenter, -self.offset_y),
                Anchor::BottomRight => (Anchor::TopRight, -self.offset_y),
                Anchor::LeftCenter => (Anchor::RightCenter, px(0.)),
                Anchor::RightCenter => (Anchor::LeftCenter, px(0.)),
            };
            let mut anchor_el = anchored()
                .anchor(self.anchor)
                .snap_to_window_with_margin(px(8.));
            if let Some(bounds) = captured_bounds {
                anchor_el = anchor_el
                    .attach(bounds, target_corner)
                    .offset(point(px(0.), signed_offset_y));
            } else {
                anchor_el = anchor_el.offset(point(px(0.), self.offset_y));
            }

            Some(deferred(anchor_el.child(card_el)).priority(35))
        } else {
            None
        };

        div()
            .id(self.id.clone())
            .flex()
            .flex_col()
            .items_start()
            .child(
                div()
                    .id((self.id, "pc-trigger"))
                    .relative()
                    .when(!self.disabled, |el| {
                        el.cursor_pointer()
                            // Enter or Space on a focused trigger (a button,
                            // say) opens it, as a click does.
                            .on_key_down(move |ev: &KeyDownEvent, window, cx| {
                                let key = ev.keystroke.key.as_str();
                                if (key == "enter" || key == "space")
                                    && !ev.keystroke.modifiers.modified()
                                {
                                    st_key_open.update(cx, |s, cx| {
                                        if !s.open {
                                            s.previous_focus = window.focused(cx);
                                            s.open = true;
                                            window.focus(&s.focus_handle, cx);
                                            cx.notify();
                                        }
                                    });
                                    cx.stop_propagation();
                                }
                            })
                            .on_click(move |_, window, cx| {
                            st_toggle.update(cx, |s, cx| {
                                if s.open {
                                    s.close(window, cx);
                                } else {
                                    s.previous_focus = window.focused(cx);
                                    s.open = true;
                                    window.focus(&s.focus_handle, cx);
                                    cx.notify();
                                }
                            });
                        })
                    })
                    .when(self.disabled, |el| el.cursor_not_allowed().opacity(0.55))
                    .child(
                        canvas(
                            move |bounds, _, _| {
                                bounds_cell.set(Some(bounds));
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    )
                    .child(self.trigger),
            )
            .when_some(popup, |el, pop| el.child(div().h_0().child(pop)))
    }
}
