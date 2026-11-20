//! Interactive drag-resizable [`SplitPane`] (`Horizontal` or `Vertical`) with live pointer
//! dragging, visual hover/active divider grip, double-click reset, ratio clamping, and keyboard
//! resize support.

use std::cell::Cell;
use std::rc::Rc;

use crate::{
    AnyElement, App, Bounds, Context, CursorStyle, ElementId, Entity, EventEmitter, FocusHandle,
    Focusable, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, RenderOnce, StyleRefinement, Styled, Window, canvas, div, prelude::*, px,
};

use super::theme::ActiveTheme;

/// Axis along which a [`SplitPane`] divides its two child panes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SplitDirection {
    /// Left / Right panes separated by a vertical draggable bar (default).
    #[default]
    Horizontal,
    /// Top / Bottom panes separated by a horizontal draggable bar.
    Vertical,
}

/// Events emitted by [`SplitPaneState`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SplitPaneEvent {
    /// Emitted whenever the primary pane's fraction (`0.0..=1.0`) changes.
    Resize(f32),
}

/// Entity state backing an interactive [`SplitPane`].
pub struct SplitPaneState {
    focus_handle: FocusHandle,
    direction: SplitDirection,
    ratio: f32,
    default_ratio: f32,
    raw_default_ratio: f32,
    min_ratio: f32,
    max_ratio: f32,
    dragging: bool,
    disabled: bool,
    bounds_cell: Rc<Cell<Option<Bounds<Pixels>>>>,
    handle_size_cell: Rc<Cell<f32>>,
}

impl EventEmitter<SplitPaneEvent> for SplitPaneState {}

impl Focusable for SplitPaneState {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl SplitPaneState {
    /// Creates a horizontal [`SplitPaneState`] starting at a `50% / 50%` split (`0.15..=0.85`).
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            direction: SplitDirection::Horizontal,
            ratio: 0.5,
            default_ratio: 0.5,
            raw_default_ratio: 0.5,
            min_ratio: 0.15,
            max_ratio: 0.85,
            dragging: false,
            disabled: false,
            bounds_cell: Rc::new(Cell::new(None)),
            handle_size_cell: Rc::new(Cell::new(8.0)),
        }
    }

    /// Sets the split orientation (`Horizontal` left/right or `Vertical` top/bottom).
    pub fn direction(mut self, direction: SplitDirection) -> Self {
        self.direction = direction;
        self
    }

    /// Sets the initial and double-click reset split fraction (clamped to `min_ratio..=max_ratio`).
    pub fn default_ratio(mut self, ratio: f32) -> Self {
        let raw = if ratio.is_finite() {
            ratio.clamp(0.0, 1.0)
        } else {
            0.5
        };
        self.raw_default_ratio = raw;
        let clamped = raw.clamp(self.min_ratio, self.max_ratio);
        self.default_ratio = clamped;
        self.ratio = clamped;
        self
    }

    /// Sets the allowed `[min, max]` fraction range for the first pane.
    pub fn clamp_range(mut self, min_ratio: f32, max_ratio: f32) -> Self {
        let min = if min_ratio.is_finite() {
            min_ratio.clamp(0.05, 0.90)
        } else {
            0.15
        };
        let max = if max_ratio.is_finite() {
            max_ratio.clamp(min + 0.05, 0.95)
        } else {
            0.85_f32.clamp(min + 0.05, 0.95)
        };
        self.min_ratio = min;
        self.max_ratio = max;
        self.default_ratio = self.raw_default_ratio.clamp(min, max);
        self.ratio = self.default_ratio;
        self
    }

    /// Disables dragging and keyboard resizing.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        if disabled {
            self.dragging = false;
        }
        self
    }

    /// Dynamically updates whether resizing is disabled.
    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        if self.disabled != disabled {
            self.disabled = disabled;
            if disabled {
                self.dragging = false;
            }
            cx.notify();
        }
    }

    /// Returns the current split orientation.
    pub fn split_direction(&self) -> SplitDirection {
        self.direction
    }

    /// Returns the current primary pane ratio in `0.0..=1.0`.
    pub fn ratio(&self) -> f32 {
        self.ratio
    }

    /// Returns whether the user is currently dragging the divider handle.
    pub fn is_dragging(&self) -> bool {
        self.dragging
    }

    /// Programmatically updates the split ratio (clamped) and emits [`SplitPaneEvent::Resize`].
    pub fn set_ratio(&mut self, ratio: f32, cx: &mut Context<Self>) {
        if !ratio.is_finite() {
            return;
        }
        let clamped = ratio.clamp(self.min_ratio, self.max_ratio);
        if (clamped - self.ratio).abs() > 0.0005 {
            self.ratio = clamped;
            cx.emit(SplitPaneEvent::Resize(clamped));
            cx.notify();
        }
    }

    /// Resets the split ratio back to `default_ratio`.
    pub fn reset(&mut self, cx: &mut Context<Self>) {
        self.set_ratio(self.default_ratio, cx);
    }

    fn update_from_position(&mut self, pos: crate::Point<Pixels>, cx: &mut Context<Self>) {
        let Some(bounds) = self.bounds_cell.get() else {
            return;
        };
        let handle = self.handle_size_cell.get();
        let raw = match self.direction {
            SplitDirection::Horizontal => {
                let usable = (bounds.size.width.0 - handle).max(1.0);
                (pos.x.0 - bounds.origin.x.0 - handle * 0.5) / usable
            }
            SplitDirection::Vertical => {
                let usable = (bounds.size.height.0 - handle).max(1.0);
                (pos.y.0 - bounds.origin.y.0 - handle * 0.5) / usable
            }
        };
        self.set_ratio(raw, cx);
    }

    fn start_drag(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        window.focus(&self.focus_handle, cx);
        if event.click_count >= 2 {
            self.dragging = false;
            self.reset(cx);
            return;
        }
        self.dragging = true;
        self.update_from_position(event.position, cx);
        cx.notify();
    }

    fn move_drag(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if self.dragging && !self.disabled {
            self.update_from_position(event.position, cx);
        }
    }

    fn end_drag(&mut self, cx: &mut Context<Self>) {
        if self.dragging {
            self.dragging = false;
            cx.notify();
        }
    }

    fn handle_key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        let mods = &event.keystroke.modifiers;
        if mods.control || mods.alt || mods.platform {
            return;
        }
        let step = if mods.shift { 0.10 } else { 0.025 };
        match (self.direction, event.keystroke.key.as_str()) {
            (SplitDirection::Horizontal, "left") | (SplitDirection::Vertical, "up") => {
                self.set_ratio(self.ratio - step, cx);
                cx.stop_propagation();
            }
            (SplitDirection::Horizontal, "right") | (SplitDirection::Vertical, "down") => {
                self.set_ratio(self.ratio + step, cx);
                cx.stop_propagation();
            }
            (_, "home") | (_, "0") => {
                self.reset(cx);
                cx.stop_propagation();
            }
            _ => {}
        }
    }
}

