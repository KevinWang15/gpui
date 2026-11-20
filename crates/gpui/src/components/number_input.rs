//! Interactive numeric stepper input ([`NumberInput`] / [`NumberInputState`]) with
//! increment/decrement buttons, direct text editing, clamping, decimal precision, prefix/suffix
//! labels, and `Up`/`Down` (+ `Shift` 10×) keyboard stepping.

use crate::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, IntoElement,
    KeyDownEvent, Render, RenderOnce, SharedString, StyleRefinement, Styled, Window, div,
    prelude::*, px,
};
use crate::icons::{Icon, IconSize, LucideIcon};

use super::input::{Input, InputEvent, InputState};
use super::layout::focus_ring_shadow;
use super::theme::{ActiveTheme, ComponentSize, tabular_numerals};

/// Events emitted by [`NumberInputState`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NumberInputEvent {
    /// Emitted whenever the validated numeric value changes.
    Change(f64),
    /// Emitted when the user presses Enter to commit the numeric value.
    Submit(f64),
}

/// Entity state backing an interactive [`NumberInput`].
pub struct NumberInputState {
    focus_handle: FocusHandle,
    input: Entity<InputState>,
    value: f64,
    min: Option<f64>,
    max: Option<f64>,
    step: f64,
    precision: usize,
    explicit_precision: bool,
    prefix: Option<SharedString>,
    suffix: Option<SharedString>,
    size: ComponentSize,
    synced_size: ComponentSize,
    disabled: bool,
    invalid: bool,
    frame_style: StyleRefinement,
}

fn infer_step_precision(step: f64) -> usize {
    for p in 0..=6 {
        let factor = 10f64.powi(p as i32);
        let scaled = step * factor;
        if (scaled - scaled.round()).abs() < 1e-6 {
            return p;
        }
    }
    6
}

impl EventEmitter<NumberInputEvent> for NumberInputState {}

impl Focusable for NumberInputState {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl NumberInputState {
    /// Creates a new [`NumberInputState`] starting at `0` with `step = 1.0`.
    pub fn new(cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(cx).default_value("0"));
        cx.subscribe(&input, |this, _, ev: &InputEvent, cx| match ev {
            InputEvent::Change(text) => {
                if let Ok(parsed) = text.trim().parse::<f64>()
                    && parsed.is_finite()
                {
                    let in_min = this.min.is_none_or(|m| parsed >= m - 1e-9);
                    let in_max = this.max.is_none_or(|m| parsed <= m + 1e-9);
                    if in_min && in_max {
                        let normalized = this.normalize(parsed);
                        if (normalized - this.value).abs() > 1e-9 {
                            this.value = normalized;
                            cx.emit(NumberInputEvent::Change(normalized));
                            cx.notify();
                        }
                    }
                }
            }
            InputEvent::Blur => {
                this.commit_text(cx, false);
            }
            InputEvent::Submit(_) => {
                this.commit_text(cx, true);
            }
            InputEvent::Focus => {
                cx.notify();
            }
        })
        .detach();

        Self {
            focus_handle: cx.focus_handle(),
            input,
            value: 0.0,
            min: None,
            max: None,
            step: 1.0,
            precision: 0,
            explicit_precision: false,
            prefix: None,
            suffix: None,
            size: ComponentSize::Md,
            synced_size: ComponentSize::Md,
            disabled: false,
            invalid: false,
            frame_style: StyleRefinement::default(),
        }
    }

    /// Sets the initial value.
    pub fn default_value(mut self, value: f64, cx: &mut Context<Self>) -> Self {
        if value.is_finite() {
            self.value = self.normalize(value);
            self.sync_input_text(cx);
        }
        self
    }

    /// Sets minimum and maximum bounds.
    pub fn range(mut self, min: f64, max: f64, cx: &mut Context<Self>) -> Self {
        if min.is_finite() && max.is_finite() {
            let (lo, hi) = if min <= max { (min, max) } else { (max, min) };
            self.min = Some(lo);
            self.max = Some(hi);
            self.value = self.normalize(self.value);
            self.sync_input_text(cx);
        }
        self
    }

    /// Sets minimum allowed value.
    pub fn min(mut self, min: f64, cx: &mut Context<Self>) -> Self {
        if min.is_finite() {
            self.min = Some(min);
            if let Some(max) = self.max
                && max < min
            {
                self.max = Some(min);
            }
            self.value = self.normalize(self.value);
            self.sync_input_text(cx);
        }
        self
    }

    /// Sets maximum allowed value.
    pub fn max(mut self, max: f64, cx: &mut Context<Self>) -> Self {
        if max.is_finite() {
            self.max = Some(max);
            if let Some(min) = self.min
                && min > max
            {
                self.min = Some(max);
            }
            self.value = self.normalize(self.value);
            self.sync_input_text(cx);
        }
        self
    }

