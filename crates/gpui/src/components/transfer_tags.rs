//! Dual-column virtualized shuttle [`Transfer`] (`Available ⇄ Selected` with search & checkboxes)
//! and free-form chip [`TagInput`] (`Enter`/`,` creation, `Backspace`/click removal, max cap).

use std::collections::HashSet;
use std::ops::Range;
use std::rc::Rc;

use crate::icons::{Icon, IconSize, LucideIcon};
use crate::{
    App, Context, ElementId, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
    IntoElement, KeyDownEvent, MouseButton, Pixels, Render, RenderOnce, SharedString,
    StyleRefinement, Styled, UniformListScrollHandle, Window, div, prelude::*, px,
    transparent_black, uniform_list,
};

use super::badge::{BadgeColor, badge};
use super::button::button;
use super::input::{Input, InputEvent, InputState, input_actions};
use super::theme::{ActiveTheme, ComponentSize, focus_ring_shadow, tabular_numerals};

// ============================================================================
// Transfer (Dual-List Shuttle — Virtualized via uniform_list)
// ============================================================================

/// Single selectable option inside a [`Transfer`] shuttle.
#[derive(Clone, Debug)]
pub struct TransferItem {
    /// Unique identifier.
    pub key: SharedString,
    /// Display title.
    pub label: SharedString,
    /// Optional muted secondary description.
    pub description: Option<SharedString>,
    /// Prevents selecting or moving this item.
    pub disabled: bool,
}

impl TransferItem {
    /// Creates a new [`TransferItem`].
    pub fn new(key: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            description: None,
            disabled: false,
        }
    }

    /// Sets muted description text.
    pub fn description(mut self, desc: impl Into<SharedString>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Disables moving this item.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

struct IndexedTransferItem {
    item: TransferItem,
    search_blob: String,
}

impl IndexedTransferItem {
    fn from_item(item: TransferItem) -> Self {
        let mut search_blob = item.label.to_lowercase();
        if let Some(ref desc) = item.description {
            search_blob.push('\n');
            search_blob.push_str(&desc.to_lowercase());
        }
        Self { item, search_blob }
    }
}

/// Events emitted by [`TransferState`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TransferEvent {
    /// Emitted whenever the target (right-side) key list changes.
    Change(Vec<SharedString>),
}

/// Entity state backing a dual-list [`Transfer`] component.
pub struct TransferState {
    items: Rc<Vec<IndexedTransferItem>>,
    has_descriptions: bool,
    target_keys: HashSet<SharedString>,
    left_checked: HashSet<SharedString>,
    right_checked: HashSet<SharedString>,
    left_search: Entity<InputState>,
    right_search: Entity<InputState>,
    left_scroll: UniformListScrollHandle,
    right_scroll: UniformListScrollHandle,
    left_title: SharedString,
    right_title: SharedString,
    searchable: bool,
    disabled: bool,
    list_height: Pixels,
    frame_style: StyleRefinement,
}

impl EventEmitter<TransferEvent> for TransferState {}