/// Interactive drag-to-resize split container holding two child panes (`first` and `second`).
#[derive(IntoElement)]
pub struct SplitPane {
    id: ElementId,
    state: Entity<SplitPaneState>,
    first: Option<AnyElement>,
    second: Option<AnyElement>,
    handle_size: Pixels,
    show_grip: bool,
    style: StyleRefinement,
}

/// Convenience constructor for [`SplitPane::new`].
pub fn split_pane(id: impl Into<ElementId>, state: &Entity<SplitPaneState>) -> SplitPane {
    SplitPane::new(id, state)
}

impl SplitPane {
    /// Creates a [`SplitPane`] bound to `state`.
    pub fn new(id: impl Into<ElementId>, state: &Entity<SplitPaneState>) -> Self {
        Self {
            id: id.into(),
            state: state.clone(),
            first: None,
            second: None,
            handle_size: px(8.),
            show_grip: true,
            style: StyleRefinement::default(),
        }
    }

    /// Sets the first pane (left in `Horizontal` mode, top in `Vertical` mode).
    pub fn first(mut self, element: impl IntoElement) -> Self {
        self.first = Some(element.into_any_element());
        self
    }

    /// Sets the second pane (right in `Horizontal` mode, bottom in `Vertical` mode).
    pub fn second(mut self, element: impl IntoElement) -> Self {
        self.second = Some(element.into_any_element());
        self
    }

    /// Sets the interactive hit-area width/height of the divider handle (default `8px`).
    pub fn handle_size(mut self, size: Pixels) -> Self {
        self.handle_size = if size.0.is_finite() {
            size.max(px(4.))
        } else {
            px(8.)
        };
        self
    }

    /// Shows or hides the subtle centered grip pill inside the divider (default `true`).
    pub fn show_grip(mut self, show: bool) -> Self {
        self.show_grip = show;
        self
    }
}

impl Styled for SplitPane {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for SplitPane {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (direction, ratio, dragging, disabled, focus_handle, bounds_cell, handle_size_cell) = {
            let st = self.state.read(cx);
            (
                st.direction,
                st.ratio,
                st.dragging,
                st.disabled,
                st.focus_handle.clone(),
                st.bounds_cell.clone(),
                st.handle_size_cell.clone(),
            )
        };
        handle_size_cell.set(self.handle_size.0);
        // A click focuses the divider too; the accent shows keyboard focus only.
        let focused = !disabled && focus_handle.is_focus_visible(window);
        let is_horizontal = direction == SplitDirection::Horizontal;

