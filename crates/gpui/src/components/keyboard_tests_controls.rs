//! Keyboard focus tests for the controls (see `keyboard_tests.rs` for the harness).

use std::cell::RefCell;
use std::rc::Rc;

use super::keyboard_tests::keys;
use crate::components::*;
use crate::{Context, IntoElement, Render, TestAppContext, Window, prelude::*};

#[derive(Default)]
struct Page {
    log: Rc<RefCell<Vec<&'static str>>>,
    checked: bool,
    switch_on: bool,
    switch_changes: usize,
    radio: usize,
    tab: usize,
    open: bool,
    rating: usize,
}

impl Render for Page {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let log = |name: &'static str, log: &Rc<RefCell<Vec<&'static str>>>| {
            let log = log.clone();
            move |_: &bool, _: &mut Window, _: &mut crate::App| log.borrow_mut().push(name)
        };
        // In tab order: every disabled control sits between two enabled ones,
        // so a test that counts Tabs fails if Tab stops on one.
        v_stack()
            .child(
                checkbox("check", self.checked)
                    .label("Check")
                    .on_change(cx.listener(|this, value: &bool, _, cx| {
                        this.checked = *value;
                        cx.notify();
                    })),
            )
            .child(
                checkbox("check-off", false)
                    .label("Disabled")
                    .disabled(true)
                    .on_change(log("check-off", &self.log)),
            )
            .child(
                switch("switch", self.switch_on).on_change(cx.listener(
                    |this, value: &bool, _, cx| {
                        this.switch_on = *value;
                        this.switch_changes += 1;
                        cx.notify();
                    },
                )),
            )
            .child(
                switch("switch-off", false)
                    .disabled(true)
                    .on_change(log("switch-off", &self.log)),
            )
            .child(
                radio_group(
                    "radio",
                    vec![
                        RadioOption::new(0usize, "Zero"),
                        RadioOption::new(1usize, "One"),
                        RadioOption::new(2usize, "Two"),
                    ],
                )
                .selected(Some(self.radio))
                .on_change(cx.listener(|this, value: &usize, _, cx| {
                    this.radio = *value;
                    cx.notify();
                })),
            )
            .child(
                tabs(
                    "tabs",
                    vec![
                        TabItem::new("A"),
                        TabItem::new("B").disabled(true),
                        TabItem::new("C"),
                    ],
                )
                .selected_index(self.tab)
                .on_change(cx.listener(|this, index: &usize, _, cx| {
                    this.tab = *index;
                    cx.notify();
                })),
            )
            .child(
                disclosure("faq", "Question", self.open)
                    .on_toggle(cx.listener(|this, open: &bool, _, cx| {
                        this.open = *open;
                        cx.notify();
                    }))
                    .child("Answer"),
            )
            .child(
                rating("rating", self.rating).on_change(cx.listener(
                    |this, value: &usize, _, cx| {
                        this.rating = *value;
                        cx.notify();
                    },
                )),
            )
            .child(button("last").label("Last").on_click({
                let log = self.log.clone();
                move |_, _, _| log.borrow_mut().push("last")
            }))
    }
}

fn setup(cx: &mut TestAppContext) -> (crate::Entity<Page>, &mut crate::VisualTestContext) {
    cx.update(init_components);
    let (view, cx) = cx.add_window_view(|_, _| Page::default());
    cx.run_until_parked();
    (view, cx)
}

#[crate::test]
fn space_toggles_a_checkbox_reached_by_tab(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx);
    keys(cx, "tab space");
    assert!(view.read_with(cx, |p, _| p.checked));
    keys(cx, "enter");
    assert!(!view.read_with(cx, |p, _| p.checked));
}

#[crate::test]
fn space_toggles_a_switch_once(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx);
    // The disabled checkbox between them isn't a tab stop.
    keys(cx, "tab tab space");
    assert_eq!(view.read_with(cx, |p, _| (p.switch_on, p.switch_changes)), (true, 1));
    keys(cx, "space");
    assert_eq!(view.read_with(cx, |p, _| (p.switch_on, p.switch_changes)), (false, 2));
    assert!(!view.read_with(cx, |p, _| p.checked));
}

#[crate::test]
fn radio_group_and_tabs_are_one_tab_stop_each(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx);
    // Checkbox, switch, then the radio group (the disabled switch is skipped).
    keys(cx, "tab tab tab down");
    assert_eq!(view.read_with(cx, |p, _| p.radio), 1);
    keys(cx, "down down");
    assert_eq!(view.read_with(cx, |p, _| p.radio), 0, "wraps around");
    keys(cx, "up");
    assert_eq!(view.read_with(cx, |p, _| p.radio), 2);
    // One Tab leaves the radio group for the tabs; right skips disabled B.
    keys(cx, "tab right");
    assert_eq!(view.read_with(cx, |p, _| (p.radio, p.tab)), (2, 2));
    keys(cx, "home");
    assert_eq!(view.read_with(cx, |p, _| p.tab), 0);
    // The selected tab is the tab stop, so Tab and Shift+Tab come back to it.
    keys(cx, "tab shift-tab left");
    assert_eq!(view.read_with(cx, |p, _| p.tab), 2);
}

#[crate::test]
fn enter_toggles_an_accordion_header(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx);
    keys(cx, "tab tab tab tab tab enter");
    assert!(view.read_with(cx, |p, _| p.open));
    keys(cx, "space");
    assert!(!view.read_with(cx, |p, _| p.open));
}

#[crate::test]
fn rating_is_one_tab_stop_with_arrows(cx: &mut TestAppContext) {
    let (view, cx) = setup(cx);
    for _ in 0..6 {
        keys(cx, "tab");
    }
    keys(cx, "right");
    assert_eq!(view.read_with(cx, |p, _| p.rating), 2);
    keys(cx, "end");
    assert_eq!(view.read_with(cx, |p, _| p.rating), 5);
    // Tab leaves the stars for the button after them; nothing disabled
    // fired along the way.
    keys(cx, "tab enter");
    let log = view.read_with(cx, |p, _| p.log.borrow().clone());
    assert_eq!(log, ["last"]);
}
