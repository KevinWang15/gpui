//! Keyboard-first Spotlight / `⌘K` [`CommandPalette`] with cached fuzzy search, grouped
//! virtualized commands, [`kbd`] shortcut badges, synchronized keyboard navigation
//! (`Up`/`Down`/`Enter`/`Escape`), focus trapping & restoration, and GPU-blurred modal backdrop.

use std::ops::Range;
use std::rc::Rc;

use crate::icons::{Icon, IconSize, LucideIcon};
use crate::{
    Animation, AnimationExt, App, Context, Entity, EventEmitter, FocusHandle, Focusable,
    FontWeight, IntoElement, KeyDownEvent, MouseButton, Pixels, Render, RenderOnce, ScrollStrategy,
    SharedString, StyleRefinement, Styled, UniformListScrollHandle, Window, deferred, div,
    ease_out_quint, prelude::*, px, uniform_list,
};

use super::completion::{fuzzy_match_detailed, fuzzy_match_score_lower};
use super::field::kbd;
use super::focus::{is_list_nav_key, list_step, step_enabled};
use super::input::{Input, InputEvent, InputState};
use super::theme::{ANIM_FAST, ActiveTheme, ComponentSize};

const CMD_ROW_HEIGHT: f32 = 44.0;

/// Secondary action available in the [`CommandPalette`] action panel (`Tab` / `Ctrl+K`) for a
/// selected [`CommandItem`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandAction {
    /// Action identifier emitted in [`CommandPaletteEvent::ExecuteAction`].
    pub id: SharedString,
    /// Human-readable label (e.g. `"Reveal in Explorer"`, `"Copy Path"`).
    pub label: SharedString,
    /// Optional leading [`LucideIcon`].
    pub icon: Option<LucideIcon>,
    /// Optional shortcut keycaps shown on the right.
    pub shortcut: Vec<SharedString>,
}

impl CommandAction {
    /// Creates a new [`CommandAction`].
    pub fn new(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: None,
            shortcut: Vec::new(),
        }
    }

    /// Sets an optional leading icon.
    pub fn icon(mut self, icon: LucideIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Sets shortcut hint keycaps.
    pub fn shortcut(mut self, keys: &[&str]) -> Self {
        self.shortcut = keys.iter().map(|k| SharedString::from(*k)).collect();
        self
    }
}

/// Single actionable command listed inside a [`CommandPalette`].
#[derive(Clone)]
pub struct CommandItem {
    /// Unique identifier emitted via [`CommandPaletteEvent::Execute`].
    pub id: SharedString,
    /// Primary display title.
    pub title: SharedString,
    /// Optional muted subtitle or description.
    pub subtitle: Option<SharedString>,
    /// Optional section group header (e.g. `"Navigation"`, `"Theme"`, `"Actions"`).
    pub group: SharedString,
    /// Optional leading [`LucideIcon`].
    pub icon: Option<LucideIcon>,
    /// Optional keyboard shortcut hint badges (e.g. `["⌘", "K"]` or `["G", "A"]`).
    pub shortcut: Vec<SharedString>,
    /// Extra search keywords matched by fuzzy filtering (not rendered visibly).
    pub keywords: SharedString,
    /// Optional secondary actions shown in the `Tab` / `Ctrl+K` action panel.
    pub actions: Vec<CommandAction>,
    /// Disables selection and execution.
    pub disabled: bool,
}

