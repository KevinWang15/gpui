//! Multi-level navigation [`Menu`] (`Vertical` collapsible nested submenus or `Horizontal`
//! top menu bar with anchored scrollable dropdown submenus).

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::icons::{Icon, IconSize, LucideIcon};
use crate::{
    Anchor, Animation, AnimationExt, AnyElement, App, Bounds, Context, ElementId, Entity,
    EventEmitter, FocusHandle, Focusable, FontWeight, IntoElement, KeyDownEvent, MouseDownEvent,
    Pixels, Render, RenderOnce, SharedString, StyleRefinement, Styled, Window, anchored, canvas,
    deferred, div, ease_out_quint, point, prelude::*, px,
};

use super::badge::{BadgeColor, badge};
use super::theme::{ANIM_FAST, ActiveTheme, Theme};

/// Layout mode for [`Menu`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MenuMode {
    /// Collapsible multi-level sidebar navigation menu (default).
    #[default]
    Vertical,
    /// Horizontal top navigation bar with anchored dropdown submenus.
    Horizontal,
}

/// Recursive item or submenu entry inside a [`Menu`].
#[derive(Clone, Debug)]
pub enum MenuNode {
    /// Clickable leaf menu item.
    Item {
        /// Unique key emitted on click.
        key: SharedString,
        /// Display label.
        label: SharedString,
        /// Optional leading [`LucideIcon`].
        icon: Option<LucideIcon>,
        /// Optional trailing badge text.
        badge: Option<SharedString>,
        /// Disables clicking this item.
        disabled: bool,
    },
    /// Expandable submenu containing nested [`MenuNode`]s.
    SubMenu {
        /// Unique submenu key.
        key: SharedString,
        /// Submenu heading label.
        label: SharedString,
        /// Optional leading [`LucideIcon`].
        icon: Option<LucideIcon>,
        /// Optional trailing badge text.
        badge: Option<SharedString>,
        /// Disables expanding or opening this submenu.
        disabled: bool,
        /// Nested children (items, deeper submenus, or dividers).
        children: Vec<MenuNode>,
    },
    /// Visual divider rule.
    Divider,
}

impl MenuNode {
    /// Creates a leaf menu item.
    pub fn item(key: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self::Item {
            key: key.into(),
            label: label.into(),
            icon: None,
            badge: None,
            disabled: false,
        }
    }

    /// Creates an expandable submenu containing `children`.
    pub fn submenu(
        key: impl Into<SharedString>,
        label: impl Into<SharedString>,
        children: impl IntoIterator<Item = MenuNode>,
    ) -> Self {
        Self::SubMenu {
            key: key.into(),
            label: label.into(),
            icon: None,
            badge: None,
            disabled: false,
            children: children.into_iter().collect(),
        }
    }

    /// Attaches a leading [`LucideIcon`] to an item or submenu.
    pub fn icon(mut self, ic: LucideIcon) -> Self {
        match &mut self {
            Self::Item { icon, .. } | Self::SubMenu { icon, .. } => *icon = Some(ic),
            Self::Divider => {}
        }
        self
    }

    /// Attaches a trailing badge to an item or submenu.
    pub fn badge(mut self, text: impl Into<SharedString>) -> Self {
        match &mut self {
            Self::Item { badge, .. } | Self::SubMenu { badge, .. } => *badge = Some(text.into()),
            Self::Divider => {}
        }
        self
    }

    /// Disables an item or submenu.
    pub fn disabled(mut self, is_disabled: bool) -> Self {
        match &mut self {
            Self::Item { disabled, .. } | Self::SubMenu { disabled, .. } => *disabled = is_disabled,
            Self::Divider => {}
        }
        self
    }
}

/// Computes the set of submenu keys that contain `target_key` in a single O(N) traversal.
fn collect_active_ancestors(
    nodes: &[MenuNode],
    target_key: &str,
    ancestors: &mut HashSet<SharedString>,
) -> bool {
    for node in nodes {
        match node {
            MenuNode::Item { key, .. } => {
                if key.as_ref() == target_key {
                    return true;
                }
            }
            MenuNode::SubMenu { key, children, .. } => {
                if collect_active_ancestors(children, target_key, ancestors) {
                    ancestors.insert(key.clone());
                    return true;
                }
            }
            MenuNode::Divider => {}
        }
    }
    false
}

