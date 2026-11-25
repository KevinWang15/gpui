//! Positioned floating menus of [`MenuEntry`] items:
//!
//! - [`ContextMenu`] wraps any child element and opens automatically on secondary-click
//!   (right-click) at the cursor position.
//! - [`PopupMenu`] is a controlled floating menu anchored at a caller-supplied window coordinate
//!   (`Option<Point<Pixels>>`), suitable for keyboard-invoked menus (`Menu` / `Shift+F10`),
//!   custom hit-testing (canvases, grids, virtualized views), or coordinating multiple overlays.
//!
//! Both share automatic viewport edge flipping, window-bound clamping, focus trapping, arrow-key
//! navigation, Escape/Tab/outside-click dismissal, and focus restoration.

use std::rc::Rc;

use crate::{
    Anchor, Animation, AnimationExt, AnyElement, App, ElementId, Entity, FocusHandle, IntoElement,
    KeyDownEvent, MouseButton, MouseDownEvent, Pixels, Point, RenderOnce, StyleRefinement, Styled,
    Window, anchored, deferred, div, ease_out_quint, prelude::*, px,
};

use super::popover::{MenuEntry, menu_arrow_key, render_menu_entries};
use super::theme::{ANIM_FAST, ActiveTheme};

fn restore_previous_focus(
    focus_handle: &FocusHandle,
    previous: Option<FocusHandle>,
    window: &mut Window,
    cx: &mut App,
) {
    let should_restore = focus_handle.contains_focused(window, cx)
        || window.focused(cx).is_none_or(|f| !f.is_rendered(window));
    if should_restore {
        if let Some(prev) = previous {
            window.focus(&prev, cx);
        } else {
            window.blur(cx);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_positioned_menu_panel(
    id: ElementId,
    pos: Point<Pixels>,
    anchor_override: Option<Anchor>,
    entries: Vec<MenuEntry>,
    min_width: Pixels,
    focus_handle: FocusHandle,
    on_close_item: Option<Rc<dyn Fn(&(), &mut Window, &mut App)>>,
    on_close_out: impl Fn(&mut Window, &mut App) + 'static,
    on_key_close: impl Fn(bool, &mut Window, &mut App) + 'static,
    panel_style: Option<&StyleRefinement>,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let reduce_motion = cx.reduce_motion();
    let vp = window.viewport_size();
    let anchor = anchor_override.unwrap_or_else(|| {
        let flip_x = pos.x > vp.width * 0.65;
        let flip_y = pos.y > vp.height * 0.65;
        match (flip_x, flip_y) {
            (false, false) => Anchor::TopLeft,
            (true, false) => Anchor::TopRight,
            (false, true) => Anchor::BottomLeft,
            (true, true) => Anchor::BottomRight,
        }
    });

    let menu_list = render_menu_entries(
        (id.clone(), "ctx-entries").into(),
        entries,
        min_width,
        on_close_item,
        &theme,
    );

    let mut card = div()
        .id((id.clone(), "ctx-panel"))
        .track_focus(&focus_handle)
        // The arrow keys move through the items and wrap inside.
        .focus_trap()
        .occlude()
        .flex()
        .flex_col()
        .p(px(4.))
        .max_h((vp.height - px(16.)).max(px(120.)))
        .overflow_y_scroll()
        .show_scrollbar_always()
        .rounded(theme.radius_lg)
        .bg(theme.popover)
        .text_color(theme.popover_foreground)
        .border_1()
        .border_color(theme.border)
        .when(theme.elevation_shadows, |el| el.shadow_lg())
        .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
        .on_mouse_down_out(move |_, window, cx| {
            on_close_out(window, cx);
        })
        .on_key_down(move |ev: &KeyDownEvent, window, cx| {
            if menu_arrow_key(ev, window, cx) {
                return;
            }
            let mods = &ev.keystroke.modifiers;
            if ev.keystroke.key == "escape" && !mods.control && !mods.alt && !mods.platform {
                on_key_close(false, window, cx);
                window.prevent_default();
                cx.stop_propagation();
            } else if ev.keystroke.key == "tab" && !mods.control && !mods.alt && !mods.platform {
                // Tab closes the menu (focus goes back to where it was) and moves on from there.
                on_key_close(true, window, cx);
            }
        })
        .child(menu_list);

    if let Some(style) = panel_style {
        crate::Refineable::refine(card.style(), style);
    }

    let menu_el = if reduce_motion {
        card.into_any_element()
    } else {
        card.with_animation(
            (id, "ctx-anim"),
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
    // Transient context menus must float above drawers (48), dialogs (50), command palettes (55),
    // floating windows (75), and floating AI overlays (80–82).
    .priority(120)
    .into_any_element()
}

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
    if was_open {
        restore_previous_focus(&focus_handle, prev, window, cx);
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

        let floating_menu = open_pos.filter(|_| can_open).map(|pos| {
            render_positioned_menu_panel(
                self.id.clone(),
                pos,
                None,
                self.entries,
                self.min_width,
                focus_handle,
                Some(on_close),
                move |window, cx| close_context_menu(&state_close_out, window, cx),
                move |_, window, cx| close_context_menu(&state_close_key, window, cx),
                None,
                window,
                cx,
            )
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

struct PopupMenuFocusState {
    handle: FocusHandle,
    last_position: Option<Point<Pixels>>,
    previous: Option<FocusHandle>,
}

/// Controlled floating menu of [`MenuEntry`] items anchored at a window coordinate.
///
/// Render this element unconditionally while closed (`position: None`) so it can restore focus
/// to the previously focused element when transitioning from open to closed.
#[derive(IntoElement)]
pub struct PopupMenu {
    id: ElementId,
    position: Option<Point<Pixels>>,
    anchor: Option<Anchor>,
    entries: Vec<MenuEntry>,
    min_width: Pixels,
    disabled: bool,
    on_close: Option<Rc<dyn Fn(&(), &mut Window, &mut App) + 'static>>,
    style: StyleRefinement,
}

/// Convenience constructor for [`PopupMenu::new`].
pub fn popup_menu(id: impl Into<ElementId>, position: Option<Point<Pixels>>) -> PopupMenu {
    PopupMenu::new(id, position)
}

impl PopupMenu {
    /// Creates a new controlled [`PopupMenu`]. When `position` is `Some(point)`, the floating
    /// menu panel is rendered anchored at `point`.
    pub fn new(id: impl Into<ElementId>, position: Option<Point<Pixels>>) -> Self {
        Self {
            id: id.into(),
            position,
            anchor: None,
            entries: Vec::new(),
            min_width: px(196.),
            disabled: false,
            on_close: None,
            style: StyleRefinement::default(),
        }
    }

    /// Appends a single [`MenuEntry`].
    pub fn entry(mut self, entry: MenuEntry) -> Self {
        self.entries.push(entry);
        self
    }

    /// Appends multiple [`MenuEntry`] items.
    pub fn entries(mut self, entries: impl IntoIterator<Item = MenuEntry>) -> Self {
        self.entries.extend(entries);
        self
    }

    /// Sets the minimum menu panel width (default `196px`).
    pub fn min_width(mut self, width: Pixels) -> Self {
        self.min_width = width;
        self
    }

    /// Overrides the automatic viewport-aware anchor corner.
    pub fn anchor(mut self, anchor: Anchor) -> Self {
        self.anchor = Some(anchor);
        self
    }

    /// Disables showing the popup menu even when `position` is `Some`.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Registers a close handler invoked on item selection, outside click, Escape, or Tab.
    ///
    /// Accepts `cx.listener(|this, _, window, cx| ...)` directly.
    pub fn on_close(mut self, handler: impl Fn(&(), &mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(handler));
        self
    }
}

impl Styled for PopupMenu {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for PopupMenu {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window.use_keyed_state(
            (self.id.clone(), "popup-menu-focus"),
            cx,
            |_, cx| PopupMenuFocusState {
                handle: cx.focus_handle(),
                last_position: None,
                previous: None,
            },
        );
        let (handle, last_position) = {
            let s = state.read(cx);
            (s.handle.clone(), s.last_position)
        };
        let open_pos = self
            .position
            .filter(|_| !self.disabled && !self.entries.is_empty());

        match (last_position, open_pos) {
            (None, Some(pos)) => {
                let previous = window.focused(cx).filter(|f| f != &handle);
                state.update(cx, |s, _| {
                    s.last_position = Some(pos);
                    s.previous = previous;
                });
                let focus_handle = handle.clone();
                window.defer(cx, move |window, cx| window.focus(&focus_handle, cx));
            }
            (Some(prev_pos), Some(pos)) if prev_pos != pos => {
                let focus_outside = !handle.contains_focused(window, cx);
                let current = focus_outside
                    .then(|| window.focused(cx).filter(|f| f != &handle))
                    .flatten();
                state.update(cx, |s, _| {
                    s.last_position = Some(pos);
                    if let Some(current) = current {
                        s.previous = Some(current);
                    }
                });
                if focus_outside {
                    let focus_handle = handle.clone();
                    window.defer(cx, move |window, cx| window.focus(&focus_handle, cx));
                }
            }
            (Some(_), None) => {
                let previous = state.update(cx, |s, _| {
                    s.last_position = None;
                    s.previous.take()
                });
                if previous.is_some() || handle.contains_focused(window, cx) {
                    let focus_handle = handle.clone();
                    window.defer(cx, move |window, cx| {
                        restore_previous_focus(&focus_handle, previous, window, cx);
                    });
                }
            }
            _ => {}
        }

        let Some(pos) = open_pos else {
            return div().id(self.id).size_0().into_any_element();
        };

        let on_close_item_cb = self.on_close.clone();
        let state_item = state.clone();
        let handle_item = handle.clone();
        let on_close_item: Option<Rc<dyn Fn(&(), &mut Window, &mut App)>> =
            Some(Rc::new(move |_, window, cx| {
                if let Some(close) = &on_close_item_cb {
                    close(&(), window, cx);
                }
                // Restore previous focus synchronously before the menu unmounts on the next
                // frame so host widgets don't receive a spurious focus-out when a menu item
                // (such as "Select All") completes without moving focus elsewhere.
                if handle_item.contains_focused(window, cx) {
                    let previous = state_item.update(cx, |s, _| s.previous.take());
                    restore_previous_focus(&handle_item, previous, window, cx);
                }
            }));

        let on_close_out = self.on_close.clone();
        let on_close_key = self.on_close;
        let state_key = state;
        let handle_key = handle.clone();

        let panel = render_positioned_menu_panel(
            self.id.clone(),
            pos,
            self.anchor,
            self.entries,
            self.min_width,
            handle,
            on_close_item,
            move |window, cx| {
                if let Some(close) = &on_close_out {
                    close(&(), window, cx);
                }
            },
            move |is_tab, window, cx| {
                if let Some(close) = &on_close_key {
                    close(&(), window, cx);
                }
                if is_tab {
                    let previous = state_key.update(cx, |s, _| s.previous.take());
                    if let Some(previous) = previous {
                        window.focus(&previous, cx);
                    }
                } else if handle_key.contains_focused(window, cx) {
                    let previous = state_key.update(cx, |s, _| s.previous.take());
                    restore_previous_focus(&handle_key, previous, window, cx);
                }
            },
            Some(&self.style),
            window,
            cx,
        );

        div()
            .id(self.id)
            .absolute()
            .size_0()
            .child(panel)
            .into_any_element()
    }
}
