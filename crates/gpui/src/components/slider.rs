//! Interactive Tailwind-style range [`Slider`] / [`SliderState`] and dual-thumb
//! [`RangeSlider`] / [`RangeSliderState`] supporting mouse click, window-tracked pointer drag,
//! optional clickable [`SliderMark`] tick labels, and full keyboard navigation.

use std::cell::Cell;
use std::rc::Rc;

use crate::{
    App, Bounds, Context, DispatchPhase, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
    IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    Render, RenderOnce, SharedString, StyleRefinement, Styled, Window, canvas, div, prelude::*, px,
    relative,
};

use super::theme::{ActiveTheme, tabular_numerals};

fn sanitize_range(min: f32, max: f32) -> (f32, f32) {
    let min = if min.is_finite() { min } else { 0.0 };
    let raw_max = if max.is_finite() { max } else { min + 100.0 };
    let min_span = (min.abs() * 1e-4).max(1e-4);
    let max = if raw_max > min { raw_max } else { min + min_span };
    (min, max)
}

fn sanitize_step(step: f32, min: f32, max: f32) -> f32 {
    let span = (max - min).max(1e-4);
    if !step.is_finite() || step <= 0.0 {
        return (1.0f32).min(span);
    }
    step.clamp(0.0001, span)
}

fn snap_value(raw: f32, min: f32, max: f32, step: f32) -> f32 {
    if !raw.is_finite() {
        return min;
    }
    let clamped = raw.clamp(min, max);
    let mut snapped = if clamped >= max - step * 0.25 {
        max
    } else {
        let steps = ((clamped - min) / step).round();
        (min + steps * step).clamp(min, max)
    };
    if snapped == 0.0 {
        snapped = 0.0; // Eliminate -0.0
    }
    snapped
}

fn safe_fraction(value: f32, min: f32, max: f32) -> f32 {
    let span = max - min;
    if !span.is_finite() || span <= 0.0 || !value.is_finite() {
        return 0.0;
    }
    ((value - min) / span).clamp(0.0, 1.0)
}

fn format_slider_value(v: f32, step: f32) -> String {
    let v = if v.abs() < 1e-6 { 0.0 } else { v };
    let decimals = if step.fract().abs() < 1e-4 && v.fract().abs() < 1e-4 {
        0
    } else if (step * 10.0).fract().abs() < 1e-3 {
        1
    } else if (step * 100.0).fract().abs() < 1e-2 {
        2
    } else {
        3
    };
    match decimals {
        0 => format!("{}", v.round() as i64),
        1 => format!("{v:.1}"),
        2 => format!("{v:.2}"),
        _ => format!("{v:.3}"),
    }
}

/// Labeled tick mark displayed beneath a [`Slider`] or [`RangeSlider`] track.
#[derive(Clone, Debug)]
pub struct SliderMark {
    /// Numeric value position along `[min, max]`.
    pub value: f32,
    /// Display label rendered beneath the mark.
    pub label: SharedString,
}

impl SliderMark {
    /// Creates a new [`SliderMark`] at `value` with `label`.
    pub fn new(value: f32, label: impl Into<SharedString>) -> Self {
        Self {
            value: if value.is_finite() { value } else { 0.0 },
            label: label.into(),
        }
    }
}

/// Events emitted by [`SliderState`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SliderEvent {
    /// Emitted whenever the numeric slider value changes.
    Change(f32),
}

/// Entity state backing an interactive range [`Slider`].
pub struct SliderState {
    focus_handle: FocusHandle,
    value: f32,
    min: f32,
    max: f32,
    step: f32,
    disabled: bool,
    dragging: bool,
    label: Option<SharedString>,
    show_value: bool,
    marks: Vec<SliderMark>,
    track_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    frame_style: StyleRefinement,
}

impl EventEmitter<SliderEvent> for SliderState {}

