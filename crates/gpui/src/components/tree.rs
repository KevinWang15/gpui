//! Hierarchical [`Tree`] / [`TreeState`] view with expand/collapse, indentation guides,
//! custom icons, single selection, tri-state cascading checkboxes, live search filtering,
//! `uniform_list` virtualization, and full keyboard navigation (`Up`/`Down`/`Left`/`Right`/`Home`/`End`/`Enter`/`Space`).

use std::collections::HashSet;
use std::ops::Range;
use std::rc::Rc;

use crate::icons::{Icon, IconSize, LucideIcon};
use crate::{
    App, Bounds, Context, ElementId, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
    IntoElement, KeyDownEvent, Pixels, Render, RenderOnce, ScrollStrategy, SharedString,
    StyleRefinement, Styled, UniformListScrollHandle, Window, canvas, div, fill, point, prelude::*,
    px, size, uniform_list,
};

use super::badge::{BadgeColor, badge};
use super::checkbox::CheckState;
use super::input::{Input, InputEvent, InputState};
use super::theme::{ActiveTheme, ComponentSize, focus_ring_shadow};

const TREE_ROW_HEIGHT: f32 = 30.0;
const TREE_INDENT_STEP: f32 = 18.0;

/// Node inside a hierarchical [`Tree`].
#[derive(Clone, Debug)]
pub struct TreeNode {
    /// Unique identifier for this node.
    pub id: SharedString,
    /// Display label.
    pub label: SharedString,
    /// Optional custom [`LucideIcon`] (folders default to `Folder`/`FolderOpen`, leaves to `FileText`).
    pub icon: Option<LucideIcon>,
    /// Optional trailing badge text (e.g. item count or type tag).
    pub badge: Option<SharedString>,
    /// Disables selection and checkbox toggling for this node.
    pub disabled: bool,
    /// Nested child nodes.
    pub children: Vec<TreeNode>,
}

impl TreeNode {
    /// Creates a leaf [`TreeNode`].
    pub fn new(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: None,
            badge: None,
            disabled: false,
            children: Vec::new(),
        }
    }

    /// Sets a custom [`LucideIcon`].
    pub fn icon(mut self, icon: LucideIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Sets a trailing metadata badge.
    pub fn badge(mut self, text: impl Into<SharedString>) -> Self {
        self.badge = Some(text.into());
        self
    }

    /// Disables interaction for this node.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Attaches child nodes (making this node expandable).
    pub fn children(mut self, children: impl IntoIterator<Item = TreeNode>) -> Self {
        self.children = children.into_iter().collect();
        self
    }

    /// Appends a single child node.
    pub fn child(mut self, child: TreeNode) -> Self {
        self.children.push(child);
        self
    }

    /// Returns whether this node has nested children.
    pub fn is_branch(&self) -> bool {
        !self.children.is_empty()
    }
}

/// Events emitted by [`TreeState`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreeEvent {
    /// Emitted when the active selected node changes.
    Select(SharedString),
    /// Emitted when a branch node is expanded or collapsed.
    ToggleExpand {
        /// Identifier of the toggled node.
        id: SharedString,
        /// `true` when expanded, `false` when collapsed.
        expanded: bool,
    },
    /// Emitted when the set of checked leaf/node IDs changes (in checkable mode).
    CheckChange(Vec<SharedString>),
}

#[derive(Clone)]
struct FlatTreeRow {
    id: SharedString,
    label: SharedString,
    icon: Option<LucideIcon>,
    badge: Option<SharedString>,
    depth: usize,
    parent_id: Option<SharedString>,
    is_branch: bool,
    is_expanded: bool,
    disabled: bool,
    check_state: CheckState,
}

/// Entity state backing an interactive, virtualized [`Tree`].
pub struct TreeState {
    focus_handle: FocusHandle,
    nodes: Vec<TreeNode>,
    expanded: HashSet<SharedString>,
    collapsed_during_search: HashSet<SharedString>,
    selected: Option<SharedString>,
    checked: HashSet<SharedString>,
    checkable: bool,
    show_lines: bool,
    show_icons: bool,
    searchable: bool,
    max_height: Pixels,
    search_input: Entity<InputState>,
    scroll_handle: UniformListScrollHandle,
    cached_rows: Rc<Vec<FlatTreeRow>>,
    cached_query: SharedString,
    cache_dirty: bool,
    frame_style: StyleRefinement,
}