impl TransferState {
    /// Creates a [`TransferState`] with `items` and initial right-side `target_keys`.
    pub fn new(
        items: Vec<TransferItem>,
        target_keys: impl IntoIterator<Item = impl Into<SharedString>>,
        cx: &mut Context<Self>,
    ) -> Self {
        let left_search = cx.new(|cx| {
            InputState::new(cx)
                .placeholder("Search available...")
                .leading_icon(LucideIcon::Search)
                .clearable(true)
                .size(ComponentSize::Xs)
        });
        let right_search = cx.new(|cx| {
            InputState::new(cx)
                .placeholder("Search selected...")
                .leading_icon(LucideIcon::Search)
                .clearable(true)
                .size(ComponentSize::Xs)
        });
        cx.subscribe(&left_search, |_, _, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::Change(_)) {
                cx.notify();
            }
        })
        .detach();
        cx.subscribe(&right_search, |_, _, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::Change(_)) {
                cx.notify();
            }
        })
        .detach();

        let has_descriptions = items.iter().any(|i| i.description.is_some());
        let indexed: Vec<IndexedTransferItem> = items
            .into_iter()
            .map(IndexedTransferItem::from_item)
            .collect();

        Self {
            items: Rc::new(indexed),
            has_descriptions,
            target_keys: target_keys.into_iter().map(Into::into).collect(),
            left_checked: HashSet::new(),
            right_checked: HashSet::new(),
            left_search,
            right_search,
            left_scroll: UniformListScrollHandle::new(),
            right_scroll: UniformListScrollHandle::new(),
            left_title: "Available".into(),
            right_title: "Selected".into(),
            searchable: true,
            disabled: false,
            list_height: px(210.),
            frame_style: StyleRefinement::default(),
        }
    }

    /// Sets custom left & right panel titles.
    pub fn titles(
        mut self,
        left: impl Into<SharedString>,
        right: impl Into<SharedString>,
    ) -> Self {
        self.left_title = left.into();
        self.right_title = right.into();
        self
    }

    /// Shows or hides the per-column search inputs (default `true`).
    pub fn searchable(mut self, searchable: bool) -> Self {
        self.searchable = searchable;
        self
    }

    /// Disables all transfers and item selection.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Sets the scrollable viewport height of each column list (default `210px`).
    pub fn list_height(mut self, height: Pixels) -> Self {
        self.list_height = if height.0.is_finite() {
            height.max(px(80.))
        } else {
            px(210.)
        };
        self
    }

    /// Replaces all available items and prunes stale target/checked keys.
    pub fn set_items(&mut self, items: Vec<TransferItem>, cx: &mut Context<Self>) {
        let valid: HashSet<SharedString> = items.iter().map(|i| i.key.clone()).collect();
        self.has_descriptions = items.iter().any(|i| i.description.is_some());
        self.items = Rc::new(
            items
                .into_iter()
                .map(IndexedTransferItem::from_item)
                .collect(),
        );
        self.target_keys.retain(|k| valid.contains(k));
        self.left_checked.retain(|k| valid.contains(k));
        self.right_checked.retain(|k| valid.contains(k));
        cx.notify();
    }

    /// Returns the ordered list of keys currently in the target (right) column.
    pub fn target_keys(&self) -> Vec<SharedString> {
        self.items
            .iter()
            .filter(|it| self.target_keys.contains(&it.item.key))
            .map(|it| it.item.key.clone())
            .collect()
    }

    fn visible_indices(&self, is_right: bool, query: &str) -> Vec<usize> {
        let q = query.trim().to_lowercase();
        self.items
            .iter()
            .enumerate()
            .filter(|(_, it)| self.target_keys.contains(&it.item.key) == is_right)
            .filter(|(_, it)| q.is_empty() || it.search_blob.contains(&q))
            .map(|(idx, _)| idx)
            .collect()
    }

    /// Moves all **visible** checked items from Available → Selected.
    pub fn move_right(&mut self, cx: &mut Context<Self>) {
        if self.disabled || self.left_checked.is_empty() {
            return;
        }
        let query = if self.searchable {
            self.left_search.read(cx).value().to_string()
        } else {
            String::new()
        };
        let visible = self.visible_indices(false, &query);
        let mut moved = false;
        for idx in visible {
            let item = &self.items[idx].item;
            if !item.disabled && self.left_checked.remove(&item.key) {
                self.target_keys.insert(item.key.clone());
                moved = true;
            }
        }
        if moved {
            cx.emit(TransferEvent::Change(self.target_keys()));
            cx.notify();
        }
    }

    /// Moves all **visible** checked items from Selected → Available.
    pub fn move_left(&mut self, cx: &mut Context<Self>) {
        if self.disabled || self.right_checked.is_empty() {
            return;
        }
        let query = if self.searchable {
            self.right_search.read(cx).value().to_string()
        } else {
            String::new()
        };
        let visible = self.visible_indices(true, &query);
        let mut moved = false;
        for idx in visible {
            let item = &self.items[idx].item;
            if !item.disabled && self.right_checked.remove(&item.key) {
                self.target_keys.remove(&item.key);
                moved = true;
            }
        }
        if moved {
            cx.emit(TransferEvent::Change(self.target_keys()));
            cx.notify();
        }
    }

    #[cfg(test)]
    pub(crate) fn left_search_input(&self) -> &Entity<InputState> {
        &self.left_search
    }

    pub(crate) fn toggle_item(&mut self, is_right: bool, key: SharedString, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        let set = if is_right {
            &mut self.right_checked
        } else {
            &mut self.left_checked
        };
        if !set.remove(&key) {
            set.insert(key);
        }
        cx.notify();
    }

    fn toggle_all_visible(&mut self, is_right: bool, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        let query = if self.searchable {
            if is_right {
                self.right_search.read(cx).value().to_string()
            } else {
                self.left_search.read(cx).value().to_string()
            }
        } else {
            String::new()
        };
        let visible_enabled: Vec<SharedString> = self
            .visible_indices(is_right, &query)
            .into_iter()
            .filter_map(|idx| {
                let it = &self.items[idx].item;
                (!it.disabled).then(|| it.key.clone())
            })
            .collect();
        if visible_enabled.is_empty() {
            return;
        }
        let set = if is_right {
            &mut self.right_checked
        } else {
            &mut self.left_checked
        };
        let all_checked = visible_enabled.iter().all(|k| set.contains(k));
        if all_checked {
            for k in visible_enabled {
                set.remove(&k);
            }
        } else {
            for k in visible_enabled {
                set.insert(k);
            }
        }
        cx.notify();
    }

    fn render_column(
        &self,
        is_right: bool,
        visible: Rc<Vec<usize>>,
        enabled_count: usize,
        checked_count: usize,
        cx: &mut Context<Self>,
    ) -> crate::Div {
        let theme = cx.theme();
        let entity = cx.entity();
        let entity_id = cx.entity_id();
        let all_checked = enabled_count > 0 && checked_count == enabled_count;
        let some_checked = checked_count > 0 && !all_checked;
        let can_select_all = !self.disabled && enabled_count > 0;
        let title = if is_right {
            self.right_title.clone()
        } else {
            self.left_title.clone()
        };
        let search_entity = if is_right {
            self.right_search.clone()
        } else {
            self.left_search.clone()
        };
        let scroll_handle = if is_right {
            self.right_scroll.clone()
        } else {
            self.left_scroll.clone()
        };
        let row_height = if self.has_descriptions {
            px(46.)
        } else {
            px(32.)
        };
        let items_rc = Rc::clone(&self.items);
        let checked_snapshot = if is_right {
            self.right_checked.clone()
        } else {
            self.left_checked.clone()
        };
        let col_disabled = self.disabled;
        let prefix = if is_right { "tr-r" } else { "tr-l" };
        let visible_len = visible.len();

        let body: crate::AnyElement = if visible_len == 0 {
            div()
                .flex()
                .items_center()
                .justify_center()
                .h(self.list_height)
                .text_size(px(12.5))
                .text_color(theme.muted_foreground)
                .child("No items")
                .into_any_element()
        } else {
            uniform_list(
                (if is_right { "tr-r-vlist" } else { "tr-l-vlist" }, entity_id),
                visible_len,
                move |range: Range<usize>, _window: &mut Window, cx: &mut App| {
                    let theme = cx.theme();
                    let hover_bg = theme.surface_hover;
                    range
                        .map(|vis_pos| {
                            let item = &items_rc[visible[vis_pos]].item;
                            let key = item.key.clone();
                            let is_checked = checked_snapshot.contains(&key);
                            let disabled = col_disabled || item.disabled;
                            let ent = entity.clone();

                            let row = div()
                                .id((ElementId::from(prefix), key.clone()));
                            // Each row is a checkbox, so a tab stop; Enter or
                            // Space toggles it.
                            super::focus::tab_stop(row, !disabled, &theme)
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(8.))
                                .w_full()
                                .h(row_height)
                                .px(px(10.))
                                .rounded(theme.radius_md)
                                .bg(if is_checked {
                                    theme.primary_soft.opacity(0.6)
                                } else {
                                    transparent_black()
                                })
                                .when(!disabled, |el| {
                                    el.cursor_pointer()
                                        .when(!is_checked, |el| el.hover(move |s| s.bg(hover_bg)))
                                        .on_click(move |_, _, cx| {
                                            ent.update(cx, |this, cx| {
                                                this.toggle_item(is_right, key.clone(), cx);
                                            });
                                        })
                                })
                                .when(disabled, |el| el.opacity(0.45).cursor_not_allowed())
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .flex_shrink_0()
                                        .w(px(15.))
                                        .h(px(15.))
                                        .rounded(px(4.))
                                        .border_1()
                                        .border_color(if is_checked {
                                            theme.primary
                                        } else {
                                            theme.input_border
                                        })
                                        .bg(if is_checked {
                                            theme.primary
                                        } else {
                                            theme.surface
                                        })
                                        .when(is_checked, |el| {
                                            el.child(
                                                Icon::new(LucideIcon::Check)
                                                    .size(IconSize::Xs)
                                                    .color(theme.primary_foreground),
                                            )
                                        }),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .justify_center()
                                        .min_w_0()
                                        .flex_1()
                                        .child(
                                            div()
                                                .text_size(px(13.))
                                                .text_color(theme.foreground)
                                                .truncate()
                                                .child(item.label.clone()),
                                        )
                                        .when_some(item.description.clone(), |el, d| {
                                            el.child(
                                                div()
                                                    .text_size(px(11.5))
                                                    .text_color(theme.muted_foreground)
                                                    .truncate()
                                                    .child(d),
                                            )
                                        }),
                                )
                        })
                        .collect()
                },
            )
            .track_scroll(&scroll_handle)
            .w_full()
            .h(self.list_height)
            .p(px(4.))
            .into_any_element()
        };

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .rounded(theme.radius_xl)
            .border_1()
            .border_color(theme.border)
            .bg(theme.surface)
            .overflow_hidden()
            // Header
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .px(px(12.))
                    .py(px(9.))
                    .bg(theme.surface_muted.opacity(0.6))
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .id((
                                if is_right {
                                    "tr-r-sel-all"
                                } else {
                                    "tr-l-sel-all"
                                },
                                entity_id,
                            ))
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.))
                            .when(can_select_all, |el| {
                                super::focus::tab_stop(el, true, &theme)
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                    this.toggle_all_visible(is_right, cx);
                                }))
                            })
                            .when(!can_select_all, |el| el.opacity(0.55).cursor_not_allowed())
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .w(px(16.))
                                    .h(px(16.))
                                    .rounded(px(4.))
                                    .border_1()
                                    .border_color(if all_checked || some_checked {
                                        theme.primary
                                    } else {
                                        theme.input_border
                                    })
                                    .bg(if all_checked || some_checked {
                                        theme.primary
                                    } else {
                                        theme.surface
                                    })
                                    .when(all_checked, |el| {
                                        el.child(
                                            Icon::new(LucideIcon::Check)
                                                .size(IconSize::Xs)
                                                .color(theme.primary_foreground),
                                        )
                                    })
                                    .when(some_checked, |el| {
                                        el.child(
                                            Icon::new(LucideIcon::Minus)
                                                .size(IconSize::Xs)
                                                .color(theme.primary_foreground),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme.foreground)
                                    .child(title),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(11.5))
                            .font_features(tabular_numerals())
                            .text_color(theme.muted_foreground)
                            .child(format!("{checked_count}/{enabled_count}")),
                    ),
            )
            // Optional Search
            .when(self.searchable, |el| {
                el.child(
                    div()
                        .p(px(8.))
                        .border_b_1()
                        .border_color(theme.border)
                        .child(Input::new(&search_entity)),
                )
            })
            // Virtualized items list
            .child(body)
    }
}