/// Events emitted by [`MenuState`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuEvent {
    /// Emitted when the user selects a leaf item key.
    Select(SharedString),
}

/// Entity state backing a multi-level [`Menu`].
pub struct MenuState {
    focus_handle: FocusHandle,
    nodes: Vec<MenuNode>,
    mode: MenuMode,
    active_key: Option<SharedString>,
    active_ancestors: HashSet<SharedString>,
    open_submenus: HashSet<SharedString>,
    /// Active top-level dropdown key when in [`MenuMode::Horizontal`].
    open_dropdown: Option<SharedString>,
    dropdown_trigger_bounds: Rc<RefCell<HashMap<SharedString, Bounds<Pixels>>>>,
    /// The open dropdown's handle (a focus trap the arrow keys move through).
    dropdown_focus: FocusHandle,
    /// The trigger to give focus back to when the dropdown closes.
    dropdown_return: Option<FocusHandle>,
    frame_style: StyleRefinement,
}

impl EventEmitter<MenuEvent> for MenuState {}

impl Focusable for MenuState {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl MenuState {
    /// Creates a new vertical [`MenuState`] with the given [`MenuNode`] hierarchy.
    pub fn new(nodes: Vec<MenuNode>, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            nodes,
            mode: MenuMode::Vertical,
            active_key: None,
            active_ancestors: HashSet::new(),
            open_submenus: HashSet::new(),
            open_dropdown: None,
            dropdown_trigger_bounds: Rc::new(RefCell::new(HashMap::new())),
            dropdown_focus: cx.focus_handle(),
            dropdown_return: None,
            frame_style: StyleRefinement::default(),
        }
    }

    fn rebuild_active_ancestors(&mut self) {
        self.active_ancestors.clear();
        if let Some(ref ak) = self.active_key {
            collect_active_ancestors(&self.nodes, ak, &mut self.active_ancestors);
        }
    }

    /// Switches between [`MenuMode::Vertical`] and [`MenuMode::Horizontal`].
    pub fn mode(mut self, mode: MenuMode) -> Self {
        self.mode = mode;
        self
    }

    /// Dynamically switches menu layout mode.
    pub fn set_mode(&mut self, mode: MenuMode, cx: &mut Context<Self>) {
        if self.mode != mode {
            self.mode = mode;
            self.open_dropdown = None;
            cx.notify();
        }
    }

    /// Dynamically replaces the menu node tree.
    pub fn set_nodes(&mut self, nodes: Vec<MenuNode>, cx: &mut Context<Self>) {
        self.nodes = nodes;
        self.open_dropdown = None;
        self.rebuild_active_ancestors();
        cx.notify();
    }

    /// Sets the initially active item key.
    pub fn default_active(mut self, key: impl Into<SharedString>) -> Self {
        self.active_key = Some(key.into());
        self.rebuild_active_ancestors();
        self
    }

    /// Sets initially expanded vertical submenu keys.
    pub fn default_open(mut self, keys: impl IntoIterator<Item = impl Into<SharedString>>) -> Self {
        self.open_submenus = keys.into_iter().map(Into::into).collect();
        self
    }

    /// Returns the currently selected item key.
    pub fn active_key(&self) -> Option<&SharedString> {
        self.active_key.as_ref()
    }

    /// Programmatically selects a leaf item key and emits [`MenuEvent::Select`].
    pub fn select(&mut self, key: impl Into<SharedString>, cx: &mut Context<Self>) {
        let key = key.into();
        let closed_dropdown = self.open_dropdown.take().is_some();
        if self.active_key.as_ref() != Some(&key) {
            self.active_key = Some(key.clone());
            self.rebuild_active_ancestors();
            cx.emit(MenuEvent::Select(key));
            cx.notify();
        } else if closed_dropdown {
            cx.notify();
        }
    }