impl EventEmitter<TreeEvent> for TreeState {}

impl Focusable for TreeState {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl TreeState {
    /// Creates a new [`TreeState`] with the given root nodes.
    pub fn new(nodes: Vec<TreeNode>, cx: &mut Context<Self>) -> Self {
        let search_input = cx.new(|cx| {
            InputState::new(cx)
                .placeholder("Filter tree...")
                .leading_icon(LucideIcon::Search)
                .clearable(true)
                .size(ComponentSize::Sm)
        });
        cx.subscribe(&search_input, |this, _, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::Change(_)) {
                this.collapsed_during_search.clear();
                this.cache_dirty = true;
                cx.notify();
            }
        })
        .detach();

        Self {
            // The rows are one tab stop; the arrow keys move inside.
            focus_handle: cx.focus_handle().tab_stop(true),
            nodes,
            expanded: HashSet::new(),
            collapsed_during_search: HashSet::new(),
            selected: None,
            checked: HashSet::new(),
            checkable: false,
            show_lines: true,
            show_icons: true,
            searchable: false,
            max_height: px(420.),
            search_input,
            scroll_handle: UniformListScrollHandle::new(),
            cached_rows: Rc::new(Vec::new()),
            cached_query: SharedString::default(),
            cache_dirty: true,
            frame_style: StyleRefinement::default(),
        }
    }

    /// Enables tri-state cascading checkboxes beside every node.
    pub fn checkable(mut self, checkable: bool) -> Self {
        self.checkable = checkable;
        self.cache_dirty = true;
        self
    }

    /// Shows or hides vertical indentation guide lines (default `true`).
    pub fn show_lines(mut self, show: bool) -> Self {
        self.show_lines = show;
        self
    }

    /// Shows or hides default folder/file icons (default `true`).
    pub fn show_icons(mut self, show: bool) -> Self {
        self.show_icons = show;
        self
    }

    /// Shows an integrated live filter search bar above the tree.
    pub fn searchable(mut self, searchable: bool) -> Self {
        self.searchable = searchable;
        self
    }

    /// Sets the maximum scrollable height of the virtualized tree body (default `420px`).
    pub fn max_height(mut self, height: Pixels) -> Self {
        self.max_height = if height.0.is_finite() {
            height.max(px(TREE_ROW_HEIGHT))
        } else {
            px(420.)
        };
        self
    }

    /// Returns a reference to the embedded search [`InputState`].
    pub fn search_input(&self) -> &Entity<InputState> {
        &self.search_input
    }

    /// Clears the active search query without emitting redundant work.
    pub fn clear_search(&mut self, cx: &mut Context<Self>) {
        if !self.search_input.read(cx).value().is_empty() {
            self.search_input
                .update(cx, |inp, cx| inp.set_value_silent(String::new(), cx));
            self.collapsed_during_search.clear();
            self.cache_dirty = true;
            cx.notify();
        }
    }

    /// Sets the initially expanded branch node IDs.
    pub fn default_expanded(
        mut self,
        ids: impl IntoIterator<Item = impl Into<SharedString>>,
    ) -> Self {
        self.expanded = ids.into_iter().map(Into::into).collect();
        self.cache_dirty = true;
        self
    }

    /// Expands all branch nodes recursively.
    pub fn expand_all(&mut self, cx: &mut Context<Self>) {
        fn collect(nodes: &[TreeNode], out: &mut HashSet<SharedString>) {
            for n in nodes {
                if n.is_branch() {
                    out.insert(n.id.clone());
                    collect(&n.children, out);
                }
            }
        }
        collect(&self.nodes, &mut self.expanded);
        self.collapsed_during_search.clear();
        self.cache_dirty = true;
        cx.notify();
    }

    /// Collapses all branch nodes.
    pub fn collapse_all(&mut self, cx: &mut Context<Self>) {
        self.expanded.clear();
        self.cache_dirty = true;
        cx.notify();
    }

    /// Sets the initially selected node ID.
    pub fn default_selected(mut self, id: impl Into<SharedString>) -> Self {
        self.selected = Some(id.into());
        self
    }

    /// Sets initially checked node IDs (in checkable mode).
    ///
    /// Branch IDs are automatically normalized to their non-disabled leaf descendants so
    /// tri-state counts and [`Self::checked_ids`] stay strictly consistent.
    pub fn default_checked(
        mut self,
        ids: impl IntoIterator<Item = impl Into<SharedString>>,
    ) -> Self {
        let requested: HashSet<SharedString> = ids.into_iter().map(Into::into).collect();
        let mut normalized = HashSet::new();

        fn expand_checked(
            nodes: &[TreeNode],
            requested: &HashSet<SharedString>,
            ancestor_checked: bool,
            out: &mut HashSet<SharedString>,
        ) {
            for node in nodes {
                if node.disabled {
                    continue;
                }
                let matched = ancestor_checked || requested.contains(&node.id);
                if node.is_branch() {
                    let before = out.len();
                    expand_checked(&node.children, requested, matched, out);
                    if matched && out.len() == before && !has_enabled_leaf(node) {
                        out.insert(node.id.clone());
                    }
                } else if matched {
                    out.insert(node.id.clone());
                }
            }
        }

        expand_checked(&self.nodes, &requested, false, &mut normalized);
        self.checked = normalized;
        self.cache_dirty = true;
        self
    }

    /// Replaces the tree's node hierarchy and prunes stale expanded/selected/checked IDs.
    pub fn set_nodes(&mut self, nodes: Vec<TreeNode>, cx: &mut Context<Self>) {
        fn collect_all_ids(nodes: &[TreeNode], out: &mut HashSet<SharedString>) {
            for n in nodes {
                out.insert(n.id.clone());
                collect_all_ids(&n.children, out);
            }
        }
        let mut valid_ids = HashSet::new();
        collect_all_ids(&nodes, &mut valid_ids);
        self.nodes = nodes;
        self.expanded.retain(|id| valid_ids.contains(id));
        self.collapsed_during_search
            .retain(|id| valid_ids.contains(id));
        self.checked.retain(|id| valid_ids.contains(id));
        if let Some(ref sel) = self.selected
            && !valid_ids.contains(sel)
        {
            self.selected = None;
        }
        self.cache_dirty = true;
        cx.notify();
    }

    /// Returns the currently selected node ID, if any.
    pub fn selected(&self) -> Option<&SharedString> {
        self.selected.as_ref()
    }

    /// Returns all currently checked node IDs (sorted deterministically).
    pub fn checked_ids(&self) -> Vec<SharedString> {
        let mut ids: Vec<SharedString> = self.checked.iter().cloned().collect();
        ids.sort();
        ids
    }

    /// Returns whether `id` is currently expanded.
    pub fn is_expanded(&self, id: &str) -> bool {
        self.expanded.contains(id)
    }

    /// Programmatically selects a node and emits [`TreeEvent::Select`].
    pub fn select(&mut self, id: impl Into<SharedString>, cx: &mut Context<Self>) {
        let id = id.into();
        if self.selected.as_ref() != Some(&id) {
            self.selected = Some(id.clone());
            cx.emit(TreeEvent::Select(id));
            cx.notify();
        }
    }

    /// Programmatically updates the selected node without emitting [`TreeEvent::Select`].
    pub fn select_silent(&mut self, id: Option<SharedString>, cx: &mut Context<Self>) {
        if self.selected != id {
            self.selected = id;
            cx.notify();
        }
    }

    /// Clears the current selection.
    pub fn clear_selection(&mut self, cx: &mut Context<Self>) {
        if self.selected.take().is_some() {
            cx.notify();
        }
    }

    /// Toggles expansion for branch node `id` and emits [`TreeEvent::ToggleExpand`].
    pub fn toggle_expand(&mut self, id: impl Into<SharedString>, cx: &mut Context<Self>) {
        let id = id.into();
        let query_active = !self.search_input.read(cx).value().trim().is_empty();
        let expanded = if query_active {
            if self.collapsed_during_search.contains(&id) {
                self.collapsed_during_search.remove(&id);
                self.expanded.insert(id.clone());
                true
            } else {
                self.collapsed_during_search.insert(id.clone());
                self.expanded.remove(&id);
                false
            }
        } else if self.expanded.contains(&id) {
            self.expanded.remove(&id);
            false
        } else {
            self.expanded.insert(id.clone());
            true
        };
        self.cache_dirty = true;
        cx.emit(TreeEvent::ToggleExpand { id, expanded });
        cx.notify();
    }

    /// Computes tri-state [`CheckState`] for `node` by inspecting its non-disabled subtree.
    pub fn node_check_state(&self, node: &TreeNode) -> CheckState {
        if !node.is_branch() {
            return if self.checked.contains(&node.id) {
                CheckState::Checked
            } else {
                CheckState::Unchecked
            };
        }
        let (checked_count, total) = count_branch_leaves(node, &self.checked);
        check_state_from_counts(checked_count, total, self.checked.contains(&node.id))
    }

    /// Toggles check state for `id` (cascading cleanly to all non-disabled leaf descendants).
    pub fn toggle_check(&mut self, id: &str, cx: &mut Context<Self>) {
        fn find_node<'a>(nodes: &'a [TreeNode], id: &str) -> Option<&'a TreeNode> {
            for n in nodes {
                if n.id.as_ref() == id {
                    return Some(n);
                }
                if let Some(found) = find_node(&n.children, id) {
                    return Some(found);
                }
            }
            None
        }
        fn collect_leaves(node: &TreeNode, out: &mut Vec<SharedString>) {
            if node.disabled {
                return;
            }
            if !node.is_branch() {
                out.push(node.id.clone());
            } else {
                for ch in &node.children {
                    collect_leaves(ch, out);
                }
            }
        }

        let Some(node) = find_node(&self.nodes, id) else {
            return;
        };
        if node.disabled {
            return;
        }
        let next_checked = self.node_check_state(node) != CheckState::Checked;
        let mut targets = Vec::new();
        collect_leaves(node, &mut targets);
        if targets.is_empty() {
            targets.push(node.id.clone());
        }
        for leaf in targets {
            if next_checked {
                self.checked.insert(leaf);
            } else {
                self.checked.remove(&leaf);
            }
        }
        self.cache_dirty = true;
        let snapshot = self.checked_ids();
        cx.emit(TreeEvent::CheckChange(snapshot));
        cx.notify();
    }

    /// Ensures `self.cached_rows` is up to date using a single O(N) bottom-up + pre-order pass.
    fn ensure_cache(&mut self, query: &str) {
        if !self.cache_dirty && self.cached_query.as_ref() == query {
            return;
        }
        let q = query.trim().to_lowercase();
        let mut rows = Vec::new();

        #[allow(clippy::too_many_arguments)]
        fn visit(
            node: &TreeNode,
            depth: usize,
            parent_id: Option<&SharedString>,
            expanded: &HashSet<SharedString>,
            collapsed_during_search: &HashSet<SharedString>,
            checked: &HashSet<SharedString>,
            checkable: bool,
            q: &str,
            out: &mut Vec<FlatTreeRow>,
        ) -> (bool, usize, usize) {
            let self_matches = q.is_empty() || contains_lowercase(&node.label, q);
            let is_branch = node.is_branch();

            if !is_branch {
                let (chk, tot) = if checkable && !node.disabled {
                    (usize::from(checked.contains(&node.id)), 1)
                } else {
                    (0, 0)
                };
                if self_matches {
                    out.push(FlatTreeRow {
                        id: node.id.clone(),
                        label: node.label.clone(),
                        icon: node.icon,
                        badge: node.badge.clone(),
                        depth,
                        parent_id: parent_id.cloned(),
                        is_branch: false,
                        is_expanded: false,
                        disabled: node.disabled,
                        check_state: if checkable && checked.contains(&node.id) {
                            CheckState::Checked
                        } else {
                            CheckState::Unchecked
                        },
                    });
                }
                return (self_matches, chk, tot);
            }

            // Reserve a slot so pre-order layout is preserved while computing child matches
            // and tri-state check counts in a single pass.
            let slot_idx = out.len();
            let initially_expanded = if !q.is_empty() {
                !collapsed_during_search.contains(&node.id)
            } else {
                expanded.contains(&node.id)
            };

            out.push(FlatTreeRow {
                id: node.id.clone(),
                label: node.label.clone(),
                icon: node.icon,
                badge: node.badge.clone(),
                depth,
                parent_id: parent_id.cloned(),
                is_branch: true,
                is_expanded: initially_expanded,
                disabled: node.disabled,
                check_state: CheckState::Unchecked,
            });

            let mut any_child_matched = false;
            let mut checked_count = 0usize;
            let mut total_count = 0usize;

            if initially_expanded {
                for ch in &node.children {
                    let (cm, c_chk, c_tot) = visit(
                        ch,
                        depth + 1,
                        Some(&node.id),
                        expanded,
                        collapsed_during_search,
                        checked,
                        checkable,
                        q,
                        out,
                    );
                    any_child_matched |= cm;
                    if checkable && !node.disabled {
                        checked_count += c_chk;
                        total_count += c_tot;
                    }
                }
            } else {
                // Collapsed branch: only walk children if search is active or checkable counts needed.
                if !q.is_empty() && !self_matches {
                    any_child_matched = node.children.iter().any(|c| subtree_matches_fast(c, q));
                }
                if checkable && !node.disabled {
                    let (c_chk, c_tot) = count_branch_leaves(node, checked);
                    checked_count += c_chk;
                    total_count += c_tot;
                }
            }

            let matched = self_matches || any_child_matched;
            if !matched {
                out.truncate(slot_idx);
                return (false, checked_count, total_count);
            }

            // During active search, if a branch matches only itself and has no matching visible children,
            // reflect its visual expansion accurately.
            if !q.is_empty() && !any_child_matched {
                out[slot_idx].is_expanded = false;
            }
            if checkable {
                out[slot_idx].check_state = check_state_from_counts(
                    checked_count,
                    total_count,
                    checked.contains(&node.id),
                );
            }

            (true, checked_count, total_count)
        }

        for node in &self.nodes {
            visit(
                node,
                0,
                None,
                &self.expanded,
                &self.collapsed_during_search,
                &self.checked,
                self.checkable,
                &q,
                &mut rows,
            );
        }

        self.cached_rows = Rc::new(rows);
        self.cached_query = SharedString::from(query.to_owned());
        self.cache_dirty = false;
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        // Never hijack typing (e.g. Space / Arrow keys) when the embedded search input is focused.
        if self.searchable && self.search_input.read(cx).focus_handle(cx).is_focused(window) {
            return;
        }
        let mods = event.keystroke.modifiers;
        if mods.control || mods.platform || mods.alt {
            return;
        }

        let query = self.search_input.read(cx).value().to_string();
        self.ensure_cache(&query);
        let rows = Rc::clone(&self.cached_rows);
        if rows.is_empty() {
            return;
        }

        let current_idx = self
            .selected
            .as_ref()
            .and_then(|sel| rows.iter().position(|r| &r.id == sel));

        match event.keystroke.key.as_str() {
            "down" => {
                let start = match current_idx {
                    Some(i) => i + 1,
                    None => 0,
                };
                if let Some(next) = (start..rows.len()).find(|&idx| !rows[idx].disabled) {
                    self.select(rows[next].id.clone(), cx);
                    self.scroll_handle
                        .scroll_to_item(next, ScrollStrategy::Nearest);
                }
                cx.stop_propagation();
            }
            "up" => {
                let target = match current_idx {
                    Some(i) => (0..i).rev().find(|&idx| !rows[idx].disabled),
                    None => (0..rows.len()).rev().find(|&idx| !rows[idx].disabled),
                };
                if let Some(prev) = target {
                    self.select(rows[prev].id.clone(), cx);
                    self.scroll_handle
                        .scroll_to_item(prev, ScrollStrategy::Nearest);
                }
                cx.stop_propagation();
            }
            "home" => {
                if let Some(first) = (0..rows.len()).find(|&idx| !rows[idx].disabled) {
                    self.select(rows[first].id.clone(), cx);
                    self.scroll_handle
                        .scroll_to_item(first, ScrollStrategy::Nearest);
                }
                cx.stop_propagation();
            }
            "end" => {
                if let Some(last) = (0..rows.len()).rev().find(|&idx| !rows[idx].disabled) {
                    self.select(rows[last].id.clone(), cx);
                    self.scroll_handle
                        .scroll_to_item(last, ScrollStrategy::Nearest);
                }
                cx.stop_propagation();
            }
            "right" => {
                if let Some(i) = current_idx {
                    let row = &rows[i];
                    if row.is_branch && !row.disabled {
                        if !row.is_expanded {
                            self.toggle_expand(row.id.clone(), cx);
                        } else if let Some(child_idx) = ((i + 1)..rows.len())
                            .take_while(|&k| rows[k].depth > row.depth)
                            .find(|&k| !rows[k].disabled)
                        {
                            self.select(rows[child_idx].id.clone(), cx);
                            self.scroll_handle
                                .scroll_to_item(child_idx, ScrollStrategy::Nearest);
                        }
                    }
                    cx.stop_propagation();
                }
            }
            "left" => {
                if let Some(i) = current_idx {
                    let row = &rows[i];
                    if row.is_branch && row.is_expanded && !row.disabled {
                        self.toggle_expand(row.id.clone(), cx);
                    } else if let Some(ref parent_id) = row.parent_id
                        && let Some(parent_idx) =
                            rows.iter().position(|r| &r.id == parent_id && !r.disabled)
                    {
                        self.select(parent_id.clone(), cx);
                        self.scroll_handle
                            .scroll_to_item(parent_idx, ScrollStrategy::Nearest);
                    }
                    cx.stop_propagation();
                }
            }
            "enter" => {
                if let Some(i) = current_idx {
                    let row = &rows[i];
                    if row.is_branch && !row.disabled {
                        self.toggle_expand(row.id.clone(), cx);
                        cx.stop_propagation();
                    }
                }
            }
            "space" if self.checkable => {
                if let Some(i) = current_idx {
                    let row = &rows[i];
                    if !row.disabled {
                        let id = row.id.clone();
                        self.toggle_check(&id, cx);
                        cx.stop_propagation();
                    }
                }
            }
            _ => {}
        }
    }
}

