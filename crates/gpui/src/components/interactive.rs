//! High-craft interactive micro-components:
//! - [`Rating`]: interactive star/icon rating control with live hover preview & clear-on-reclick.
//! - [`CopyButton`]: one-click clipboard copy button with timed `"Copied!"` checkmark feedback.
//! - [`PinInput`] / [`PinInputState`]: segmented OTP / verification code input with auto-advance,
//!   backspace navigation, clipboard paste, and completion events.

use std::rc::Rc;
use std::time::Duration;

use crate::{
    App, ClipboardItem, Context, ElementId, Entity, EventEmitter, FocusHandle, Focusable,
    FontWeight, Hsla, IntoElement, KeyDownEvent, MouseButton, Render, RenderOnce, SharedString,
    StyleRefinement, Styled, Task, Window, div, prelude::*, px,
};
use crate::icons::{Icon, IconSize, LucideIcon};

use super::button::ButtonVariant;
use super::layout::focus_ring_shadow;
use super::palette as tw;
use super::theme::{ActiveTheme, ComponentSize, tabular_numerals};

// ============================================================================
// Rating
// ============================================================================

#[derive(Default)]
struct RatingHoverState {
    hovered: Option<usize>,
}

/// Interactive star (or custom icon) rating control with live hover preview.
#[derive(IntoElement)]
pub struct Rating {
    id: ElementId,
    value: usize,
    max: usize,
    icon: LucideIcon,
    size: IconSize,
    color: Option<Hsla>,
    allow_clear: bool,
    read_only: bool,
    show_value: bool,
    on_change: Option<Rc<dyn Fn(&usize, &mut Window, &mut App) + 'static>>,
    style: StyleRefinement,
}

/// Convenience constructor for [`Rating::new`].
pub fn rating(id: impl Into<ElementId>, value: usize) -> Rating {
    Rating::new(id, value)
}

impl Rating {
    /// Creates an interactive 5-star [`Rating`] control.
    pub fn new(id: impl Into<ElementId>, value: usize) -> Self {
        Self {
            id: id.into(),
            value,
            max: 5,
            icon: LucideIcon::Star,
            size: IconSize::Lg,
            color: None,
            allow_clear: true,
            read_only: false,
            show_value: false,
            on_change: None,
            style: StyleRefinement::default(),
        }
    }

    /// Sets the maximum rating count (default `5`).
    pub fn max(mut self, max: usize) -> Self {
        self.max = max.clamp(1, 10);
        self
    }

    /// Overrides the rating icon (default [`LucideIcon::Star`]; try `Heart`, `Flame`, `Zap`).
    pub fn icon(mut self, icon: LucideIcon) -> Self {
        self.icon = icon;
        self
    }

    /// Sets the icon size (default `IconSize::Lg`).
    pub fn size(mut self, size: impl Into<IconSize>) -> Self {
        self.size = size.into();
        self
    }

    /// Overrides the filled icon color (defaults to Tailwind Amber 400).
    pub fn color(mut self, color: impl Into<Hsla>) -> Self {
        self.color = Some(color.into());
        self
    }

    /// Allows clicking the currently active star again to reset the rating to `0` (default `true`).
    pub fn allow_clear(mut self, allow: bool) -> Self {
        self.allow_clear = allow;
        self
    }

    /// Makes the rating display-only without hover or click interaction.
    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    /// Displays a numeric `"4 / 5"` readout next to the stars.
    pub fn show_value(mut self, show: bool) -> Self {
        self.show_value = show;
        self
    }