impl Focusable for SliderState {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl SliderState {
    /// Creates a new slider defaulting to `0.0..=100.0` with step `1.0`.
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            value: 50.0,
            min: 0.0,
            max: 100.0,
            step: 1.0,
            disabled: false,
            dragging: false,
            label: None,
            show_value: true,
            marks: Vec::new(),
            track_bounds: Rc::new(Cell::new(None)),
            frame_style: StyleRefinement::default(),
        }
    }

    /// Adds labeled tick marks beneath the slider track.
    pub fn marks(mut self, marks: impl IntoIterator<Item = SliderMark>) -> Self {
        self.marks = marks.into_iter().collect();
        self
    }

    /// Sets the allowed `[min, max]` range.
    pub fn range(mut self, min: f32, max: f32) -> Self {
        let (min, max) = sanitize_range(min, max);
        self.min = min;
        self.max = max;
        self.step = sanitize_step(self.step, self.min, self.max);
        self.value = self.snap(self.value);
        self
    }

    /// Sets step increment granularity.
    pub fn step(mut self, step: f32) -> Self {
        self.step = sanitize_step(step, self.min, self.max);
        self.value = self.snap(self.value);
        self
    }

    /// Sets initial value.
    pub fn default_value(mut self, value: f32) -> Self {
        self.value = self.snap(value);
        self
    }

    /// Sets an optional inline heading label above the slider.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Controls whether the numeric value readout is shown.
    pub fn show_value(mut self, show: bool) -> Self {
        self.show_value = show;
        self
    }

    /// Disables interaction.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Dynamically updates disabled state.
    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        if self.disabled != disabled {
            self.disabled = disabled;
            if disabled {
                self.dragging = false;
            }
            cx.notify();
        }
    }

    /// Returns the current slider value.
    pub fn value(&self) -> f32 {
        self.value
    }

    /// Returns normalized progress in `0.0..=1.0`.
    pub fn fraction(&self) -> f32 {
        safe_fraction(self.value, self.min, self.max)
    }

    /// Sets the value (clamped & snapped to `step`) and emits [`SliderEvent::Change`].
    pub fn set_value(&mut self, value: f32, cx: &mut Context<Self>) {
        if !value.is_finite() {
            return;
        }
        let snapped = self.snap(value);
        if (snapped - self.value).abs() > f32::EPSILON {
            self.value = snapped;
            cx.emit(SliderEvent::Change(self.value));
            cx.notify();
        }
    }

    fn snap(&self, raw: f32) -> f32 {
        snap_value(raw, self.min, self.max, self.step)
    }

    fn update_from_x(&mut self, x: Pixels, cx: &mut Context<Self>) {
        let Some(bounds) = self.track_bounds.get() else {
            return;
        };
        let width = bounds.size.width.0.max(1.0);
        let ratio = ((x.0 - bounds.left().0) / width).clamp(0.0, 1.0);
        let target = self.min + ratio * (self.max - self.min);
        self.set_value(target, cx);
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return;
        }
        window.focus(&self.focus_handle, cx);
        self.dragging = true;
        self.update_from_x(event.position.x, cx);
        cx.notify();
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
        let mods = event.keystroke.modifiers;
        if mods.control || mods.platform || mods.alt {
            return;
        }
        let mult = if mods.shift { 10.0 } else { 1.0 };
        match event.keystroke.key.as_str() {
            "left" | "down" => {
                self.set_value(self.value - self.step * mult, cx);
                cx.stop_propagation();
            }
            "right" | "up" => {
                self.set_value(self.value + self.step * mult, cx);
                cx.stop_propagation();
            }
            "pagedown" => {
                self.set_value(self.value - self.step * 10.0, cx);
                cx.stop_propagation();
            }
            "pageup" => {
                self.set_value(self.value + self.step * 10.0, cx);
                cx.stop_propagation();
            }
            "home" => {
                self.set_value(self.min, cx);
                cx.stop_propagation();
            }
            "end" => {
                self.set_value(self.max, cx);
                cx.stop_propagation();
            }
            _ => {}
        }
    }
}

