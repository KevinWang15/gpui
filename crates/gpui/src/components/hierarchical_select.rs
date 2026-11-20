//! Hierarchical dropdown pickers:
//! - [`TreeSelect`] / [`TreeSelectState`]: searchable virtualized tree dropdown built on [`TreeNode`].
//! - [`Cascader`] / [`CascaderState`]: multi-column Miller-column cascading path selector.

use std::cell::Cell;
use std::rc::Rc;

use crate::icons::{Icon, IconSize, LucideIcon};
use crate::{
    Anchor, Animation, AnimationExt, App, Bounds, Context, ElementId, Entity, EventEmitter,
    FocusHandle, Focusable, FontWeight, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent,
    Pixels, Render, RenderOnce, SharedString, StyleRefinement, Styled, Window, anchored, canvas,
    deferred, div, ease_out_quint, point, prelude::*, px,
};

use super::theme::{ANIM_FAST, ActiveTheme, ComponentSize};
use super::tree::{Tree, TreeEvent, TreeNode, TreeState};

// ============================================================================
// TreeSelect (Searchable Hierarchical Dropdown)
// ============================================================================

/// Events emitted by [`TreeSelectState`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreeSelectEvent {
    /// Emitted when the selected node ID changes (or is cleared).
    Change(Option<SharedString>),
}

/// Entity state backing a searchable hierarchical [`TreeSelect`].
pub struct TreeSelectState {
    focus_handle: FocusHandle,
    tree: Entity<TreeState>,
    nodes: Vec<TreeNode>,
    selected: Option<SharedString>,
    cached_label: Option<SharedString>,
    placeholder: SharedString,
    clearable: bool,
    disabled: bool,
    open: bool,
    size: ComponentSize,
    trigger_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    frame_style: StyleRefinement,
}

impl EventEmitter<TreeSelectEvent> for TreeSelectState {}

impl Focusable for TreeSelectState {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

fn find_tree_label(nodes: &[TreeNode], id: &str) -> Option<SharedString> {
    for n in nodes {
        if n.id.as_ref() == id {
            return Some(n.label.clone());
        }
        if let Some(found) = find_tree_label(&n.children, id) {
            return Some(found);
        }
    }
    None
}

impl TreeSelectState {
    /// Creates a new searchable [`TreeSelectState`] from hierarchical [`TreeNode`]s.
    pub fn new(nodes: Vec<TreeNode>, cx: &mut Context<Self>) -> Self {
        let initial_expanded: Vec<SharedString> = nodes
            .iter()
            .filter(|n| n.is_branch())
            .map(|n| n.id.clone())
            .collect();
        let tree_nodes = nodes.clone();
        let tree = cx.new(|cx| {
            TreeState::new(tree_nodes, cx)
                .searchable(true)
                .max_height(px(240.))
                .default_expanded(initial_expanded)
        });
        cx.subscribe(&tree, |this, _, ev: &TreeEvent, cx| {
            if let TreeEvent::Select(id) = ev {
                let next = Some(id.clone());
                let changed = this.selected != next;
                this.selected = next.clone();
                this.cached_label = find_tree_label(&this.nodes, id);
                this.open = false;
                this.tree.update(cx, |t, cx| t.clear_search(cx));
                if changed {
                    cx.emit(TreeSelectEvent::Change(next));
                }
                cx.notify();
            }
        })
        .detach();

        Self {
            focus_handle: cx.focus_handle(),
            tree,
            nodes,
            selected: None,
            cached_label: None,
            placeholder: "Select from hierarchy...".into(),
            clearable: true,
            disabled: false,
            open: false,
            size: ComponentSize::Md,
            trigger_bounds: Rc::new(Cell::new(None)),
            frame_style: StyleRefinement::default(),
        }
    }

    /// Sets placeholder text.
    pub fn placeholder(mut self, text: impl Into<SharedString>) -> Self {
        self.placeholder = text.into();
        self
    }