impl Render for TransferState {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity_id = cx.entity_id();
        let left_q = if self.searchable {
            self.left_search.read(cx).value().to_string()
        } else {
            String::new()
        };
        let right_q = if self.searchable {
            self.right_search.read(cx).value().to_string()
        } else {
            String::new()
        };

        let left_vis = Rc::new(self.visible_indices(false, &left_q));
        let right_vis = Rc::new(self.visible_indices(true, &right_q));

        let mut left_enabled = 0usize;
        let mut left_checked_vis = 0usize;
        for &idx in left_vis.iter() {
            let it = &self.items[idx].item;
            if !it.disabled {
                left_enabled += 1;
                if self.left_checked.contains(&it.key) {
                    left_checked_vis += 1;
                }
            }
        }
        let mut right_enabled = 0usize;
        let mut right_checked_vis = 0usize;
        for &idx in right_vis.iter() {
            let it = &self.items[idx].item;
            if !it.disabled {
                right_enabled += 1;
                if self.right_checked.contains(&it.key) {
                    right_checked_vis += 1;
                }
            }
        }

        let can_right = !self.disabled && left_checked_vis > 0;
        let can_left = !self.disabled && right_checked_vis > 0;

        let left_col = self.render_column(false, left_vis, left_enabled, left_checked_vis, cx);
        let right_col = self.render_column(true, right_vis, right_enabled, right_checked_vis, cx);

