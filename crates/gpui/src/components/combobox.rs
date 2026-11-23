//! Searchable [`ComboBox`] / [`ComboBoxState`] supporting both single-select
//! and multi-select (with removable tag chips), inline typing directly inside the main input
//! frame, live fuzzy filtering, keyboard navigation, and optional custom-entry creation.

use std::cell::Cell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use crate::icons::{Icon, IconSize, LucideIcon};
use crate::{
    Anchor, Animation, AnimationExt, AnyElement, App, Bounds, Context, Entity, EventEmitter,
    FocusHandle, Focusable, FontWeight, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent,
    Pixels, Render, RenderOnce, ScrollHandle, SharedString, StyleRefinement, Styled, Subscription,
    Window, anchored, canvas, deferred, div, ease_out_quint, point, prelude::*, px,
    transparent_black,
};

use super::completion::fuzzy_match_score_lower;
use super::focus::{is_list_nav_key, list_step};
use super::input::{Input, InputEvent, InputState, input_actions};
use super::layout::focus_ring_shadow;
use super::theme::{ANIM_FAST, ActiveTheme, ComponentSize};

/// An item in a [`ComboBox`].
#[derive(Clone)]
pub struct ComboBoxOption<T> {
    /// Underlying value.
    pub value: T,
    /// Primary display label (matched by search).
    pub label: SharedString,
    /// Optional description (also matched by search).
    pub description: Option<SharedString>,
    /// Optional leading [`LucideIcon`].
    pub icon: Option<LucideIcon>,
    /// Optional custom leading visual element rendered in the trigger (single-select) and
    /// dropdown option rows (e.g. brand icon, color swatch, status dot, or avatar).
    /// Takes precedence over [`Self::icon`] when set.
    pub leading: Option<Rc<dyn Fn(&mut Window, &mut App) -> AnyElement + 'static>>,
    /// Optional group heading.
    pub group: Option<SharedString>,
    /// Additional search keywords.
    pub keywords: Vec<SharedString>,
    /// Whether this option is disabled.
    pub disabled: bool,
}

impl<T: fmt::Debug> fmt::Debug for ComboBoxOption<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ComboBoxOption")
            .field("value", &self.value)
            .field("label", &self.label)
            .field("description", &self.description)
            .field("icon", &self.icon)
            .field("leading", &self.leading.as_ref().map(|_| "<fn>"))
            .field("group", &self.group)
            .field("keywords", &self.keywords)
            .field("disabled", &self.disabled)
            .finish()
    }
}

impl<T> ComboBoxOption<T> {
    /// Creates a new combobox option.
    pub fn new(value: T, label: impl Into<SharedString>) -> Self {
        Self {
            value,
            label: label.into(),
            description: None,
            icon: None,
            leading: None,
            group: None,
            keywords: Vec::new(),
            disabled: false,
        }
    }

    /// Sets a muted description line.
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Sets a leading [`LucideIcon`].
    pub fn icon(mut self, icon: LucideIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Sets a custom leading visual element rendered in the trigger (single-select) and
    /// dropdown option rows (for example a brand SVG, raster image, color swatch, status dot,
    /// or avatar).
    ///
    /// When set, this takes precedence over [`Self::icon`].
    pub fn leading<E: IntoElement>(
        mut self,
        render: impl Fn(&mut Window, &mut App) -> E + 'static,
    ) -> Self {
        self.leading = Some(Rc::new(move |window, cx| {
            render(window, cx).into_any_element()
        }));
        self
    }

    /// Assigns this option to a group section.
    pub fn group(mut self, group: impl Into<SharedString>) -> Self {
        self.group = Some(group.into());
        self
    }

    /// Adds extra search keywords.
    pub fn keywords(mut self, keywords: impl IntoIterator<Item = impl Into<SharedString>>) -> Self {
        self.keywords = keywords.into_iter().map(Into::into).collect();
        self
    }

    /// Disables this option.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    fn fuzzy_score(&self, query_lower: &str) -> Option<i32> {
        if query_lower.is_empty() {
            return Some(0);
        }
        let label_score = fuzzy_match_score_lower(query_lower, &self.label.to_lowercase());
        let desc_score = self
            .description
            .as_deref()
            .and_then(|d| fuzzy_match_score_lower(query_lower, &d.to_lowercase()))
            .map(|s| s - 15);
        let kw_score = self
            .keywords
            .iter()
            .filter_map(|kw| fuzzy_match_score_lower(query_lower, &kw.to_lowercase()))
            .max()
            .map(|s| s - 20);
        let grp_score = self
            .group
            .as_deref()
            .and_then(|g| fuzzy_match_score_lower(query_lower, &g.to_lowercase()))
            .map(|s| s - 25);

        [label_score, desc_score, kw_score, grp_score]
            .into_iter()
            .flatten()
            .max()
    }
}

/// Events emitted by [`ComboBoxState`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ComboBoxEvent<T> {
    /// Emitted when the selected value(s) change.
    Change(Vec<T>),
    /// Emitted when the user submits a custom non-matching query (when `allow_custom` is enabled).
    Create(SharedString),
}

