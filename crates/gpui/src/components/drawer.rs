//! Slide-over [`Drawer`] (sheet) panel with GPU-blurred backdrop, edge selection
//! (`Left`, `Right`, `Top`, `Bottom`), smooth entrance animation, header/body/footer slots,
//! and backdrop/Escape dismissal.

use std::rc::Rc;

use smallvec::SmallVec;

use crate::{
    Animation, AnimationExt, AnyElement, App, ElementId, FocusHandle, FontWeight, IntoElement,
    KeyDownEvent, MouseButton, ParentElement, Pixels, RenderOnce, SharedString, StyleRefinement,
    Styled, Window, anchored, deferred, div, ease_out_quint, point, prelude::*, px,
};
use crate::icons::{Icon, IconSize, LucideIcon};

use super::theme::{ANIM_NORMAL, ActiveTheme};

struct DrawerFocusState {
    focus_handle: FocusHandle,
    previous_focus: Option<FocusHandle>,
    was_open: bool,
}

/// Edge from which a [`Drawer`] slides into the window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DrawerSide {
    /// Slides in from the right edge (default — inspectors, details, settings).
    #[default]
    Right,
    /// Slides in from the left edge (navigation drawers, filters).
    Left,
    /// Slides down from the top edge (banners, search sheets).
    Top,
    /// Slides up from the bottom edge (logs, command output, mobile-style sheets).
    Bottom,
}

/// Dimension presets for [`Drawer`] width (Left/Right) or height (Top/Bottom).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum DrawerSize {
    /// `320px`.
    Sm,
    /// `420px` (default).
    #[default]
    Md,
    /// `540px`.
    Lg,
    /// `680px`.
    Xl,
    /// Explicit pixel extent.
    Custom(Pixels),
}

impl DrawerSize {
    /// Converts this preset into logical pixels.
    pub const fn pixels(self) -> Pixels {
        match self {
            Self::Sm => px(320.),
            Self::Md => px(420.),
            Self::Lg => px(540.),
            Self::Xl => px(680.),
            Self::Custom(p) => p,
        }
    }
}

/// Slide-over modal sheet panel with GPU-accelerated backdrop blur.
#[derive(IntoElement)]
pub struct Drawer {
    id: ElementId,
    open: bool,
    side: DrawerSide,
    size: DrawerSize,
    title: Option<SharedString>,
    description: Option<SharedString>,
    icon: Option<LucideIcon>,
    backdrop_blur: Pixels,
    dismiss_on_backdrop: bool,
    show_close_button: bool,
    body: SmallVec<[AnyElement; 4]>,
    footer: SmallVec<[AnyElement; 3]>,
    on_close: Option<Rc<dyn Fn(&(), &mut Window, &mut App) + 'static>>,
    style: StyleRefinement,
}

/// Convenience constructor for [`Drawer::new`].
pub fn drawer(id: impl Into<ElementId>, open: bool) -> Drawer {
    Drawer::new(id, open)
}

impl Drawer {
    /// Creates a new right-docked [`Drawer`].
    pub fn new(id: impl Into<ElementId>, open: bool) -> Self {
        Self {
            id: id.into(),
            open,
            side: DrawerSide::Right,
            size: DrawerSize::Md,
            title: None,
            description: None,
            icon: None,
            backdrop_blur: px(6.),
            dismiss_on_backdrop: true,
            show_close_button: true,
            body: SmallVec::new(),
            footer: SmallVec::new(),
            on_close: None,
            style: StyleRefinement::default(),
        }
    }

    /// Sets which window edge the drawer attaches to (`Right`, `Left`, `Top`, `Bottom`).
    pub fn side(mut self, side: DrawerSide) -> Self {
        self.side = side;
        self
    }

    /// Sets the drawer width (Left/Right) or height (Top/Bottom).
    pub fn size(mut self, size: DrawerSize) -> Self {
        self.size = size;
        self
    }