        let mut root = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(12.))
            .w_full()
            .when(self.disabled, |el| el.opacity(0.6))
            .child(left_col)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .flex_shrink_0()
                    .child(
                        button(("tr-move-right", entity_id))
                            .primary()
                            .size(ComponentSize::Sm)
                            .icon_only(true)
                            .disabled(!can_right)
                            .leading_icon(LucideIcon::ChevronRight)
                            .on_click(cx.listener(|this, _, _, cx| this.move_right(cx))),
                    )
                    .child(
                        button(("tr-move-left", entity_id))
                            .secondary()
                            .size(ComponentSize::Sm)
                            .icon_only(true)
                            .disabled(!can_left)
                            .leading_icon(LucideIcon::ChevronLeft)
                            .on_click(cx.listener(|this, _, _, cx| this.move_left(cx))),
                    ),
            )
            .child(right_col);

        crate::Refineable::refine(root.style(), &self.frame_style);
        root
    }
}

/// Renderable dual-list [`Transfer`] element wrapping [`Entity<TransferState>`].
#[derive(IntoElement)]
pub struct Transfer {
    state: Entity<TransferState>,
    style: StyleRefinement,
}

/// Convenience constructor for [`Transfer::new`].
pub fn transfer(state: &Entity<TransferState>) -> Transfer {
    Transfer::new(state)
}

