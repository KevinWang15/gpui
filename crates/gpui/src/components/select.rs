//! Tailwind-inspired [`Select`] / [`SelectState`] dropdown listbox with keyboard navigation,
//! option groups, icons, descriptions, disabled options, clearable button, and anchored popover.

use std::cell::Cell;
use std::rc::Rc;

use crate::{
    Anchor, Animation, AnimationExt, App, Bounds, Context, Entity, EventEmitter, FocusHandle,
    Focusable, FontWeight, IntoElement, KeyDownEvent, MouseButton, Pixels, Render, RenderOnce,
    SharedString, StyleRefinement, Styled, Window, anchored, canvas, deferred, div,
    ease_out_quint, point, prelude::*, px,
};
use crate::icons::{Icon, IconSize, LucideIcon};

use super::layout::focus_ring_shadow;
use super::theme::{ANIM_FAST, ActiveTheme, ComponentSize};

/// A selectable option inside a [`Select`] control.
#[derive(Clone, Debug)]
pub struct SelectOption<T> {
    /// Backing value for this option.
    pub value: T,
    /// Display label.
    pub label: SharedString,
    /// Optional muted subtitle.
    pub description: Option<SharedString>,
    /// Optional leading [`LucideIcon`].
    pub icon: Option<LucideIcon>,
    /// Optional section group header name.
    pub group: Option<SharedString>,
    /// Whether this option cannot be chosen.
    pub disabled: bool,
}

impl<T> SelectOption<T> {
    /// Creates a new select option with `value` and `label`.
    pub fn new(value: T, label: impl Into<SharedString>) -> Self {
        Self {
            value,
            label: label.into(),
            description: None,
            icon: None,
            group: None,
            disabled: false,
        }
    }

    /// Sets a muted description line shown below the label.
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Sets a leading [`LucideIcon`].
    pub fn icon(mut self, icon: LucideIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Assigns this option to a named visual group section.
    pub fn group(mut self, group: impl Into<SharedString>) -> Self {
        self.group = Some(group.into());
        self
    }

    /// Disables selection of this option.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

/// Events emitted by [`SelectState`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SelectEvent<T> {
    /// Emitted whenever the selected value changes (or is cleared to `None`).
    Change(Option<T>),
    /// Emitted when the dropdown opens.
    Open,
    /// Emitted when the dropdown closes.
    Close,
}

/// Entity state backing a Tailwind-style [`Select`] listbox.
pub struct SelectState<T: Clone + PartialEq + 'static> {
    focus_handle: FocusHandle,
    options: Vec<SelectOption<T>>,
    selected_index: Option<usize>,
    highlighted_index: Option<usize>,
    open: bool,
    placeholder: SharedString,
    size: ComponentSize,
    disabled: bool,
    invalid: bool,
    clearable: bool,
    max_menu_height: Pixels,
    trigger_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    frame_style: StyleRefinement,
}

impl<T: Clone + PartialEq + 'static> EventEmitter<SelectEvent<T>> for SelectState<T> {}