    /// Sets the drawer header title.
    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Sets a muted header subtitle.
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Sets an optional leading icon badge in the header.
    pub fn icon(mut self, icon: LucideIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Sets the GPU backdrop blur radius on the scrim (default `6px`; `px(0.)` disables blur).
    pub fn backdrop_blur(mut self, radius: impl Into<Pixels>) -> Self {
        self.backdrop_blur = radius.into();
        self
    }

    /// Controls whether clicking the scrim closes the drawer (default `true`).
    pub fn dismiss_on_backdrop(mut self, dismiss: bool) -> Self {
        self.dismiss_on_backdrop = dismiss;
        self
    }

    /// Shows or hides the top-right `X` close button (default `true`).
    pub fn show_close_button(mut self, show: bool) -> Self {
        self.show_close_button = show;
        self
    }

    /// Appends a footer action element (e.g. Save / Cancel buttons).
    pub fn footer(mut self, element: impl IntoElement) -> Self {
        self.footer.push(element.into_any_element());
        self
    }

    /// Registers a callback invoked when closing via the `X` button, backdrop click, or Escape.
    pub fn on_close(mut self, handler: impl Fn(&(), &mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(handler));
        self
    }
}

impl ParentElement for Drawer {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.body.extend(elements);
    }
}

impl Styled for Drawer {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Drawer {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focus_state = window.use_keyed_state(
            (self.id.clone(), "drawer-focus"),
            cx,
            |_, cx| DrawerFocusState {
                focus_handle: cx.focus_handle(),
                previous_focus: None,
                was_open: false,
            },
        );
        let (focus_handle, just_opened, just_closed) = {
            let st = focus_state.read(cx);
            (
                st.focus_handle.clone(),
                self.open && !st.was_open,
                !self.open && st.was_open,
            )
        };
        if just_opened {
            let prev = window.focused(cx);
            focus_state.update(cx, |s, _| {
                s.previous_focus = prev;
                s.was_open = true;
            });
            window.focus(&focus_handle, cx);
        } else if just_closed {
            let prev = focus_state.update(cx, |s, _| {
                s.was_open = false;
                s.previous_focus.take()
            });
            if focus_handle.contains_focused(window, cx) || window.focused(cx).is_none() {
                if let Some(prev) = prev {
                    window.focus(&prev, cx);
                } else {
                    window.blur(cx);
                }
            }
        }

        if !self.open {
            return div().into_any_element();
        }

        let theme = cx.theme();
        let reduce_motion = cx.reduce_motion();
        let raw_extent = self.size.pixels();
        let extent = if raw_extent.0.is_finite() {
            raw_extent.max(px(120.))
        } else {
            px(420.)
        };
        let backdrop_blur = if self.backdrop_blur.0.is_finite() {
            self.backdrop_blur.max(px(0.))
        } else {
            px(0.)
        };
        let side = self.side;
        let show_close_button = self.show_close_button && self.on_close.is_some();
        let on_close_backdrop = self.on_close.clone();
        let on_close_key = self.on_close.clone();
        let on_close_btn = self.on_close.clone();

        let mut panel = div()
            .id((self.id.clone(), "drawer-panel"))
            .track_focus(&focus_handle)
            // Modal: Tab stays inside (`focus` module docs).
            .focus_trap()
            .occlude()
            .flex()
            .flex_col()
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .border_color(theme.border)
            .when(theme.elevation_shadows, |el| el.shadow_xl())
            .overflow_hidden()
            .on_any_mouse_down(|_, _, cx| cx.stop_propagation());

        panel = match side {
            DrawerSide::Right => panel.w(extent).max_w_full().h_full().border_l_1(),
            DrawerSide::Left => panel.w(extent).max_w_full().h_full().border_r_1(),
            DrawerSide::Top => panel.w_full().h(extent).max_h_full().border_b_1(),
            DrawerSide::Bottom => panel.w_full().h(extent).max_h_full().border_t_1(),
        };