    /// Sets initial selected node ID without emitting a change event.
    pub fn default_selected(
        mut self,
        id: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) -> Self {
        let id: SharedString = id.into();
        self.cached_label = find_tree_label(&self.nodes, &id);
        self.selected = Some(id.clone());
        self.tree.update(cx, |t, cx| t.select_silent(Some(id), cx));
        self
    }

    /// Enables or disables the clear (`×`) button (default `true`).
    pub fn clearable(mut self, clearable: bool) -> Self {
        self.clearable = clearable;
        self
    }

    /// Sets trigger size.
    pub fn size(mut self, size: ComponentSize) -> Self {
        self.size = size;
        self
    }

    /// Disables user interaction.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Dynamically replaces the available tree nodes and prunes stale selection if missing.
    pub fn set_nodes(&mut self, nodes: Vec<TreeNode>, cx: &mut Context<Self>) {
        self.nodes = nodes.clone();
        self.tree.update(cx, |t, cx| t.set_nodes(nodes, cx));
        self.cached_label = self
            .selected
            .as_deref()
            .and_then(|id| find_tree_label(&self.nodes, id));
        if self.cached_label.is_none() && self.selected.is_some() {
            self.selected = None;
            self.tree.update(cx, |t, cx| t.clear_selection(cx));
        }
        cx.notify();
    }

    /// Returns the currently selected node ID.
    pub fn selected(&self) -> Option<&SharedString> {
        self.selected.as_ref()
    }

    /// Returns whether the dropdown popover is currently open.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Opens the dropdown popover and focuses the embedded search bar.
    pub fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.disabled && !self.open {
            self.open = true;
            let search_fh = self
                .tree
                .read(cx)
                .search_input()
                .read(cx)
                .focus_handle(cx);
            window.focus(&search_fh, cx);
            cx.notify();
        }
    }

    /// Closes the dropdown popover and clears any active search query.
    pub fn close(&mut self, cx: &mut Context<Self>) {
        if self.open {
            self.open = false;
            self.tree.update(cx, |t, cx| t.clear_search(cx));
            cx.notify();
        }
    }

    /// Programmatically updates selection and emits [`TreeSelectEvent::Change`] once.
    pub fn set_selected(&mut self, id: Option<SharedString>, cx: &mut Context<Self>) {
        if self.selected != id {
            self.selected = id.clone();
            self.cached_label = id
                .as_deref()
                .and_then(|k| find_tree_label(&self.nodes, k));
            self.tree.update(cx, |t, cx| {
                if let Some(ref sel) = id {
                    t.select_silent(Some(sel.clone()), cx);
                } else {
                    t.clear_selection(cx);
                }
            });
            cx.emit(TreeSelectEvent::Change(id));
            cx.notify();
        }
    }

    fn on_key_down(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        let mods = ev.keystroke.modifiers;
        if mods.control || mods.platform || mods.alt {
            return;
        }
        match ev.keystroke.key.as_str() {
            "escape" if self.open => {
                self.close(cx);
                window.focus(&self.focus_handle, cx);
                cx.stop_propagation();
            }
            "tab" if self.open => self.close(cx),
            "enter" | "space" if !self.open => {
                self.open(window, cx);
                cx.stop_propagation();
            }
            "down" if !self.open => {
                self.open(window, cx);
                cx.stop_propagation();
            }
            _ => {}
        }
    }

    /// Tab inside the popup moves between its search field and its tree;
    /// Tab out of either end closes it and moves on from the trigger, as a
    /// browser's select does (the popup comes last in the tab order).
    fn on_popup_tab(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let tree = self.tree.read(cx);
        let leaving = if ev.keystroke.modifiers.shift {
            tree.search_input().read(cx).focus_handle(cx)
        } else {
            tree.focus_handle(cx)
        };
        if leaving.is_focused(window) {
            self.close(cx);
            window.focus(&self.focus_handle, cx);
        }
    }
}