impl<T: Clone + PartialEq + 'static> Focusable for SelectState<T> {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl<T: Clone + PartialEq + 'static> SelectState<T> {
    /// Creates a new [`SelectState`] with the given options.
    pub fn new(options: Vec<SelectOption<T>>, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            options,
            selected_index: None,
            highlighted_index: None,
            open: false,
            placeholder: SharedString::new_static("Select an option..."),
            size: ComponentSize::Md,
            disabled: false,
            invalid: false,
            clearable: false,
            max_menu_height: px(280.),
            trigger_bounds: Rc::new(Cell::new(None)),
            frame_style: StyleRefinement::default(),
        }
    }

    /// Sets placeholder text when no option is selected.
    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Sets initial selected value if present in `options`.
    pub fn default_value(mut self, value: T) -> Self {
        self.selected_index = self.options.iter().position(|o| o.value == value);
        self
    }

    /// Sets control size.
    pub fn size(mut self, size: ComponentSize) -> Self {
        self.size = size;
        self
    }

    /// Disables user interaction.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Highlights the trigger with destructive error styling.
    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    /// Shows an inline clear (`X`) button when a value is selected.
    pub fn clearable(mut self, clearable: bool) -> Self {
        self.clearable = clearable;
        self
    }

    /// Sets the maximum scrollable height of the dropdown popover.
    pub fn max_menu_height(mut self, height: Pixels) -> Self {
        self.max_menu_height = height;
        self
    }

    /// Returns the currently selected option, if any.
    pub fn selected_option(&self) -> Option<&SelectOption<T>> {
        self.selected_index.and_then(|idx| self.options.get(idx))
    }

    /// Returns a reference to the currently selected value.
    pub fn selected_value(&self) -> Option<&T> {
        self.selected_option().map(|opt| &opt.value)
    }

    /// Returns the index of the currently selected option.
    pub fn selected_index(&self) -> Option<usize> {
        self.selected_index
    }

    /// Returns `true` if the dropdown menu is currently open.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Returns all options.
    pub fn options(&self) -> &[SelectOption<T>] {
        &self.options
    }

    /// Replaces the option list and preserves selection if the value still exists.
    pub fn set_options(&mut self, options: Vec<SelectOption<T>>, cx: &mut Context<Self>) {
        let prev_val = self.selected_value().cloned();
        self.options = options;
        self.selected_index = prev_val.and_then(|val| self.options.iter().position(|o| o.value == val));
        self.highlighted_index = self.selected_index;
        cx.notify();
    }

    /// Opens the dropdown menu.
    pub fn open_menu(&mut self, cx: &mut Context<Self>) {
        if self.disabled || self.open {
            return;
        }
        self.open = true;
        self.highlighted_index = self
            .selected_index
            .or_else(|| self.options.iter().position(|o| !o.disabled));
        cx.emit(SelectEvent::Open);
        cx.notify();
    }

    /// Closes the dropdown menu.
    pub fn close_menu(&mut self, cx: &mut Context<Self>) {
        if !self.open {
            return;
        }
        self.open = false;
        cx.emit(SelectEvent::Close);
        cx.notify();
    }

    /// Toggles the dropdown menu open/closed.
    pub fn toggle_menu(&mut self, cx: &mut Context<Self>) {
        if self.open {
            self.close_menu(cx);
        } else {
            self.open_menu(cx);
        }
    }

    /// Selects the option at `index`, closes the menu, and emits [`SelectEvent::Change`].
    pub fn select_index(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(opt) = self.options.get(index) else {
            return;
        };
        if opt.disabled {
            return;
        }
        self.selected_index = Some(index);
        self.highlighted_index = Some(index);
        let value = opt.value.clone();
        self.open = false;
        cx.emit(SelectEvent::Change(Some(value)));
        cx.emit(SelectEvent::Close);
        cx.notify();
    }

    /// Selects the first option matching `value`.
    pub fn select_value(&mut self, value: &T, cx: &mut Context<Self>) {
        if let Some(idx) = self.options.iter().position(|o| &o.value == value) {
            self.select_index(idx, cx);
        }
    }

    /// Clears the current selection and emits [`SelectEvent::Change`] with `None`.
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        if self.selected_index.is_none() {
            return;
        }
        self.selected_index = None;
        self.highlighted_index = None;
        cx.emit(SelectEvent::Change(None));
        cx.notify();
    }

    fn move_highlight(&mut self, delta: isize) {
        if self.options.is_empty() {
            return;
        }
        let len = self.options.len();
        let start = match self.highlighted_index {
            Some(idx) => idx,
            None => {
                self.highlighted_index = if delta > 0 {
                    self.options.iter().position(|o| !o.disabled)
                } else {
                    self.options.iter().rposition(|o| !o.disabled)
                };
                return;
            }
        };
        for step in 1..=len {
            let candidate = ((start as isize + delta * step as isize).rem_euclid(len as isize)) as usize;
            if !self.options[candidate].disabled {
                self.highlighted_index = Some(candidate);
                return;
            }
        }
    }

    fn on_key_down(
        &mut self,
        event: &KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return;
        }
        match event.keystroke.key.as_str() {
            "down" => {
                if !self.open {
                    self.open_menu(cx);
                } else {
                    self.move_highlight(1);
                    cx.notify();
                }
                cx.stop_propagation();
            }
            "up" => {
                if !self.open {
                    self.open_menu(cx);
                } else {
                    self.move_highlight(-1);
                    cx.notify();
                }
                cx.stop_propagation();
            }
            "home" if self.open => {
                self.highlighted_index = self.options.iter().position(|o| !o.disabled);
                cx.notify();
                cx.stop_propagation();
            }
            "end" if self.open => {
                self.highlighted_index = self.options.iter().rposition(|o| !o.disabled);
                cx.notify();
                cx.stop_propagation();
            }
            "enter" | "space" => {
                if !self.open {
                    self.open_menu(cx);
                } else if let Some(idx) = self.highlighted_index {
                    self.select_index(idx, cx);
                }
                cx.stop_propagation();
            }
            "escape" if self.open => {
                self.close_menu(cx);
                cx.stop_propagation();
            }
            _ => {
                // Single-character typeahead jump
                if let Some(ch) = event.keystroke.key_char.as_deref()
                    && ch.len() == 1
                    && !event.keystroke.modifiers.control
                    && !event.keystroke.modifiers.platform
                {
                    let needle = ch.to_lowercase();
                    let start = self.highlighted_index.map_or(0, |i| i + 1);
                    let len = self.options.len();
                    for offset in 0..len {
                        let idx = (start + offset) % len;
                        let opt = &self.options[idx];
                        if !opt.disabled && opt.label.to_lowercase().starts_with(&needle) {
                            self.highlighted_index = Some(idx);
                            if !self.open {
                                self.select_index(idx, cx);
                            } else {
                                cx.notify();
                            }
                            break;
                        }
                    }
                }
            }
        }
    }
}