    /// Registers a callback invoked with the newly selected rating (`0..=max`).
    pub fn on_change(
        mut self,
        handler: impl Fn(&usize, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

impl Styled for Rating {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Rating {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let hover_state = window.use_keyed_state(
            (self.id.clone(), "rating-hover"),
            cx,
            |_, _| RatingHoverState::default(),
        );
        let hovered = if self.read_only {
            None
        } else {
            hover_state.read(cx).hovered
        };
        let display_val = hovered.unwrap_or(self.value).min(self.max);
        let active_color = self.color.unwrap_or_else(|| Hsla::from(tw::AMBER_400));
        let inactive_color = theme.border;

        let mut row = div()
            .id(self.id.clone())
            .flex()
            .flex_row()
            .items_center()
            .gap(px(4.));

        for idx in 1..=self.max {
            let filled = idx <= display_val;
            let color = if filled { active_color } else { inactive_color };
            let hs = hover_state.clone();
            let on_change = self.on_change.clone();
            let current = self.value;
            let allow_clear = self.allow_clear;

            let star = div()
                .id((self.id.clone(), format!("star-{idx}")))
                .flex()
                .items_center()
                .justify_center()
                .p(px(2.))
                .rounded(theme.radius_sm)
                .when(!self.read_only, |el| {
                    el.cursor_pointer()
                        .on_hover(move |is_hovered, _, cx| {
                            hs.update(cx, |st, cx| {
                                if *is_hovered {
                                    if st.hovered != Some(idx) {
                                        st.hovered = Some(idx);
                                        cx.notify();
                                    }
                                } else if st.hovered == Some(idx) {
                                    st.hovered = None;
                                    cx.notify();
                                }
                            });
                        })
                        .when_some(on_change, |el, cb| {
                            el.on_click(move |_, window, cx| {
                                let next = if allow_clear && current == idx { 0 } else { idx };
                                cb(&next, window, cx);
                            })
                        })
                })
                .child(Icon::new(self.icon).size(self.size).color(color));

            row = row.child(star);
        }

        if self.show_value {
            row = row.child(
                div()
                    .ml(px(6.))
                    .text_size(px(13.))
                    .font_weight(FontWeight::MEDIUM)
                    .font_features(tabular_numerals())
                    .text_color(theme.muted_foreground)
                    .child(format!("{display_val} / {}", self.max)),
            );
        }

        crate::Refineable::refine(row.style(), &self.style);
        row
    }
}

// ============================================================================
// CopyButton
// ============================================================================

struct CopyButtonState {
    copied: bool,
    _reset_task: Option<Task<()>>,
}

/// Interactive one-click clipboard copy button that writes `value` to the OS clipboard and
/// temporarily switches its icon/label to a green `"Copied!"` confirmation state.
#[derive(IntoElement)]
pub struct CopyButton {
    id: ElementId,
    value: SharedString,
    label: Option<SharedString>,
    copied_label: SharedString,
    icon_only: bool,
    size: ComponentSize,
    variant: ButtonVariant,
    reset_after: Duration,
    on_copy: Option<Rc<dyn Fn(&SharedString, &mut Window, &mut App) + 'static>>,
    style: StyleRefinement,
}

/// Convenience constructor for [`CopyButton::new`].
pub fn copy_button(id: impl Into<ElementId>, value: impl Into<SharedString>) -> CopyButton {
    CopyButton::new(id, value)
}

impl CopyButton {
    /// Creates a [`CopyButton`] that copies `value` to the system clipboard on click.
    pub fn new(id: impl Into<ElementId>, value: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            value: value.into(),
            label: Some("Copy".into()),
            copied_label: "Copied!".into(),
            icon_only: false,
            size: ComponentSize::Sm,
            variant: ButtonVariant::Secondary,
            reset_after: Duration::from_millis(1800),
            on_copy: None,
            style: StyleRefinement::default(),
        }
    }

    /// Sets the idle button label (default `"Copy"`).
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self.icon_only = false;
        self
    }

    /// Sets the temporary confirmation label after copying (default `"Copied!"`).
    pub fn copied_label(mut self, label: impl Into<SharedString>) -> Self {
        self.copied_label = label.into();
        self
    }

    /// Renders a compact square icon-only copy button with no text label.
    pub fn icon_only(mut self) -> Self {
        self.icon_only = true;
        self.label = None;
        self
    }

    /// Sets button size (default `ComponentSize::Sm`).
    pub fn size(mut self, size: ComponentSize) -> Self {
        self.size = size;
        self
    }

    /// Sets button visual variant (default `ButtonVariant::Secondary`).
    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Sets how long the `"Copied!"` confirmation persists before reverting (default `1.8s`).
    pub fn reset_after(mut self, duration: Duration) -> Self {
        self.reset_after = duration;
        self
    }

    /// Registers an optional callback invoked with the copied string right after writing to clipboard.
    pub fn on_copy(
        mut self,
        handler: impl Fn(&SharedString, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_copy = Some(Rc::new(handler));
        self
    }
}

impl Styled for CopyButton {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for CopyButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let state = window.use_keyed_state(
            (self.id.clone(), "copy-btn-state"),
            cx,
            |_, _| CopyButtonState {
                copied: false,
                _reset_task: None,
            },
        );
        let copied = state.read(cx).copied;
        let value = self.value;
        let reset_after = self.reset_after;
        let on_copy = self.on_copy;

        let height = self.size.height();
        let font_size = self.size.font_size();
        let icon_size = self.size.icon_size();

        let (bg, hover_bg, fg, border_color) = match self.variant {
            ButtonVariant::Primary => (
                theme.primary,
                theme.primary_hover,
                theme.primary_foreground,
                crate::transparent_black(),
            ),
            ButtonVariant::Ghost => (
                crate::transparent_black(),
                theme.surface_hover,
                theme.foreground,
                crate::transparent_black(),
            ),
            ButtonVariant::Soft => (
                theme.primary_soft,
                theme.primary_soft,
                theme.primary_soft_foreground,
                crate::transparent_black(),
            ),
            _ => (
                theme.secondary,
                theme.secondary_hover,
                theme.secondary_foreground,
                theme.input_border,
            ),
        };

        let icon_kind = if copied {
            LucideIcon::Check
        } else {
            LucideIcon::Copy
        };
        let icon_color = if copied && self.variant != ButtonVariant::Primary {
            theme.success
        } else {
            fg
        };

        let text_label = if self.icon_only {
            None
        } else if copied {
            Some(self.copied_label)
        } else {
            self.label
        };

        let mut btn = div()
            .id(self.id)
            .flex()
            .flex_row()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .h(height)
            .when(self.icon_only, |el| el.w(height))
            .when(!self.icon_only, |el| el.px(self.size.px_x()))
            .rounded(theme.radius_md)
            .bg(bg)
            .border_1()
            .border_color(if copied && self.variant == ButtonVariant::Secondary {
                theme.success.opacity(0.6)
            } else {
                border_color
            })
            .text_size(font_size)
            .font_weight(FontWeight::MEDIUM)
            .text_color(if copied && self.variant != ButtonVariant::Primary {
                theme.success
            } else {
                fg
            })
            .cursor_pointer()
            .hover(move |s| s.bg(hover_bg))
            .on_click(move |_, window, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(value.to_string()));
                if let Some(ref cb) = on_copy {
                    cb(&value, window, cx);
                }
                let state_weak = state.downgrade();
                state.update(cx, |s, cx| {
                    s.copied = true;
                    s._reset_task = Some(cx.spawn(async move |_, cx| {
                        cx.background_executor().timer(reset_after).await;
                        let _ = state_weak.update(cx, |s, cx| {
                            s.copied = false;
                            cx.notify();
                        });
                    }));
                    cx.notify();
                });
            })
            .child(Icon::new(icon_kind).size(icon_size).color(icon_color))
            .when_some(text_label, |el, lbl| el.child(lbl));