    /// Sets the increment/decrement step (default `1.0`).
    ///
    /// If [`Self::precision`] has not been explicitly set, decimal precision is automatically
    /// expanded to represent fractional steps cleanly (up to 6 decimal places).
    pub fn step(mut self, step: f64) -> Self {
        if step.is_finite() {
            self.step = step.abs().max(1e-6);
            if !self.explicit_precision {
                self.precision = self.precision.max(infer_step_precision(self.step));
            }
        }
        self
    }

    /// Sets decimal places shown when formatting (default `0`).
    pub fn precision(mut self, precision: usize, cx: &mut Context<Self>) -> Self {
        self.explicit_precision = true;
        self.precision = precision.min(6);
        self.value = self.normalize(self.value);
        self.sync_input_text(cx);
        self
    }

    /// Sets an inline leading prefix (e.g. `"$"`).
    pub fn prefix(mut self, prefix: impl Into<SharedString>) -> Self {
        self.prefix = Some(prefix.into());
        self
    }

    /// Sets an inline trailing unit suffix (e.g. `"px"`, `"%"`, `"ms"`).
    pub fn suffix(mut self, suffix: impl Into<SharedString>) -> Self {
        self.suffix = Some(suffix.into());
        self
    }

    /// Sets control size (`Xs`..`Xl`).
    pub fn size(mut self, size: ComponentSize) -> Self {
        self.size = size;
        self
    }

    /// Marks the control invalid (red border & focus ring).
    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    /// Disables editing and stepping.
    pub fn disabled(mut self, disabled: bool, cx: &mut Context<Self>) -> Self {
        self.disabled = disabled;
        self.input.update(cx, |inp, cx| inp.set_disabled(disabled, cx));
        self
    }

    /// Returns the current numeric value.
    pub fn value(&self) -> f64 {
        self.value
    }

    /// Programmatically sets the value (clamped & rounded to precision) and emits `Change`.
    pub fn set_value(&mut self, value: f64, cx: &mut Context<Self>) {
        if !value.is_finite() {
            self.sync_input_text(cx);
            return;
        }
        let clamped = self.normalize(value);
        let changed = (clamped - self.value).abs() > 1e-9;
        self.value = clamped;
        self.sync_input_text(cx);
        if changed {
            cx.emit(NumberInputEvent::Change(clamped));
        }
        cx.notify();
    }

    /// Increments the value by `step * multiplier`.
    pub fn increment(&mut self, multiplier: f64, cx: &mut Context<Self>) {
        if !self.disabled && multiplier.is_finite() {
            self.set_value(self.value + self.step * multiplier, cx);
        }
    }

    /// Decrements the value by `step * multiplier`.
    pub fn decrement(&mut self, multiplier: f64, cx: &mut Context<Self>) {
        if !self.disabled && multiplier.is_finite() {
            self.set_value(self.value - self.step * multiplier, cx);
        }
    }

    fn clamp(&self, raw: f64) -> f64 {
        let mut v = raw;
        if let Some(min) = self.min {
            v = v.max(min);
        }
        if let Some(max) = self.max {
            v = v.min(max);
        }
        v
    }

    fn round_precision(&self, v: f64) -> f64 {
        let factor = 10f64.powi(self.precision as i32);
        let scaled = v * factor;
        let rounded = if scaled.is_finite() {
            scaled.round() / factor
        } else {
            v
        };
        if rounded.abs() < 1e-12 { 0.0 } else { rounded }
    }

    fn normalize(&self, raw: f64) -> f64 {
        self.clamp(self.round_precision(self.clamp(raw)))
    }

    fn format_value(&self) -> String {
        format!("{:.*}", self.precision, self.value)
    }

    fn sync_input_text(&mut self, cx: &mut Context<Self>) {
        let formatted = self.format_value();
        self.input.update(cx, |inp, cx| {
            if inp.value() != formatted {
                inp.set_value_silent(formatted, cx);
            }
        });
    }

    fn commit_text(&mut self, cx: &mut Context<Self>, emit_submit: bool) {
        let raw = self.input.read(cx).value().trim().to_string();
        if let Ok(parsed) = raw.parse::<f64>()
            && parsed.is_finite()
        {
            self.set_value(parsed, cx);
        } else {
            self.sync_input_text(cx);
        }
        if emit_submit {
            cx.emit(NumberInputEvent::Submit(self.value));
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        let mods = &event.keystroke.modifiers;
        if mods.control || mods.alt || mods.platform {
            return;
        }
        let mult = if mods.shift { 10.0 } else { 1.0 };
        match event.keystroke.key.as_str() {
            "up" => {
                self.increment(mult, cx);
                cx.stop_propagation();
            }
            "down" => {
                self.decrement(mult, cx);
                cx.stop_propagation();
            }
            _ => {}
        }
    }
}

impl Render for NumberInputState {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.synced_size != self.size {
            self.synced_size = self.size;
            let size = self.size;
            self.input.update(cx, |inp, cx| inp.set_size(size, cx));
        }
        let theme = cx.theme();
        let focused = !self.disabled
            && (self.focus_handle.is_focused(window)
                || self.input.read(cx).focus_handle(cx).is_focused(window));
        let height = self.size.height();
        let font_size = self.size.font_size();
        let btn_w = match self.size {
            ComponentSize::Xs => px(26.),
            ComponentSize::Sm => px(30.),
            ComponentSize::Md => px(34.),
            ComponentSize::Lg => px(38.),
            ComponentSize::Xl => px(42.),
        };