/// Entity state backing a searchable [`ComboBox`] (single or multi-select).
///
/// Users type directly inside the main control (alongside selected chips in multi-select mode);
/// the anchored popover displays only the filtered suggestion list.
pub struct ComboBoxState<T: Clone + PartialEq + 'static> {
    focus_handle: FocusHandle,
    search_input: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
    options: Vec<ComboBoxOption<T>>,
    selected: Vec<T>,
    multiple: bool,
    allow_custom: bool,
    highlighted_filtered_pos: Option<usize>,
    open: bool,
    placeholder: SharedString,
    search_placeholder: SharedString,
    empty_message: SharedString,
    size: ComponentSize,
    disabled: bool,
    invalid: bool,
    clearable: bool,
    max_menu_height: Pixels,
    scroll_handle: ScrollHandle,
    trigger_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    frame_style: StyleRefinement,
}

impl<T: Clone + PartialEq + 'static> EventEmitter<ComboBoxEvent<T>> for ComboBoxState<T> {}

impl<T: Clone + PartialEq + 'static> Focusable for ComboBoxState<T> {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        if !self.disabled {
            self.search_input.read(cx).focus_handle(cx)
        } else {
            self.focus_handle.clone()
        }
    }
}

impl<T: Clone + PartialEq + 'static> ComboBoxState<T> {
    /// Creates a new single-select [`ComboBoxState`] with `options`.
    pub fn new(options: Vec<ComboBoxOption<T>>, cx: &mut Context<Self>) -> Self {
        let search_input = cx.new(|cx| {
            InputState::new(cx)
                .placeholder("Select or search...")
                .size(ComponentSize::Md)
                .content_px(px(0.))
        });
        let sub = cx.subscribe(&search_input, |this, _input, event: &InputEvent, cx| {
            if this.disabled {
                return;
            }
            match event {
                InputEvent::Change(query) => {
                    this.open = true;
                    let filtered = this.filtered_indices_for_query(query);
                    this.highlighted_filtered_pos = filtered
                        .iter()
                        .position(|&idx| !this.options[idx].disabled);
                    this.scroll_handle.scroll_to_item(0);
                    cx.notify();
                }
                InputEvent::Submit(query) => {
                    if !this.open {
                        this.open = true;
                        this.highlighted_filtered_pos = this.first_enabled_filtered_pos_with_app(cx);
                        cx.notify();
                        return;
                    }
                    let filtered = this.filtered_indices_for_query(query);
                    if let Some(pos) = this.highlighted_filtered_pos
                        && let Some(&opt_idx) = filtered.get(pos)
                    {
                        this.toggle_option_index(opt_idx, cx);
                    } else if this.allow_custom && !query.trim().is_empty() {
                        let created = SharedString::from(query.trim().to_owned());
                        cx.emit(ComboBoxEvent::Create(created));
                        this.close_menu(cx);
                    }
                }
                InputEvent::Focus => {
                    cx.notify();
                }
                InputEvent::Blur => {
                    this.close_menu(cx);
                    cx.notify();
                }
                InputEvent::AcceptSuggestion(_) | InputEvent::CommittedChange(_) => {}
            }
        });

        Self {
            focus_handle: cx.focus_handle(),
            search_input,
            _subscriptions: vec![sub],
            options,
            selected: Vec::new(),
            multiple: false,
            allow_custom: false,
            highlighted_filtered_pos: Some(0),
            open: false,
            placeholder: SharedString::new_static("Select or search..."),
            search_placeholder: SharedString::new_static("Filter options..."),
            empty_message: SharedString::new_static("No matching options found."),
            size: ComponentSize::Md,
            disabled: false,
            invalid: false,
            clearable: true,
            max_menu_height: px(280.),
            scroll_handle: ScrollHandle::new(),
            trigger_bounds: Rc::new(Cell::new(None)),
            frame_style: StyleRefinement::default(),
        }
    }

    /// Enables multi-select mode (renders selected items as removable tag chips).
    pub fn multiple(mut self, multiple: bool) -> Self {
        self.multiple = multiple;
        self
    }

    /// Allows emitting [`ComboBoxEvent::Create`] when pressing Enter with a custom query.
    pub fn allow_custom(mut self, allow: bool) -> Self {
        self.allow_custom = allow;
        self
    }

    /// Sets trigger placeholder text when nothing is selected.
    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Sets inline filter placeholder text shown while typing alongside selected chips.
    pub fn search_placeholder(mut self, placeholder: impl Into<SharedString>, cx: &mut App) -> Self {
        let placeholder: SharedString = placeholder.into();
        self.search_placeholder = placeholder.clone();
        self.search_input.update(cx, |input, cx| {
            input.set_placeholder(placeholder, cx);
        });
        self
    }

    /// Sets message shown when no options match the query.
    pub fn empty_message(mut self, message: impl Into<SharedString>) -> Self {
        self.empty_message = message.into();
        self
    }

    /// Sets initial selected value(s).
    pub fn default_values(mut self, values: Vec<T>) -> Self {
        self.selected = values;
        self
    }

    /// Sets control size.
    pub fn size(mut self, size: ComponentSize) -> Self {
        self.size = size;
        self
    }

    /// Disables interaction.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Highlights the control with error styling.
    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    /// Controls whether the inline clear button is shown.
    pub fn clearable(mut self, clearable: bool) -> Self {
        self.clearable = clearable;
        self
    }

    /// Returns all options.
    pub fn options(&self) -> &[ComboBoxOption<T>] {
        &self.options
    }

    /// Returns the first selected option (convenient for single-select comboboxes).
    pub fn selected_option(&self) -> Option<&ComboBoxOption<T>> {
        self.selected
            .first()
            .and_then(|val| self.options.iter().find(|o| &o.value == val))
    }

    /// Returns the currently selected values slice.
    pub fn selected_values(&self) -> &[T] {
        &self.selected
    }

    /// Returns the first selected value (convenient for single-select comboboxes).
    pub fn selected_value(&self) -> Option<&T> {
        self.selected.first()
    }

    /// Returns whether interaction is currently disabled.
    pub fn is_disabled(&self) -> bool {
        self.disabled
    }

    /// Replaces the option list in place and retains any selected values that still exist.
    ///
    /// Does not emit [`ComboBoxEvent::Change`].
    pub fn set_options(&mut self, options: Vec<ComboBoxOption<T>>, cx: &mut Context<Self>) {
        self.options = options;
        self.selected
            .retain(|val| self.options.iter().any(|o| &o.value == val));
        self.highlighted_filtered_pos = self.first_enabled_filtered_pos_with_app(cx);
        self.scroll_handle.scroll_to_item(0);
        cx.notify();
    }

    fn normalize_values(&self, values: impl IntoIterator<Item = T>) -> Vec<T> {
        let mut normalized = Vec::new();
        for value in values {
            if !normalized.contains(&value) {
                normalized.push(value);
                if !self.multiple {
                    break;
                }
            }
        }
        normalized
    }

    /// Programmatically replaces the selected values and emits [`ComboBoxEvent::Change`] if
    /// the selection changed.
    ///
    /// In single-select mode, at most the first value is kept and the menu is closed.
    pub fn set_values(&mut self, values: impl IntoIterator<Item = T>, cx: &mut Context<Self>) {
        let next = self.normalize_values(values);
        if self.selected == next {
            return;
        }
        self.selected = next;
        if !self.multiple {
            self.close_menu(cx);
        }
        cx.emit(ComboBoxEvent::Change(self.selected.clone()));
        cx.notify();
    }

    /// Programmatically replaces the selected values without emitting [`ComboBoxEvent::Change`].
    ///
    /// Useful when a parent view synchronizes dependent comboboxes (for example after loading
    /// asynchronous options) without triggering re-entrant change handlers.
    pub fn set_values_silent(
        &mut self,
        values: impl IntoIterator<Item = T>,
        cx: &mut Context<Self>,
    ) {
        let next = self.normalize_values(values);
        if self.selected != next {
            self.selected = next;
            cx.notify();
        }
    }

    /// Updates the disabled state dynamically, closing the menu if disabling.
    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        if self.disabled == disabled {
            return;
        }
        self.disabled = disabled;
        if disabled {
            self.close_menu(cx);
        }
        self.search_input
            .update(cx, |input, cx| input.set_disabled(disabled, cx));
        cx.notify();
    }

    /// Updates the invalid error state dynamically.
    pub fn set_invalid(&mut self, invalid: bool, cx: &mut Context<Self>) {
        if self.invalid != invalid {
            self.invalid = invalid;
            cx.notify();
        }
    }

    /// Updates the trigger placeholder text dynamically.
    pub fn set_placeholder(
        &mut self,
        placeholder: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        let placeholder = placeholder.into();
        if self.placeholder != placeholder {
            self.placeholder = placeholder;
            cx.notify();
        }
    }

    /// Updates the empty-results message dynamically.
    pub fn set_empty_message(
        &mut self,
        message: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        let message = message.into();
        if self.empty_message != message {
            self.empty_message = message;
            cx.notify();
        }
    }

    /// Updates whether the inline clear button is shown.
    pub fn set_clearable(&mut self, clearable: bool, cx: &mut Context<Self>) {
        if self.clearable != clearable {
            self.clearable = clearable;
            cx.notify();
        }
    }

    /// Updates the control size dynamically.
    pub fn set_size(&mut self, size: ComponentSize, cx: &mut Context<Self>) {
        if self.size != size {
            self.size = size;
            cx.notify();
        }
    }

    /// Returns the underlying inline search [`InputState`] entity.
    pub fn search_input(&self) -> &Entity<InputState> {
        &self.search_input
    }

    /// Returns current search query text.
    pub fn query<'a>(&self, cx: &'a App) -> &'a str {
        self.search_input.read(cx).value()
    }

    /// Programmatically sets the search filter query.
    pub fn set_query(&mut self, query: impl Into<String>, cx: &mut Context<Self>) {
        let q = query.into();
        self.search_input.update(cx, |input, cx| {
            input.set_value(q, cx);
        });
        self.highlighted_filtered_pos = self.first_enabled_filtered_pos_with_app(cx);
        cx.notify();
    }

    /// Returns `true` if the dropdown popover is currently open.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Returns the filtered option indices matching `query` in exact visual display order.
    pub fn filtered_indices_for_query(&self, query: &str) -> Vec<usize> {
        let q = query.trim().to_lowercase();
        let scored_indices: Vec<usize> = if q.is_empty() {
            (0..self.options.len()).collect()
        } else {
            let mut scored: Vec<(i32, usize)> = self
                .options
                .iter()
                .enumerate()
                .filter_map(|(idx, opt)| opt.fuzzy_score(&q).map(|score| (score, idx)))
                .collect();
            scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
            scored.into_iter().map(|(_, idx)| idx).collect()
        };

        // Keep grouped items clustered in first-seen group order so Up/Down matches visual rows.
        if scored_indices
            .iter()
            .any(|&idx| self.options[idx].group.is_some())
        {
            let mut group_map: HashMap<Option<SharedString>, usize> = HashMap::new();
            let mut grouped: Vec<Vec<usize>> = Vec::new();
            for idx in scored_indices {
                let key = self.options[idx].group.clone();
                if let Some(&pos) = group_map.get(&key) {
                    grouped[pos].push(idx);
                } else {
                    let pos = grouped.len();
                    group_map.insert(key, pos);
                    grouped.push(vec![idx]);
                }
            }
            grouped.into_iter().flatten().collect()
        } else {
            scored_indices
        }
    }

    /// Returns the filtered option indices matching the current search input.
    pub fn filtered_option_indices(&self, cx: &App) -> Vec<usize> {
        self.filtered_indices_for_query(self.search_input.read(cx).value())
    }

    fn first_enabled_filtered_pos_with_app(&self, cx: &App) -> Option<usize> {
        let indices = self.filtered_option_indices(cx);
        indices.iter().position(|&idx| !self.options[idx].disabled)
    }

    /// Opens the combobox dropdown and focuses its inline search input.
    pub fn open_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        self.open = true;
        self.highlighted_filtered_pos = self.first_enabled_filtered_pos_with_app(cx);
        self.scroll_handle.scroll_to_item(0);
        let search_focus = self.search_input.focus_handle(cx);
        window.focus(&search_focus, cx);
        cx.notify();
    }

    /// Closes the combobox dropdown and clears the filter query silently.
    pub fn close_menu(&mut self, cx: &mut Context<Self>) {
        if !self.open {
            if !self.search_input.read(cx).value().is_empty() {
                self.search_input
                    .update(cx, |input, cx| input.set_value_silent(String::new(), cx));
            }
            return;
        }
        self.open = false;
        self.search_input.update(cx, |input, cx| {
            if !input.value().is_empty() {
                input.set_value_silent(String::new(), cx);
            }
        });
        cx.notify();
    }

    /// Adds a new option dynamically (useful alongside `allow_custom`).
    pub fn push_option(&mut self, option: ComboBoxOption<T>, select: bool, cx: &mut Context<Self>) {
        let value = option.value.clone();
        self.options.push(option);
        if select {
            self.select_value(value, cx);
        } else {
            cx.notify();
        }
    }

    /// Programmatically selects `value`.
    pub fn select_value(&mut self, value: T, cx: &mut Context<Self>) {
        if self.multiple {
            if !self.selected.contains(&value) {
                self.selected.push(value);
            }
        } else {
            self.selected = vec![value];
            self.close_menu(cx);
        }
        cx.emit(ComboBoxEvent::Change(self.selected.clone()));
        cx.notify();
    }

    /// Removes a specific value from the selection.
    pub fn remove_value(&mut self, value: &T, cx: &mut Context<Self>) {
        let before = self.selected.len();
        self.selected.retain(|v| v != value);
        if self.selected.len() != before {
            cx.emit(ComboBoxEvent::Change(self.selected.clone()));
            cx.notify();
        }
    }

    /// Clears all selected values (and any active filter query).
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        let had_query = !self.search_input.read(cx).value().is_empty();
        if had_query {
            self.search_input
                .update(cx, |input, cx| input.set_value_silent(String::new(), cx));
        }
        if self.selected.is_empty() {
            if had_query {
                cx.notify();
            }
            return;
        }
        self.selected.clear();
        cx.emit(ComboBoxEvent::Change(Vec::new()));
        cx.notify();
    }

    /// Toggles selection of the option at `opt_idx`.
    pub fn toggle_option_index(&mut self, opt_idx: usize, cx: &mut Context<Self>) {
        let Some(option) = self.options.get(opt_idx) else {
            return;
        };
        if option.disabled {
            return;
        }
        let value = option.value.clone();
        if self.multiple {
            if let Some(pos) = self.selected.iter().position(|v| v == &value) {
                self.selected.remove(pos);
            } else {
                self.selected.push(value);
            }
            if !self.search_input.read(cx).value().is_empty() {
                self.search_input
                    .update(cx, |input, cx| input.set_value_silent(String::new(), cx));
                self.highlighted_filtered_pos = self.first_enabled_filtered_pos_with_app(cx);
            }
        } else {
            self.selected = vec![value];
            self.close_menu(cx);
        }
        cx.emit(ComboBoxEvent::Change(self.selected.clone()));
        cx.notify();
    }

    fn pop_last_if_query_empty(&mut self, cx: &mut Context<Self>) -> bool {
        if self.disabled || !self.multiple || !self.search_input.read(cx).value().is_empty() {
            return false;
        }
        if self.selected.pop().is_some() {
            cx.emit(ComboBoxEvent::Change(self.selected.clone()));
            cx.notify();
            return true;
        }
        false
    }

    fn child_index_for_filtered_pos(&self, filtered: &[usize], target_pos: usize) -> usize {
        let mut child_idx = 0usize;
        let mut last_group: Option<&SharedString> = None;
        for (pos, &opt_idx) in filtered.iter().enumerate() {
            let option = &self.options[opt_idx];
            if option.group.as_ref() != last_group && option.group.is_some() {
                child_idx += 1;
            }
            last_group = option.group.as_ref();
            if pos == target_pos {
                return child_idx;
            }
            child_idx += 1;
        }
        child_idx
    }

    fn on_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return;
        }
        let mods = &event.keystroke.modifiers;
        let key = event.keystroke.key.as_str();
        if !mods.modified() && is_list_nav_key(key, false) {
            if !self.open {
                if matches!(key, "down" | "up") {
                    self.open_menu(window, cx);
                    cx.stop_propagation();
                }
                return;
            }
            let filtered = self.filtered_option_indices(cx);
            if filtered.is_empty() {
                self.highlighted_filtered_pos = None;
            } else {
                let enabled: Vec<bool> = filtered
                    .iter()
                    .map(|&idx| !self.options[idx].disabled)
                    .collect();
                if let Some(next) =
                    list_step(key, self.highlighted_filtered_pos, &enabled, 6, false)
                {
                    self.highlighted_filtered_pos = Some(next);
                    self.scroll_handle
                        .scroll_to_item(self.child_index_for_filtered_pos(&filtered, next));
                }
            }
            cx.notify();
            cx.stop_propagation();
            return;
        }
        match key {
            // Fallback when input keybindings aren't installed.
            "enter" => {
                if !self.open {
                    self.open_menu(window, cx);
                } else {
                    let filtered = self.filtered_option_indices(cx);
                    if let Some(pos) = self.highlighted_filtered_pos
                        && let Some(&opt_idx) = filtered.get(pos)
                    {
                        self.toggle_option_index(opt_idx, cx);
                    } else if self.allow_custom {
                        let query = self.search_input.read(cx).value().trim().to_owned();
                        if !query.is_empty() {
                            cx.emit(ComboBoxEvent::Create(SharedString::from(query)));
                            self.close_menu(cx);
                        }
                    }
                }
                cx.stop_propagation();
            }
            "escape" if self.open => {
                self.close_menu(cx);
                cx.stop_propagation();
            }
            // As a browser's select: Tab closes the menu and moves focus on naturally.
            "tab" if self.open => {
                self.close_menu(cx);
            }
            "backspace" if self.pop_last_if_query_empty(cx) => {
                cx.stop_propagation();
            }
            _ => {}
        }
    }

    fn label_for_value(&self, value: &T) -> Option<SharedString> {
        self.options
            .iter()
            .find(|o| &o.value == value)
            .map(|o| o.label.clone())
    }
}

