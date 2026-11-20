//! Shared caret blink state machine for text-editing components (`Input`,
//! `TextBox` / `TextArea`, `PinInput`, `FilterBar`).
//!
//! Design goals:
//! - **Zero idle cost**: no timer exists while unfocused, blurred, disabled,
//!   in an inactive OS window, showing a non-empty selection, or when
//!   [`App::reduce_motion`] is enabled.
//! - **Solid while typing or navigating**: [`CursorBlink::reset`] immediately
//!   shows the caret and pushes the next blink-off out by a full
//!   [`CURSOR_BLINK_INTERVAL`] without respawning the timer task on every
//!   keystroke.
//! - **Self-cleaning on unmount**: every tick checks that the owning focus
//!   handle is still mounted in the window's rendered dispatch tree and that
//!   the window is still open and active, so unmounting an entity (e.g.
//!   switching tabs) never leaves a stray background timer running.

use std::time::Duration;

use scheduler::Instant;

use crate::{App, Context, FocusHandle, Task, Window, WindowId};

/// Half-period of the text cursor blink cycle (on for 530 ms, off for 530 ms).
pub(crate) const CURSOR_BLINK_INTERVAL: Duration = Duration::from_millis(530);

/// Returns `true` when `handle` is both focused and still mounted in the
/// window's most recently rendered dispatch tree.
pub(crate) fn is_mounted_and_focused(handle: &FocusHandle, window: &Window) -> bool {
    handle.is_focused(window)
        && window
            .rendered_frame
            .dispatch_tree
            .focusable_node_id(handle.id)
            .is_some()
}

/// Timer-driven caret blink controller owned by a text-editing entity.
pub(crate) struct CursorBlink {
    visible: bool,
    active: bool,
    window_id: Option<WindowId>,
    phase_start: Option<Instant>,
    _task: Option<Task<()>>,
}

impl Default for CursorBlink {
    fn default() -> Self {
        Self::new()
    }
}

impl CursorBlink {
    /// Creates a new idle [`CursorBlink`] starting in the visible phase.
    pub(crate) fn new() -> Self {
        Self {
            visible: true,
            active: false,
            window_id: None,
            phase_start: None,
            _task: None,
        }
    }

    /// Returns whether the caret should be painted on this frame.
    #[inline]
    pub(crate) fn is_visible(&self) -> bool {
        self.visible
    }

    /// Returns whether a blink timer is currently running.
    #[cfg(test)]
    #[inline]
    pub(crate) fn is_active(&self) -> bool {
        self.active
    }

    /// Returns `true` if [`Self::sync`] would transition the blink state.
    ///
    /// Useful for `RenderOnce` wrappers (like `FilterBar`) to avoid taking a
    /// mutable entity lease on steady frames.
    #[inline]
    pub(crate) fn needs_sync(&self, active: bool, window: &Window) -> bool {
        self.active != active
            || (active && self.window_id != Some(window.window_handle().window_id()))
    }