        let has_header =
            self.title.is_some() || self.description.is_some() || self.icon.is_some() || show_close_button;
        if has_header {
            let hover_bg = theme.surface_hover;
            panel = panel.child(
                div()
                    .flex()
                    .flex_row()
                    .items_start()
                    .justify_between()
                    .gap(px(12.))
                    .px(px(20.))
                    .py(px(16.))
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_start()
                            .gap(px(12.))
                            .flex_1()
                            .min_w_0()
                            .when_some(self.icon, |el, ic| {
                                el.child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .flex_shrink_0()
                                        .w(px(34.))
                                        .h(px(34.))
                                        .rounded(theme.radius_lg)
                                        .bg(theme.primary_soft)
                                        .child(
                                            Icon::new(ic)
                                                .size(IconSize::Md)
                                                .color(theme.primary),
                                        ),
                                )
                            })
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(3.))
                                    .flex_1()
                                    .min_w_0()
                                    .when_some(self.title, |el, title| {
                                        el.child(
                                            div()
                                                .text_size(px(16.))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(theme.popover_foreground)
                                                .child(title),
                                        )
                                    })
                                    .when_some(self.description, |el, desc| {
                                        el.child(
                                            div()
                                                .text_size(px(13.))
                                                .text_color(theme.muted_foreground)
                                                .child(desc),
                                        )
                                    }),
                            ),
                    )
                    .when(show_close_button, |el| {
                        el.child(
                            div()
                                .id((self.id.clone(), "drawer-close"))
                                .flex()
                                .items_center()
                                .justify_center()
                                .flex_shrink_0()
                                .w(px(28.))
                                .h(px(28.))
                                .rounded(theme.radius_md)
                                .cursor_pointer()
                                .hover(move |s| s.bg(hover_bg))
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

        // Scrollable body
        panel = panel.child(
            div()
                .id((self.id.clone(), "drawer-body"))
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(14.))
                        .p(px(20.))
                        .children(self.body),
                ),
        );

        // Optional pinned footer
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

        let slide_px = (extent.0 * 0.25).clamp(28.0, 96.0);
        let animated_panel = if reduce_motion {
            panel.into_any_element()
        } else {
            panel
                .with_animation(
                    (self.id.clone(), "drawer-slide"),
                    Animation::new(ANIM_NORMAL).with_easing(ease_out_quint()),
                    move |el, delta| {
                        let offset = slide_px * (1.0 - delta);
                        match side {
                            DrawerSide::Right => el.opacity(delta).mr(px(-offset)),
                            DrawerSide::Left => el.opacity(delta).ml(px(-offset)),
                            DrawerSide::Top => el.opacity(delta).mt(px(-offset)),
                            DrawerSide::Bottom => el.opacity(delta).mb(px(-offset)),
                        }
                    },
                )
                .into_any_element()
        };

        let backdrop_color = theme.backdrop;
        let vp = window.viewport_size();

        let mut overlay = div()
            .id(self.id.clone())
            .w(vp.width)
            .h(vp.height)
            .occlude()
            .flex()
            .when(self.dismiss_on_backdrop, |el| {
                el.when_some(on_close_backdrop, |el, cb| {
                    el.on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        cb(&(), window, cx);
                        cx.stop_propagation();
                    })
                })
            })
            .when_some(on_close_key, |el, cb| {
                el.on_key_down(move |ev: &KeyDownEvent, window, cx| {
                    let mods = &ev.keystroke.modifiers;
                    if ev.keystroke.key == "escape"
                        && !mods.control
                        && !mods.alt
                        && !mods.platform
                    {
                        cb(&(), window, cx);
                        cx.stop_propagation();
                    }
                })
            });

        overlay = match side {
            DrawerSide::Right => overlay.flex_row().justify_end(),
            DrawerSide::Left => overlay.flex_row().justify_start(),
            DrawerSide::Top => overlay.flex_col().justify_start(),
            DrawerSide::Bottom => overlay.flex_col().justify_end(),
        };

        let overlay = overlay.child(animated_panel);

        let final_overlay = if reduce_motion {
            overlay
                .bg(backdrop_color)
                .when(backdrop_blur > px(0.), |el| el.backdrop_blur(backdrop_blur))
                .into_any_element()
        } else {
            overlay
                .with_animation(
                    (self.id, "drawer-backdrop"),
                    Animation::new(ANIM_NORMAL).with_easing(ease_out_quint()),
                    move |el, delta| {
                        el.bg(backdrop_color.opacity(delta))
                            .when(backdrop_blur > px(0.), |el| {
                                el.backdrop_blur(px(backdrop_blur.0 * delta))
                            })
                    },
                )
                .into_any_element()
        };

        deferred(
            anchored()
                .position(point(px(0.), px(0.)))
                .child(final_overlay),
        )
        .priority(48)
        .into_any_element()
    }
}