fn has_enabled_leaf(node: &TreeNode) -> bool {
    if node.disabled {
        return false;
    }
    if !node.is_branch() {
        return true;
    }
    node.children.iter().any(has_enabled_leaf)
}

fn count_branch_leaves(node: &TreeNode, checked: &HashSet<SharedString>) -> (usize, usize) {
    let mut checked_count = 0usize;
    let mut total = 0usize;
    fn walk(
        node: &TreeNode,
        checked: &HashSet<SharedString>,
        total: &mut usize,
        checked_count: &mut usize,
    ) {
        if node.disabled {
            return;
        }
        if !node.is_branch() {
            *total += 1;
            if checked.contains(&node.id) {
                *checked_count += 1;
            }
        } else {
            for ch in &node.children {
                walk(ch, checked, total, checked_count);
            }
        }
    }
    walk(node, checked, &mut total, &mut checked_count);
    (checked_count, total)
}

fn check_state_from_counts(checked_count: usize, total: usize, self_checked: bool) -> CheckState {
    if total == 0 {
        if self_checked {
            CheckState::Checked
        } else {
            CheckState::Unchecked
        }
    } else if checked_count == 0 {
        CheckState::Unchecked
    } else if checked_count == total {
        CheckState::Checked
    } else {
        CheckState::Indeterminate
    }
}