impl<T: Clone + PartialEq + 'static> Render for SelectState<T> {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let focused = self.focus_handle.is_focused(window);
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
            theme.input_bg
        };

        let selected = self.selected_option().cloned();
        let show_clear = self.clearable && selected.is_some() && !self.disabled;
        let bounds_cell = self.trigger_bounds.clone();
        let captured_bounds = self.trigger_bounds.get();
        let menu_min_w = captured_bounds
            .map(|b| b.size.width)
            .unwrap_or(px(200.))
            .max(px(160.));

        let mut trigger = div()
            .id("gpui-select-trigger")
            .relative()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .w_full()
            .h(self.size.height())
            .px(self.size.px_x())
            .rounded(theme.radius_md)
            .bg(bg)
            .border_1()
            .border_color(border_color)
            .text_size(self.size.font_size())
            .when(!self.disabled, |el| {
                el.cursor_pointer()
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
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key_down))
            .when(!self.disabled, |el| {
                el.on_click(cx.listener(|this, _, window, cx| {
                    window.focus(&this.focus_handle, cx);
                    this.toggle_menu(cx);
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
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .flex_1()
                    .min_w_0()
                    .when_some(selected.as_ref().and_then(|s| s.icon), |el, icon| {
                        el.child(
                            Icon::new(icon)
                                .size(self.size.icon_size())
                                .color(theme.muted_foreground),
                        )
                    })
                    .child(
                        div()
                            .truncate()
                            .text_color(if selected.is_some() {
                                if self.disabled {
                                    theme.disabled_foreground
                                } else {
                                    theme.foreground
                                }
                            } else {
                                theme.placeholder
                            })
                            .child(
                                selected
                                    .as_ref()
                                    .map(|s| s.label.clone())
                                    .unwrap_or_else(|| self.placeholder.clone()),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(4.))
                    .when(show_clear, |el| {
                        let entity_clear = entity.clone();
                        el.child(
                            div()
                                .id("gpui-select-clear")
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
            let mut list_items = div().flex().flex_col().w_full().p(px(4.));

            let mut last_group: Option<SharedString> = None;
            for (index, option) in self.options.iter().enumerate() {
                if option.group != last_group
                    && let Some(group_name) = option.group.clone()
                {
                    list_items = list_items.child(
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

                let is_selected = self.selected_index == Some(index);
                let is_highlighted = self.highlighted_index == Some(index);
                let row_bg = if is_highlighted {
                    theme.surface_hover
                } else {
                    crate::transparent_black()
                };
                let entity_opt = entity.clone();

                list_items = list_items.child(
                    div()
                        .id(("gpui-select-option", index))
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap(px(10.))
                        .w_full()
                        .px(px(10.))
                        .py(px(7.))
                        .rounded(theme.radius_sm)
                        .bg(row_bg)
                        .when(!option.disabled, |el| {
                            el.cursor_pointer()
                                .hover(|s| s.bg(theme.surface_hover))
                                .on_click(move |_, _, cx| {
                                    entity_opt.update(cx, |state, cx| {
                                        state.select_index(index, cx);
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

            let list = div()
                .id("gpui-select-menu")
                .min_w(menu_min_w)
                .max_h(self.max_menu_height)
                .overflow_y_scroll()
                .rounded(theme.radius_lg)
                .bg(theme.popover)
                .text_color(theme.popover_foreground)
                .border_1()
                .border_color(theme.border)
                .shadow_lg()
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    this.close_menu(cx);
                }))
                .child(list_items)
                .with_animation(
                    "gpui-select-menu-anim",
                    Animation::new(ANIM_FAST).with_easing(ease_out_quint()),
                    |el, delta| el.opacity(delta).mt(px(-6.0 * (1.0 - delta))),
                );

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

            Some(deferred(anchor.child(list)).priority(20))
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

/// Renderable Tailwind-style select element wrapping an [`Entity<SelectState<T>>`].
#[derive(IntoElement)]
pub struct Select<T: Clone + PartialEq + 'static> {
    state: Entity<SelectState<T>>,
    style: StyleRefinement,
}

/// Convenience constructor for [`Select::new`].
pub fn select<T: Clone + PartialEq + 'static>(state: &Entity<SelectState<T>>) -> Select<T> {
    Select::new(state)
}

impl<T: Clone + PartialEq + 'static> Select<T> {
    /// Creates a [`Select`] element backed by `state`.
    pub fn new(state: &Entity<SelectState<T>>) -> Self {
        Self {
            state: state.clone(),
            style: StyleRefinement::default(),
        }
    }
}

impl<T: Clone + PartialEq + 'static> Styled for Select<T> {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl<T: Clone + PartialEq + 'static> RenderOnce for Select<T> {
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