        crate::Refineable::refine(btn.style(), &self.style);
        btn
    }
}

// ============================================================================
// PinInput (Segmented OTP / Verification Code Input)
// ============================================================================

/// Events emitted by [`PinInputState`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PinInputEvent {
    /// Emitted whenever any slot character changes.
    Change(SharedString),
    /// Emitted once every slot is filled with a valid character.
    Complete(SharedString),
}

/// Entity state backing a segmented [`PinInput`] (OTP / 2FA / verification code input).
pub struct PinInputState {
    focus_handle: FocusHandle,
    length: usize,
    chars: Vec<Option<char>>,
    active_index: usize,
    alphanumeric: bool,
    masked: bool,
    invalid: bool,
    disabled: bool,
    size: ComponentSize,
    frame_style: StyleRefinement,
}

impl EventEmitter<PinInputEvent> for PinInputState {}

impl Focusable for PinInputState {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl PinInputState {
    /// Creates a numeric [`PinInputState`] with `length` slots (clamped to `2..=12`, commonly `4` or `6`).
    pub fn new(length: usize, cx: &mut Context<Self>) -> Self {
        let len = length.clamp(2, 12);
        Self {
            focus_handle: cx.focus_handle(),
            length: len,
            chars: vec![None; len],
            active_index: 0,
            alphanumeric: false,
            masked: false,
            invalid: false,
            disabled: false,
            size: ComponentSize::Lg,
            frame_style: StyleRefinement::default(),
        }
    }