impl Render for TreeSelectState {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let reduce_motion = cx.reduce_motion();
        let hover_bg = theme.surface_hover;
        let entity_id = cx.entity_id();
        let is_focused = self.focus_handle.is_focused(window);
        let focus_visible = self.focus_handle.is_focus_visible(window);
        let bounds_cell = self.trigger_bounds.clone();
        let bounds_for_out = self.trigger_bounds.clone();
        let captured_bounds = self.trigger_bounds.get();

        let display_label = self.cached_label.clone();

        let popover = (self.open && !self.disabled).then(|| {
            let pop_w = captured_bounds
                .map(|b| b.size.width.max(px(280.)))
                .unwrap_or(px(300.));
            let panel = div()
                .id(("ts-popover", entity_id))
                .flex()
                .flex_col()
                .w(pop_w)
                .p(px(10.))
                .rounded(theme.radius_xl)
                .bg(theme.popover)
                .text_color(theme.popover_foreground)
                .border_1()
                .border_color(theme.border)
                .when(theme.elevation_shadows, |el| el.shadow_xl())
                // A click in the popup keeps focus where it was (the trigger,
                // or the field or tree the click focused).
                .on_any_mouse_down(|_, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation()
                })
                .on_mouse_down_out(cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                    if bounds_for_out
                        .get()
                        .is_some_and(|b| b.contains(&ev.position))
                    {
                        return;
                    }
                    this.close(cx);
                }))
                .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                    if ev.keystroke.key == "escape" && this.open {
                        this.close(cx);
                        window.focus(&this.focus_handle, cx);
                        cx.stop_propagation();
                    } else if ev.keystroke.key == "tab" && this.open {
                        this.on_popup_tab(ev, window, cx);
                    }
                }))
                // `TreeState` already renders its search bar pinned above its virtualized body!
                .child(Tree::new(&self.tree));

            let anim = if reduce_motion {
                panel.into_any_element()
            } else {
                panel
                    .with_animation(
                        ("ts-anim", entity_id),
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

        let has_resolved_selection = display_label.is_some();
        let mut trigger = div()
            .id(("ts-trigger", entity_id))
            .relative()
            .when(!self.disabled, |el| {
                el.track_focus(&self.focus_handle.clone().tab_stop(true))
                    .on_key_down(cx.listener(Self::on_key_down))
            })
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .w_full()
            .h(self.size.height())
            .px(self.size.px_x())
            .rounded(theme.radius_md)
            .border_1()
            .border_color(if (self.open || is_focused) && !self.disabled {
                theme.ring
            } else {
                theme.input_border
            })
            // Dashed outline for keyboard focus only (`focus` module docs).
            .when(focus_visible && !self.disabled, |el| {
                el.when_some(super::focus::focus_outline(&theme), |el, outline| {
                    el.outline(outline)
                })
            })
            .bg(if self.disabled {
                theme.disabled_bg
            } else {
                theme.input_bg
            })
            .text_size(self.size.font_size())
            .when(!self.disabled, |el| {
                el.cursor_pointer()
                    .hover(move |s| s.bg(hover_bg))
                    .on_click(cx.listener(|this, _, window, cx| {
                        if this.open {
                            this.close(cx);
                            window.focus(&this.focus_handle, cx);
                        } else {
                            this.open(window, cx);
                        }
                    }))
            })
            .when(self.disabled, |el| el.opacity(0.55).cursor_not_allowed())
            .child(
                canvas(
                    move |bounds, _, _| bounds_cell.set(Some(bounds)),
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .flex_1()
                    .min_w_0()
                    .child(
                        Icon::new(LucideIcon::FolderTree)
                            .size(IconSize::Sm)
                            .color(theme.muted_foreground),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_color(if has_resolved_selection {
                                theme.foreground
                            } else {
                                theme.placeholder
                            })
                            .child(display_label.unwrap_or_else(|| self.placeholder.clone())),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .flex_shrink_0()
                    .gap(px(4.))
                    .when(self.clearable && has_resolved_selection && !self.disabled, |el| {
                        el.child(
                            div()
                                .id(("ts-clear", entity_id))
                                .p(px(2.))
                                .rounded(theme.radius_sm)
                                .hover(move |s| s.bg(hover_bg))
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.set_selected(None, cx);
                                    cx.stop_propagation();
                                }))
                                .child(
                                    Icon::new(LucideIcon::X)
                                        .size(IconSize::Xs)
                                        .color(theme.muted_foreground),
                                ),
                        )
                    })
                    .child(
                        Icon::new(LucideIcon::ChevronDown)
                            .size(IconSize::Sm)
                            .color(theme.muted_foreground),
                    ),
            );

        crate::Refineable::refine(trigger.style(), &self.frame_style);
        let has_explicit_width = self.frame_style.size.width.is_some();

        div()
            .flex()
            .flex_col()
            .when(!has_explicit_width, |el| el.w_full())
            .child(trigger)
            .when_some(popover, |el, pop| el.child(div().h_0().child(pop)))
    }
}