impl Render for SliderState {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let focused = self.focus_handle.is_focused(window);
        let pct = self.fraction();
        let bounds_cell = self.track_bounds.clone();
        let dragging = self.dragging;
        let disabled = self.disabled;
        let entity = cx.entity();
        let entity_id = cx.entity_id();

        let formatted_value = self
            .show_value
            .then(|| format_slider_value(self.value, self.step));

        let mut root = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .w_full()
            .when(disabled, |el| el.opacity(0.55))
            .when(self.label.is_some() || self.show_value, |el| {
                el.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .w_full()
                        .child(
                            div()
                                .text_size(px(13.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.foreground)
                                .children(self.label.clone()),
                        )
                        .when_some(formatted_value, |el, val| {
                            el.child(
                                div()
                                    .text_size(px(12.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .font_features(tabular_numerals())
                                    .text_color(theme.muted_foreground)
                                    .child(val),
                            )
                        }),
                )
            })
            .child(
                div()
                    .id(("gpui-slider-track", entity_id))
                    .relative()
                    .flex()
                    .items_center()
                    .w_full()
                    .h(px(22.))
                    .when(!disabled, |el| {
                        el.track_focus(&self.focus_handle)
                            .when(dragging, |el| el.cursor_grabbing())
                            .when(!dragging, |el| el.cursor_pointer())
                            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
                            .on_key_down(cx.listener(Self::on_key_down))
                    })
                    .when(disabled, |el| el.cursor_not_allowed())
                    .child(
                        canvas(
                            move |bounds, _, _| {
                                bounds_cell.set(Some(bounds));
                            },
                            move |_, _, window, _| {
                                if !dragging {
                                    return;
                                }
                                let ent_move = entity.clone();
                                window.on_mouse_event(move |ev: &MouseMoveEvent, phase, _, cx| {
                                    if phase != DispatchPhase::Capture {
                                        return;
                                    }
                                    if ev.pressed_button != Some(MouseButton::Left) {
                                        ent_move.update(cx, |s, cx| {
                                            if s.dragging {
                                                s.dragging = false;
                                                cx.notify();
                                            }
                                        });
                                        return;
                                    }
                                    ent_move.update(cx, |s, cx| {
                                        if s.dragging && !s.disabled {
                                            s.update_from_x(ev.position.x, cx);
                                        }
                                    });
                                });
                                let ent_up = entity;
                                window.on_mouse_event(move |ev: &MouseUpEvent, phase, _, cx| {
                                    if phase == DispatchPhase::Capture
                                        && ev.button == MouseButton::Left
                                    {
                                        ent_up.update(cx, |s, cx| {
                                            if s.dragging {
                                                s.dragging = false;
                                                cx.notify();
                                            }
                                        });
                                    }
                                });
                            },
                        )
                        .absolute()
                        .size_full(),
                    )
                    // Background rail
                    .child(
                        div()
                            .w_full()
                            .h(px(6.))
                            .rounded_full()
                            .bg(theme.slider_track)
                            .overflow_hidden()
                            .child(
                                div()
                                    .h_full()
                                    .w(relative(pct))
                                    .rounded_full()
                                    .bg(theme.slider_fill),
                            ),
                    )
                    // Thumb positioned at fraction
                    .child(
                        div()
                            .absolute()
                            .left(relative(pct))
                            .ml(px(-9.))
                            .w(px(18.))
                            .h(px(18.))
                            .rounded_full()
                            .bg(theme.slider_thumb)
                            .border_2()
                            .border_color(theme.slider_fill)
                            .when(theme.elevation_shadows, |el| el.shadow_sm())
                            .when(focused && !disabled, |el| {
                                let ring = super::layout::focus_ring_shadow(&theme, false);
                                el.when(!ring.is_empty(), |el| el.shadow(ring))
                            }),
                    ),
            )
            .when(!self.marks.is_empty(), |el| {
                let min = self.min;
                let max = self.max;
                el.child(
                    div()
                        .relative()
                        .w_full()
                        .h(px(18.))
                        .children(self.marks.iter().enumerate().map(|(i, mark)| {
                            let frac = safe_fraction(mark.value, min, max);
                            let target_val = mark.value;
                            div()
                                .id(("sl-mark", i))
                                .absolute()
                                .left(relative(frac))
                                .ml(px(-22.))
                                .w(px(44.))
                                .flex()
                                .justify_center()
                                .text_size(px(11.))
                                .font_features(tabular_numerals())
                                .text_color(theme.muted_foreground)
                                .when(!disabled, |el| {
                                    el.cursor_pointer()
                                        .hover(|s| s.text_color(theme.foreground))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            window.focus(&this.focus_handle, cx);
                                            this.set_value(target_val, cx);
                                        }))
                                })
                                .child(mark.label.clone())
                        })),
                )
            });
        crate::Refineable::refine(root.style(), &self.frame_style);
        root
    }
}