impl<T: Clone + PartialEq + 'static> Render for ComboBoxState<T> {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let entity = cx.entity();
        let entity_id = cx.entity_id();
        let search_focused = !self.disabled && self.search_input.focus_handle(cx).is_focused(window);
        let focused = !self.disabled && (self.focus_handle.is_focused(window) || search_focused);
        let query_is_empty = self.search_input.read(cx).value().is_empty();

        let (has_selected, selected_leading_fn, selected_icon, selected_label) = if !self.multiple {
            let opt = self
                .selected
                .first()
                .and_then(|val| self.options.iter().find(|o| &o.value == val));
            (
                opt.is_some(),
                opt.and_then(|o| o.leading.clone()),
                opt.and_then(|o| o.icon),
                opt.map(|o| o.label.clone()),
            )
        } else {
            (false, None, None, None)
        };

        // Keep embedded input size and placeholder synchronized without triggering extra notifies.
        let desired_input_size = if self.multiple {
            ComponentSize::Sm
        } else {
            self.size
        };
        let desired_placeholder: SharedString = if self.multiple {
            if self.selected.is_empty() {
                self.placeholder.clone()
            } else if self.open || focused {
                self.search_placeholder.clone()
            } else {
                SharedString::new_static("")
            }
        } else if has_selected {
            SharedString::new_static("")
        } else {
            self.placeholder.clone()
        };
        {
            let inp = self.search_input.read(cx);
            let needs_size = inp.size_value() != desired_input_size;
            let needs_ph = *inp.placeholder_text() != desired_placeholder;
            let needs_dis = inp.is_disabled() != self.disabled;
            if needs_size || needs_ph || needs_dis {
                self.search_input.update(cx, |inp, cx| {
                    if needs_size {
                        inp.set_size_silent(desired_input_size);
                    }
                    if needs_ph {
                        inp.set_placeholder_silent(desired_placeholder);
                    }
                    if needs_dis {
                        inp.set_disabled(self.disabled, cx);
                    }
                });
            }
        }

        let border_color = if self.invalid {
            theme.destructive
        } else if focused || self.open {
            theme.ring
        } else {
            theme.input_border
        };

        let bg = if self.disabled {
            theme.disabled_bg
        } else {
            theme.input_bg
        };

        let bounds_cell = self.trigger_bounds.clone();
        let captured_bounds = self.trigger_bounds.get();
        let menu_min_w = captured_bounds
            .map(|b| b.size.width)
            .unwrap_or(px(240.))
            .max(px(220.));

        let show_clear =
            self.clearable && (!self.selected.is_empty() || !query_is_empty) && !self.disabled;

        let trigger_content = if self.multiple {
            let mut chips = div()
                .flex()
                .flex_row()
                .flex_wrap()
                .items_center()
                .gap(px(4.))
                .py(px(3.));

            for (idx, val) in self.selected.iter().enumerate() {
                let label = self
                    .label_for_value(val)
                    .unwrap_or_else(|| SharedString::new_static("Item"));
                let remove_val = val.clone();
                let entity_rm = entity.clone();
                chips = chips.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(4.))
                        .px(px(7.))
                        .py(px(2.))
                        .rounded(theme.radius_sm)
                        .bg(theme.primary_soft)
                        .text_color(theme.primary_soft_foreground)
                        .text_size(px(12.))
                        .font_weight(FontWeight::MEDIUM)
                        .child(label)
                        .when(!self.disabled, |el| {
                            el.child(
                                div()
                                    .id(("gpui-combobox-chip-rm", idx))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_full()
                                    .cursor_pointer()
                                    .hover(|s| s.bg(theme.primary.opacity(0.2)))
                                    .on_mouse_down(MouseButton::Left, |_, window, cx| {
                                        window.prevent_default();
                                        cx.stop_propagation();
                                    })
                                    .on_click(move |_, _, cx| {
                                        cx.stop_propagation();
                                        entity_rm.update(cx, |state, cx| {
                                            state.remove_value(&remove_val, cx);
                                        });
                                    })
                                    .child(
                                        Icon::new(LucideIcon::X)
                                            .size(IconSize::Xs)
                                            .color(theme.primary_soft_foreground),
                                    ),
                            )
                        }),
                );
            }

            if !self.disabled {
                chips = chips.child(
                    div().flex_1().min_w(px(72.)).child(
                        Input::new(&self.search_input)
                            .border_0()
                            .shadow_none()
                            .bg(transparent_black())
                            .h(px(26.)),
                    ),
                );
            } else if self.selected.is_empty() {
                chips = chips.child(
                    div()
                        .truncate()
                        .text_color(theme.placeholder)
                        .child(self.placeholder.clone()),
                );
            }

            chips.into_any_element()
        } else {
            let input_h = px((self.size.height().0 - 2.0).max(20.0));
            let selected_leading = query_is_empty
                .then_some(selected_leading_fn)
                .flatten()
                .map(|render| render(window, cx));
            let has_selected_leading = selected_leading.is_some();
            let selected_icon = (query_is_empty && !has_selected_leading)
                .then_some(selected_icon)
                .flatten();
            let selected_label = query_is_empty.then_some(selected_label).flatten();

            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .w_full()
                .min_w_0()
                .when_some(selected_leading, |el, leading| {
                    el.child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(self.open, |el| el.opacity(0.6))
                            .child(leading),
                    )
                })
                .when_some(selected_icon, |el, icon| {
                    el.child(
                        Icon::new(icon)
                            .size(self.size.icon_size())
                            .color(if self.open {
                                theme.placeholder
                            } else {
                                theme.muted_foreground
                            }),
                    )
                })
                .child(
                    div()
                        .relative()
                        .flex()
                        .items_center()
                        .flex_1()
                        .min_w_0()
                        .h(input_h)
                        .when_some(selected_label, |el, label| {
                            let label_color = if self.disabled {
                                theme.disabled_foreground
                            } else if self.open {
                                theme.placeholder
                            } else {
                                theme.foreground
                            };
                            el.child(
                                div()
                                    .absolute()
                                    .inset_0()
                                    .flex()
                                    .items_center()
                                    .overflow_hidden()
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(self.size.font_size())
                                            .text_color(label_color)
                                            .child(label),
                                    ),
                            )
                        })
                        .when(!self.disabled, |el| {
                            el.child(
                                Input::new(&self.search_input)
                                    .border_0()
                                    .shadow_none()
                                    .bg(transparent_black())
                                    .h(input_h),
                            )
                        })
                        .when(self.disabled && !has_selected, |el| {
                            el.child(
                                div()
                                    .truncate()
                                    .text_color(theme.placeholder)
                                    .child(self.placeholder.clone()),
                            )
                        }),
                )
                .into_any_element()
        };

        let mut trigger = div()
            .id(("gpui-combobox-trigger", entity_id))
            .relative()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .w_full()
            .min_h(self.size.height())
            .px(self.size.px_x())
            .rounded(theme.radius_md)
            .bg(bg)
            .border_1()
            .border_color(border_color)
            .text_size(self.size.font_size())
            .when(!self.disabled, |el| {
                el.cursor_text()
                    .when(theme.elevation_shadows, |el| el.shadow_2xs())
            })
            .when((focused || self.open) && !self.disabled, |el| {
                let ring = focus_ring_shadow(&theme, self.invalid);
                el.when(!ring.is_empty(), |el| el.shadow(ring))
            })
            .when(!focused && !self.open && !self.disabled, |el| {
                let hover_border = if self.invalid {
                    theme.destructive
                } else {
                    theme.input_border_hover
                };
                el.hover(move |s| s.border_color(hover_border))
            })
            .when(self.disabled, |el| el.opacity(0.6).cursor_not_allowed())
            .when(!self.disabled, |el| {
                el.track_focus(&self.focus_handle.clone().tab_stop(false))
                    .capture_action(cx.listener(
                        |this, _: &input_actions::Backspace, _, cx| {
                            if this.pop_last_if_query_empty(cx) {
                                cx.stop_propagation();
                            }
                        },
                    ))
                    .on_key_down(cx.listener(Self::on_key_down))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            let search_focus = this.search_input.focus_handle(cx);
                            window.focus(&search_focus, cx);
                            if !this.open {
                                this.open_menu(window, cx);
                            }
                        }),
                    )
            })
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
            .child(div().flex_1().min_w_0().child(trigger_content))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(4.))
                    .flex_shrink_0()
                    .when(show_clear, |el| {
                        let entity_clear = entity.clone();
                        el.child(
                            div()
                                .id(("gpui-combobox-clear", entity_id))
                                .flex()
                                .items_center()
                                .justify_center()
                                .p(px(2.))
                                .rounded_full()
                                .cursor_pointer()
                                .hover(|s| s.bg(theme.surface_hover))
                                .on_mouse_down(MouseButton::Left, |_, window, cx| {
                                    window.prevent_default();
                                    cx.stop_propagation();
                                })
                                .on_click(move |_, _, cx| {
                                    cx.stop_propagation();
                                    entity_clear.update(cx, |state, cx| state.clear(cx));
                                })
                                .child(
                                    Icon::new(LucideIcon::X)
                                        .size(IconSize::Xs)
                                        .color(theme.muted_foreground),
                                ),
                        )
                    })
                    .child(
                        div()
                            .id(("gpui-combobox-toggle", entity_id))
                            .flex()
                            .items_center()
                            .justify_center()
                            .p(px(2.))
                            .rounded(theme.radius_sm)
                            .when(!self.disabled, |el| {
                                el.cursor_pointer()
                                    .hover(|s| s.bg(theme.surface_hover))
                                    .on_mouse_down(MouseButton::Left, |_, window, cx| {
                                        window.prevent_default();
                                        cx.stop_propagation();
                                    })
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        cx.stop_propagation();
                                        if this.open {
                                            this.close_menu(cx);
                                        } else {
                                            this.open_menu(window, cx);
                                        }
                                    }))
                            })
                            .child(
                                Icon::new(LucideIcon::ChevronsUpDown)
                                    .size(IconSize::Sm)
                                    .color(theme.muted_foreground),
                            ),
                    ),
            );

        let menu = if self.open {
            let reduce_motion = cx.reduce_motion();
            let filtered = self.filtered_option_indices(cx);
            let query_text = self.search_input.read(cx).value().trim().to_owned();

            let mut list_body = div()
                .id(("gpui-combobox-list", entity_id))
                .flex()
                .flex_col()
                .w_full()
                .max_h(self.max_menu_height)
                .p(px(4.))
                .overflow_y_scroll()
                .track_scroll(&self.scroll_handle);

            if filtered.is_empty() {
                list_body = list_body.child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap(px(6.))
                        .py(px(14.))
                        .px(px(12.))
                        .text_size(px(13.))
                        .text_color(theme.muted_foreground)
                        .child(self.empty_message.clone()),
                );
                if self.allow_custom && !query_text.is_empty() {
                    let create_label = SharedString::from(format!("Create \"{query_text}\""));
                    let create_value = SharedString::from(query_text);
                    let entity_create = entity;
                    list_body = list_body.child(
                        div()
                            .id(("gpui-combobox-create", entity_id))
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(6.))
                            .w_full()
                            .px(px(10.))
                            .py(px(7.))
                            .rounded(theme.radius_sm)
                            .cursor_pointer()
                            .text_size(px(13.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.primary)
                            .hover(|s| s.bg(theme.primary_soft))
                            .on_click(move |_, _, cx| {
                                let val = create_value.clone();
                                entity_create.update(cx, |state, cx| {
                                    cx.emit(ComboBoxEvent::Create(val));
                                    state.close_menu(cx);
                                });
                            })
                            .child(
                                Icon::new(LucideIcon::Plus)
                                    .size(IconSize::Sm)
                                    .color(theme.primary),
                            )
                            .child(create_label),
                    );
                }
            } else {
                let mut last_group: Option<SharedString> = None;
                for (filtered_pos, opt_idx) in filtered.iter().copied().enumerate() {
                    let (
                        opt_group,
                        is_selected,
                        opt_disabled,
                        opt_leading,
                        opt_icon,
                        opt_label,
                        opt_desc,
                    ) = {
                        let option = &self.options[opt_idx];
                        (
                            option.group.clone(),
                            self.selected.contains(&option.value),
                            option.disabled,
                            option.leading.clone(),
                            option.icon,
                            option.label.clone(),
                            option.description.clone(),
                        )
                    };
                    if opt_group != last_group
                        && let Some(group_name) = opt_group.clone()
                    {
                        list_body = list_body.child(
                            div()
                                .px(px(10.))
                                .pt(px(6.))
                                .pb(px(3.))
                                .text_size(px(11.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.muted_foreground)
                                .child(group_name),
                        );
                    }
                    last_group = opt_group;

                    let leading_el = opt_leading.map(|render| render(window, cx));
                    let opt_icon = leading_el.is_none().then_some(opt_icon).flatten();
                    let is_highlighted = self.highlighted_filtered_pos == Some(filtered_pos);
                    let entity_hover = entity.clone();
                    let entity_opt = entity.clone();

                    list_body = list_body.child(
                        div()
                            .id(("gpui-combobox-option", opt_idx))
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .gap(px(10.))
                            .w_full()
                            .px(px(10.))
                            .py(px(7.))
                            .rounded(theme.radius_sm)
                            .when(is_highlighted, |el| el.bg(theme.primary_soft))
                            .when(!opt_disabled, |el| {
                                el.cursor_pointer()
                                    .when(!is_highlighted, |el| {
                                        el.hover(|s| s.bg(theme.surface_hover))
                                    })
                                    .on_mouse_move(move |_, _, cx| {
                                        entity_hover.update(cx, |state, cx| {
                                            if state.highlighted_filtered_pos != Some(filtered_pos)
                                            {
                                                state.highlighted_filtered_pos = Some(filtered_pos);
                                                cx.notify();
                                            }
                                        });
                                    })
                                    .on_click(move |_, _, cx| {
                                        entity_opt.update(cx, |state, cx| {
                                            state.toggle_option_index(opt_idx, cx);
                                        });
                                    })
                            })
                            .when(opt_disabled, |el| {
                                el.opacity(0.45).cursor_not_allowed()
                            })
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(8.))
                                    .min_w_0()
                                    .when_some(leading_el, |el, leading| {
                                        el.child(
                                            div()
                                                .flex_none()
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .child(leading),
                                        )
                                    })
                                    .when_some(opt_icon, |el, icon| {
                                        el.child(
                                            Icon::new(icon)
                                                .size(IconSize::Sm)
                                                .color(if is_selected {
                                                    theme.primary
                                                } else if is_highlighted {
                                                    theme.primary_soft_foreground
                                                } else {
                                                    theme.muted_foreground
                                                }),
                                        )
                                    })
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .min_w_0()
                                            .child(
                                                div()
                                                    .truncate()
                                                    .text_size(px(13.5))
                                                    .font_weight(if is_selected {
                                                        FontWeight::SEMIBOLD
                                                    } else {
                                                        FontWeight::NORMAL
                                                    })
                                                    .text_color(if is_highlighted {
                                                        theme.primary_soft_foreground
                                                    } else {
                                                        theme.popover_foreground
                                                    })
                                                    .child(opt_label),
                                            )
                                            .when_some(opt_desc, |el, desc| {
                                                el.child(
                                                    div()
                                                        .truncate()
                                                        .text_size(px(12.))
                                                        .text_color(theme.muted_foreground)
                                                        .child(desc),
                                                )
                                            }),
                                    ),
                            )
                            .when(is_selected, |el| {
                                el.child(
                                    Icon::new(LucideIcon::Check)
                                        .size(IconSize::Sm)
                                        .color(theme.primary),
                                )
                            }),
                    );
                }
            }

            let panel = div()
                .flex()
                .flex_col()
                .min_w(menu_min_w)
                .max_w(menu_min_w.max(px(520.)))
                .rounded(theme.radius_lg)
                .bg(theme.popover)
                .text_color(theme.popover_foreground)
                .border_1()
                .border_color(theme.border)
                .shadow_lg()
                // Clicking an option keeps focus in the inline search field.
                .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                .on_mouse_down_out(cx.listener(|this, ev: &MouseDownEvent, _, cx| {
                    if this
                        .trigger_bounds
                        .get()
                        .is_some_and(|b| b.contains(&ev.position))
                    {
                        return;
                    }
                    this.close_menu(cx);
                }))
                .child(list_body);

            let panel_el = if reduce_motion {
                panel.into_any_element()
            } else {
                panel
                    .with_animation(
                        ("gpui-combobox-menu-anim", entity_id),
                        Animation::new(ANIM_FAST).with_easing(ease_out_quint()),
                        |el, delta| el.opacity(delta).mt(px(-6.0 * (1.0 - delta))),
                    )
                    .into_any_element()
            };

            let mut anchor = anchored()
                .anchor(Anchor::TopLeft)
                .snap_to_window_with_margin(px(8.));
            if let Some(bounds) = captured_bounds {
                anchor = anchor
                    .attach(bounds, Anchor::BottomLeft)
                    .offset(point(px(0.), px(4.)));
            } else {
                anchor = anchor.offset(point(px(0.), px(4.)));
            }

            Some(deferred(anchor.child(panel_el)).priority(20))
        } else {
            None
        };

        crate::Refineable::refine(trigger.style(), &self.frame_style);
        let has_explicit_width = self.frame_style.size.width.is_some();

        div()
            .flex()
            .flex_col()
            .when(!has_explicit_width, |el| el.w_full())
            .child(trigger)
            .when_some(menu, |el, menu| el.child(div().h_0().w_full().child(menu)))
    }
}

/// Renderable searchable combobox wrapping an [`Entity<ComboBoxState<T>>`].
#[derive(IntoElement)]
pub struct ComboBox<T: Clone + PartialEq + 'static> {
    state: Entity<ComboBoxState<T>>,
    style: StyleRefinement,
}

/// Convenience constructor for [`ComboBox::new`].
pub fn combo_box<T: Clone + PartialEq + 'static>(
    state: &Entity<ComboBoxState<T>>,
) -> ComboBox<T> {
    ComboBox::new(state)
}

impl<T: Clone + PartialEq + 'static> ComboBox<T> {
    /// Creates a [`ComboBox`] element backed by `state`.
    pub fn new(state: &Entity<ComboBoxState<T>>) -> Self {
        Self {
            state: state.clone(),
            style: StyleRefinement::default(),
        }
    }
}

impl<T: Clone + PartialEq + 'static> Styled for ComboBox<T> {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl<T: Clone + PartialEq + 'static> RenderOnce for ComboBox<T> {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        if self.state.read(cx).frame_style != self.style {
            self.state.update(cx, |state, cx| {
                state.frame_style = self.style;
                cx.notify();
            });
        }
        self.state
    }
}