/// Renderable [`TreeSelect`] wrapping [`Entity<TreeSelectState>`].
#[derive(IntoElement)]
pub struct TreeSelect {
    state: Entity<TreeSelectState>,
    style: StyleRefinement,
}

/// Convenience constructor for [`TreeSelect::new`].
pub fn tree_select(state: &Entity<TreeSelectState>) -> TreeSelect {
    TreeSelect::new(state)
}

impl TreeSelect {
    /// Creates a [`TreeSelect`] bound to `state`.
    pub fn new(state: &Entity<TreeSelectState>) -> Self {
        Self {
            state: state.clone(),
            style: StyleRefinement::default(),
        }
    }
}

impl Styled for TreeSelect {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for TreeSelect {
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

// ============================================================================
// Cascader (Multi-Column Miller-Column Cascading Selector)
// ============================================================================

/// Option node inside a multi-level [`Cascader`].
#[derive(Clone, Debug)]
pub struct CascaderOption {
    /// Machine value for this segment.
    pub value: SharedString,
    /// Display label.
    pub label: SharedString,
    /// Disables picking or expanding this node.
    pub disabled: bool,
    /// Next-level nested options.
    pub children: Vec<CascaderOption>,
}

impl CascaderOption {
    /// Creates a leaf [`CascaderOption`].
    pub fn new(value: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
            disabled: false,
            children: Vec::new(),
        }
    }

    /// Attaches nested child options.
    pub fn children(mut self, children: impl IntoIterator<Item = CascaderOption>) -> Self {
        self.children = children.into_iter().collect();
        self
    }

    /// Marks this option disabled.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

/// Events emitted by [`CascaderState`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CascaderEvent {
    /// Emitted when the selected path of values changes.
    Change(Vec<SharedString>),
}

/// Entity state backing a multi-column [`Cascader`].
pub struct CascaderState {
    focus_handle: FocusHandle,
    options: Vec<CascaderOption>,
    /// Committed value path (e.g. `["us", "ca", "sf"]`).
    selected_path: Vec<SharedString>,
    /// Currently expanded column path while browsing inside the popover.
    active_path: Vec<SharedString>,
    placeholder: SharedString,
    separator: SharedString,
    clearable: bool,
    disabled: bool,
    open: bool,
    size: ComponentSize,
    trigger_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    frame_style: StyleRefinement,
}

impl EventEmitter<CascaderEvent> for CascaderState {}

impl Focusable for CascaderState {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl CascaderState {
    /// Creates a new [`CascaderState`] from top-level [`CascaderOption`]s.
    pub fn new(options: Vec<CascaderOption>, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            options,
            selected_path: Vec::new(),
            active_path: Vec::new(),
            placeholder: "Select nested option...".into(),
            separator: " / ".into(),
            clearable: true,
            disabled: false,
            open: false,
            size: ComponentSize::Md,
            trigger_bounds: Rc::new(Cell::new(None)),
            frame_style: StyleRefinement::default(),
        }
    }

    /// Sets the initial selected path.
    pub fn default_path(mut self, path: impl IntoIterator<Item = impl Into<SharedString>>) -> Self {
        let p: Vec<SharedString> = path.into_iter().map(Into::into).collect();
        self.selected_path = p.clone();
        self.active_path = p;
        self
    }