        let at_min = self.min.is_some_and(|m| self.value <= m + 1e-9);
        let at_max = self.max.is_some_and(|m| self.value >= m - 1e-9);
        let can_dec = !self.disabled && !at_min;
        let can_inc = !self.disabled && !at_max;

        let border_color = if self.invalid {
            theme.destructive
        } else if focused {
            theme.ring
        } else {
            theme.input_border
        };
        let hover_bg = theme.surface_hover;
        let active_bg = theme.surface_active;

        let mut root = div()
            .id("number-input-shell")
            .when(!self.disabled, |el| el.track_focus(&self.focus_handle))
            .on_key_down(cx.listener(Self::on_key_down))
            .flex()
            .flex_row()
            .items_center()
            .w_full()
            .h(height)
            .rounded(theme.radius_md)
            .border_1()
            .border_color(border_color)
            .bg(if self.disabled {
                theme.disabled_bg
            } else {
                theme.input_bg
            })
            .overflow_hidden()
            .when(focused && !self.disabled, |el| {
                let ring = focus_ring_shadow(&theme, self.invalid);
                el.when(!ring.is_empty(), |el| el.shadow(ring))
            })
            .when(self.disabled, |el| el.opacity(0.6))
            // Decrement button
            .child(
                div()
                    .id("num-dec")
                    .flex()
                    .items_center()
                    .justify_center()
                    .flex_shrink_0()
                    .w(btn_w)
                    .h_full()
                    .border_r_1()
                    .border_color(theme.border)
                    .bg(theme.surface_muted.opacity(0.45))
                    .when(can_dec, |el| {
                        el.cursor_pointer()
                            .hover(move |s| s.bg(hover_bg))
                            .active(move |s| s.bg(active_bg))
                            .on_click(cx.listener(|this, ev: &crate::ClickEvent, _, cx| {
                                let mult = if ev.modifiers().shift { 10.0 } else { 1.0 };
                                this.decrement(mult, cx);
                            }))
                    })
                    .when(!can_dec, |el| el.opacity(0.38).cursor_not_allowed())
                    .child(
                        Icon::new(LucideIcon::Minus)
                            .size(IconSize::Sm)
                            .color(theme.foreground),
                    ),
            )
            // Optional prefix
            .when_some(self.prefix.clone(), |el, prefix| {
                el.child(
                    div()
                        .pl(px(10.))
                        .text_size(font_size)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.muted_foreground)
                        .child(prefix),
                )
            })
            // Numeric input core
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .items_center()
                    .font_features(tabular_numerals())
                    .child(
                        Input::new(&self.input)
                            .border_0()
                            .shadow_none()
                            .bg(crate::transparent_black())
                            .h_full()
                            .text_size(font_size),
                    ),
            )
            // Optional suffix
            .when_some(self.suffix.clone(), |el, suffix| {
                el.child(
                    div()
                        .pr(px(10.))
                        .text_size(font_size)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.muted_foreground)
                        .child(suffix),
                )
            })
            // Increment button
            .child(
                div()
                    .id("num-inc")
                    .flex()
                    .items_center()
                    .justify_center()
                    .flex_shrink_0()
                    .w(btn_w)
                    .h_full()
                    .border_l_1()
                    .border_color(theme.border)
                    .bg(theme.surface_muted.opacity(0.45))
                    .when(can_inc, |el| {
                        el.cursor_pointer()
                            .hover(move |s| s.bg(hover_bg))
                            .active(move |s| s.bg(active_bg))
                            .on_click(cx.listener(|this, ev: &crate::ClickEvent, _, cx| {
                                let mult = if ev.modifiers().shift { 10.0 } else { 1.0 };
                                this.increment(mult, cx);
                            }))
                    })
                    .when(!can_inc, |el| el.opacity(0.38).cursor_not_allowed())
                    .child(
                        Icon::new(LucideIcon::Plus)
                            .size(IconSize::Sm)
                            .color(theme.foreground),
                    ),
            );

        crate::Refineable::refine(root.style(), &self.frame_style);
        root
    }
}

/// Renderable numeric stepper element wrapping [`Entity<NumberInputState>`].
#[derive(IntoElement)]
pub struct NumberInput {
    state: Entity<NumberInputState>,
    style: StyleRefinement,
}

/// Convenience constructor for [`NumberInput::new`].
pub fn number_input(state: &Entity<NumberInputState>) -> NumberInput {
    NumberInput::new(state)
}

impl NumberInput {
    /// Creates a [`NumberInput`] bound to `state`.
    pub fn new(state: &Entity<NumberInputState>) -> Self {
        Self {
            state: state.clone(),
            style: StyleRefinement::default(),
        }
    }
}

impl Styled for NumberInput {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for NumberInput {
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