    /// Allows ASCII letters as well as digits (default `false`, digits only).
    pub fn alphanumeric(mut self, allow: bool) -> Self {
        self.alphanumeric = allow;
        self
    }

    /// Masks entered characters as `"•"` dots (for secure PIN entry).
    pub fn masked(mut self, masked: bool) -> Self {
        self.masked = masked;
        self
    }

    /// Marks all slots invalid with a destructive border & focus ring.
    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    /// Disables keyboard and pointer input.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Sets slot box size (`Sm`, `Md`, `Lg`, `Xl`).
    pub fn size(mut self, size: ComponentSize) -> Self {
        self.size = size;
        self
    }

    /// Returns the concatenated non-empty characters entered so far.
    pub fn value(&self) -> String {
        self.chars.iter().flatten().collect()
    }

    /// Returns whether every slot is filled.
    pub fn is_complete(&self) -> bool {
        self.chars.iter().all(Option::is_some)
    }

    /// Replaces the current slots from `text` (filtering to valid characters).
    pub fn set_value(&mut self, text: &str, cx: &mut Context<Self>) {
        self.chars.fill(None);
        let mut idx = 0;
        for ch in text.chars() {
            if idx >= self.length {
                break;
            }
            if let Some(valid) = self.normalize_char(ch) {
                self.chars[idx] = Some(valid);
                idx += 1;
            }
        }
        self.active_index = idx.min(self.length - 1);
        self.emit_after_mutation(cx);
    }

    /// Clears all slots and moves the caret back to slot `0`.
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.chars.fill(None);
        self.active_index = 0;
        self.emit_after_mutation(cx);
    }

    /// Updates the invalid state dynamically.
    pub fn set_invalid(&mut self, invalid: bool, cx: &mut Context<Self>) {
        self.invalid = invalid;
        cx.notify();
    }

    /// Toggles PIN character masking (`"•"`) dynamically.
    pub fn set_masked(&mut self, masked: bool, cx: &mut Context<Self>) {
        self.masked = masked;
        cx.notify();
    }

    fn normalize_char(&self, ch: char) -> Option<char> {
        if ch.is_ascii_digit() {
            Some(ch)
        } else if self.alphanumeric && ch.is_ascii_alphanumeric() {
            Some(ch.to_ascii_uppercase())
        } else {
            None
        }
    }

    fn emit_after_mutation(&mut self, cx: &mut Context<Self>) {
        let val: SharedString = self.value().into();
        cx.emit(PinInputEvent::Change(val.clone()));
        if self.is_complete() {
            cx.emit(PinInputEvent::Complete(val));
        }
        cx.notify();
    }

    fn paste_text(&mut self, text: &str, cx: &mut Context<Self>) {
        let mut cursor = self.active_index;
        let mut changed = false;
        for ch in text.chars() {
            if cursor >= self.length {
                break;
            }
            if let Some(valid) = self.normalize_char(ch) {
                self.chars[cursor] = Some(valid);
                cursor += 1;
                changed = true;
            }
        }
        if changed {
            self.active_index = cursor.min(self.length - 1);
            self.emit_after_mutation(cx);
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        let mods = event.keystroke.modifiers;
        let key = event.keystroke.key.as_str();

        // Support Cmd+V / Ctrl+V paste of multi-digit verification codes
        if (mods.platform || mods.control) && key.eq_ignore_ascii_case("v") {
            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                self.paste_text(&text, cx);
            }
            cx.stop_propagation();
            return;
        }

        if mods.control || mods.platform || mods.alt {
            return;
        }

        match key {
            "backspace" => {
                if self.chars[self.active_index].is_some() {
                    self.chars[self.active_index] = None;
                } else if self.active_index > 0 {
                    self.active_index -= 1;
                    self.chars[self.active_index] = None;
                }
                self.emit_after_mutation(cx);
                cx.stop_propagation();
            }
            "delete" => {
                self.chars[self.active_index] = None;
                self.emit_after_mutation(cx);
                cx.stop_propagation();
            }
            "left" => {
                self.active_index = self.active_index.saturating_sub(1);
                cx.notify();
                cx.stop_propagation();
            }
            "right" => {
                self.active_index = (self.active_index + 1).min(self.length - 1);
                cx.notify();
                cx.stop_propagation();
            }
            "home" => {
                self.active_index = 0;
                cx.notify();
                cx.stop_propagation();
            }
            "end" => {
                self.active_index = self.length - 1;
                cx.notify();
                cx.stop_propagation();
            }
            _ => {
                let candidate = event
                    .keystroke
                    .key_char
                    .as_deref()
                    .and_then(|s| s.chars().next())
                    .or_else(|| (key.len() == 1).then(|| key.chars().next()).flatten());
                if let Some(ch) = candidate.and_then(|c| self.normalize_char(c)) {
                    self.chars[self.active_index] = Some(ch);
                    if self.active_index + 1 < self.length {
                        self.active_index += 1;
                    }
                    self.emit_after_mutation(cx);
                    cx.stop_propagation();
                }
            }
        }
    }
}