    /// Sets placeholder text.
    pub fn placeholder(mut self, text: impl Into<SharedString>) -> Self {
        self.placeholder = text.into();
        self
    }

    /// Customizes the path separator string displayed in the trigger (default `" / "`).
    pub fn separator(mut self, sep: impl Into<SharedString>) -> Self {
        self.separator = sep.into();
        self
    }

    /// Enables or disables the inline clear (`×`) button (default `true`).
    pub fn clearable(mut self, clearable: bool) -> Self {
        self.clearable = clearable;
        self
    }

    /// Sets trigger size.
    pub fn size(mut self, size: ComponentSize) -> Self {
        self.size = size;
        self
    }

    /// Disables interaction.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Dynamically replaces options and clears stale paths that no longer exist.
    pub fn set_options(&mut self, options: Vec<CascaderOption>, cx: &mut Context<Self>) {
        self.options = options;
        if self.selected_labels().len() != self.selected_path.len() {
            self.selected_path.clear();
            self.active_path.clear();
        }
        cx.notify();
    }

    /// Returns whether the cascading popover is open.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Closes the popover if open.
    pub fn close(&mut self, cx: &mut Context<Self>) {
        if self.open {
            self.open = false;
            cx.notify();
        }
    }

    /// Returns the committed value path.
    pub fn selected_path(&self) -> &[SharedString] {
        &self.selected_path
    }

    /// Resolves the human-readable labels for the committed path.
    pub fn selected_labels(&self) -> Vec<SharedString> {
        let mut labels = Vec::new();
        let mut level = self.options.as_slice();
        for seg in &self.selected_path {
            if let Some(opt) = level.iter().find(|o| &o.value == seg) {
                labels.push(opt.label.clone());
                level = &opt.children;
            } else {
                break;
            }
        }
        labels
    }

    /// Programmatically sets the selected path and emits [`CascaderEvent::Change`].
    pub fn set_path(&mut self, path: Vec<SharedString>, cx: &mut Context<Self>) {
        if self.selected_path != path {
            self.selected_path = path.clone();
            self.active_path = path.clone();
            cx.emit(CascaderEvent::Change(path));
            cx.notify();
        }
    }

    pub(crate) fn click_option(
        &mut self,
        depth: usize,
        value: SharedString,
        is_leaf: bool,
        cx: &mut Context<Self>,
    ) {
        self.active_path.truncate(depth);
        self.active_path.push(value);
        if is_leaf {
            let committed = self.active_path.clone();
            self.open = false;
            if self.selected_path != committed {
                self.set_path(committed, cx);
            } else {
                // Re-clicking the already-selected leaf must still close visually!
                cx.notify();
            }
        } else {
            cx.notify();
        }
    }

    fn on_key_down(&mut self, ev: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        let mods = ev.keystroke.modifiers;
        if mods.control || mods.platform || mods.alt {
            return;
        }
        match ev.keystroke.key.as_str() {
            "escape" if self.open => {
                self.close(cx);
                cx.stop_propagation();
            }
            // As a browser's select: Tab closes the popup and still moves on.
            "tab" if self.open => self.close(cx),
            "enter" | "space" => {
                self.open = !self.open;
                if self.open {
                    self.active_path = self.selected_path.clone();
                }
                cx.notify();
                cx.stop_propagation();
            }
            "down" if !self.open => {
                self.open = true;
                self.active_path = self.selected_path.clone();
                cx.notify();
                cx.stop_propagation();
            }
            _ => {}
        }
    }
}

impl Render for CascaderState {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let reduce_motion = cx.reduce_motion();
        let hover_bg = theme.surface_hover;
        let entity_id = cx.entity_id();
        let is_focused = self.focus_handle.is_focused(window);
        let focus_visible = self.focus_handle.is_focus_visible(window);
        let bounds_cell = self.trigger_bounds.clone();
        let bounds_for_out = self.trigger_bounds.clone();
        let captured_bounds = self.trigger_bounds.get();

