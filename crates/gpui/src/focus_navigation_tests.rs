//! Focus moves as in a browser: Tab and Shift+Tab go through the tab stops
//! (inside a focus trap, around it), and a mouse down focuses the focusable
//! element it's inside, or blurs the window when there's none.

use crate::{
    Context, FocusHandle, InteractiveElement, IntoElement, KeyDownEvent, Keystroke, Modifiers,
    MouseButton, ParentElement, Render, Styled, TestAppContext, VisualTestContext, Window,
    deferred, div, point, px,
};

/// A column of 20 px rows, 100 px wide in a 200 px wide window: `a` (a tab
/// stop, with a popup at y 400 that's its child), `b` (focusable, not a
/// tab stop), `c` (a tab stop, with a deferred popup 450 px below its top, at y 490), then a
/// focus trap holding the tab stops `d` and `e`.
struct Fields {
    a: FocusHandle,
    b: FocusHandle,
    c: FocusHandle,
    trap: FocusHandle,
    d: FocusHandle,
    e: FocusHandle,
    stop_tab_in_c: bool,
}

impl Render for Fields {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let row = |handle: &FocusHandle| div().track_focus(handle).w(px(100.)).h(px(20.));
        let stop_tab = self.stop_tab_in_c;
        div()
            .size_full()
            .relative()
            .child(
                row(&self.a).relative().child(
                    div()
                        .absolute()
                        .top(px(400.))
                        .w(px(50.))
                        .h(px(20.))
                        .id("popup")
                        .occlude(),
                ),
            )
            .child(row(&self.b))
            .child(
                row(&self.c)
                    .on_key_down(move |event, window, cx| {
                        if stop_tab && event.keystroke.key == "tab" {
                            window.prevent_default();
                            cx.stop_propagation();
                        }
                    })
                    .child(deferred(
                        div()
                            .absolute()
                            .top(px(450.))
                            .w(px(50.))
                            .h(px(20.))
                            .id("deferred-popup")
                            .occlude(),
                    )),
            )
            .child(
                div()
                    .track_focus(&self.trap)
                    .focus_trap()
                    .child(row(&self.d))
                    .child(row(&self.e)),
            )
    }
}

fn setup(
    cx: &mut TestAppContext,
    stop_tab_in_c: bool,
) -> (&mut VisualTestContext, Vec<FocusHandle>) {
    let (view, cx) = cx.add_window_view(move |_, cx| Fields {
        a: cx.focus_handle().tab_stop(true),
        b: cx.focus_handle(),
        c: cx.focus_handle().tab_stop(true),
        trap: cx.focus_handle(),
        d: cx.focus_handle().tab_stop(true),
        e: cx.focus_handle().tab_stop(true),
        stop_tab_in_c,
    });
    cx.run_until_parked();
    let handles = cx.update(|_, cx| {
        let view = view.read(cx);
        vec![
            view.a.clone(),
            view.b.clone(),
            view.c.clone(),
            view.trap.clone(),
            view.d.clone(),
            view.e.clone(),
        ]
    });
    (cx, handles)
}

fn focused(cx: &mut VisualTestContext, handles: &[FocusHandle]) -> Option<usize> {
    cx.update(|window, _| handles.iter().position(|h| h.is_focused(window)))
}

fn tab(cx: &mut VisualTestContext, shift: bool) {
    cx.simulate_event(KeyDownEvent {
        keystroke: Keystroke {
            modifiers: if shift {
                Modifiers::shift()
            } else {
                Modifiers::none()
            },
            key: "tab".into(),
            key_char: None,
        },
        is_held: false,
        prefer_character_input: false,
    });
    cx.run_until_parked();
}

fn press(cx: &mut VisualTestContext, x: f32, y: f32) {
    cx.simulate_mouse_down(point(px(x), px(y)), MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
}

const A: usize = 0;
const B: usize = 1;
const C: usize = 2;
const D: usize = 4;
const E: usize = 5;

#[crate::test]
fn tab_goes_through_the_tab_stops_and_shift_tab_back(cx: &mut TestAppContext) {
    let (cx, h) = setup(cx, false);
    tab(cx, false);
    assert_eq!(focused(cx, &h), Some(A));
    tab(cx, false);
    assert_eq!(focused(cx, &h), Some(C), "b isn't a tab stop");
    tab(cx, true);
    assert_eq!(focused(cx, &h), Some(A));
}

#[crate::test]
fn tab_stays_inside_a_focus_trap(cx: &mut TestAppContext) {
    let (cx, h) = setup(cx, false);
    press(cx, 10., 70.);
    assert_eq!(focused(cx, &h), Some(D));
    tab(cx, false);
    assert_eq!(focused(cx, &h), Some(E));
    tab(cx, false);
    assert_eq!(focused(cx, &h), Some(D), "wraps around inside the trap");
    tab(cx, true);
    assert_eq!(focused(cx, &h), Some(E));
}

#[crate::test]
fn a_handled_tab_doesnt_move_focus(cx: &mut TestAppContext) {
    let (cx, h) = setup(cx, true);
    press(cx, 10., 50.);
    assert_eq!(focused(cx, &h), Some(C));
    tab(cx, false);
    assert_eq!(focused(cx, &h), Some(C));
}

#[crate::test]
fn pressing_outside_blurs(cx: &mut TestAppContext) {
    let (cx, h) = setup(cx, false);
    press(cx, 10., 10.);
    assert_eq!(focused(cx, &h), Some(A));
    press(cx, 150., 10.);
    assert_eq!(focused(cx, &h), None);
}

#[crate::test]
fn pressing_a_focused_elements_popup_keeps_its_focus(cx: &mut TestAppContext) {
    let (cx, h) = setup(cx, false);
    press(cx, 10., 10.);
    press(cx, 10., 410.);
    assert_eq!(focused(cx, &h), Some(A), "the popup is a's child");
    press(cx, 10., 50.);
    assert_eq!(focused(cx, &h), Some(C));
    press(cx, 10., 500.);
    assert_eq!(focused(cx, &h), Some(C), "the deferred popup is c's child");
}

#[crate::test]
fn pressing_inside_a_focusable_container_focuses_it(cx: &mut TestAppContext) {
    let (cx, h) = setup(cx, false);
    press(cx, 10., 10.);
    // The trap is 100 px wide like its rows, so press it between them: there
    // is no gap, so press a row, then check the row (the nearest) took it.
    press(cx, 10., 90.);
    assert_eq!(focused(cx, &h), Some(E));
}