/// Renderable Tailwind-style slider element wrapping an [`Entity<SliderState>`].
#[derive(IntoElement)]
pub struct Slider {
    state: Entity<SliderState>,
    style: StyleRefinement,
}

/// Convenience constructor for [`Slider::new`].
pub fn slider(state: &Entity<SliderState>) -> Slider {
    Slider::new(state)
}

impl Slider {
    /// Creates a [`Slider`] element backed by `state`.
    pub fn new(state: &Entity<SliderState>) -> Self {
        Self {
            state: state.clone(),
            style: StyleRefinement::default(),
        }
    }
}

impl Styled for Slider {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Slider {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        if self.state.read(cx).frame_style != self.style {
            let style = self.style;
            self.state.update(cx, |state, _| {
                state.frame_style = style;
            });
        }
        self.state
    }
}

// ============================================================================
// Dual-Thumb RangeSlider
// ============================================================================

/// Events emitted by [`RangeSliderState`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RangeSliderEvent {
    /// Emitted whenever either `[start, end]` bound changes.
    Change(f32, f32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ActiveThumb {
    Start,
    End,
}

/// Entity state backing a dual-thumb [`RangeSlider`].
pub struct RangeSliderState {
    focus_handle: FocusHandle,
    start: f32,
    end: f32,
    min: f32,
    max: f32,
    step: f32,
    disabled: bool,
    dragging: Option<ActiveThumb>,
    active_thumb: ActiveThumb,
    label: Option<SharedString>,
    show_value: bool,
    marks: Vec<SliderMark>,
    track_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    frame_style: StyleRefinement,
}

impl EventEmitter<RangeSliderEvent> for RangeSliderState {}

impl Focusable for RangeSliderState {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl RangeSliderState {
    /// Creates a dual-thumb range slider defaulting to `20..=80` within `0..=100`.
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            start: 20.0,
            end: 80.0,
            min: 0.0,
            max: 100.0,
            step: 1.0,
            disabled: false,
            dragging: None,
            active_thumb: ActiveThumb::Start,
            label: None,
            show_value: true,
            marks: Vec::new(),
            track_bounds: Rc::new(Cell::new(None)),
            frame_style: StyleRefinement::default(),
        }
    }

    /// Sets the allowed `[min, max]` domain.
    pub fn range(mut self, min: f32, max: f32) -> Self {
        let (min, max) = sanitize_range(min, max);
        self.min = min;
        self.max = max;
        self.step = sanitize_step(self.step, self.min, self.max);
        self.start = self.snap(self.start);
        self.end = self.snap(self.end).max(self.start);
        self
    }

    /// Sets step increment granularity.
    pub fn step(mut self, step: f32) -> Self {
        self.step = sanitize_step(step, self.min, self.max);
        self.start = self.snap(self.start);
        self.end = self.snap(self.end).max(self.start);
        self
    }

    /// Sets initial `[start, end]` selected range.
    pub fn default_range(mut self, start: f32, end: f32) -> Self {
        let a = self.snap(start);
        let b = self.snap(end);
        self.start = a.min(b);
        self.end = a.max(b);
        self
    }

    /// Sets an optional heading label above the track.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Controls whether the numeric `"start – end"` readout is displayed.
    pub fn show_value(mut self, show: bool) -> Self {
        self.show_value = show;
        self
    }

    /// Adds labeled tick marks beneath the range slider track.
    pub fn marks(mut self, marks: impl IntoIterator<Item = SliderMark>) -> Self {
        self.marks = marks.into_iter().collect();
        self
    }

    /// Disables user interaction.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Dynamically updates disabled state.
    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        if self.disabled != disabled {
            self.disabled = disabled;
            if disabled {
                self.dragging = None;
            }
            cx.notify();
        }
    }

    /// Returns the current `(start, end)` selected values.
    pub fn values(&self) -> (f32, f32) {
        (self.start, self.end)
    }

    /// Programmatically sets `(start, end)` (clamped, ordered, snapped) and emits `Change`.
    pub fn set_values(&mut self, start: f32, end: f32, cx: &mut Context<Self>) {
        if !start.is_finite() || !end.is_finite() {
            return;
        }
        let a = self.snap(start);
        let b = self.snap(end);
        let next_start = a.min(b);
        let next_end = a.max(b);
        if (next_start - self.start).abs() > f32::EPSILON
            || (next_end - self.end).abs() > f32::EPSILON
        {
            self.start = next_start;
            self.end = next_end;
            cx.emit(RangeSliderEvent::Change(self.start, self.end));
            cx.notify();
        }
    }

    fn snap(&self, raw: f32) -> f32 {
        snap_value(raw, self.min, self.max, self.step)
    }

    fn value_at_x(&self, x: Pixels) -> Option<f32> {
        let bounds = self.track_bounds.get()?;
        let width = bounds.size.width.0.max(1.0);
        let ratio = ((x.0 - bounds.left().0) / width).clamp(0.0, 1.0);
        Some(self.min + ratio * (self.max - self.min))
    }

    fn pick_thumb_for_target(&self, target: f32) -> ActiveThumb {
        // When both thumbs overlap, move `End` for clicks to the right and `Start` to the left
        // so overlapping thumbs at `(min, min)` or `(max, max)` never trap mouse interaction.
        if (self.end - self.start).abs() <= f32::EPSILON {
            if target > self.end {
                ActiveThumb::End
            } else {
                ActiveThumb::Start
            }
        } else {
            let d_start = (target - self.start).abs();
            let d_end = (target - self.end).abs();
            if d_start <= d_end {
                ActiveThumb::Start
            } else {
                ActiveThumb::End
            }
        }
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return;
        }
        window.focus(&self.focus_handle, cx);
        let Some(target) = self.value_at_x(event.position.x) else {
            return;
        };
        let thumb = self.pick_thumb_for_target(target);
        self.active_thumb = thumb;
        self.dragging = Some(thumb);
        self.apply_thumb_value(thumb, target, cx);
        cx.notify();
    }

    fn apply_thumb_value(&mut self, thumb: ActiveThumb, raw: f32, cx: &mut Context<Self>) {
        let snapped = self.snap(raw);
        match thumb {
            ActiveThumb::Start => {
                let clamped = snapped.min(self.end);
                self.set_values(clamped, self.end, cx);
            }
            ActiveThumb::End => {
                let clamped = snapped.max(self.start);
                self.set_values(self.start, clamped, cx);
            }
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        let mods = event.keystroke.modifiers;
        if mods.control || mods.platform || mods.alt {
            return;
        }
        let mult = if mods.shift { 10.0 } else { 1.0 };
        match event.keystroke.key.as_str() {
            // Avoid WCAG 2.1.2 keyboard trap: only consume Tab between Start->End and Shift+Tab between End->Start.
            "tab" => {
                if !mods.shift && self.active_thumb == ActiveThumb::Start {
                    self.active_thumb = ActiveThumb::End;
                    cx.notify();
                    cx.stop_propagation();
                } else if mods.shift && self.active_thumb == ActiveThumb::End {
                    self.active_thumb = ActiveThumb::Start;
                    cx.notify();
                    cx.stop_propagation();
                }
            }
            "left" | "down" => {
                let cur = if self.active_thumb == ActiveThumb::Start {
                    self.start
                } else {
                    self.end
                };
                self.apply_thumb_value(self.active_thumb, cur - self.step * mult, cx);
                cx.stop_propagation();
            }
            "right" | "up" => {
                let cur = if self.active_thumb == ActiveThumb::Start {
                    self.start
                } else {
                    self.end
                };
                self.apply_thumb_value(self.active_thumb, cur + self.step * mult, cx);
                cx.stop_propagation();
            }
            "pagedown" => {
                let cur = if self.active_thumb == ActiveThumb::Start {
                    self.start
                } else {
                    self.end
                };
                self.apply_thumb_value(self.active_thumb, cur - self.step * 10.0, cx);
                cx.stop_propagation();
            }
            "pageup" => {
                let cur = if self.active_thumb == ActiveThumb::Start {
                    self.start
                } else {
                    self.end
                };
                self.apply_thumb_value(self.active_thumb, cur + self.step * 10.0, cx);
                cx.stop_propagation();
            }
            "home" => {
                let bound = match self.active_thumb {
                    ActiveThumb::Start => self.min,
                    ActiveThumb::End => self.start,
                };
                self.apply_thumb_value(self.active_thumb, bound, cx);
                cx.stop_propagation();
            }
            "end" => {
                let bound = match self.active_thumb {
                    ActiveThumb::Start => self.end,
                    ActiveThumb::End => self.max,
                };
                self.apply_thumb_value(self.active_thumb, bound, cx);
                cx.stop_propagation();
            }
            _ => {}
        }
    }
}