        let state_down = self.state.clone();
        let state_key = self.state.clone();
        let state_window_drag = self.state.clone();

        let divider_cursor = if disabled {
            CursorStyle::OperationNotAllowed
        } else if is_horizontal {
            CursorStyle::ResizeLeftRight
        } else {
            CursorStyle::ResizeUpDown
        };

        let active_accent = (dragging || focused) && !disabled;
        let line_color = if active_accent {
            theme.primary
        } else {
            theme.border
        };
        let hover_bg = theme.primary.opacity(0.14);
        let grip_color = if active_accent {
            theme.primary
        } else {
            theme.muted_foreground.opacity(0.5)
        };

        let first_pane = div()
            .flex()
            .flex_col()
            .flex_basis(px(0.))
            .flex_grow(ratio)
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .when(is_horizontal, |el| el.h_full())
            .when(!is_horizontal, |el| el.w_full())
            .children(self.first);

        let second_pane = div()
            .flex()
            .flex_col()
            .flex_basis(px(0.))
            .flex_grow((1.0 - ratio).max(0.0))
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .when(is_horizontal, |el| el.h_full())
            .when(!is_horizontal, |el| el.w_full())
            .children(self.second);

        let handle = div()
            .id((self.id.clone(), "split-handle"))
            .when(!disabled, |el| el.track_focus(&focus_handle).tab_stop(true))
            .relative()
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .cursor(divider_cursor)
            .when(is_horizontal, |el| el.w(self.handle_size).h_full())
            .when(!is_horizontal, |el| el.h(self.handle_size).w_full())
            .when(dragging, |el| el.bg(theme.primary.opacity(0.12)))
            .when(!disabled, |el| {
                el.hover(move |s| s.bg(hover_bg))
                    .on_mouse_down(MouseButton::Left, move |ev, window, cx| {
                        state_down.update(cx, |s, cx| s.start_drag(ev, window, cx));
                        cx.stop_propagation();
                    })
                    .on_key_down(move |ev, _, cx| {
                        state_key.update(cx, |s, cx| s.handle_key_down(ev, cx));
                    })
            })
            // Crisp hairline rule
            .child(
                div()
                    .when(is_horizontal, |el| {
                        el.w(if dragging { px(2.) } else { px(1.) }).h_full()
                    })
                    .when(!is_horizontal, |el| {
                        el.h(if dragging { px(2.) } else { px(1.) }).w_full()
                    })
                    .bg(line_color),
            )
            // Optional centered tactile grip pill
            .when(self.show_grip, |el| {
                el.child(
                    div()
                        .absolute()
                        .rounded_full()
                        .bg(grip_color)
                        .when(is_horizontal, |el| el.w(px(3.)).h(px(28.)))
                        .when(!is_horizontal, |el| el.h(px(3.)).w(px(28.))),
                )
            });

        let mut root = div()
            .id(self.id)
            .relative()
            .flex()
            .w_full()
            .h_full()
            .overflow_hidden()
            .when(is_horizontal, |el| el.flex_row())
            .when(!is_horizontal, |el| el.flex_col())
            .when(dragging, |el| el.cursor(divider_cursor))
            .child(
                canvas(
                    move |bounds, _, _| {
                        bounds_cell.set(Some(bounds));
                    },
                    move |_, _, window, _| {
                        if dragging {
                            window.set_window_cursor_style(divider_cursor);
                            let st_move = state_window_drag.clone();
                            let st_up = state_window_drag;
                            window.on_mouse_event(move |ev: &MouseMoveEvent, phase, _, cx| {
                                if phase == crate::DispatchPhase::Capture {
                                    if ev.pressed_button != Some(MouseButton::Left) {
                                        st_move.update(cx, |s, cx| s.end_drag(cx));
                                    } else {
                                        st_move.update(cx, |s, cx| s.move_drag(ev, cx));
                                    }
                                }
                            });
                            window.on_mouse_event(move |ev: &MouseUpEvent, phase, _, cx| {
                                if phase == crate::DispatchPhase::Capture
                                    && ev.button == MouseButton::Left
                                {
                                    st_up.update(cx, |s, cx| s.end_drag(cx));
                                }
                            });
                        }
                    },
                )
                .absolute()
                .size_full(),
            )
            .child(first_pane)
            .child(handle)
            .child(second_pane);

        crate::Refineable::refine(root.style(), &self.style);
        root
    }
}