impl Transfer {
    /// Creates a [`Transfer`] bound to `state`.
    pub fn new(state: &Entity<TransferState>) -> Self {
        Self {
            state: state.clone(),
            style: StyleRefinement::default(),
        }
    }
}

impl Styled for Transfer {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Transfer {
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
// TagInput (Free-Form Pill Chip Input)
// ============================================================================

/// Events emitted by [`TagInputState`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TagInputEvent {
    /// Emitted whenever tags are added or removed.
    Change(Vec<SharedString>),
}

/// Entity state backing a free-form pill [`TagInput`].
pub struct TagInputState {
    focus_handle: FocusHandle,
    tags: Vec<SharedString>,
    input: Entity<InputState>,
    max_tags: Option<usize>,
    allow_duplicates: bool,
    tag_color: BadgeColor,
    disabled: bool,
    frame_style: StyleRefinement,
}

impl EventEmitter<TagInputEvent> for TagInputState {}

impl Focusable for TagInputState {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        let at_cap = self.max_tags.is_some_and(|m| self.tags.len() >= m);
        if !at_cap && !self.disabled {
            self.input.read(cx).focus_handle(cx)
        } else {
            self.focus_handle.clone()
        }
    }
}

impl TagInputState {
    /// Creates a new [`TagInputState`].
    pub fn new(cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            InputState::new(cx)
                .placeholder("Type tag and press Enter...")
                .size(ComponentSize::Sm)
        });
        cx.subscribe(&input, |this, inp, ev: &InputEvent, cx| match ev {
            InputEvent::Submit(text) => {
                let raw = text.to_string();
                if this.add_tag_str(&raw, cx) {
                    inp.update(cx, |s, cx| s.set_value_silent(String::new(), cx));
                }
            }
            InputEvent::Change(text) => {
                if let Some((before_comma, tail)) = text.rsplit_once(',') {
                    for part in before_comma.split(',') {
                        let cleaned = part.trim();
                        if !cleaned.is_empty() {
                            this.add_tag_str(cleaned, cx);
                        }
                    }
                    // Always clear processed comma-delimited tokens while preserving any trailing text
                    // so rejected duplicates never poison subsequent keystrokes.
                    let remainder = tail.trim_start().to_owned();
                    inp.update(cx, |s, cx| s.set_value_silent(remainder, cx));
                }
            }
            InputEvent::Focus | InputEvent::Blur => {
                cx.notify();
            }
        })
        .detach();

        Self {
            focus_handle: cx.focus_handle(),
            tags: Vec::new(),
            input,
            max_tags: None,
            allow_duplicates: false,
            tag_color: BadgeColor::Indigo,
            disabled: false,
            frame_style: StyleRefinement::default(),
        }
    }

    fn normalize_existing_tags(&mut self) {
        let mut normalized: Vec<SharedString> = Vec::with_capacity(self.tags.len());
        let mut seen_lower: HashSet<String> = HashSet::new();
        for tag in self.tags.drain(..) {
            let trimmed = tag.trim();
            if trimmed.is_empty() {
                continue;
            }
            if !self.allow_duplicates && !seen_lower.insert(trimmed.to_lowercase()) {
                continue;
            }
            if let Some(max) = self.max_tags
                && normalized.len() >= max
            {
                break;
            }
            normalized.push(SharedString::from(trimmed.to_owned()));
        }
        self.tags = normalized;
    }

    /// Pre-populates initial tags (trimmed, deduplicated if `!allow_duplicates`, and capped by `max_tags`).
    pub fn default_tags(mut self, tags: impl IntoIterator<Item = impl Into<SharedString>>) -> Self {
        self.tags = tags.into_iter().map(Into::into).collect();
        self.normalize_existing_tags();
        self
    }

    /// Sets a maximum number of tags allowed (clamping any existing tags).
    pub fn max_tags(mut self, max: usize) -> Self {
        self.max_tags = Some(max.max(1));
        self.normalize_existing_tags();
        self
    }

    /// Allows duplicate tags when `true` (default `false`).
    pub fn allow_duplicates(mut self, allow: bool) -> Self {
        self.allow_duplicates = allow;
        if !allow {
            self.normalize_existing_tags();
        }
        self
    }

    /// Sets the pill color for rendered tags (default [`BadgeColor::Indigo`]).
    pub fn color(mut self, color: BadgeColor) -> Self {
        self.tag_color = color;
        self
    }

    /// Disables adding or removing tags.
    pub fn disabled(mut self, disabled: bool, cx: &mut Context<Self>) -> Self {
        self.disabled = disabled;
        self.input.update(cx, |s, cx| s.set_disabled(disabled, cx));
        self
    }

    /// Dynamically updates disabled state.
    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        if self.disabled != disabled {
            self.disabled = disabled;
            self.input.update(cx, |s, cx| s.set_disabled(disabled, cx));
            cx.notify();
        }
    }

    /// Replaces all tags programmatically (normalized) and emits [`TagInputEvent::Change`].
    pub fn set_tags(
        &mut self,
        tags: impl IntoIterator<Item = impl Into<SharedString>>,
        cx: &mut Context<Self>,
    ) {
        self.tags = tags.into_iter().map(Into::into).collect();
        self.normalize_existing_tags();
        cx.emit(TagInputEvent::Change(self.tags.clone()));
        cx.notify();
    }

    /// Removes all tags and emits [`TagInputEvent::Change`] if non-empty.
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        if !self.tags.is_empty() {
            self.tags.clear();
            cx.emit(TagInputEvent::Change(Vec::new()));
            cx.notify();
        }
    }

    /// Returns the current tag list.
    pub fn tags(&self) -> &[SharedString] {
        &self.tags
    }

    #[cfg(test)]
    pub(crate) fn text_input(&self) -> &Entity<InputState> {
        &self.input
    }

    /// Adds a single tag (trimmed, Unicode case-insensitive dedup, respecting `max_tags`). Returns `true` if added.
    pub fn add_tag_str(&mut self, raw: &str, cx: &mut Context<Self>) -> bool {
        if self.disabled {
            return false;
        }
        let cleaned = raw.trim();
        if cleaned.is_empty() {
            return false;
        }
        if let Some(max) = self.max_tags
            && self.tags.len() >= max
        {
            return false;
        }
        if !self.allow_duplicates {
            let lowered = cleaned.to_lowercase();
            if self.tags.iter().any(|t| t.to_lowercase() == lowered) {
                return false;
            }
        }
        self.tags.push(SharedString::from(cleaned.to_owned()));
        cx.emit(TagInputEvent::Change(self.tags.clone()));
        cx.notify();
        true
    }

    /// Removes the tag at `index`.
    pub fn remove_at(&mut self, index: usize, cx: &mut Context<Self>) {
        if !self.disabled && index < self.tags.len() {
            self.tags.remove(index);
            cx.emit(TagInputEvent::Change(self.tags.clone()));
            cx.notify();
        }
    }

    fn remove_last_if_input_empty(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.disabled || self.tags.is_empty() {
            return false;
        }
        let at_cap = self.max_tags.is_some_and(|m| self.tags.len() >= m);
        if at_cap || self.input.read(cx).value().is_empty() {
            let last = self.tags.len() - 1;
            self.remove_at(last, cx);
            // If we just dropped below cap, move focus back to the restored text input.
            if at_cap {
                let fh = self.input.read(cx).focus_handle(cx);
                window.focus(&fh, cx);
            }
            return true;
        }
        false
    }
}