impl Render for RangeSliderState {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let focused = self.focus_handle.is_focused(window);
        let start_pct = safe_fraction(self.start, self.min, self.max);
        let end_pct = safe_fraction(self.end, self.min, self.max).max(start_pct);
        let bounds_cell = self.track_bounds.clone();
        let dragging = self.dragging;
        let disabled = self.disabled;
        let entity = cx.entity();
        let entity_id = cx.entity_id();

        let readout = self.show_value.then(|| {
            format!(
                "{} – {}",
                format_slider_value(self.start, self.step),
                format_slider_value(self.end, self.step),
            )
        });

        let make_thumb = |pct: f32, is_active: bool| {
            div()
                .absolute()
                .left(relative(pct))
                .ml(px(-9.))
                .w(px(18.))
                .h(px(18.))
                .rounded_full()
                .bg(theme.slider_thumb)
                .border_2()
                .border_color(theme.slider_fill)
                .when(theme.elevation_shadows, |el| el.shadow_sm())
                .when(focused && !disabled && is_active, |el| {
                    let ring = super::layout::focus_ring_shadow(&theme, false);
                    el.when(!ring.is_empty(), |el| el.shadow(ring))
                })
        };

        // Render active thumb second so it always paints on top when start == end.
        let (first_thumb, second_thumb) = if self.active_thumb == ActiveThumb::Start {
            (make_thumb(end_pct, false), make_thumb(start_pct, true))
        } else {
            (make_thumb(start_pct, false), make_thumb(end_pct, true))
        };

