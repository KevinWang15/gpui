//! Tailwind-inspired searchable [`ComboBox`] / [`ComboBoxState`] supporting both single-select
//! and multi-select (with removable tag chips), live filtering, keyboard navigation, and optional
//! custom-entry creation.

use std::cell::Cell;
use std::rc::Rc;

use crate::{
    Anchor, App, Bounds, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
    IntoElement, KeyDownEvent, MouseButton, Pixels, Render, RenderOnce, SharedString,
    StyleRefinement, Styled, Subscription, Window, anchored, canvas, deferred, div, point,
    prelude::*, px,
};
use crate::icons::{Icon, IconSize, LucideIcon};

use super::input::{InputEvent, InputState};
use super::layout::focus_ring_shadow;
use super::theme::{ActiveTheme, ComponentSize};

/// An item in a [`ComboBox`].
#[derive(Clone, Debug)]
pub struct ComboBoxOption<T> {
    /// Underlying value.
    pub value: T,
    /// Primary display label (matched by search).
    pub label: SharedString,
    /// Optional description (also matched by search).
    pub description: Option<SharedString>,
    /// Optional leading [`LucideIcon`].
    pub icon: Option<LucideIcon>,
    /// Optional group heading.
    pub group: Option<SharedString>,
    /// Additional search keywords.
    pub keywords: Vec<SharedString>,
    /// Whether this option is disabled.
    pub disabled: bool,
}