impl Render for PinInputState {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let focused = self.focus_handle.is_focused(window);
        let (box_w, box_h, font_px) = match self.size {
            ComponentSize::Xs => (px(30.), px(34.), px(14.)),
            ComponentSize::Sm => (px(36.), px(40.), px(16.)),
            ComponentSize::Md => (px(42.), px(46.), px(18.)),
            ComponentSize::Lg => (px(46.), px(52.), px(20.)),
            ComponentSize::Xl => (px(52.), px(58.), px(22.)),
        };
        let split_gap_after = (self.length == 6).then_some(2);

        let mut root = div()
            .id("pin-input-root")
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key_down))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .when(self.disabled, |el| el.opacity(0.55));

        for idx in 0..self.length {
            let is_active = focused && self.active_index == idx && !self.disabled;
            let val = self.chars[idx];
            let border_color = if self.invalid {
                theme.destructive
            } else if is_active {
                theme.ring
            } else if val.is_some() {
                theme.input_border_hover
            } else {
                theme.input_border
            };

            let display = match val {
                Some(_) if self.masked => "•".to_string(),
                Some(ch) => ch.to_string(),
                None => String::new(),
            };

            let cell = div()
                .id(("pin-slot", idx))
                .flex()
                .items_center()
                .justify_center()
                .w(box_w)
                .h(box_h)
                .rounded(theme.radius_lg)
                .border_1()
                .border_color(border_color)
                .bg(if self.disabled {
                    theme.disabled_bg
                } else {
                    theme.input_bg
                })
                .text_size(font_px)
                .font_weight(FontWeight::SEMIBOLD)
                .font_features(tabular_numerals())
                .text_color(theme.foreground)
                .when(is_active, |el| {
                    let ring = focus_ring_shadow(&theme, self.invalid);
                    el.when(!ring.is_empty(), |el| el.shadow(ring))
                })
                .when(!self.disabled, |el| {
                    el.cursor_text()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, window, cx| {
                                this.active_index = idx;
                                window.focus(&this.focus_handle, cx);
                                cx.notify();
                            }),
                        )
                })
                .child(if !display.is_empty() {
                    div().child(display).into_any_element()
                } else if is_active {
                    div()
                        .w(px(2.))
                        .h(px(20.))
                        .rounded_full()
                        .bg(theme.primary)
                        .into_any_element()
                } else {
                    div()
                        .w(px(6.))
                        .h(px(6.))
                        .rounded_full()
                        .bg(theme.border)
                        .into_any_element()
                });

            root = root.child(cell);

            if split_gap_after == Some(idx) {
                root = root.child(
                    div()
                        .w(px(8.))
                        .h(px(2.))
                        .rounded_full()
                        .bg(theme.border),
                );
            }
        }

        crate::Refineable::refine(root.style(), &self.frame_style);
        root
    }
}

/// Renderable segmented OTP / verification code input wrapping [`Entity<PinInputState>`].
#[derive(IntoElement)]
pub struct PinInput {
    state: Entity<PinInputState>,
    style: StyleRefinement,
}

/// Convenience constructor for [`PinInput::new`].
pub fn pin_input(state: &Entity<PinInputState>) -> PinInput {
    PinInput::new(state)
}

impl PinInput {
    /// Creates a [`PinInput`] element bound to `state`.
    pub fn new(state: &Entity<PinInputState>) -> Self {
        Self {
            state: state.clone(),
            style: StyleRefinement::default(),
        }
    }
}

impl Styled for PinInput {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for PinInput {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        if self.state.read(cx).frame_style != self.style {
            let style = self.style;
            self.state.update(cx, |s, cx| {
                s.frame_style = style;
                cx.notify();
            });
        }
        self.state
    }
}