    /// Toggles a submenu open/closed in vertical mode.
    pub fn toggle_submenu(&mut self, key: SharedString, cx: &mut Context<Self>) {
        if !self.open_submenus.remove(&key) {
            self.open_submenus.insert(key);
        }
        cx.notify();
    }

    fn on_key_down(&mut self, ev: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if ev.keystroke.key == "escape" && self.open_dropdown.take().is_some() {
            cx.notify();
            cx.stop_propagation();
        }
    }

    /// Opens the dropdown under the top-level trigger `trigger`; from the
    /// keyboard, focus goes into it (Down then reaches its first item).
    fn open_dropdown_from(
        &mut self,
        key: SharedString,
        trigger: FocusHandle,
        keyboard: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_dropdown = Some(key);
        self.dropdown_return = Some(trigger);
        if keyboard {
            window.focus(&self.dropdown_focus, cx);
        }
        cx.notify();
    }
}

#[allow(clippy::too_many_arguments)]
fn render_vertical_nodes(
    nodes: &[MenuNode],
    depth: usize,
    active_key: Option<&SharedString>,
    active_ancestors: &HashSet<SharedString>,
    open_submenus: &HashSet<SharedString>,
    in_popover: bool,
    entity: &Entity<MenuState>,
    theme: &Theme,
    out: &mut Vec<AnyElement>,
) {
    let hover_bg = theme.surface_hover;
    let indent = px(10.0 + depth as f32 * 16.0);
    let base_fg = if in_popover {
        theme.popover_foreground
    } else {
        theme.foreground
    };

    for node in nodes {
        match node {
            MenuNode::Divider => {
                out.push(
                    div()
                        .w_full()
                        .h(px(1.))
                        .my(px(4.))
                        .bg(theme.border)
                        .into_any_element(),
                );
            }
            MenuNode::Item {
                key,
                label,
                icon,
                badge: badge_txt,
                disabled,
            } => {
                let is_active = active_key == Some(key);
                let key_clone = key.clone();
                let ent = entity.clone();
                let (active_bg, fg, icon_fg) = if is_active {
                    if in_popover {
                        (
                            theme.primary_soft,
                            theme.primary_soft_foreground,
                            theme.primary_soft_foreground,
                        )
                    } else {
                        (
                            theme.sidebar_active_bg,
                            theme.sidebar_active_foreground,
                            theme.sidebar_active_icon,
                        )
                    }
                } else {
                    (crate::transparent_black(), base_fg, theme.muted_foreground)
                };

                let item = div().id((ElementId::from("vmenu-item"), key.clone()));
                out.push(
                    // Each item is a tab stop, as a link in a sidebar is.
                    super::focus::tab_stop(item, !*disabled, theme)
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap(px(8.))
                        .w_full()
                        .pl(indent)
                        .pr(px(10.))
                        .py(px(7.))
                        .rounded(theme.radius_md)
                        .bg(active_bg)
                        .text_size(px(13.))
                        .font_weight(if is_active {
                            FontWeight::SEMIBOLD
                        } else {
                            FontWeight::MEDIUM
                        })
                        .text_color(fg)
                        .when(!*disabled, |el| {
                            el.cursor_pointer()
                                .when(!is_active, |el| el.hover(move |s| s.bg(hover_bg)))
                                .on_click(move |_, _, cx| {
                                    ent.update(cx, |this, cx| this.select(key_clone.clone(), cx));
                                })
                        })
                        .when(*disabled, |el| el.opacity(0.45).cursor_not_allowed())
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(8.))
                                .flex_1()
                                .min_w_0()
                                .when_some(*icon, |el, ic| {
                                    el.child(Icon::new(ic).size(IconSize::Sm).color(icon_fg))
                                })
                                .child(div().truncate().child(label.clone())),
                        )
                        .when_some(badge_txt.clone(), |el, b| {
                            el.child(badge(b).color(BadgeColor::Gray).pill(true))
                        })
                        .into_any_element(),
                );
            }
            MenuNode::SubMenu {
                key,
                label,
                icon,
                badge: badge_txt,
                disabled,
                children,
            } => {
                let is_open = !*disabled && open_submenus.contains(key);
                let has_active_child = active_ancestors.contains(key);
                let key_clone = key.clone();
                let ent = entity.clone();

                let header = div().id((ElementId::from("vmenu-sub"), key.clone()));
                let header = super::focus::tab_stop(header, !*disabled, theme)
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .w_full()
                    .pl(indent)
                    .pr(px(10.))
                    .py(px(7.))
                    .rounded(theme.radius_md)
                    .text_size(px(13.))
                    .font_weight(if has_active_child {
                        FontWeight::SEMIBOLD
                    } else {
                        FontWeight::MEDIUM
                    })
                    .text_color(if has_active_child {
                        theme.primary
                    } else {
                        base_fg
                    })
                    .when(!*disabled, |el| {
                        el.cursor_pointer()
                            .hover(move |s| s.bg(hover_bg))
                            .on_click(move |_, _, cx| {
                                ent.update(cx, |this, cx| {
                                    this.toggle_submenu(key_clone.clone(), cx);
                                });
                            })
                    })
                    .when(*disabled, |el| el.opacity(0.45).cursor_not_allowed())
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.))
                            .flex_1()
                            .min_w_0()
                            .when_some(*icon, |el, ic| {
                                el.child(
                                    Icon::new(ic)
                                        .size(IconSize::Sm)
                                        .color(if has_active_child {
                                            theme.primary
                                        } else {
                                            theme.muted_foreground
                                        }),
                                )
                            })
                            .child(div().truncate().child(label.clone())),
                    )
                    .when_some(badge_txt.clone(), |el, b| {
                        el.child(badge(b).color(BadgeColor::Gray).pill(true))
                    })
                    .child(
                        Icon::new(if is_open {
                            LucideIcon::ChevronDown
                        } else {
                            LucideIcon::ChevronRight
                        })
                        .size(IconSize::Xs)
                        .color(theme.muted_foreground),
                    );

                out.push(header.into_any_element());
                if is_open {
                    render_vertical_nodes(
                        children,
                        depth + 1,
                        active_key,
                        active_ancestors,
                        open_submenus,
                        in_popover,
                        entity,
                        theme,
                        out,
                    );
                }
            }
        }
    }
}