impl<T> ComboBoxOption<T> {
    /// Creates a new combobox option.
    pub fn new(value: T, label: impl Into<SharedString>) -> Self {
        Self {
            value,
            label: label.into(),
            description: None,
            icon: None,
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

    fn matches(&self, query_lower: &str) -> bool {
        if query_lower.is_empty() {
            return true;
        }
        if self.label.to_lowercase().contains(query_lower) {
            return true;
        }
        if let Some(desc) = &self.description
            && desc.to_lowercase().contains(query_lower)
        {
            return true;
        }
        self.keywords
            .iter()
            .any(|kw| kw.to_lowercase().contains(query_lower))
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

/// Entity state backing a Tailwind-style searchable [`ComboBox`] (single or multi-select).
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
    trigger_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
}

impl<T: Clone + PartialEq + 'static> EventEmitter<ComboBoxEvent<T>> for ComboBoxState<T> {}

impl<T: Clone + PartialEq + 'static> Focusable for ComboBoxState<T> {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl<T: Clone + PartialEq + 'static> ComboBoxState<T> {
    /// Creates a new single-select [`ComboBoxState`] with `options`.
    pub fn new(options: Vec<ComboBoxOption<T>>, cx: &mut Context<Self>) -> Self {
        let search_input = cx.new(|cx| {
            InputState::new(cx)
                .placeholder("Search...")
                .size(ComponentSize::Sm)
                .leading_icon(LucideIcon::Search)
                .clearable(true)
        });
        let sub = cx.subscribe(&search_input, |this, _input, event: &InputEvent, cx| {
            match event {
                InputEvent::Change(query) => {
                    this.open = true;
                    let filtered = this.filtered_indices_for_query(query);
                    this.highlighted_filtered_pos = filtered
                        .iter()
                        .position(|&idx| !this.options[idx].disabled);
                    cx.notify();
                }
                InputEvent::Submit(query) => {
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
                InputEvent::Focus | InputEvent::Blur => {}
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
            trigger_bounds: Rc::new(Cell::new(None)),
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

    /// Sets placeholder text inside the search input.
    pub fn search_placeholder(mut self, placeholder: impl Into<SharedString>, cx: &mut App) -> Self {
        let placeholder: SharedString = placeholder.into();
        self.search_placeholder = placeholder.clone();
        self.search_input.update(cx, |input, cx| {
            // Re-initialize placeholder via setter-friendly replacement
            let value = input.value().to_owned();
            *input = InputState::new(cx)
                .default_value(value)
                .placeholder(placeholder)
                .size(ComponentSize::Sm)
                .leading_icon(LucideIcon::Search)
                .clearable(true);
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

    /// Returns the currently selected values slice.
    pub fn selected_values(&self) -> &[T] {
        &self.selected
    }

    /// Returns the first selected value (convenient for single-select comboboxes).
    pub fn selected_value(&self) -> Option<&T> {
        self.selected.first()
    }

    /// Returns the underlying search [`InputState`] entity.
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

    /// Returns the filtered option indices matching `query`.
    pub fn filtered_indices_for_query(&self, query: &str) -> Vec<usize> {
        let query = query.trim().to_lowercase();
        self.options
            .iter()
            .enumerate()
            .filter_map(|(idx, opt)| opt.matches(&query).then_some(idx))
            .collect()
    }

    /// Returns the filtered option indices matching the current search input.
    pub fn filtered_option_indices(&self, cx: &App) -> Vec<usize> {
        self.filtered_indices_for_query(self.search_input.read(cx).value())
    }

    fn first_enabled_filtered_pos_with_app(&self, cx: &App) -> Option<usize> {
        let indices = self.filtered_option_indices(cx);
        indices.iter().position(|&idx| !self.options[idx].disabled)
    }

    /// Opens the combobox dropdown and focuses its search input.
    pub fn open_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        self.open = true;
        self.highlighted_filtered_pos = self.first_enabled_filtered_pos_with_app(cx);
        let search_focus = self.search_input.focus_handle(cx);
        window.focus(&search_focus, cx);
        cx.notify();
    }

    /// Closes the combobox dropdown and clears the filter query.
    pub fn close_menu(&mut self, cx: &mut Context<Self>) {
        if !self.open {
            return;
        }
        self.open = false;
        self.search_input.update(cx, |input, cx| {
            if !input.value().is_empty() {
                input.set_value(String::new(), cx);
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
            self.open = false;
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

    /// Clears all selected values.
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        if self.selected.is_empty() {
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
        } else {
            self.selected = vec![value];
            self.close_menu(cx);
        }
        cx.emit(ComboBoxEvent::Change(self.selected.clone()));
        cx.notify();
    }

    fn move_highlight(&mut self, delta: isize, cx: &Context<Self>) {
        let filtered = self.filtered_option_indices(cx);
        if filtered.is_empty() {
            self.highlighted_filtered_pos = None;
            return;
        }
        let len = filtered.len();
        let start = self.highlighted_filtered_pos.unwrap_or(0).min(len - 1);
        for step in 1..=len {
            let candidate =
                ((start as isize + delta * step as isize).rem_euclid(len as isize)) as usize;
            if !self.options[filtered[candidate]].disabled {
                self.highlighted_filtered_pos = Some(candidate);
                return;
            }
        }
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
        match event.keystroke.key.as_str() {
            "down" => {
                if !self.open {
                    self.open_menu(window, cx);
                } else {
                    self.move_highlight(1, cx);
                    cx.notify();
                }
                cx.stop_propagation();
            }
            "up" => {
                if !self.open {
                    self.open_menu(window, cx);
                } else {
                    self.move_highlight(-1, cx);
                    cx.notify();
                }
                cx.stop_propagation();
            }
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
                window.focus(&self.focus_handle, cx);
                cx.stop_propagation();
            }
            "backspace" if self.multiple && self.search_input.read(cx).value().is_empty() => {
                if self.selected.pop().is_some() {
                    cx.emit(ComboBoxEvent::Change(self.selected.clone()));
                    cx.notify();
                }
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
        let focused = self.focus_handle.is_focused(window)
            || self.search_input.focus_handle(cx).is_focused(window);
        let entity = cx.entity();

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
            theme.surface
        };

        let bounds_cell = self.trigger_bounds.clone();
        let captured_bounds = self.trigger_bounds.get();
        let menu_min_w = captured_bounds
            .map(|b| b.size.width)
            .unwrap_or(px(240.))
            .max(px(220.));

        let show_clear = self.clearable && !self.selected.is_empty() && !self.disabled;

        let trigger_content = if self.selected.is_empty() {
            div()
                .truncate()
                .text_color(theme.placeholder)
                .child(self.placeholder.clone())
                .into_any_element()
        } else if self.multiple {
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
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation()
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
            chips.into_any_element()
        } else {
            let selected_opt = self
                .selected
                .first()
                .and_then(|val| self.options.iter().find(|o| &o.value == val));
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .min_w_0()
                .when_some(selected_opt.and_then(|o| o.icon), |el, icon| {
                    el.child(
                        Icon::new(icon)
                            .size(self.size.icon_size())
                            .color(theme.muted_foreground),
                    )
                })
                .child(
                    div()
                        .truncate()
                        .text_color(if self.disabled {
                            theme.disabled_foreground
                        } else {
                            theme.foreground
                        })
                        .child(
                            selected_opt
                                .map(|o| o.label.clone())
                                .unwrap_or_else(|| self.placeholder.clone()),
                        ),
                )
                .into_any_element()
        };

        let trigger = div()
            .id("gpui-combobox-trigger")
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
            .when(!self.disabled, |el| el.shadow_2xs().cursor_pointer())
            .when((focused || self.open) && !self.disabled, |el| {
                el.shadow(focus_ring_shadow(&theme, self.invalid))
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
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key_down))
            .when(!self.disabled, |el| {
                el.on_click(cx.listener(|this, _, window, cx| {
                    if this.open {
                        this.close_menu(cx);
                    } else {
                        this.open_menu(window, cx);
                    }
                }))
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
                                .id("gpui-combobox-clear")
                                .flex()
                                .items_center()
                                .justify_center()
                                .p(px(2.))
                                .rounded_full()
                                .hover(|s| s.bg(theme.surface_hover))
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
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
                        Icon::new(LucideIcon::ChevronsUpDown)
                            .size(IconSize::Sm)
                            .color(theme.muted_foreground),
                    ),
            );

        let menu = if self.open {
            let filtered = self.filtered_option_indices(cx);
            let query_text = self.search_input.read(cx).value().trim().to_owned();

            let mut list_body = div()
                .id("gpui-combobox-list")
                .flex()
                .flex_col()
                .max_h(self.max_menu_height)
                .overflow_y_scroll()
                .p(px(4.));

            if filtered.is_empty() {
                list_body = list_body.child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap(px(6.))
                        .py(px(18.))
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
                            .id("gpui-combobox-create")
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
                    let option = &self.options[opt_idx];
                    if option.group != last_group
                        && let Some(group_name) = option.group.clone()
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
                    last_group = option.group.clone();

                    let is_selected = self.selected.contains(&option.value);
                    let is_highlighted = self.highlighted_filtered_pos == Some(filtered_pos);
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
                            .when(is_highlighted, |el| el.bg(theme.surface_hover))
                            .when(!option.disabled, |el| {
                                el.cursor_pointer()
                                    .hover(|s| s.bg(theme.surface_hover))
                                    .on_click(move |_, _, cx| {
                                        entity_opt.update(cx, |state, cx| {
                                            state.toggle_option_index(opt_idx, cx);
                                        });
                                    })
                            })
                            .when(option.disabled, |el| {
                                el.opacity(0.45).cursor_not_allowed()
                            })
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(8.))
                                    .min_w_0()
                                    .when_some(option.icon, |el, icon| {
                                        el.child(
                                            Icon::new(icon)
                                                .size(IconSize::Sm)
                                                .color(if is_selected {
                                                    theme.primary
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
                                                    .text_color(theme.popover_foreground)
                                                    .child(option.label.clone()),
                                            )
                                            .when_some(option.description.clone(), |el, desc| {
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
                .rounded(theme.radius_lg)
                .bg(theme.popover)
                .text_color(theme.popover_foreground)
                .border_1()
                .border_color(theme.border)
                .shadow_lg()
                .on_key_down(cx.listener(Self::on_key_down))
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    this.close_menu(cx);
                }))
                .child(
                    div()
                        .p(px(6.))
                        .border_b_1()
                        .border_color(theme.border)
                        .child(self.search_input.clone()),
                )
                .child(list_body);

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

            Some(deferred(anchor.child(panel)).priority(20))
        } else {
            None
        };

        div()
            .flex()
            .flex_col()
            .w_full()
            .child(trigger)
            .when_some(menu, |el, menu| el.child(div().h_0().w_full().child(menu)))
    }
}

/// Renderable Tailwind-style searchable combobox wrapping an [`Entity<ComboBoxState<T>>`].
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
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let mut wrapper = div().w_full().child(self.state);
        crate::Refineable::refine(wrapper.style(), &self.style);
        wrapper
    }
}
