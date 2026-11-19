//! Keyboard-first Spotlight / `⌘K` [`CommandPalette`] with fuzzy search, grouped commands,
//! [`Kbd`] shortcut badges, keyboard navigation (`Up`/`Down`/`Enter`/`Escape`), and GPU-blurred
//! modal backdrop.

use crate::{
    Animation, AnimationExt, App, Context, Entity, EventEmitter, FocusHandle, Focusable,
    FontWeight, IntoElement, KeyDownEvent, MouseButton, Pixels, Render, RenderOnce, ScrollHandle,
    SharedString, StyleRefinement, Styled, Window, deferred, div, ease_out_quint, prelude::*, px,
};
use crate::icons::{Icon, IconSize, LucideIcon};

use super::field::kbd;
use super::input::{Input, InputEvent, InputState};
use super::theme::{ANIM_FAST, ActiveTheme, ComponentSize};

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

    /// Marks this command disabled.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

/// Events emitted by [`CommandPaletteState`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommandPaletteEvent {
    /// Emitted when the user executes a command via `Enter` or mouse click.
    Execute(SharedString),
    /// Emitted when the palette opens.
    Open,
    /// Emitted when the palette closes.
    Close,
}

/// Entity state backing a [`CommandPalette`].
pub struct CommandPaletteState {
    focus_handle: FocusHandle,
    search_input: Entity<InputState>,
    scroll_handle: ScrollHandle,
    open: bool,
    items: Vec<CommandItem>,
    selected_idx: usize,
    placeholder: SharedString,
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
pub(crate) fn fuzzy_match_score(query: &str, text: &str) -> Option<i32> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Some(0);
    }
    let hay = text.to_lowercase();
    if let Some(pos) = hay.find(&q) {
        // Exact prefix / substring match gets highest priority.
        let prefix_bonus = if pos == 0 { 120 } else { 60 };
        return Some(prefix_bonus - (pos as i32).min(40));
    }
    // Ordered character subsequence match with word-boundary & consecutive bonuses.
    let mut score = 0i32;
    let mut q_chars = q.chars().peekable();
    let mut prev_matched = false;
    let mut prev_char = ' ';
    for ch in hay.chars() {
        if let Some(&target) = q_chars.peek()
            && ch == target
        {
            q_chars.next();
            score += 10;
            if prev_matched {
                score += 8;
            }
            if prev_char == ' ' || prev_char == '-' || prev_char == '_' || prev_char == '/' {
                score += 6;
            }
            prev_matched = true;
        } else {
            prev_matched = false;
        }
        prev_char = ch;
    }
    if q_chars.peek().is_none() {
        Some(score)
    } else {
        None
    }
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
            InputEvent::Change(_) => {
                this.selected_idx = 0;
                cx.notify();
            }
            InputEvent::Submit(_) => {
                this.execute_selected(cx);
            }
            _ => {}
        })
        .detach();

        Self {
            focus_handle: cx.focus_handle(),
            search_input,
            scroll_handle: ScrollHandle::new(),
            open: false,
            items,
            selected_idx: 0,
            placeholder: "Type a command or search...".into(),
            empty_message: "No matching commands found.".into(),
            backdrop_blur: px(8.),
            max_width: px(580.),
            panel_style: StyleRefinement::default(),
        }
    }

    /// Sets the search input placeholder.
    pub fn placeholder(mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        let s: SharedString = text.into();
        self.placeholder = s.clone();
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
        self.backdrop_blur = radius.into();
        self
    }

    /// Sets the maximum palette dialog width (default `580px`).
    pub fn max_width(mut self, width: Pixels) -> Self {
        self.max_width = width;
        self
    }

    /// Replaces the command catalog dynamically.
    pub fn set_items(&mut self, items: Vec<CommandItem>, cx: &mut Context<Self>) {
        self.items = items;
        self.selected_idx = 0;
        cx.notify();
    }

    /// Returns whether the palette is currently open.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Opens the command palette, clears any prior query, and focuses the search input.
    pub fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open {
            self.open = true;
            self.selected_idx = 0;
            self.search_input.update(cx, |inp, cx| inp.set_value("", cx));
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
            cx.emit(CommandPaletteEvent::Close);
            cx.notify();
        }
    }

    /// Toggles the command palette open or closed.
    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open {
            self.close(cx);
        } else {
            self.open(window, cx);
        }
    }

    /// Returns the currently filtered command indices in display order.
    pub fn filtered_indices(&self, cx: &App) -> Vec<usize> {
        let query = self.search_input.read(cx).value().to_string();
        if query.trim().is_empty() {
            return (0..self.items.len()).collect();
        }
        let mut scored: Vec<(i32, usize)> = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(idx, item)| {
                let title_score = fuzzy_match_score(&query, &item.title);
                let sub_score = item
                    .subtitle
                    .as_deref()
                    .and_then(|s| fuzzy_match_score(&query, s))
                    .map(|s| s - 15);
                let kw_score = (!item.keywords.is_empty())
                    .then(|| fuzzy_match_score(&query, &item.keywords))
                    .flatten()
                    .map(|s| s - 20);
                let grp_score = fuzzy_match_score(&query, &item.group).map(|s| s - 25);
                let best = [title_score, sub_score, kw_score, grp_score]
                    .into_iter()
                    .flatten()
                    .max()?;
                Some((best, idx))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        scored.into_iter().map(|(_, idx)| idx).collect()
    }

    fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let filtered = self.filtered_indices(cx);
        if filtered.is_empty() {
            self.selected_idx = 0;
            return;
        }
        let len = filtered.len() as isize;
        let mut next = (self.selected_idx as isize + delta).rem_euclid(len) as usize;
        // Skip disabled items if possible
        for _ in 0..filtered.len() {
            if !self.items[filtered[next]].disabled {
                break;
            }
            next = (next as isize + delta.signum()).rem_euclid(len) as usize;
        }
        self.selected_idx = next;
        cx.notify();
    }

    fn execute_at(&mut self, filtered_pos: usize, cx: &mut Context<Self>) {
        let filtered = self.filtered_indices(cx);
        let Some(&item_idx) = filtered.get(filtered_pos) else {
            return;
        };
        let item = &self.items[item_idx];
        if item.disabled {
            return;
        }
        let id = item.id.clone();
        self.open = false;
        cx.emit(CommandPaletteEvent::Execute(id));
        cx.emit(CommandPaletteEvent::Close);
        cx.notify();
    }

    fn execute_selected(&mut self, cx: &mut Context<Self>) {
        self.execute_at(self.selected_idx, cx);
    }

    fn on_key_down(
        &mut self,
        event: &KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.open {
            return;
        }
        match event.keystroke.key.as_str() {
            "escape" => {
                self.close(cx);
                cx.stop_propagation();
            }
            "down" => {
                self.move_selection(1, cx);
                cx.stop_propagation();
            }
            "up" => {
                self.move_selection(-1, cx);
                cx.stop_propagation();
            }
            "enter" => {
                self.execute_selected(cx);
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
        let filtered = self.filtered_indices(cx);
        if !filtered.is_empty() && self.selected_idx >= filtered.len() {
            self.selected_idx = 0;
        }

        // Preserve section grouping order while keeping filtered items together by group.
        let mut group_order: Vec<SharedString> = Vec::new();
        let mut grouped: Vec<(SharedString, Vec<(usize, usize)>)> = Vec::new();
        for (filtered_pos, &item_idx) in filtered.iter().enumerate() {
            let group_name = self.items[item_idx].group.clone();
            if let Some(pos) = group_order.iter().position(|g| g == &group_name) {
                grouped[pos].1.push((filtered_pos, item_idx));
            } else {
                group_order.push(group_name.clone());
                grouped.push((group_name, vec![(filtered_pos, item_idx)]));
            }
        }

        let mut list = div()
            .id("cmd-palette-scroll")
            .track_scroll(&self.scroll_handle)
            .flex_1()
            .max_h(px(360.))
            .overflow_y_scroll()
            .p(px(8.));

        if grouped.is_empty() {
            list = list.child(
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
                    ),
            );
        } else {
            let mut inner = div().flex().flex_col().gap(px(4.));
            for (group_idx, (group_name, entries)) in grouped.into_iter().enumerate() {
                inner = inner.child(
                    div()
                        .px(px(10.))
                        .pt(if group_idx == 0 { px(4.) } else { px(10.) })
                        .pb(px(3.))
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.muted_foreground)
                        .child(group_name),
                );
                for (filtered_pos, item_idx) in entries {
                    let item = &self.items[item_idx];
                    let is_selected = filtered_pos == self.selected_idx;
                    let disabled = item.disabled;
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
                        theme.primary
                    } else {
                        theme.muted_foreground
                    };
                    let hover_bg = theme.surface_hover;

                    inner = inner.child(
                        div()
                            .id(("cmd-item", item_idx))
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .gap(px(12.))
                            .w_full()
                            .px(px(10.))
                            .py(px(8.))
                            .rounded(theme.radius_md)
                            .bg(row_bg)
                            .when(!disabled, |el| {
                                el.cursor_pointer()
                                    .when(!is_selected, |el| el.hover(move |s| s.bg(hover_bg)))
                                    .on_mouse_move(cx.listener(move |this, _, _, cx| {
                                        if this.selected_idx != filtered_pos {
                                            this.selected_idx = filtered_pos;
                                            cx.notify();
                                        }
                                    }))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.execute_at(filtered_pos, cx);
                                    }))
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
                                        el.child(Icon::new(ic).size(IconSize::Md).color(icon_fg))
                                    })
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(px(1.))
                                            .min_w_0()
                                            .child(
                                                div()
                                                    .text_size(px(13.5))
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(title_fg)
                                                    .child(item.title.clone()),
                                            )
                                            .when_some(item.subtitle.clone(), |el, sub| {
                                                el.child(
                                                    div()
                                                        .text_size(px(12.))
                                                        .text_color(theme.muted_foreground)
                                                        .child(sub),
                                                )
                                            }),
                                    ),
                            )
                            .when(!item.shortcut.is_empty(), |el| {
                                el.child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(3.))
                                        .children(
                                            item.shortcut.iter().cloned().map(kbd),
                                        ),
                                )
                            }),
                    );
                }
            }
            list = list.child(inner);
        }

        let mut panel = div()
            .id("cmd-palette-panel")
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key_down))
            .flex()
            .flex_col()
            .w_full()
            .max_w(self.max_width)
            .rounded(theme.radius_xl)
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .border_1()
            .border_color(theme.border)
            .when(theme.elevation_shadows, |el| el.shadow_2xl())
            .overflow_hidden()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            // Search header
            .child(
                div()
                    .p(px(10.))
                    .border_b_1()
                    .border_color(theme.border)
                    .child(Input::new(&self.search_input).border_0().shadow_none()),
            )
            // Grouped results
            .child(list)
            // Keyboard hint footer
            .child(
                div()
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
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(4.))
                                    .child(kbd("Esc"))
                                    .child("Close"),
                            ),
                    )
                    .child(format!("{} results", filtered.len())),
            );

        crate::Refineable::refine(panel.style(), &self.panel_style);

        let animated_panel = if reduce_motion {
            panel.into_any_element()
        } else {
            panel
                .with_animation(
                    "cmd-palette-enter",
                    Animation::new(ANIM_FAST).with_easing(ease_out_quint()),
                    |el, delta| el.opacity(delta).mt(px(12.0 * (1.0 - delta))),
                )
                .into_any_element()
        };

        let backdrop_color = theme.backdrop;
        let backdrop_blur = self.backdrop_blur;

        let overlay = div()
            .id("cmd-palette-backdrop")
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .flex_col()
            .items_center()
            .pt(px(84.))
            .px(px(20.))
            .on_click(cx.listener(|this, _, _, cx| this.close(cx)))
            .child(animated_panel);

        let final_overlay = if reduce_motion {
            overlay
                .bg(backdrop_color)
                .when(backdrop_blur > px(0.), |el| el.backdrop_blur(backdrop_blur))
                .into_any_element()
        } else {
            overlay
                .with_animation(
                    "cmd-palette-backdrop-anim",
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
        if self.style != StyleRefinement::default() {
            let style = self.style;
            self.state.update(cx, |s, _| {
                s.panel_style = style;
            });
        }
        self.state
    }
}