/// Fast case-insensitive substring check (`needle` must already be lowercased).
fn contains_lowercase(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    if haystack.is_ascii() && needle.is_ascii() {
        let h = haystack.as_bytes();
        let n = needle.as_bytes();
        if n.len() > h.len() {
            return false;
        }
        return h
            .windows(n.len())
            .any(|w| w.iter().zip(n).all(|(&a, &b)| a.to_ascii_lowercase() == b));
    }
    haystack.to_lowercase().contains(needle)
}

fn subtree_matches_fast(node: &TreeNode, q: &str) -> bool {
    if contains_lowercase(&node.label, q) {
        return true;
    }
    node.children.iter().any(|c| subtree_matches_fast(c, q))
}

impl Render for TreeState {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let query = if self.searchable {
            self.search_input.read(cx).value().to_string()
        } else {
            String::new()
        };
        self.ensure_cache(&query);

        let rows = Rc::clone(&self.cached_rows);
        let row_count = rows.len();
        let entity = cx.entity();
        let entity_id = cx.entity_id();
        let selected = self.selected.clone();
        let checkable = self.checkable;
        let show_lines = self.show_lines;
        let show_icons = self.show_icons;
        // Ring for keyboard focus only (`focus` module docs).
        let is_focused = self.focus_handle.is_focus_visible(window);

