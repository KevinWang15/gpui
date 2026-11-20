//! Focus moves as in a browser: Tab and Shift+Tab go through the tab stops
//! (inside a focus trap, around it), and a mouse down focuses the focusable
//! element it's inside, or blurs the window when there's none.

use crate::{
    Context, FocusHandle, InteractiveElement, IntoElement, KeyDownEvent, Keystroke, Modifiers,
    MouseButton, ParentElement, Render, StatefulInteractiveElement, Styled, TestAppContext,
    VisualTestContext, Window,
    deferred, div, point, prelude::FluentBuilder as _, px,
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

/// A focused button inside a parent that takes Enter: `parent_takes_enter`
/// decides whether the parent stops Enter's key down.
struct EnterPage {
    button: FocusHandle,
    parent_takes_enter: bool,
    clicks: std::rc::Rc<std::cell::Cell<u32>>,
}

impl Render for EnterPage {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let takes = self.parent_takes_enter;
        let clicks = self.clicks.clone();
        div()
            .on_key_down(move |event, _, cx| {
                if takes && event.keystroke.key == "enter" {
                    cx.stop_propagation();
                }
            })
            .child(
                div()
                    .id("button")
                    .track_focus(&self.button)
                    .w(px(50.))
                    .h(px(20.))
                    .on_click(move |_, _, _| clicks.set(clicks.get() + 1)),
            )
    }
}

fn press_enter(cx: &mut TestAppContext, parent_takes_enter: bool) -> u32 {
    let clicks = std::rc::Rc::new(std::cell::Cell::new(0));
    let c = clicks.clone();
    let (view, cx) = cx.add_window_view(move |_, cx| EnterPage {
        button: cx.focus_handle(),
        parent_takes_enter,
        clicks: c,
    });
    cx.update(|window, cx| {
        let handle = view.read(cx).button.clone();
        window.focus(&handle, cx);
    });
    cx.run_until_parked();
    let keystroke = Keystroke::parse("enter").unwrap();
    cx.simulate_event(KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    cx.simulate_event(crate::KeyUpEvent { keystroke });
    cx.run_until_parked();
    clicks.get()
}

#[crate::test]
fn enter_clicks_the_focused_element_unless_something_took_it(cx: &mut TestAppContext) {
    assert_eq!(press_enter(cx, false), 1);
}

#[crate::test]
fn an_enter_a_parent_took_doesnt_click(cx: &mut TestAppContext) {
    assert_eq!(press_enter(cx, true), 0);
}

/// Three tab stops in a column with arrow navigation, then one outside it.
/// The second one takes Down itself when `second_takes_down`.
struct ArrowList {
    rows: Vec<FocusHandle>,
    outside: FocusHandle,
    second_takes_down: bool,
}

impl Render for ArrowList {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let takes = self.second_takes_down;
        div()
            .child(
                // No id needed.
                div()
                    .arrow_navigation(crate::ArrowNavigation::Vertical)
                    .children(self.rows.iter().enumerate().map(|(i, h)| {
                        div()
                            .track_focus(h)
                            .h(px(20.))
                            .when(i == 1 && takes, |d| {
                                d.on_key_down(|e, _, cx| {
                                    if e.keystroke.key == "down" {
                                        cx.stop_propagation();
                                    }
                                })
                            })
                    })),
            )
            .child(div().track_focus(&self.outside).h(px(20.)))
    }
}

fn arrow_setup(cx: &mut TestAppContext, second_takes_down: bool) -> (&mut VisualTestContext, Vec<FocusHandle>) {
    let (view, cx) = cx.add_window_view(move |_, cx| ArrowList {
        rows: (0..3).map(|_| cx.focus_handle().tab_stop(true)).collect(),
        outside: cx.focus_handle().tab_stop(true),
        second_takes_down,
    });
    cx.run_until_parked();
    let mut handles = cx.update(|_, cx| view.read(cx).rows.clone());
    handles.push(cx.update(|_, cx| view.read(cx).outside.clone()));
    (cx, handles)
}

fn key(cx: &mut VisualTestContext, key: &'static str) {
    cx.simulate_event(KeyDownEvent {
        keystroke: Keystroke::parse(key).unwrap(),
        is_held: false,
        prefer_character_input: false,
    });
    cx.run_until_parked();
}

#[crate::test]
fn arrows_move_focus_inside_a_group(cx: &mut TestAppContext) {
    let (cx, h) = arrow_setup(cx, false);
    tab(cx, false);
    assert_eq!(focused(cx, &h), Some(0));
    key(cx, "down");
    assert_eq!(focused(cx, &h), Some(1));
    key(cx, "down");
    key(cx, "down");
    assert_eq!(focused(cx, &h), Some(0), "wraps inside the group, never to the row outside");
    key(cx, "up");
    assert_eq!(focused(cx, &h), Some(2));
    key(cx, "home");
    assert_eq!(focused(cx, &h), Some(0));
    key(cx, "end");
    assert_eq!(focused(cx, &h), Some(2));
    key(cx, "left");
    assert_eq!(focused(cx, &h), Some(2), "a vertical group ignores Left");
}

#[crate::test]
fn an_arrow_the_item_handles_stays_with_it(cx: &mut TestAppContext) {
    let (cx, h) = arrow_setup(cx, true);
    tab(cx, false);
    key(cx, "down");
    assert_eq!(focused(cx, &h), Some(1));
    key(cx, "down");
    assert_eq!(focused(cx, &h), Some(1));
}
