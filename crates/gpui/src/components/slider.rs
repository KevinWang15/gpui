//! Interactive Tailwind-style range [`Slider`] / [`SliderState`] supporting mouse click,
//! pointer drag, and keyboard arrow stepping.

use std::cell::Cell;
use std::rc::Rc;

use crate::{
    App, Bounds, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, IntoElement,
    KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Render,
    RenderOnce, SharedString, StyleRefinement, Styled, Window, canvas, div, prelude::*, px,
    relative,
};

use super::theme::ActiveTheme;

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
            track_bounds: Rc::new(Cell::new(None)),
            frame_style: StyleRefinement::default(),
        }
    }

    /// Sets the allowed `[min, max]` range.
    pub fn range(mut self, min: f32, max: f32) -> Self {
        self.min = min;
        self.max = max.max(min + f32::EPSILON);
        self.value = self.snap(self.value);
        self
    }

    /// Sets step increment granularity.
    pub fn step(mut self, step: f32) -> Self {
        self.step = step.max(0.0001);
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

    /// Returns the current slider value.
    pub fn value(&self) -> f32 {
        self.value
    }

    /// Returns normalized progress in `0.0..=1.0`.
    pub fn fraction(&self) -> f32 {
        ((self.value - self.min) / (self.max - self.min)).clamp(0.0, 1.0)
    }

    /// Sets the value (clamped & snapped to `step`) and emits [`SliderEvent::Change`].
    pub fn set_value(&mut self, value: f32, cx: &mut Context<Self>) {
        let snapped = self.snap(value);
        if (snapped - self.value).abs() > f32::EPSILON {
            self.value = snapped;
            cx.emit(SliderEvent::Change(self.value));
            cx.notify();
        }
    }

    fn snap(&self, raw: f32) -> f32 {
        let clamped = raw.clamp(self.min, self.max);
        let steps = ((clamped - self.min) / self.step).round();
        (self.min + steps * self.step).clamp(self.min, self.max)
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
    }

    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.dragging && !self.disabled {
            self.update_from_x(event.position.x, cx);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _window: &mut Window, _cx: &mut Context<Self>) {
        self.dragging = false;
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
        let mult = if event.keystroke.modifiers.shift {
            10.0
        } else {
            1.0
        };
        match event.keystroke.key.as_str() {
            "left" | "down" => {
                self.set_value(self.value - self.step * mult, cx);
                cx.stop_propagation();
            }
            "right" | "up" => {
                self.set_value(self.value + self.step * mult, cx);
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
        let formatted_value = if self.step.fract().abs() < 1e-4 {
            format!("{}", self.value.round() as i64)
        } else {
            format!("{:.1}", self.value)
        };

        let mut root = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .w_full()
            .when(self.disabled, |el| el.opacity(0.55))
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
                        .when(self.show_value, |el| {
                            el.child(
                                div()
                                    .text_size(px(12.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.muted_foreground)
                                    .child(formatted_value),
                            )
                        }),
                )
            })
            .child(
                div()
                    .id("gpui-slider-track")
                    .relative()
                    .flex()
                    .items_center()
                    .w_full()
                    .h(px(22.))
                    .track_focus(&self.focus_handle)
                    .when(!self.disabled, |el| {
                        el.cursor_pointer()
                            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
                            .on_mouse_move(cx.listener(Self::on_mouse_move))
                            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
                            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
                            .on_key_down(cx.listener(Self::on_key_down))
                    })
                    .when(self.disabled, |el| el.cursor_not_allowed())
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
                            .when(focused && !self.disabled, |el| {
                                let ring = super::layout::focus_ring_shadow(&theme, false);
                                el.when(!ring.is_empty(), |el| el.shadow(ring))
                            }),
                    ),
            );
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
            self.state.update(cx, |state, cx| {
                state.frame_style = self.style;
                cx.notify();
            });
        }
        self.state
    }
}