impl CommandItem {
    /// Creates a new command item under the `"General"` group.
    pub fn new(id: impl Into<SharedString>, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            subtitle: None,
            group: "General".into(),
            icon: None,
            shortcut: Vec::new(),
            keywords: "".into(),
            actions: Vec::new(),
            disabled: false,
        }
    }

    /// Sets the command's section group header.
    pub fn group(mut self, group: impl Into<SharedString>) -> Self {
        self.group = group.into();
        self
    }

    /// Sets a muted secondary subtitle.
    pub fn subtitle(mut self, subtitle: impl Into<SharedString>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// Sets a leading [`LucideIcon`].
    pub fn icon(mut self, icon: LucideIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Sets keyboard shortcut keycap strings displayed on the right side (e.g. `&["⌘", "D"]`).
    pub fn shortcut(mut self, keys: &[&str]) -> Self {
        self.shortcut = keys.iter().map(|k| SharedString::from(*k)).collect();
        self
    }

    /// Adds extra space-separated search keywords for fuzzy matching.
    pub fn keywords(mut self, keywords: impl Into<SharedString>) -> Self {
        self.keywords = keywords.into();
        self
    }

    /// Appends a single secondary action revealed via `Tab` / `Ctrl+K` in the action panel.
    pub fn action(mut self, action: CommandAction) -> Self {
        self.actions.push(action);
        self
    }

    /// Attaches secondary actions revealed via `Tab` / `Ctrl+K` in the action panel.
    pub fn actions(mut self, actions: impl IntoIterator<Item = CommandAction>) -> Self {
        self.actions = actions.into_iter().collect();
        self
    }

    /// Marks this command disabled.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

struct IndexedCommand {
    item: CommandItem,
    subtitle_low: Option<String>,
    keywords_low: String,
    group_low: String,
}

impl IndexedCommand {
    fn new(item: CommandItem) -> Self {
        let subtitle_low = item.subtitle.as_deref().map(str::to_lowercase);
        let keywords_low = item.keywords.to_lowercase();
        let group_low = item.group.to_lowercase();
        Self {
            item,
            subtitle_low,
            keywords_low,
            group_low,
        }
    }
}

#[derive(Clone)]
enum PaletteRow {
    GroupHeader(SharedString),
    Item { visual_idx: usize, item_idx: usize },
}

/// Events emitted by [`CommandPaletteState`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommandPaletteEvent {
    /// Emitted when the user executes a command via `Enter`, `Ctrl/Cmd 1..9`, or mouse click.
    Execute(SharedString),
    /// Emitted when the user invokes a secondary [`CommandAction`] from the `Tab` / `Ctrl+K` action panel.
    ExecuteAction {
        /// Parent command ID.
        command_id: SharedString,
        /// Secondary action ID.
        action_id: SharedString,
    },
    /// Emitted when the palette opens.
    Open,
    /// Emitted when the palette closes.
    Close,
}

/// Entity state backing a [`CommandPalette`].
pub struct CommandPaletteState {
    focus_handle: FocusHandle,
    previous_focus: Option<FocusHandle>,
    search_input: Entity<InputState>,
    scroll_handle: UniformListScrollHandle,
    open: bool,
    standalone: bool,
    quick_slots: bool,
    action_panel_open: bool,
    action_selected_idx: usize,
    items: Rc<Vec<IndexedCommand>>,
    selected_idx: usize,
    cached_query: SharedString,
    cache_dirty: bool,
    /// Visual top-to-bottom order of matching command indices (strictly matching rendered groups!).
    cached_visual_items: Vec<usize>,
    /// Mapping `visual_idx -> uniform_list row_idx` for exact `scroll_to_item`.
    cached_row_indices: Vec<usize>,
    /// Flattened group headers + command rows for O(visible) `uniform_list` rendering.
    cached_rows: Rc<Vec<PaletteRow>>,
    empty_message: SharedString,
    backdrop_blur: Pixels,
    max_width: Pixels,
    panel_style: StyleRefinement,
}

impl EventEmitter<CommandPaletteEvent> for CommandPaletteState {}

impl Focusable for CommandPaletteState {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// Scores a fuzzy subsequence / substring match. Returns `None` if `query` does not match `text`.
#[cfg(test)]
pub(crate) fn fuzzy_match_score(query: &str, text: &str) -> Option<i32> {
    super::completion::fuzzy_match_score(query, text)
}

impl CommandPaletteState {
    /// Creates a closed [`CommandPaletteState`] with the given command items.
    pub fn new(items: Vec<CommandItem>, cx: &mut Context<Self>) -> Self {
        let search_input = cx.new(|cx| {
            InputState::new(cx)
                .placeholder("Type a command or search...")
                .leading_icon(LucideIcon::Search)
                .clearable(true)
                .size(ComponentSize::Lg)
        });
        cx.subscribe(&search_input, |this, _, ev: &InputEvent, cx| match ev {
            InputEvent::CommittedChange(_) => {
                this.action_panel_open = false;
                this.cache_dirty = true;
                let query = this.search_input.read(cx).committed_value();
                this.ensure_cache(&query);
                this.selected_idx = this.first_enabled_visual_idx();
                this.scroll_handle.scroll_to_item(0, ScrollStrategy::Top);
                cx.notify();
            }
            InputEvent::Submit(_) => {
                if this.action_panel_open {
                    this.execute_selected_action(None, cx);
                } else {
                    this.execute_selected(cx);
                }
            }
            _ => {}
        })
        .detach();

        let indexed: Vec<IndexedCommand> = items.into_iter().map(IndexedCommand::new).collect();

        Self {
            focus_handle: cx.focus_handle(),
            previous_focus: None,
            search_input,
            scroll_handle: UniformListScrollHandle::new(),
            open: false,
            standalone: false,
            quick_slots: true,
            action_panel_open: false,
            action_selected_idx: 0,
            items: Rc::new(indexed),
            selected_idx: 0,
            cached_query: SharedString::default(),
            cache_dirty: true,
            cached_visual_items: Vec::new(),
            cached_row_indices: Vec::new(),
            cached_rows: Rc::new(Vec::new()),
            empty_message: "No matching commands found.".into(),
            backdrop_blur: px(8.),
            max_width: px(580.),
            panel_style: StyleRefinement::default(),
        }
    }

    /// Configures standalone Spotlight / Alfred window mode (`true` renders the palette directly
    /// without a fullscreen modal backdrop scrim and keeps it open by default).
    pub fn standalone(mut self, standalone: bool) -> Self {
        self.standalone = standalone;
        if standalone {
            self.open = true;
        }
        self
    }

    /// Enables or disables quick-run slot shortcuts (`Ctrl 1..9` / `⌘1..9`) and active-row `↵`
    /// hints (enabled by default).
    pub fn quick_slots(mut self, enabled: bool) -> Self {
        self.quick_slots = enabled;
        self
    }

    /// Focuses the embedded search input (useful when showing a pre-created Spotlight popup window).
    pub fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.action_panel_open = false;
        self.search_input
            .update(cx, |inp, cx| inp.set_value_silent(String::new(), cx));
        self.cache_dirty = true;
        self.ensure_cache("");
        self.selected_idx = self.first_enabled_visual_idx();
        self.scroll_handle.scroll_to_item(0, ScrollStrategy::Top);
        let input_focus = self.search_input.read(cx).focus_handle(cx);
        window.focus(&input_focus, cx);
        cx.notify();
    }

    /// Sets the search input placeholder.
    pub fn placeholder(self, text: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        let s: SharedString = text.into();
        self.search_input.update(cx, |inp, cx| {
            inp.set_placeholder(s, cx);
        });
        self
    }

    /// Sets the empty search state message.
    pub fn empty_message(mut self, text: impl Into<SharedString>) -> Self {
        self.empty_message = text.into();
        self
    }

    /// Sets the GPU backdrop blur radius (default `8px`).
    pub fn backdrop_blur(mut self, radius: impl Into<Pixels>) -> Self {
        let r: Pixels = radius.into();
        self.backdrop_blur = if r.0.is_finite() { r.max(px(0.)) } else { px(8.) };
        self
    }

    /// Sets the maximum palette dialog width (default `580px`).
    pub fn max_width(mut self, width: Pixels) -> Self {
        self.max_width = if width.0.is_finite() {
            width.max(px(260.))
        } else {
            px(580.)
        };
        self
    }

    /// Replaces the command catalog dynamically.
    pub fn set_items(&mut self, items: Vec<CommandItem>, cx: &mut Context<Self>) {
        self.items = Rc::new(items.into_iter().map(IndexedCommand::new).collect());
        self.cache_dirty = true;
        let query = self.search_input.read(cx).value().to_string();
        self.ensure_cache(&query);
        self.selected_idx = self.first_enabled_visual_idx();
        cx.notify();
    }

    /// Returns whether the palette is currently open.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Opens the command palette, clears any prior query, and focuses the search input.
    pub fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open {
            self.previous_focus = window.focused(cx);
            self.open = true;
            self.search_input
                .update(cx, |inp, cx| inp.set_value_silent(String::new(), cx));
            self.cache_dirty = true;
            self.ensure_cache("");
            self.selected_idx = self.first_enabled_visual_idx();
            self.scroll_handle.scroll_to_item(0, ScrollStrategy::Top);
            let input_focus = self.search_input.read(cx).focus_handle(cx);
            window.focus(&input_focus, cx);
            cx.emit(CommandPaletteEvent::Open);
            cx.notify();
        }
    }

    /// Closes the command palette.
    pub fn close(&mut self, cx: &mut Context<Self>) {
        if self.open {
            self.open = false;
            self.previous_focus = None;
            cx.emit(CommandPaletteEvent::Close);
            cx.notify();
        }
    }

    /// Closes the command palette and restores previous window focus.
    pub fn close_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open {
            self.open = false;
            if let Some(prev) = self.previous_focus.take() {
                window.focus(&prev, cx);
            } else {
                window.blur(cx);
            }
            cx.emit(CommandPaletteEvent::Close);
            cx.notify();
        }
    }

    /// Toggles the command palette open or closed.
    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open {
            self.close_in(window, cx);
        } else {
            self.open(window, cx);
        }
    }

    fn first_enabled_visual_idx(&self) -> usize {
        self.cached_visual_items
            .iter()
            .position(|&item_idx| !self.items[item_idx].item.disabled)
            .unwrap_or(0)
    }

    fn compute_grouped_for_query(&self, query: &str) -> (Vec<usize>, Vec<usize>, Vec<PaletteRow>) {
        let q = query.trim().to_lowercase();
        let scored_indices: Vec<usize> = if q.is_empty() {
            (0..self.items.len()).collect()
        } else {
            let mut scored: Vec<(i32, usize)> = self
                .items
                .iter()
                .enumerate()
                .filter_map(|(idx, indexed)| {
                    let title_score = fuzzy_match_detailed(query, indexed.item.title.as_ref())
                        .map(|m| m.score);
                    let sub_score = indexed
                        .subtitle_low
                        .as_deref()
                        .and_then(|s| fuzzy_match_score_lower(&q, s))
                        .map(|s| s - 15);
                    let kw_score = (!indexed.keywords_low.is_empty())
                        .then(|| fuzzy_match_score_lower(&q, &indexed.keywords_low))
                        .flatten()
                        .map(|s| s - 20);
                    let grp_score =
                        fuzzy_match_score_lower(&q, &indexed.group_low).map(|s| s - 25);
                    let best = [title_score, sub_score, kw_score, grp_score]
                        .into_iter()
                        .flatten()
                        .max()?;
                    Some((best, idx))
                })
                .collect();
            scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
            scored.into_iter().map(|(_, idx)| idx).collect()
        };

        // Group items in exact top-to-bottom visual order so Up/Down ALWAYS matches what's on screen!
        let mut group_map: std::collections::HashMap<SharedString, usize> =
            std::collections::HashMap::new();
        let mut grouped: Vec<(SharedString, Vec<usize>)> = Vec::new();
        for item_idx in scored_indices {
            let g = self.items[item_idx].item.group.clone();
            if let Some(&pos) = group_map.get(&g) {
                grouped[pos].1.push(item_idx);
            } else {
                let pos = grouped.len();
                group_map.insert(g.clone(), pos);
                grouped.push((g, vec![item_idx]));
            }
        }

        let mut visual_items = Vec::new();
        let mut row_indices = Vec::new();
        let mut rows = Vec::new();
        for (group_name, item_idxs) in grouped {
            rows.push(PaletteRow::GroupHeader(group_name));
            for item_idx in item_idxs {
                let visual_idx = visual_items.len();
                let row_idx = rows.len();
                visual_items.push(item_idx);
                row_indices.push(row_idx);
                rows.push(PaletteRow::Item {
                    visual_idx,
                    item_idx,
                });
            }
        }
        (visual_items, row_indices, rows)
    }

    fn ensure_cache(&mut self, query: &str) {
        if !self.cache_dirty && self.cached_query.as_ref() == query {
            return;
        }
        let (visual_items, row_indices, rows) = self.compute_grouped_for_query(query);
        self.cached_visual_items = visual_items;
        self.cached_row_indices = row_indices;
        self.cached_rows = Rc::new(rows);
        self.cached_query = SharedString::from(query.to_owned());
        self.cache_dirty = false;
    }

    /// Returns the currently filtered command indices in exact visual display order.
    pub fn filtered_indices(&self, cx: &App) -> Vec<usize> {
        let query = self.search_input.read(cx).value();
        if !self.cache_dirty && self.cached_query.as_ref() == query {
            return self.cached_visual_items.clone();
        }
        self.compute_grouped_for_query(query).0
    }

    fn enabled_visual_flags(&self) -> Vec<bool> {
        self.cached_visual_items
            .iter()
            .map(|&item_idx| !self.items[item_idx].item.disabled)
            .collect()
    }

    fn apply_visual_selection(&mut self, next: Option<usize>, cx: &mut Context<Self>) {
        if let Some(next) = next {
            self.selected_idx = next;
            if let Some(&row_idx) = self.cached_row_indices.get(next) {
                self.scroll_handle
                    .scroll_to_item(row_idx, ScrollStrategy::Nearest);
            }
            cx.notify();
        }
    }

    fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let query = self.search_input.read(cx).value().to_string();
        self.ensure_cache(&query);
        if self.cached_visual_items.is_empty() {
            self.selected_idx = 0;
            return;
        }
        let enabled = self.enabled_visual_flags();
        let next = step_enabled(Some(self.selected_idx), delta, &enabled);
        self.apply_visual_selection(next, cx);
    }

    fn step_selection_key(&mut self, key: &str, cx: &mut Context<Self>) {
        let query = self.search_input.read(cx).value().to_string();
        self.ensure_cache(&query);
        if self.cached_visual_items.is_empty() {
            self.selected_idx = 0;
            return;
        }
        let enabled = self.enabled_visual_flags();
        let next = list_step(key, Some(self.selected_idx), &enabled, 6, false);
        self.apply_visual_selection(next, cx);
    }

    fn execute_at(&mut self, visual_pos: usize, window: Option<&mut Window>, cx: &mut Context<Self>) {
        let query = self.search_input.read(cx).value().to_string();
        self.ensure_cache(&query);
        let Some(&item_idx) = self.cached_visual_items.get(visual_pos) else {
            return;
        };
        let item = &self.items[item_idx].item;
        if item.disabled {
            return;
        }
        let id = item.id.clone();
        self.action_panel_open = false;
        if !self.standalone {
            self.open = false;
            if let Some(window) = window {
                if let Some(prev) = self.previous_focus.take() {
                    window.focus(&prev, cx);
                } else {
                    window.blur(cx);
                }
            } else {
                self.previous_focus = None;
            }
        }
        cx.emit(CommandPaletteEvent::Execute(id));
        if !self.standalone {
            cx.emit(CommandPaletteEvent::Close);
        }
        cx.notify();
    }

    fn execute_selected(&mut self, cx: &mut Context<Self>) {
        self.execute_at(self.selected_idx, None, cx);
    }

    fn selected_command_item(&self) -> Option<&CommandItem> {
        let &item_idx = self.cached_visual_items.get(self.selected_idx)?;
        Some(&self.items[item_idx].item)
    }

    fn execute_selected_action(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
        let Some(item) = self.selected_command_item() else {
            self.action_panel_open = false;
            return;
        };
        let Some(action) = item.actions.get(self.action_selected_idx) else {
            self.action_panel_open = false;
            return;
        };
        let command_id = item.id.clone();
        let action_id = action.id.clone();
        self.action_panel_open = false;
        if !self.standalone {
            self.open = false;
            if let Some(window) = window {
                if let Some(prev) = self.previous_focus.take() {
                    window.focus(&prev, cx);
                } else {
                    window.blur(cx);
                }
            } else {
                self.previous_focus = None;
            }
        }
        cx.emit(CommandPaletteEvent::ExecuteAction {
            command_id,
            action_id,
        });
        if !self.standalone {
            cx.emit(CommandPaletteEvent::Close);
        }
        cx.notify();
    }

    fn on_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.open {
            return;
        }
        let key = event.keystroke.key.as_str();
        let mods = &event.keystroke.modifiers;
        let primary_mod = if cfg!(target_os = "macos") {
            mods.platform && !mods.control
        } else {
            mods.control && !mods.platform
        };

        // Secondary action panel keyboard handling when open.
        if self.action_panel_open {
            let action_count = self
                .selected_command_item()
                .map_or(0, |it| it.actions.len());
            match key {
                "escape" | "tab" => {
                    self.action_panel_open = false;
                    cx.stop_propagation();
                    cx.notify();
                    return;
                }
                "up" if action_count > 0 => {
                    self.action_selected_idx = if self.action_selected_idx == 0 {
                        action_count - 1
                    } else {
                        self.action_selected_idx - 1
                    };
                    cx.stop_propagation();
                    cx.notify();
                    return;
                }
                "down" if action_count > 0 => {
                    self.action_selected_idx = (self.action_selected_idx + 1) % action_count;
                    cx.stop_propagation();
                    cx.notify();
                    return;
                }
                "enter" => {
                    self.execute_selected_action(Some(window), cx);
                    cx.stop_propagation();
                    return;
                }
                _ => {}
            }
        }

        // Ctrl+K / Cmd+K toggles the secondary action panel if the selected item has actions.
        if primary_mod && !mods.alt && !mods.shift && key == "k" {
            let has_actions = self
                .selected_command_item()
                .is_some_and(|it| !it.actions.is_empty());
            if has_actions {
                self.action_panel_open = !self.action_panel_open;
                self.action_selected_idx = 0;
                cx.stop_propagation();
                cx.notify();
                return;
            }
        }

        // Quick-run slot shortcuts: Ctrl 1..9 (Windows/Linux) or Cmd 1..9 (macOS).
        if self.quick_slots && primary_mod && !mods.alt && !mods.shift && key.len() == 1 {
            let b = key.as_bytes()[0];
            if (b'1'..=b'9').contains(&b) {
                let slot = (b - b'1') as usize;
                if slot < self.cached_visual_items.len() {
                    self.execute_at(slot, Some(window), cx);
                    cx.stop_propagation();
                    return;
                }
            }
        }

        if !mods.modified() && is_list_nav_key(key, false) {
            self.step_selection_key(key, cx);
            cx.stop_propagation();
            return;
        }
        match key {
            "escape" => {
                if self.standalone {
                    cx.emit(CommandPaletteEvent::Close);
                } else {
                    self.close_in(window, cx);
                }
                cx.stop_propagation();
            }
            "enter" => {
                self.execute_at(self.selected_idx, Some(window), cx);
                cx.stop_propagation();
            }
            "tab" => {
                let has_actions = !mods.shift
                    && self
                        .selected_command_item()
                        .is_some_and(|it| !it.actions.is_empty());
                if has_actions {
                    self.action_panel_open = true;
                    self.action_selected_idx = 0;
                    cx.notify();
                } else {
                    let delta = if mods.shift { -1 } else { 1 };
                    self.move_selection(delta, cx);
                }
                cx.stop_propagation();
            }
            _ => {}
        }
    }
}

