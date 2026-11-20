//! Right-click [`ContextMenu`] wrapper anchored at the cursor position.
//!
//! Wraps any GPUI element and opens a floating menu of [`MenuEntry`] items wherever the user
//! secondary-clicks (right-clicks), automatically clamping to the window bounds and dismissing
//! on selection, outside click, or Escape.

use std::rc::Rc;

use crate::{
    Anchor, Animation, AnimationExt, AnyElement, App, ElementId, Entity, FocusHandle, IntoElement,
    KeyDownEvent, MouseButton, MouseDownEvent, Pixels, Point, RenderOnce, StyleRefinement, Styled,
    Window, anchored, deferred, div, ease_out_quint, prelude::*, px,
};

use super::popover::{MenuEntry, render_menu_entries};
use super::theme::{ANIM_FAST, ActiveTheme};

struct ContextMenuOpenState {
    position: Option<Point<Pixels>>,
    focus_handle: FocusHandle,
    previous_focus: Option<FocusHandle>,
}

fn close_context_menu(
    state: &Entity<ContextMenuOpenState>,
    window: &mut Window,
    cx: &mut App,
) {
    let (was_open, focus_handle, prev) = state.update(cx, |s, cx| {
        let was_open = s.position.take().is_some();
        let fh = s.focus_handle.clone();
        let prev = s.previous_focus.take();
        if was_open {
            cx.notify();
        }
        (was_open, fh, prev)
    });
    if was_open && (focus_handle.contains_focused(window, cx) || window.focused(cx).is_none()) {
        if let Some(prev) = prev {
            window.focus(&prev, cx);
        } else {
            window.blur(cx);
        }
    }
}

/// Right-click context menu wrapper around any target element.
#[derive(IntoElement)]
pub struct ContextMenu {
    id: ElementId,
    child: AnyElement,
    entries: Vec<MenuEntry>,
    min_width: Pixels,
    disabled: bool,
    style: StyleRefinement,
}

/// Convenience constructor for [`ContextMenu::new`].
pub fn context_menu(id: impl Into<ElementId>, child: impl IntoElement) -> ContextMenu {
    ContextMenu::new(id, child)
}

impl ContextMenu {
    /// Wraps `child` so right-clicking anywhere within it opens a floating context menu.
    pub fn new(id: impl Into<ElementId>, child: impl IntoElement) -> Self {
        Self {
            id: id.into(),
            child: child.into_any_element(),
            entries: Vec::new(),
            min_width: px(196.),
            disabled: false,
            style: StyleRefinement::default(),
        }
    }

    /// Sets the list of [`MenuEntry`] items displayed when the context menu opens.
    pub fn entries(mut self, entries: Vec<MenuEntry>) -> Self {
        self.entries = entries;
        self
    }

    /// Appends a single [`MenuEntry`].
    pub fn entry(mut self, entry: MenuEntry) -> Self {
        self.entries.push(entry);
        self
    }

    /// Sets the minimum menu panel width (default `196px`).
    pub fn min_width(mut self, width: Pixels) -> Self {
        self.min_width = width;
        self
    }

    /// Disables opening the context menu on right-click.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl Styled for ContextMenu {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for ContextMenu {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let reduce_motion = cx.reduce_motion();
        let state = window.use_keyed_state(
            (self.id.clone(), "context-menu-state"),
            cx,
            |_, cx| ContextMenuOpenState {
                position: None,
                focus_handle: cx.focus_handle(),
                previous_focus: None,
            },
        );
        let (open_pos, focus_handle) = {
            let st = state.read(cx);
            (st.position, st.focus_handle.clone())
        };
        let can_open = !self.disabled && !self.entries.is_empty();
        if open_pos.is_some() && !can_open {
            close_context_menu(&state, window, cx);
        }

        let state_open = state.clone();
        let state_close_out = state.clone();
        let state_close_key = state.clone();
        let state_close_item = state;

        let on_close: Rc<dyn Fn(&(), &mut Window, &mut App)> = Rc::new(move |_, window, cx| {
            close_context_menu(&state_close_item, window, cx);
        });

        let vp = window.viewport_size();
        let floating_menu = open_pos.filter(|_| can_open).map(|pos| {
            let flip_x = pos.x > vp.width * 0.65;
            let flip_y = pos.y > vp.height * 0.65;
            let anchor = match (flip_x, flip_y) {
                (false, false) => Anchor::TopLeft,
                (true, false) => Anchor::TopRight,
                (false, true) => Anchor::BottomLeft,
                (true, true) => Anchor::BottomRight,
            };

            let menu_list = render_menu_entries(
                (self.id.clone(), "ctx-entries").into(),
                self.entries,
                self.min_width,
                Some(on_close),
                &theme,
            );

            let card = div()
                .id((self.id.clone(), "ctx-panel"))
                .track_focus(&focus_handle)
                .occlude()
                .flex()
                .flex_col()
                .p(px(4.))
                .max_h((vp.height - px(16.)).max(px(120.)))
                .overflow_y_scroll()
                .rounded(theme.radius_lg)
                .bg(theme.popover)
                .text_color(theme.popover_foreground)
                .border_1()
                .border_color(theme.border)
                .when(theme.elevation_shadows, |el| el.shadow_lg())
                .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
                .on_mouse_down_out(move |_, window, cx| {
                    close_context_menu(&state_close_out, window, cx);
                })
                .on_key_down(move |ev: &KeyDownEvent, window, cx| {
                    let mods = &ev.keystroke.modifiers;
                    if ev.keystroke.key == "escape"
                        && !mods.control
                        && !mods.alt
                        && !mods.platform
                    {
                        close_context_menu(&state_close_key, window, cx);
                        cx.stop_propagation();
                    }
                })
                .child(menu_list);

            let menu_el = if reduce_motion {
                card.into_any_element()
            } else {
                card.with_animation(
                    (self.id.clone(), "ctx-anim"),
                    Animation::new(ANIM_FAST).with_easing(ease_out_quint()),
                    |el, delta| el.opacity(delta),
                )
                .into_any_element()
            };

            deferred(
                anchored()
                    .position(pos)
                    .anchor(anchor)
                    .snap_to_window_with_margin(px(8.))
                    .child(menu_el),
            )
            .priority(40)
        });

        let mut wrapper = div()
            .id(self.id)
            .when(can_open, |el| {
                el.on_mouse_down(MouseButton::Right, move |ev: &MouseDownEvent, window, cx| {
                    let pos = ev.position;
                    let prev = window.focused(cx);
                    let fh = state_open.update(cx, |s, cx| {
                        if s.position.is_none() {
                            s.previous_focus = prev;
                        }
                        s.position = Some(pos);
                        cx.notify();
                        s.focus_handle.clone()
                    });
                    window.focus(&fh, cx);
                    cx.stop_propagation();
                })
            })
            .child(self.child)
            .when_some(floating_menu, |el, menu| {
                el.child(div().absolute().size_0().child(menu))
            });

        crate::Refineable::refine(wrapper.style(), &self.style);
        wrapper
    }
}