impl Render for TagInputState {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let entity_id = cx.entity_id();
        let disabled = self.disabled;
        let tag_color = self.tag_color;
        let at_cap = self.max_tags.is_some_and(|m| self.tags.len() >= m);
        let focused = !disabled
            && (self.focus_handle.is_focused(window)
                || self.input.read(cx).focus_handle(cx).is_focused(window));

        let mut root = div()
            .id(("tag-input-shell", entity_id))
            // The text field is the one tab stop; at the cap it's gone, so the
            // shell takes its place (Backspace there removes the last tag).
            .when(!disabled, |el| {
                el.track_focus(&self.focus_handle.clone().tab_stop(at_cap))
            })
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap(px(6.))
            .w_full()
            .min_h(px(40.))
            .px(px(8.))
            .py(px(5.))
            .overflow_hidden()
            .rounded(theme.radius_md)
            .border_1()
            .border_color(if focused {
                theme.ring
            } else {
                theme.input_border
            })
            .when(focused, |el| el.shadow(focus_ring_shadow(&theme)))
            .bg(if disabled {
                theme.disabled_bg
            } else {
                theme.input_bg
            })
            .when(disabled, |el| el.opacity(0.55).cursor_not_allowed())
            .when(!disabled, |el| {
                el.cursor_text()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            if at_cap {
                                window.focus(&this.focus_handle, cx);
                            } else {
                                let fh = this.input.read(cx).focus_handle(cx);
                                window.focus(&fh, cx);
                            }
                        }),
                    )
                    // Intercept Backspace action in Capture phase BEFORE `InputState` consumes it!
                    .capture_action(cx.listener(
                        |this, _: &input_actions::Backspace, window, cx| {
                            if this.remove_last_if_input_empty(window, cx) {
                                cx.stop_propagation();
                            }
                        },
                    ))
                    // Also handle raw `backspace` keydown when `at_cap` (inner Input unmounted) or unbound.
                    .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                        if ev.keystroke.key == "backspace"
                            && this.remove_last_if_input_empty(window, cx)
                        {
                            cx.stop_propagation();
                        }
                    }))
            })
            .children(self.tags.iter().enumerate().map(|(idx, tag)| {
                let mut b = badge(tag.clone())
                    .color(tag_color)
                    .pill(true)
                    .max_w_full();
                if !disabled {
                    b = b.on_remove(
                        (ElementId::from((tag.clone(), idx)), "rm"),
                        cx.listener(move |this, _, _, cx| this.remove_at(idx, cx)),
                    );
                }
                b
            }))
            .when(!at_cap && !disabled, |el| {
                el.child(
                    div().flex_1().min_w(px(130.)).child(
                        Input::new(&self.input)
                            .border_0()
                            .shadow_none()
                            .bg(transparent_black())
                            .h(px(28.)),
                    ),
                )
            });

        crate::Refineable::refine(root.style(), &self.frame_style);
        root
    }
}

/// Renderable free-form chip [`TagInput`] wrapping [`Entity<TagInputState>`].
#[derive(IntoElement)]
pub struct TagInput {
    state: Entity<TagInputState>,
    style: StyleRefinement,
}

/// Convenience constructor for [`TagInput::new`].
pub fn tag_input(state: &Entity<TagInputState>) -> TagInput {
    TagInput::new(state)
}

impl TagInput {
    /// Creates a [`TagInput`] bound to `state`.
    pub fn new(state: &Entity<TagInputState>) -> Self {
        Self {
            state: state.clone(),
            style: StyleRefinement::default(),
        }
    }
}

impl Styled for TagInput {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for TagInput {
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