impl Render for CommandPaletteState {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.open {
            return div().into_any_element();
        }

        let theme = cx.theme();
        let reduce_motion = cx.reduce_motion();
        let entity = cx.entity();
        let entity_id = cx.entity_id();
        let query = self.search_input.read(cx).value().to_string();
        self.ensure_cache(&query);

        let result_count = self.cached_visual_items.len();
        if result_count > 0 && self.selected_idx >= result_count {
            self.selected_idx = self.first_enabled_visual_idx();
        }

        let rows_rc = Rc::clone(&self.cached_rows);
        let items_rc = Rc::clone(&self.items);
        let row_count = rows_rc.len();
        let selected_idx = self.selected_idx;
        let quick_slots = self.quick_slots;
        let selected_has_actions = self
            .selected_command_item()
            .is_some_and(|it| !it.actions.is_empty());
        let action_panel_items: Vec<CommandAction> = if self.action_panel_open {
            self.selected_command_item()
                .map(|it| it.actions.clone())
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let action_selected_idx = self.action_selected_idx;

        let list_body = if row_count == 0 {
            div()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(8.))
                .py(px(36.))
                .child(
                    Icon::new(LucideIcon::SearchX)
                        .size(IconSize::Xl)
                        .color(theme.muted_foreground),
                )
                .child(
                    div()
                        .text_size(px(13.5))
                        .text_color(theme.muted_foreground)
                        .child(self.empty_message.clone()),
                )
                .into_any_element()
        } else {
            let list_h = px((row_count as f32 * CMD_ROW_HEIGHT).min(360.0));
            uniform_list(
                ("cmd-palette-vlist", entity_id),
                row_count,
                move |range: Range<usize>, _window: &mut Window, cx: &mut App| {
                    let theme = cx.theme();
                    let hover_bg = theme.surface_hover;
                    range
                        .map(|row_i| match &rows_rc[row_i] {
                            PaletteRow::GroupHeader(group_name) => div()
                                .flex()
                                .items_end()
                                .w_full()
                                .h(px(CMD_ROW_HEIGHT))
                                .px(px(10.))
                                .pb(px(6.))
                                .text_size(px(11.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.muted_foreground)
                                .child(group_name.clone())
                                .into_any_element(),
                            PaletteRow::Item {
                                visual_idx,
                                item_idx,
                            } => {
                                let item = &items_rc[*item_idx].item;
                                let visual_pos = *visual_idx;
                                let is_selected = visual_pos == selected_idx;
                                let disabled = item.disabled;
                                let ent_hover = entity.clone();
                                let ent_click = entity.clone();

                                let row_bg = if is_selected {
                                    theme.primary_soft
                                } else {
                                    crate::transparent_black()
                                };
                                let title_fg = if is_selected {
                                    theme.primary_soft_foreground
                                } else {
                                    theme.popover_foreground
                                };
                                let icon_fg = if is_selected {
                                    theme.primary_soft_foreground
                                } else {
                                    theme.muted_foreground
                                };

                                div()
                                    .id(("cmd-item", *item_idx))
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(12.))
                                    .w_full()
                                    .h(px(CMD_ROW_HEIGHT))
                                    .px(px(10.))
                                    .rounded(theme.radius_md)
                                    .bg(row_bg)
                                    .when(!disabled, |el| {
                                        el.cursor_pointer()
                                            .when(!is_selected, |el| {
                                                el.hover(move |s| s.bg(hover_bg))
                                            })
                                            .on_mouse_move(move |_, _, cx| {
                                                ent_hover.update(cx, |this, cx| {
                                                    if this.selected_idx != visual_pos {
                                                        this.selected_idx = visual_pos;
                                                        cx.notify();
                                                    }
                                                });
                                            })
                                            .on_click(move |_, window, cx| {
                                                ent_click.update(cx, |this, cx| {
                                                    this.execute_at(visual_pos, Some(window), cx);
                                                });
                                            })
                                    })
                                    .when(disabled, |el| el.opacity(0.45).cursor_not_allowed())
                                    .child(
                                        div()
                                            .flex()
                                            .flex_row()
                                            .items_center()
                                            .gap(px(10.))
                                            .flex_1()
                                            .min_w_0()
                                            .when_some(item.icon, |el, ic| {
                                                el.child(
                                                    Icon::new(ic).size(IconSize::Md).color(icon_fg),
                                                )
                                            })
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(1.))
                                                    .flex_1()
                                                    .min_w_0()
                                                    .child(
                                                        div()
                                                            .truncate()
                                                            .text_size(px(13.5))
                                                            .font_weight(FontWeight::MEDIUM)
                                                            .text_color(title_fg)
                                                            .child(item.title.clone()),
                                                    )
                                                    .when_some(
                                                        item.subtitle.clone(),
                                                        |el, sub| {
                                                            el.child(
                                                                div()
                                                                    .truncate()
                                                                    .text_size(px(11.5))
                                                                    .text_color(
                                                                        theme.muted_foreground,
                                                                    )
                                                                    .child(sub),
                                                            )
                                                        },
                                                    ),
                                            ),
                                    )
                                    .when(!item.shortcut.is_empty(), |el| {
                                        el.child(
                                            div()
                                                .flex()
                                                .flex_row()
                                                .items_center()
                                                .flex_shrink_0()
                                                .gap(px(3.))
                                                .children(item.shortcut.iter().cloned().map(kbd)),
                                        )
                                    })
                                    .when(item.shortcut.is_empty() && quick_slots && !disabled, |el| {
                                        if is_selected {
                                            el.child(kbd("↵"))
                                        } else if visual_pos < 9 {
                                            let prefix = if cfg!(target_os = "macos") { "⌘" } else { "Ctrl" };
                                            el.child(
                                                div()
                                                    .flex()
                                                    .flex_row()
                                                    .items_center()
                                                    .gap(px(2.))
                                                    .opacity(0.65)
                                                    .child(kbd(prefix))
                                                    .child(kbd(format!("{}", visual_pos + 1))),
                                            )
                                        } else {
                                            el
                                        }
                                    })
                                    .into_any_element()
                            }
                        })
                        .collect()
                },
            )
            .track_scroll(&self.scroll_handle)
            .w_full()
            .h(list_h)
            .p(px(6.))
            .into_any_element()
        };

        let count_label = if result_count == 1 {
            "1 result".to_owned()
        } else {
            format!("{result_count} results")
        };

        let mut panel = div()
            .id(("cmd-palette-panel", entity_id))
            .track_focus(&self.focus_handle)
            // Modal: Tab moves the selection (below), and anything else that
            // moves focus stays inside.
            .focus_trap()
            .on_key_down(cx.listener(Self::on_key_down))
            .occlude()
            .flex()
            .flex_col()
            .w_full()
            .max_w(self.max_width)
            .max_h_full()
            .rounded(theme.radius_xl)
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .border_1()
            .border_color(theme.border)
            .when(theme.elevation_shadows, |el| el.shadow_2xl())
            .overflow_hidden()
            // Focus-preserving click handling so clicking rows/margins never blurs the search input.
            .on_any_mouse_down(|_, window, cx| {
                window.prevent_default();
                cx.stop_propagation();
            })
            .on_click(|_, _, cx| cx.stop_propagation())
            .relative()
            // Search header
            .child(
                div()
                    .flex_shrink_0()
                    .p(px(10.))
                    .border_b_1()
                    .border_color(theme.border)
                    .child(Input::new(&self.search_input).border_0().shadow_none()),
            )
            // Virtualized grouped results
            .child(div().flex_1().min_h_0().child(list_body))
            // Secondary Action Panel popover (Tab / Ctrl+K)
            .when(!action_panel_items.is_empty(), |el| {
                let entity_act = cx.entity();
                el.child(
                    div()
                        .id(("cmd-action-panel", entity_id))
                        .occlude()
                        .absolute()
                        .right(px(10.))
                        .bottom(px(40.))
                        .w(px(230.))
                        .p(px(6.))
                        .rounded(theme.radius_lg)
                        .bg(theme.popover)
                        .border_1()
                        .border_color(theme.border)
                        .when(theme.elevation_shadows, |d| d.shadow_xl())
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(
                            div()
                                .px(px(8.))
                                .py(px(4.))
                                .text_size(px(10.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.muted_foreground)
                                .child("ACTIONS"),
                        )
                        .children(action_panel_items.into_iter().enumerate().map(|(a_idx, act)| {
                            let sel = a_idx == action_selected_idx;
                            let ent = entity_act.clone();
                            let hover_bg = theme.surface_hover;
                            div()
                                .id(("cmd-act-row", a_idx))
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .gap(px(8.))
                                .px(px(8.))
                                .py(px(6.))
                                .rounded(theme.radius_md)
                                .cursor_pointer()
                                .bg(if sel {
                                    theme.primary_soft
                                } else {
                                    crate::transparent_black()
                                })
                                .when(!sel, |d| d.hover(move |s| s.bg(hover_bg)))
                                .on_click(move |_, window, cx| {
                                    ent.update(cx, |this, cx| {
                                        this.action_selected_idx = a_idx;
                                        this.execute_selected_action(Some(window), cx);
                                    });
                                })
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(8.))
                                        .when_some(act.icon, |d, ic| {
                                            d.child(Icon::new(ic).size(IconSize::Sm).color(if sel {
                                                theme.primary_soft_foreground
                                            } else {
                                                theme.muted_foreground
                                            }))
                                        })
                                        .child(
                                            div()
                                                .text_size(px(12.5))
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(if sel {
                                                    theme.primary_soft_foreground
                                                } else {
                                                    theme.popover_foreground
                                                })
                                                .child(act.label),
                                        ),
                                )
                                .when(!act.shortcut.is_empty(), |d| {
                                    d.child(
                                        div()
                                            .flex()
                                            .flex_row()
                                            .gap(px(2.))
                                            .children(act.shortcut.into_iter().map(kbd)),
                                    )
                                })
                        })),
                )
            })
            // Keyboard hint footer
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .px(px(14.))
                    .py(px(8.))
                    .border_t_1()
                    .border_color(theme.border)
                    .bg(theme.surface_muted.opacity(0.45))
                    .text_size(px(11.5))
                    .text_color(theme.muted_foreground)
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(10.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(4.))
                                    .child(kbd("↑"))
                                    .child(kbd("↓"))
                                    .child("Navigate"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(4.))
                                    .child(kbd("↵"))
                                    .child("Run"),
                            )
                            .when(selected_has_actions, |d| {
                                d.child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(4.))
                                        .child(kbd("Tab"))
                                        .child("Actions"),
                                )
                            })
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(4.))
                                    .child(kbd("Esc"))
                                    .child("Close"),
                            ),
                    )
                    .child(count_label),
            );

        crate::Refineable::refine(panel.style(), &self.panel_style);

        // Standalone Spotlight / Alfred window mode: render the panel directly without modal scrim.
        if self.standalone {
            return panel.max_w_full().size_full().into_any_element();
        }

        let animated_panel = if reduce_motion {
            panel.into_any_element()
        } else {
            panel
                .with_animation(
                    ("cmd-palette-enter", entity_id),
                    Animation::new(ANIM_FAST).with_easing(ease_out_quint()),
                    |el, delta| el.opacity(delta).mt(px(12.0 * (1.0 - delta))),
                )
                .into_any_element()
        };

        let backdrop_color = theme.backdrop;
        let backdrop_blur = self.backdrop_blur;

        let overlay = div()
            .id(("cmd-palette-backdrop", entity_id))
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .flex_col()
            .items_center()
            .pt(px(84.))
            .pb(px(20.))
            .px(px(20.))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.close_in(window, cx)),
            )
            .child(animated_panel);

        let final_overlay = if reduce_motion {
            overlay
                .bg(backdrop_color)
                .when(backdrop_blur > px(0.), |el| el.backdrop_blur(backdrop_blur))
                .into_any_element()
        } else {
            overlay
                .with_animation(
                    ("cmd-palette-backdrop-anim", entity_id),
                    Animation::new(ANIM_FAST).with_easing(ease_out_quint()),
                    move |el, delta| {
                        el.bg(backdrop_color.opacity(delta))
                            .when(backdrop_blur > px(0.), |el| {
                                el.backdrop_blur(px(backdrop_blur.0 * delta))
                            })
                    },
                )
                .into_any_element()
        };

        deferred(final_overlay).priority(55).into_any_element()
    }
}

/// View wrapper rendering a [`CommandPaletteState`] entity.
#[derive(IntoElement)]
pub struct CommandPalette {
    state: Entity<CommandPaletteState>,
    style: StyleRefinement,
}

/// Convenience constructor for [`CommandPalette::new`].
pub fn command_palette(state: &Entity<CommandPaletteState>) -> CommandPalette {
    CommandPalette::new(state)
}

impl CommandPalette {
    /// Creates a [`CommandPalette`] element bound to `state`.
    pub fn new(state: &Entity<CommandPaletteState>) -> Self {
        Self {
            state: state.clone(),
            style: StyleRefinement::default(),
        }
    }
}

impl Styled for CommandPalette {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for CommandPalette {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        if self.state.read(cx).panel_style != self.style {
            let style = self.style;
            self.state.update(cx, |s, _| {
                s.panel_style = style;
            });
        }
        self.state
    }
}