        let labels = self.selected_labels();
        let display_text: Option<String> = (!labels.is_empty()).then(|| {
            labels
                .iter()
                .map(|s| s.as_ref())
                .collect::<Vec<_>>()
                .join(self.separator.as_ref())
        });

        let popover = (self.open && !self.disabled).then(|| {
            // Collect visible Miller columns from `active_path` (skipping disabled branches)
            let mut columns: Vec<Vec<(SharedString, SharedString, bool, bool)>> = Vec::new();
            let mut level = self.options.as_slice();
            let mut depth = 0usize;
            while !level.is_empty() {
                columns.push(
                    level
                        .iter()
                        .map(|o| {
                            (
                                o.value.clone(),
                                o.label.clone(),
                                o.disabled,
                                !o.children.is_empty(),
                            )
                        })
                        .collect(),
                );
                if let Some(active_val) = self.active_path.get(depth)
                    && let Some(found) = level
                        .iter()
                        .find(|o| &o.value == active_val && !o.disabled)
                    && !found.children.is_empty()
                {
                    level = &found.children;
                    depth += 1;
                    continue;
                }
                break;
            }

            let panel = div()
                .id(("casc-popover", entity_id))
                .flex()
                .flex_row()
                .max_w(px(560.))
                .overflow_x_scroll()
                .rounded(theme.radius_xl)
                .bg(theme.popover)
                .text_color(theme.popover_foreground)
                .border_1()
                .border_color(theme.border)
                .when(theme.elevation_shadows, |el| el.shadow_xl())
                // A click in the popup keeps focus where it was (the trigger,
                // or the field or tree the click focused).
                .on_any_mouse_down(|_, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation()
                })
                .on_mouse_down_out(cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                    if bounds_for_out
                        .get()
                        .is_some_and(|b| b.contains(&ev.position))
                    {
                        return;
                    }
                    this.close(cx);
                }))
                .on_key_down(cx.listener(Self::on_key_down))
                .when(columns.is_empty(), |el| {
                    el.px(px(16.))
                        .py(px(14.))
                        .text_size(px(12.5))
                        .text_color(theme.muted_foreground)
                        .child("No options")
                })
                .children(columns.into_iter().enumerate().map(|(col_depth, entries)| {
                    let active_val = self.active_path.get(col_depth).cloned();
                    div()
                        .id(("casc-col", col_depth))
                        .flex()
                        .flex_col()
                        .flex_shrink_0()
                        .w(px(164.))
                        .max_h(px(240.))
                        .overflow_y_scroll()
                        .p(px(5.))
                        .gap(px(2.))
                        .when(col_depth > 0, |el| {
                            el.border_l_1().border_color(theme.border)
                        })
                        .children(entries.into_iter().map(
                            |(val, lbl, disabled, has_children)| {
                                let is_active = active_val.as_ref() == Some(&val);
                                let opt_id = (ElementId::from(("casc-opt", col_depth)), val.clone());
                                div()
                                    .id(opt_id)
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(6.))
                                    .px(px(8.))
                                    .py(px(6.))
                                    .rounded(theme.radius_md)
                                    .text_size(px(13.))
                                    .font_weight(if is_active {
                                        FontWeight::MEDIUM
                                    } else {
                                        FontWeight::NORMAL
                                    })
                                    .bg(if is_active {
                                        theme.primary_soft
                                    } else {
                                        crate::transparent_black()
                                    })
                                    .text_color(if is_active {
                                        theme.primary_soft_foreground
                                    } else {
                                        theme.popover_foreground
                                    })
                                    .when(!disabled, |el| {
                                        el.cursor_pointer()
                                            .when(!is_active, |el| {
                                                el.hover(move |s| s.bg(hover_bg))
                                            })
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.click_option(
                                                    col_depth,
                                                    val.clone(),
                                                    !has_children,
                                                    cx,
                                                );
                                            }))
                                    })
                                    .when(disabled, |el| el.opacity(0.45).cursor_not_allowed())
                                    .child(div().truncate().child(lbl))
                                    .when(has_children, |el| {
                                        el.child(
                                            Icon::new(LucideIcon::ChevronRight)
                                                .size(IconSize::Xs)
                                                .color(theme.muted_foreground),
                                        )
                                    })
                            },
                        ))
                }));

            let anim = if reduce_motion {
                panel.into_any_element()
            } else {
                panel
                    .with_animation(
                        ("casc-anim", entity_id),
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

        let has_resolved = display_text.is_some();
        let mut trigger = div()
            .id(("casc-trigger", entity_id))
            .relative()
            .when(!self.disabled, |el| {
                el.track_focus(&self.focus_handle.clone().tab_stop(true))
                    .on_key_down(cx.listener(Self::on_key_down))
            })
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .w_full()
            .h(self.size.height())
            .px(self.size.px_x())
            .rounded(theme.radius_md)
            .border_1()
            .border_color(if (self.open || is_focused) && !self.disabled {
                theme.ring
            } else {
                theme.input_border
            })
            // Dashed outline for keyboard focus only (`focus` module docs).
            .when(focus_visible && !self.disabled, |el| {
                el.when_some(super::focus::focus_outline(&theme), |el, outline| {
                    el.outline(outline)
                })
            })
            .bg(if self.disabled {
                theme.disabled_bg
            } else {
                theme.input_bg
            })
            .text_size(self.size.font_size())
            .when(!self.disabled, |el| {
                el.cursor_pointer()
                    .hover(move |s| s.bg(hover_bg))
                    .on_click(cx.listener(|this, _: &crate::ClickEvent, window, cx| {
                        window.focus(&this.focus_handle, cx);
                        this.open = !this.open;
                        if this.open {
                            this.active_path = this.selected_path.clone();
                        }
                        cx.notify();
                    }))
            })
            .when(self.disabled, |el| el.opacity(0.55).cursor_not_allowed())
            .child(
                canvas(
                    move |bounds, _, _| bounds_cell.set(Some(bounds)),
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(if has_resolved {
                        theme.foreground
                    } else {
                        theme.placeholder
                    })
                    .child(
                        display_text
                            .map(SharedString::from)
                            .unwrap_or_else(|| self.placeholder.clone()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .flex_shrink_0()
                    .gap(px(4.))
                    .when(self.clearable && has_resolved && !self.disabled, |el| {
                        el.child(
                            div()
                                .id(("casc-clear", entity_id))
                                .p(px(2.))
                                .rounded(theme.radius_sm)
                                .hover(move |s| s.bg(hover_bg))
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.set_path(Vec::new(), cx);
                                    cx.stop_propagation();
                                }))
                                .child(
                                    Icon::new(LucideIcon::X)
                                        .size(IconSize::Xs)
                                        .color(theme.muted_foreground),
                                ),
                        )
                    })
                    .child(
                        Icon::new(LucideIcon::ChevronDown)
                            .size(IconSize::Sm)
                            .color(theme.muted_foreground),
                    ),
            );

        crate::Refineable::refine(trigger.style(), &self.frame_style);
        let has_explicit_width = self.frame_style.size.width.is_some();

        div()
            .flex()
            .flex_col()
            .when(!has_explicit_width, |el| el.w_full())
            .child(trigger)
            .when_some(popover, |el, pop| el.child(div().h_0().child(pop)))
    }
}

/// Renderable [`Cascader`] wrapping [`Entity<CascaderState>`].
#[derive(IntoElement)]
pub struct Cascader {
    state: Entity<CascaderState>,
    style: StyleRefinement,
}

/// Convenience constructor for [`Cascader::new`].
pub fn cascader(state: &Entity<CascaderState>) -> Cascader {
    Cascader::new(state)
}

impl Cascader {
    /// Creates a [`Cascader`] bound to `state`.
    pub fn new(state: &Entity<CascaderState>) -> Self {
        Self {
            state: state.clone(),
            style: StyleRefinement::default(),
        }
    }
}

impl Styled for Cascader {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Cascader {
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