impl Render for MenuState {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let reduce_motion = cx.reduce_motion();
        let hover_bg = theme.surface_hover;
        let entity = cx.entity();
        let entity_id = cx.entity_id();

        // A dropdown closed with focus inside it (an item was picked, or
        // Escape): focus goes back to its trigger.
        if self.open_dropdown.is_none()
            && let Some(trigger) = self.dropdown_return.take()
            && self.dropdown_focus.contains_focused(window, cx)
        {
            window.focus(&trigger, cx);
        }

        match self.mode {
            MenuMode::Vertical => {
                let mut children = Vec::with_capacity(self.nodes.len());
                render_vertical_nodes(
                    &self.nodes,
                    0,
                    self.active_key.as_ref(),
                    &self.active_ancestors,
                    &self.open_submenus,
                    false,
                    &entity,
                    &theme,
                    &mut children,
                );
                let mut root = div()
                    .id(("vmenu-root", entity_id))
                    .track_focus(&self.focus_handle)
                    .on_key_down(cx.listener(Self::on_key_down))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .w_full()
                    .p(px(6.))
                    .rounded(theme.radius_xl)
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.surface)
                    .children(children);
                crate::Refineable::refine(root.style(), &self.frame_style);
                root
            }
            MenuMode::Horizontal => {
                // The top-level items are one tab stop (the active one, or the
                // first); Left and Right move between them, as in a menubar.
                let tops: Vec<(bool, bool)> = self
                    .nodes
                    .iter()
                    .filter_map(|node| match node {
                        MenuNode::Divider => None,
                        MenuNode::Item { key, disabled, .. } => {
                            Some((!*disabled, self.active_key.as_ref() == Some(key)))
                        }
                        MenuNode::SubMenu { key, disabled, .. } => {
                            Some((!*disabled, self.active_ancestors.contains(key)))
                        }
                    })
                    .collect();
                let enabled: Vec<bool> = tops.iter().map(|t| t.0).collect();
                let tab_item =
                    super::focus::choice_tab_item(tops.iter().position(|t| t.1), &enabled);
                let handles =
                    super::focus::choice_focus(("hmenu-focus", entity_id), tops.len(), window, cx);
                let mut top_index = 0;

                let mut bar = div()
                    .id(("hmenu-root", entity_id))
                    .track_focus(&self.focus_handle)
                    .on_key_down(cx.listener(Self::on_key_down))
                    .on_key_down({
                        let handles = handles.clone();
                        let entity = entity.clone();
                        move |ev: &KeyDownEvent, window, cx| {
                            let key = ev.keystroke.key.as_str();
                            if ev.keystroke.modifiers.modified()
                                || !matches!(key, "left" | "right" | "home" | "end")
                            {
                                return;
                            }
                            let Some(from) = handles.iter().position(|h| h.is_focused(window))
                            else {
                                return;
                            };
                            if let Some(to) = super::focus::choice_step(key, from, &enabled) {
                                entity.update(cx, |this, cx| {
                                    if this.open_dropdown.take().is_some() {
                                        cx.notify();
                                    }
                                });
                                window.focus(&handles[to], cx);
                                cx.stop_propagation();
                            }
                        }
                    })
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .items_center()
                    .gap(px(4.))
                    .w_full()
                    .px(px(8.))
                    .py(px(6.))
                    .rounded(theme.radius_xl)
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.surface);