    /// Reconciles the running timer with whether the caret should be blinking
    /// in `window`.
    ///
    /// Must be called from `render()` (or right before painting). Cheap no-op
    /// on steady frames when the desired state hasn't changed.
    pub(crate) fn sync<T: 'static>(
        &mut self,
        active: bool,
        window: &Window,
        cx: &mut Context<T>,
        access: fn(&mut T) -> &mut Self,
        still_active: fn(&T, &Window, &App) -> bool,
    ) {
        let window_handle = window.window_handle();
        let window_id = window_handle.window_id();
        if !active {
            if self.active {
                self.stop();
            }
            return;
        }
        if self.active && self.window_id == Some(window_id) {
            return;
        }

        self.active = true;
        self.visible = true;
        self.window_id = Some(window_id);
        self.phase_start = Some(cx.background_executor().now());

        self._task = Some(cx.spawn(async move |weak, cx| {
            let mut wait = CURSOR_BLINK_INTERVAL;
            loop {
                cx.background_executor().timer(wait).await;
                let Ok(next_wait) = window_handle.update(cx, |_, window, cx| {
                    weak.update(cx, |entity, cx| {
                        if cx.reduce_motion() {
                            let blink = access(entity);
                            let was_hidden = !blink.visible;
                            blink.stop();
                            if was_hidden {
                                cx.notify();
                            }
                            return None;
                        }
                        if !still_active(entity, window, cx) {
                            access(entity).stop();
                            return None;
                        }
                        let now = cx.background_executor().now();
                        access(entity).tick(now, cx)
                    })
                    .ok()
                    .flatten()
                }) else {
                    // Window closed; stop blinking cleanly.
                    let _ = weak.update(cx, |entity, _| {
                        access(entity).stop();
                    });
                    break;
                };

                match next_wait {
                    Some(delay) => wait = delay,
                    None => break,
                }
            }
        }));
    }

    /// Shows the caret immediately and restarts the quiet period before the
    /// next blink-off.
    ///
    /// Returns `true` if the caret had been hidden (so callers that would
    /// otherwise skip `cx.notify()` on a no-op key can wake the caret up).
    pub(crate) fn reset(&mut self, cx: &App) -> bool {
        let was_hidden = !self.visible;
        self.visible = true;
        if self.active {
            self.phase_start = Some(cx.background_executor().now());
        }
        was_hidden
    }

    /// Stops blinking and cancels any pending timer, resetting `visible` to
    /// `true` so static rendering (e.g. `reduce_motion`) shows a solid caret.
    pub(crate) fn stop(&mut self) {
        self.active = false;
        self.visible = true;
        self.window_id = None;
        self.phase_start = None;
        self._task = None;
    }

    fn tick<T: 'static>(&mut self, now: Instant, cx: &mut Context<T>) -> Option<Duration> {
        if !self.active {
            return None;
        }
        let phase_start = self.phase_start.unwrap_or(now);
        let elapsed = now.saturating_duration_since(phase_start);
        if elapsed < CURSOR_BLINK_INTERVAL {
            return Some(CURSOR_BLINK_INTERVAL - elapsed);
        }
        self.visible = !self.visible;
        self.phase_start = Some(now);
        cx.notify();
        Some(CURSOR_BLINK_INTERVAL)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;
    use std::time::Duration;

    use super::*;
    use crate::components::filter_bar::{FilterBarState, FilterFacetOption, FilterFieldDef, filter_bar};
    use crate::components::input::{InputState, input_actions};
    use crate::components::interactive::PinInputState;
    use crate::components::textbox::{TextBoxState, textbox_actions};
    use crate::components::theme::{ActiveTheme, init_components};
    use crate::{
        Entity, EntityInputHandler, Focusable, Hsla, IntoElement, Render, TestAppContext,
        VisualTestContext, Window, div, prelude::*, px,
    };

    fn has_caret_quad(window: &Window, caret_color: Hsla, max_logical_width: f32) -> bool {
        let scale = window.scale_factor();
        let max_scaled_width = max_logical_width * scale + 1.0;
        window.rendered_frame.scene.quads.iter().any(|quad| {
            quad.background.solid == caret_color
                && quad.bounds.size.width.0 > 0.0
                && quad.bounds.size.width.0 <= max_scaled_width
                && quad.bounds.size.height.0 >= 10.0 * scale
        })
    }

    fn caret_painted(cx: &mut VisualTestContext, max_logical_width: f32) -> bool {
        cx.update(|window, cx| {
            let primary = cx.theme().primary;
            has_caret_quad(window, primary, max_logical_width)
        })
    }

    fn activate(cx: &mut VisualTestContext) {
        cx.update(|window, _| window.activate_window());
        cx.run_until_parked();
    }

    #[crate::test]
    fn input_cursor_blinks_resets_on_activity_and_stops_when_inactive(cx: &mut TestAppContext) {
        cx.update(init_components);
        let (input, cx) = cx.add_window_view(|_, cx| InputState::new(cx).default_value("hello"));
        activate(cx);

        // Unfocused: no blink timer, no caret quad.
        assert!(!input.read_with(cx, |i, _| i.blink().is_active()));
        assert!(!caret_painted(cx, 2.5));

        // Focus: caret starts solid and timer is armed.
        input.update_in(cx, |i, window, cx| window.focus(&i.focus_handle(cx), cx));
        cx.run_until_parked();
        assert!(input.read_with(cx, |i, _| i.blink().is_active()));
        assert!(input.read_with(cx, |i, _| i.blink().is_visible()));
        assert!(caret_painted(cx, 2.5));

        // Just before interval: still visible.
        cx.background_executor.advance_clock(CURSOR_BLINK_INTERVAL - Duration::from_millis(10));
        cx.run_until_parked();
        assert!(input.read_with(cx, |i, _| i.blink().is_visible()));
        assert!(caret_painted(cx, 2.5));

        // At interval: blinks off and caret quad disappears from scene.
        cx.background_executor.advance_clock(Duration::from_millis(10));
        cx.run_until_parked();
        assert!(!input.read_with(cx, |i, _| i.blink().is_visible()));
        assert!(!caret_painted(cx, 2.5));

        // Next interval: blinks back on.
        cx.background_executor.advance_clock(CURSOR_BLINK_INTERVAL);
        cx.run_until_parked();
        assert!(input.read_with(cx, |i, _| i.blink().is_visible()));
        assert!(caret_painted(cx, 2.5));

        // Typing mid-cycle resets the quiet period so the caret stays solid.
        cx.background_executor.advance_clock(Duration::from_millis(300));
        cx.run_until_parked();
        input.update_in(cx, |i, window, cx| {
            i.replace_text_in_range(None, "!", window, cx);
        });
        cx.run_until_parked();

        // 300 ms later (600 ms since last toggle, only 300 ms since typing): still visible!
        cx.background_executor.advance_clock(Duration::from_millis(300));
        cx.run_until_parked();
        assert!(input.read_with(cx, |i, _| i.blink().is_visible()));
        assert!(caret_painted(cx, 2.5));

        // After the remaining quiet period: blinks off.
        cx.background_executor.advance_clock(CURSOR_BLINK_INTERVAL - Duration::from_millis(300));
        cx.run_until_parked();
        assert!(!input.read_with(cx, |i, _| i.blink().is_visible()));
        assert!(!caret_painted(cx, 2.5));

        // Moving the cursor while hidden immediately wakes it up solid.
        cx.dispatch_action(input_actions::Left);
        cx.run_until_parked();
        assert!(input.read_with(cx, |i, _| i.blink().is_visible()));
        assert!(caret_painted(cx, 2.5));

        // Non-empty selection hides the caret and stops the timer.
        cx.dispatch_action(input_actions::SelectAll);
        cx.run_until_parked();
        assert!(!input.read_with(cx, |i, _| i.blink().is_active()));
        assert!(!caret_painted(cx, 2.5));

        // Collapsing selection resumes blinking starting solid.
        cx.dispatch_action(input_actions::Right);
        cx.run_until_parked();
        assert!(input.read_with(cx, |i, _| i.blink().is_active()));
        assert!(input.read_with(cx, |i, _| i.blink().is_visible()));
        assert!(caret_painted(cx, 2.5));

        // Window deactivation hides caret and stops timer; reactivation resumes.
        cx.deactivate_window();
        cx.run_until_parked();
        assert!(!input.read_with(cx, |i, _| i.blink().is_active()));
        assert!(!caret_painted(cx, 2.5));

        activate(cx);
        assert!(input.read_with(cx, |i, _| i.blink().is_active()));
        assert!(input.read_with(cx, |i, _| i.blink().is_visible()));
        assert!(caret_painted(cx, 2.5));

        // reduce_motion keeps the caret steadily visible with no timer.
        cx.update(|_, cx| cx.set_reduce_motion(true));
        cx.run_until_parked();
        assert!(!input.read_with(cx, |i, _| i.blink().is_active()));
        assert!(caret_painted(cx, 2.5));
        cx.background_executor.advance_clock(CURSOR_BLINK_INTERVAL * 3);
        cx.run_until_parked();
        assert!(caret_painted(cx, 2.5));

        cx.update(|_, cx| cx.set_reduce_motion(false));
        cx.run_until_parked();
        assert!(input.read_with(cx, |i, _| i.blink().is_active()));

        // Disabling stops blinking and hides the caret.
        input.update(cx, |i, cx| i.set_disabled(true, cx));
        cx.run_until_parked();
        assert!(!input.read_with(cx, |i, _| i.blink().is_active()));
        assert!(!caret_painted(cx, 2.5));

        // Re-enabling and blurring stops blinking cleanly.
        input.update(cx, |i, cx| i.set_disabled(false, cx));
        cx.run_until_parked();
        assert!(input.read_with(cx, |i, _| i.blink().is_active()));
        cx.update(|window, cx| window.blur(cx));
        cx.run_until_parked();
        assert!(!input.read_with(cx, |i, _| i.blink().is_active()));
        assert!(!caret_painted(cx, 2.5));
    }

    #[crate::test]
    fn input_blink_stops_when_unmounted_without_blur(cx: &mut TestAppContext) {
        cx.update(init_components);

        struct Host {
            show: bool,
            input: Entity<InputState>,
            renders: Rc<Cell<usize>>,
        }

        impl Render for Host {
            fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
                self.renders.set(self.renders.get() + 1);
                div()
                    .size_full()
                    .when(self.show, |el| el.child(self.input.clone()))
            }
        }

        let renders = Rc::new(Cell::new(0));
        let renders_ref = renders.clone();
        let (host, cx) = cx.add_window_view(move |_, cx| Host {
            show: true,
            input: cx.new(InputState::new),
            renders: renders_ref,
        });
        activate(cx);

        let input = host.read_with(cx, |h, _| h.input.clone());
        input.update_in(cx, |i, window, cx| window.focus(&i.focus_handle(cx), cx));
        cx.run_until_parked();
        assert!(input.read_with(cx, |i, _| i.blink().is_active()));

        // Hide the input without blurring its focus handle (simulates switching tabs).
        host.update(cx, |h, cx| {
            h.show = false;
            cx.notify();
        });
        cx.run_until_parked();

        // At the next tick the blink task sees the handle is no longer mounted and stops.
        cx.background_executor.advance_clock(CURSOR_BLINK_INTERVAL);
        cx.run_until_parked();
        assert!(!input.read_with(cx, |i, _| i.blink().is_active()));

        let renders_after_stop = renders.get();
        cx.background_executor.advance_clock(CURSOR_BLINK_INTERVAL * 4);
        cx.run_until_parked();
        assert_eq!(renders.get(), renders_after_stop);
    }

    #[crate::test]
    fn textbox_cursor_blinks_and_resets_on_edit_and_navigation(cx: &mut TestAppContext) {
        cx.update(init_components);
        let (tb, cx) = cx.add_window_view(|_, cx| {
            TextBoxState::new(cx)
                .default_value("line one\nline two")
                .rows(4)
        });
        activate(cx);

        tb.update_in(cx, |t, window, cx| window.focus(&t.focus_handle(cx), cx));
        cx.run_until_parked();
        assert!(tb.read_with(cx, |t, _| t.blink().is_active()));
        assert!(caret_painted(cx, 2.5));

        cx.background_executor.advance_clock(CURSOR_BLINK_INTERVAL);
        cx.run_until_parked();
        assert!(!tb.read_with(cx, |t, _| t.blink().is_visible()));
        assert!(!caret_painted(cx, 2.5));

        // Vertical navigation wakes the caret immediately.
        cx.dispatch_action(textbox_actions::Up);
        cx.run_until_parked();
        assert!(tb.read_with(cx, |t, _| t.blink().is_visible()));
        assert!(caret_painted(cx, 2.5));

        // Selecting all stops the blink timer; collapsing resumes it.
        cx.dispatch_action(textbox_actions::SelectAll);
        cx.run_until_parked();
        assert!(!tb.read_with(cx, |t, _| t.blink().is_active()));
        assert!(!caret_painted(cx, 2.5));

        cx.dispatch_action(textbox_actions::Right);
        cx.run_until_parked();
        assert!(tb.read_with(cx, |t, _| t.blink().is_active()));
        assert!(caret_painted(cx, 2.5));

        cx.update(|window, cx| window.blur(cx));
        cx.run_until_parked();
        assert!(!tb.read_with(cx, |t, _| t.blink().is_active()));
        assert!(!caret_painted(cx, 2.5));
    }

    #[crate::test]
    fn pin_input_cursor_blinks_only_in_empty_focused_slot(cx: &mut TestAppContext) {
        cx.update(init_components);
        let (pin, cx) = cx.add_window_view(|_, cx| PinInputState::new(4, cx));
        activate(cx);

        pin.update_in(cx, |p, window, cx| window.focus(&p.focus_handle(cx), cx));
        cx.run_until_parked();
        assert!(pin.read_with(cx, |p, _| p.blink().is_active()));
        assert!(caret_painted(cx, 2.5));

        cx.background_executor.advance_clock(CURSOR_BLINK_INTERVAL);
        cx.run_until_parked();
        assert!(!pin.read_with(cx, |p, _| p.blink().is_visible()));
        assert!(!caret_painted(cx, 2.5));

        // Typing a digit moves to the next empty slot with the caret solid.
        cx.simulate_keystrokes("1");
        cx.run_until_parked();
        assert!(pin.read_with(cx, |p, _| p.blink().is_visible()));
        assert!(caret_painted(cx, 2.5));

        // Filling every slot means no empty caret is shown, so the timer stops.
        cx.simulate_keystrokes("2 3 4");
        cx.run_until_parked();
        assert!(!pin.read_with(cx, |p, _| p.blink().is_active()));
        assert!(!caret_painted(cx, 2.5));

        // Backspace empties the last slot and resumes blinking.
        cx.simulate_keystrokes("backspace");
        cx.run_until_parked();
        assert!(pin.read_with(cx, |p, _| p.blink().is_active()));
        assert!(pin.read_with(cx, |p, _| p.blink().is_visible()));
        assert!(caret_painted(cx, 2.5));
    }

    #[crate::test]
    fn filter_bar_cursor_blinks_and_resets_on_typing(cx: &mut TestAppContext) {
        cx.update(init_components);

        struct FilterHost {
            filter: Entity<FilterBarState>,
        }

        impl Render for FilterHost {
            fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
                div()
                    .w(px(600.))
                    .p(px(16.))
                    .child(filter_bar("blink-fb", &self.filter))
            }
        }

        let (host, cx) = cx.add_window_view(|_, cx| {
            let filter = cx.new(|cx| {
                FilterBarState::new(
                    vec![FilterFieldDef::new("status", "Status")],
                    |_, _, _| vec![FilterFacetOption::new("ok", None)],
                    cx,
                )
            });
            FilterHost { filter }
        });
        activate(cx);

        let filter = host.read_with(cx, |h, _| h.filter.clone());
        filter.update_in(cx, |f, window, cx| window.focus(&f.focus_handle(cx), cx));
        cx.run_until_parked();
        assert!(filter.read_with(cx, |f, _| f.blink().is_active()));
        assert!(caret_painted(cx, 2.5));

        cx.background_executor.advance_clock(CURSOR_BLINK_INTERVAL);
        cx.run_until_parked();
        assert!(!filter.read_with(cx, |f, _| f.blink().is_visible()));
        assert!(!caret_painted(cx, 2.5));

        // Typing wakes the caret back up solid.
        cx.simulate_keystrokes("s");
        cx.run_until_parked();
        assert!(filter.read_with(cx, |f, _| f.blink().is_visible()));
        assert!(caret_painted(cx, 2.5));

        // Blurring stops the blink timer and hides the caret.
        cx.update(|window, cx| window.blur(cx));
        cx.run_until_parked();
        assert!(!filter.read_with(cx, |f, _| f.blink().is_active()));
        assert!(!caret_painted(cx, 2.5));
    }
}