        let list_body = if row_count == 0 {
            div()
                .flex()
                .items_center()
                .justify_center()
                .py(px(20.))
                .text_size(px(13.))
                .text_color(theme.muted_foreground)
                .child("No matching tree items.")
                .into_any_element()
        } else {
            let list_h = px((row_count as f32 * TREE_ROW_HEIGHT).min(self.max_height.0));
            uniform_list(
                ("tree-vlist", entity_id),
                row_count,
                move |range: Range<usize>, _window: &mut Window, cx: &mut App| {
                    let theme = cx.theme();
                    let hover_bg = theme.surface_hover;
                    let guide_color = theme.border.opacity(0.75);

                    range
                        .map(|idx| {
                            let r = &rows[idx];
                            let is_selected = selected.as_ref() == Some(&r.id);
                            let id_for_select = r.id.clone();
                            let id_for_toggle = r.id.clone();
                            let id_for_check = r.id.clone();
                            let ent_select = entity.clone();
                            let ent_toggle = entity.clone();
                            let ent_check = entity.clone();
                            let disabled = r.disabled;
                            let is_branch = r.is_branch;
                            let is_expanded = r.is_expanded;
                            let depth = r.depth;
                            let check_state = r.check_state;

                            let node_icon = r.icon.or_else(|| {
                                show_icons.then_some(if is_branch {
                                    if is_expanded {
                                        LucideIcon::FolderOpen
                                    } else {
                                        LucideIcon::Folder
                                    }
                                } else {
                                    LucideIcon::FileText
                                })
                            });

                            let row_bg = if is_selected {
                                theme.primary_soft
                            } else {
                                crate::transparent_black()
                            };
                            let fg = if is_selected {
                                theme.primary_soft_foreground
                            } else {
                                theme.foreground
                            };
                            let icon_fg = if is_selected {
                                theme.primary_soft_foreground
                            } else if is_branch {
                                theme.primary.opacity(0.85)
                            } else {
                                theme.muted_foreground
                            };

                            let left_pad = 6.0 + depth as f32 * TREE_INDENT_STEP;

                            let mut row = div()
                                .id((ElementId::from("tree-row"), r.id.clone()))
                                .relative()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(4.))
                                .w_full()
                                .h(px(TREE_ROW_HEIGHT))
                                .pr(px(8.))
                                .pl(px(left_pad))
                                .rounded(theme.radius_md)
                                .bg(row_bg)
                                .text_size(px(13.))
                                .text_color(fg)
                                .when(!disabled, |el| {
                                    el.cursor_pointer()
                                        .when(!is_selected, |el| {
                                            el.hover(move |s| s.bg(hover_bg))
                                        })
                                        .on_click(move |_, window, cx| {
                                            ent_select.update(cx, |this, cx| {
                                                window.focus(&this.focus_handle, cx);
                                                this.select(id_for_select.clone(), cx);
                                            });
                                        })
                                })
                                .when(disabled, |el| el.opacity(0.45).cursor_not_allowed());

                            // Paint all vertical indentation guide lines in a single zero-div canvas.
                            if show_lines && depth > 0 {
                                row = row.child(
                                    canvas(
                                        |_, _, _| {},
                                        move |bounds, _, window, _| {
                                            for d in 0..depth {
                                                let gx = bounds.origin.x
                                                    + px(6.0
                                                        + d as f32 * TREE_INDENT_STEP
                                                        + TREE_INDENT_STEP * 0.5);
                                                window.paint_quad(fill(
                                                    Bounds::new(
                                                        point(gx, bounds.origin.y),
                                                        size(px(1.), bounds.size.height),
                                                    ),
                                                    guide_color,
                                                ));
                                            }
                                        },
                                    )
                                    .absolute()
                                    .inset_0(),
                                );
                            }

                            // Expand / collapse chevron
                            row = row.child(
                                div()
                                    .id((ElementId::from("tree-exp"), r.id.clone()))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .flex_shrink_0()
                                    .w(px(20.))
                                    .h(px(20.))
                                    .rounded(theme.radius_sm)
                                    .when(is_branch, |el| {
                                        el.when(!disabled, |el| {
                                            el.cursor_pointer()
                                                .hover(move |s| s.bg(hover_bg))
                                                .on_mouse_down(
                                                    crate::MouseButton::Left,
                                                    |_, _, cx| cx.stop_propagation(),
                                                )
                                                .on_click(move |_, window, cx| {
                                                    ent_toggle.update(cx, |this, cx| {
                                                        window.focus(&this.focus_handle, cx);
                                                        this.toggle_expand(
                                                            id_for_toggle.clone(),
                                                            cx,
                                                        );
                                                    });
                                                    cx.stop_propagation();
                                                })
                                        })
                                        .child(
                                            Icon::new(if is_expanded {
                                                LucideIcon::ChevronDown
                                            } else {
                                                LucideIcon::ChevronRight
                                            })
                                            .size(IconSize::Xs)
                                            .color(theme.muted_foreground),
                                        )
                                    }),
                            );

                            // Optional tri-state checkbox
                            if checkable {
                                let is_on = check_state != CheckState::Unchecked;
                                row = row.child(
                                    div()
                                        .id((ElementId::from("tree-chk"), r.id.clone()))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .flex_shrink_0()
                                        .w(px(16.))
                                        .h(px(16.))
                                        .mr(px(4.))
                                        .rounded(px(4.))
                                        .border_1()
                                        .border_color(if is_on {
                                            theme.primary
                                        } else {
                                            theme.input_border
                                        })
                                        .bg(if is_on {
                                            theme.primary
                                        } else {
                                            theme.surface
                                        })
                                        .when(!disabled, |el| {
                                            el.cursor_pointer()
                                                .on_mouse_down(
                                                    crate::MouseButton::Left,
                                                    |_, _, cx| cx.stop_propagation(),
                                                )
                                                .on_click(move |_, window, cx| {
                                                    ent_check.update(cx, |this, cx| {
                                                        window.focus(&this.focus_handle, cx);
                                                        this.toggle_check(&id_for_check, cx);
                                                    });
                                                    cx.stop_propagation();
                                                })
                                        })
                                        .when(check_state == CheckState::Checked, |el| {
                                            el.child(
                                                Icon::new(LucideIcon::Check)
                                                    .size(IconSize::Xs)
                                                    .color(theme.primary_foreground),
                                            )
                                        })
                                        .when(check_state == CheckState::Indeterminate, |el| {
                                            el.child(
                                                Icon::new(LucideIcon::Minus)
                                                    .size(IconSize::Xs)
                                                    .color(theme.primary_foreground),
                                            )
                                        }),
                                );
                            }

                            // Node icon
                            if let Some(ic) = node_icon {
                                row = row.child(
                                    Icon::new(ic).size(IconSize::Sm).color(icon_fg),
                                );
                            }

                            // Label
                            row = row.child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .font_weight(if is_selected {
                                        FontWeight::MEDIUM
                                    } else {
                                        FontWeight::NORMAL
                                    })
                                    .child(r.label.clone()),
                            );

                            // Optional trailing badge
                            if let Some(ref b) = r.badge {
                                row = row.child(
                                    badge(b.clone()).color(BadgeColor::Gray).pill(true),
                                );
                            }

                            row
                        })
                        .collect()
                },
            )
            .track_scroll(&self.scroll_handle)
            .w_full()
            .h(list_h)
            .into_any_element()
        };

        let list_container = div()
            .id(("tree-list-focus", entity_id))
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key_down))
            .w_full()
            .rounded(theme.radius_md)
            .when(is_focused, |el| el.shadow(focus_ring_shadow(&theme)))
            .child(list_body);

        let mut root = div()
            .id(("tree-root", entity_id))
            .flex()
            .flex_col()
            .gap(px(8.))
            .w_full()
            .when(self.searchable, |el| {
                el.child(Input::new(&self.search_input))
            })
            .child(list_container);

        crate::Refineable::refine(root.style(), &self.frame_style);
        root
    }
}

/// Renderable hierarchical [`Tree`] element bound to an [`Entity<TreeState>`].
#[derive(IntoElement)]
pub struct Tree {
    state: Entity<TreeState>,
    style: StyleRefinement,
}

/// Convenience constructor for [`Tree::new`].
pub fn tree(state: &Entity<TreeState>) -> Tree {
    Tree::new(state)
}

impl Tree {
    /// Creates a [`Tree`] bound to `state`.
    pub fn new(state: &Entity<TreeState>) -> Self {
        Self {
            state: state.clone(),
            style: StyleRefinement::default(),
        }
    }
}

impl Styled for Tree {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Tree {
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