        let mut root = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .w_full()
            .when(disabled, |el| el.opacity(0.55))
            .when(self.label.is_some() || self.show_value, |el| {
                el.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .text_size(px(13.))
                        .child(
                            div()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.foreground)
                                .children(self.label.clone()),
                        )
                        .when_some(readout, |el, val| {
                            el.child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .font_features(tabular_numerals())
                                    .text_color(theme.muted_foreground)
                                    .child(val),
                            )
                        }),
                )
            })
            .child(
                div()
                    .id(("range-slider-track", entity_id))
                    .relative()
                    .flex()
                    .items_center()
                    .w_full()
                    .h(px(22.))
                    .when(!disabled, |el| {
                        el.track_focus(&self.focus_handle)
                            .when(dragging.is_some(), |el| el.cursor_grabbing())
                            .when(dragging.is_none(), |el| el.cursor_pointer())
                            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
                            .on_key_down(cx.listener(Self::on_key_down))
                    })
                    .when(disabled, |el| el.cursor_not_allowed())
                    .child(
                        canvas(
                            move |bounds, _, _| bounds_cell.set(Some(bounds)),
                            move |_, _, window, _| {
                                let Some(thumb) = dragging else {
                                    return;
                                };
                                let ent_move = entity.clone();
                                window.on_mouse_event(move |ev: &MouseMoveEvent, phase, _, cx| {
                                    if phase != DispatchPhase::Capture {
                                        return;
                                    }
                                    if ev.pressed_button != Some(MouseButton::Left) {
                                        ent_move.update(cx, |s, cx| {
                                            if s.dragging.take().is_some() {
                                                cx.notify();
                                            }
                                        });
                                        return;
                                    }
                                    ent_move.update(cx, |s, cx| {
                                        if s.dragging == Some(thumb)
                                            && !s.disabled
                                            && let Some(val) = s.value_at_x(ev.position.x)
                                        {
                                            s.apply_thumb_value(thumb, val, cx);
                                        }
                                    });
                                });
                                let ent_up = entity;
                                window.on_mouse_event(move |ev: &MouseUpEvent, phase, _, cx| {
                                    if phase == DispatchPhase::Capture
                                        && ev.button == MouseButton::Left
                                    {
                                        ent_up.update(cx, |s, cx| {
                                            if s.dragging.take().is_some() {
                                                cx.notify();
                                            }
                                        });
                                    }
                                });
                            },
                        )
                        .absolute()
                        .size_full(),
                    )
                    // Background rail + highlighted interior segment
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .h(px(6.))
                            .rounded_full()
                            .bg(theme.slider_track)
                            .overflow_hidden()
                            .child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .bottom_0()
                                    .left(relative(start_pct))
                                    .w(relative((end_pct - start_pct).max(0.0)))
                                    .rounded_full()
                                    .bg(theme.slider_fill),
                            ),
                    )
                    .child(first_thumb)
                    .child(second_thumb),
            )
            .when(!self.marks.is_empty(), |el| {
                let min = self.min;
                let max = self.max;
                el.child(
                    div()
                        .relative()
                        .w_full()
                        .h(px(18.))
                        .children(self.marks.iter().enumerate().map(|(i, mark)| {
                            let frac = safe_fraction(mark.value, min, max);
                            let target_val = mark.value;
                            div()
                                .id(("rsl-mark", i))
                                .absolute()
                                .left(relative(frac))
                                .ml(px(-22.))
                                .w(px(44.))
                                .flex()
                                .justify_center()
                                .text_size(px(11.))
                                .font_features(tabular_numerals())
                                .text_color(theme.muted_foreground)
                                .when(!disabled, |el| {
                                    el.cursor_pointer()
                                        .hover(|s| s.text_color(theme.foreground))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            window.focus(&this.focus_handle, cx);
                                            let thumb = this.pick_thumb_for_target(target_val);
                                            this.active_thumb = thumb;
                                            this.apply_thumb_value(thumb, target_val, cx);
                                        }))
                                })
                                .child(mark.label.clone())
                        })),
                )
            });

        crate::Refineable::refine(root.style(), &self.frame_style);
        root
    }
}

/// Renderable dual-thumb range slider wrapping [`Entity<RangeSliderState>`].
#[derive(IntoElement)]
pub struct RangeSlider {
    state: Entity<RangeSliderState>,
    style: StyleRefinement,
}

/// Convenience constructor for [`RangeSlider::new`].
pub fn range_slider(state: &Entity<RangeSliderState>) -> RangeSlider {
    RangeSlider::new(state)
}

impl RangeSlider {
    /// Creates a [`RangeSlider`] backed by `state`.
    pub fn new(state: &Entity<RangeSliderState>) -> Self {
        Self {
            state: state.clone(),
            style: StyleRefinement::default(),
        }
    }
}

impl Styled for RangeSlider {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for RangeSlider {
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