                for node in &self.nodes {
                    match node {
                        MenuNode::Divider => {
                            bar = bar.child(div().w(px(1.)).h(px(18.)).mx(px(4.)).bg(theme.border));
                        }
                        MenuNode::Item {
                            key,
                            label,
                            icon,
                            badge: badge_txt,
                            disabled,
                        } => {
                            let is_active = self.active_key.as_ref() == Some(key);
                            let key_click = key.clone();
                            let ent = entity.clone();
                            let index = top_index;
                            top_index += 1;
                            let handle =
                                super::focus::choice_item_handle(&handles, index, tab_item);
                            let ring = super::focus::focus_visible_ring(&handle, window, &theme);
                            bar = bar.child(
                                div()
                                    .id((ElementId::from("hmenu-item"), key.clone()))
                                    .when(!*disabled, |el| el.track_focus(&handle))
                                    .when_some(ring, |el, ring| el.shadow(ring))
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(6.))
                                    .px(px(10.))
                                    .py(px(6.))
                                    .rounded(theme.radius_md)
                                    .bg(if is_active {
                                        theme.primary_soft
                                    } else {
                                        crate::transparent_black()
                                    })
                                    .text_size(px(13.))
                                    .font_weight(if is_active {
                                        FontWeight::SEMIBOLD
                                    } else {
                                        FontWeight::MEDIUM
                                    })
                                    .text_color(if is_active {
                                        theme.primary_soft_foreground
                                    } else {
                                        theme.foreground
                                    })
                                    .when(!*disabled, |el| {
                                        el.cursor_pointer()
                                            .when(!is_active, |el| el.hover(move |s| s.bg(hover_bg)))
                                            .on_click(move |_, _, cx| {
                                                ent.update(cx, |this, cx| {
                                                    this.select(key_click.clone(), cx);
                                                });
                                            })
                                    })
                                    .when(*disabled, |el| el.opacity(0.45).cursor_not_allowed())
                                    .when_some(*icon, |el, ic| {
                                        el.child(Icon::new(ic).size(IconSize::Sm))
                                    })
                                    .child(label.clone())
                                    .when_some(badge_txt.clone(), |el, b| {
                                        el.child(badge(b).color(BadgeColor::Gray).pill(true))
                                    }),
                            );
                        }
                        MenuNode::SubMenu {
                            key,
                            label,
                            icon,
                            badge: badge_txt,
                            disabled,
                            children,
                        } => {
                            let is_open = !*disabled && self.open_dropdown.as_ref() == Some(key);
                            let has_active = self.active_ancestors.contains(key);
                            let index = top_index;
                            top_index += 1;
                            let handle =
                                super::focus::choice_item_handle(&handles, index, tab_item);
                            let ring = super::focus::focus_visible_ring(&handle, window, &theme);
                            let trigger_click = handle.clone();
                            let trigger_down = handle.clone();
                            let trigger_tab = handle.clone();
                            let key_down = key.clone();
                            let ent_down = entity.clone();
                            let key_toggle = key.clone();
                            let key_bounds = key.clone();
                            let key_out = key.clone();
                            let bounds_map_write = self.dropdown_trigger_bounds.clone();
                            let bounds_map_out = self.dropdown_trigger_bounds.clone();
                            let captured_bounds =
                                self.dropdown_trigger_bounds.borrow().get(key).copied();
                            let ent_toggle = entity.clone();

                            let dropdown = is_open.then(|| {
                                let mut rendered_sub = Vec::with_capacity(children.len());
                                render_vertical_nodes(
                                    children,
                                    0,
                                    self.active_key.as_ref(),
                                    &self.active_ancestors,
                                    &self.open_submenus,
                                    true,
                                    &entity,
                                    &theme,
                                    &mut rendered_sub,
                                );
                                let ent_out = entity.clone();
                                let ent_key = entity.clone();
                                let card = div()
                                    .id((ElementId::from("hmenu-pop"), key.clone()))
                                    // Up and Down move through the items, wrapping.
                                    .track_focus(&self.dropdown_focus)
                                    .focus_trap()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .min_w(px(200.))
                                    .max_w(px(320.))
                                    .max_h(px(360.))
                                    .overflow_y_scroll()
                                    .p(px(6.))
                                    .rounded(theme.radius_lg)
                                    .bg(theme.popover)
                                    .text_color(theme.popover_foreground)
                                    .border_1()
                                    .border_color(theme.border)
                                    .when(theme.elevation_shadows, |el| el.shadow_lg())
                                    .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
                                    .on_mouse_down_out(move |ev: &MouseDownEvent, _, cx| {
                                        if bounds_map_out
                                            .borrow()
                                            .get(&key_out)
                                            .is_some_and(|b| b.contains(&ev.position))
                                        {
                                            return;
                                        }
                                        ent_out.update(cx, |this, cx| {
                                            if this.open_dropdown.take().is_some() {
                                                cx.notify();
                                            }
                                        });
                                    })
                                    .on_key_down(move |ev: &KeyDownEvent, window, cx| {
                                        if super::popover::menu_arrow_key(ev, window, cx) {
                                            return;
                                        }
                                        let key = ev.keystroke.key.as_str();
                                        if key == "escape" || key == "tab" {
                                            ent_key.update(cx, |this, cx| {
                                                if this.open_dropdown.take().is_some() {
                                                    cx.notify();
                                                }
                                            });
                                        }
                                        if key == "escape" {
                                            // Focus goes back to the trigger on the next render.
                                            cx.stop_propagation();
                                        } else if key == "tab" {
                                            // Tab closes it and moves on from the trigger.
                                            window.focus(&trigger_tab, cx);
                                        }
                                    })
                                    .children(rendered_sub);

                                let anim = if reduce_motion {
                                    card.into_any_element()
                                } else {
                                    card.with_animation(
                                        (ElementId::from("hmenu-anim"), key.clone()),
                                        Animation::new(ANIM_FAST).with_easing(ease_out_quint()),
                                        |el, delta| el.opacity(delta),
                                    )
                                    .into_any_element()
                                };

                                let mut anchor_el = anchored()
                                    .anchor(Anchor::TopLeft)
                                    .snap_to_window_with_margin(px(8.));
                                if let Some(bounds) = captured_bounds {
                                    anchor_el = anchor_el
                                        .attach(bounds, Anchor::BottomLeft)
                                        .offset(point(px(0.), px(6.)));
                                } else {
                                    anchor_el = anchor_el.offset(point(px(0.), px(6.)));
                                }
                                deferred(anchor_el.child(anim)).priority(35)
                            });

                            bar = bar.child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .id((ElementId::from("hmenu-sub"), key.clone()))
                                            .when(!*disabled, |el| {
                                                el.track_focus(&handle).on_key_down(
                                                    move |ev: &KeyDownEvent, window, cx| {
                                                        // Down opens the dropdown and goes into it.
                                                        if ev.keystroke.key != "down"
                                                            || ev.keystroke.modifiers.modified()
                                                        {
                                                            return;
                                                        }
                                                        let key = key_down.clone();
                                                        let trigger = trigger_down.clone();
                                                        ent_down.update(cx, |this, cx| {
                                                            this.open_dropdown_from(
                                                                key, trigger, true, window, cx,
                                                            );
                                                        });
                                                        cx.stop_propagation();
                                                    },
                                                )
                                            })
                                            .when_some(ring, |el, ring| el.shadow(ring))
                                            .relative()
                                            .flex()
                                            .flex_row()
                                            .items_center()
                                            .gap(px(6.))
                                            .px(px(10.))
                                            .py(px(6.))
                                            .rounded(theme.radius_md)
                                            .bg(if is_open || has_active {
                                                theme.primary_soft
                                            } else {
                                                crate::transparent_black()
                                            })
                                            .text_size(px(13.))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(if is_open || has_active {
                                                theme.primary_soft_foreground
                                            } else {
                                                theme.foreground
                                            })
                                            .when(!*disabled, |el| {
                                                el.cursor_pointer()
                                                    .when(!is_open && !has_active, |el| {
                                                        el.hover(move |s| s.bg(hover_bg))
                                                    })
                                                    .on_click(move |ev, window, cx| {
                                                        ent_toggle.update(cx, |this, cx| {
                                                            if this.open_dropdown.as_ref()
                                                                == Some(&key_toggle)
                                                            {
                                                                this.open_dropdown = None;
                                                                cx.notify();
                                                            } else {
                                                                // Focus stays on the trigger the
                                                                // press focused.
                                                                this.open_dropdown_from(
                                                                    key_toggle.clone(),
                                                                    trigger_click.clone(),
                                                                    ev.is_keyboard(),
                                                                    window,
                                                                    cx,
                                                                );
                                                            }
                                                        });
                                                    })
                                            })
                                            .when(*disabled, |el| {
                                                el.opacity(0.45).cursor_not_allowed()
                                            })
                                            .child(
                                                canvas(
                                                    move |bounds, _, _| {
                                                        bounds_map_write
                                                            .borrow_mut()
                                                            .insert(key_bounds.clone(), bounds);
                                                    },
                                                    |_, _, _, _| {},
                                                )
                                                .absolute()
                                                .size_full(),
                                            )
                                            .when_some(*icon, |el, ic| {
                                                el.child(Icon::new(ic).size(IconSize::Sm))
                                            })
                                            .child(label.clone())
                                            .when_some(badge_txt.clone(), |el, b| {
                                                el.child(
                                                    badge(b).color(BadgeColor::Gray).pill(true),
                                                )
                                            })
                                            .child(
                                                Icon::new(LucideIcon::ChevronDown)
                                                    .size(IconSize::Xs)
                                                    .color(theme.muted_foreground),
                                            ),
                                    )
                                    .when_some(dropdown, |el, dd| el.child(div().h_0().child(dd))),
                            );
                        }
                    }
                }

                crate::Refineable::refine(bar.style(), &self.frame_style);
                bar
            }
        }
    }
}

/// Renderable multi-level navigation [`Menu`] wrapping [`Entity<MenuState>`].
#[derive(IntoElement)]
pub struct Menu {
    state: Entity<MenuState>,
    style: StyleRefinement,
}

/// Convenience constructor for [`Menu::new`].
pub fn nav_menu(state: &Entity<MenuState>) -> Menu {
    Menu::new(state)
}

impl Menu {
    /// Creates a [`Menu`] bound to `state`.
    pub fn new(state: &Entity<MenuState>) -> Self {
        Self {
            state: state.clone(),
            style: StyleRefinement::default(),
        }
    }
}

impl Styled for Menu {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Menu {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        if self.state.read(cx).frame_style != self.style {
            let style = self.style;
            self.state.update(cx, |s, _| {
                s.frame_style = style;
            });
        }
        self.state
    }
}
