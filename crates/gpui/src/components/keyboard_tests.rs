//! Keyboard focus through the components (`focus` module docs): Tab and
//! Shift+Tab, choice groups' arrow keys, modal dialogs.

use std::cell::RefCell;
use std::rc::Rc;

use crate::components::*;
use crate::{Context, IntoElement, Render, SharedString, TestAppContext, Window, prelude::*};

struct Page {
    log: Rc<RefCell<Vec<&'static str>>>,
    choice: SharedString,
    open: bool,
}

impl Render for Page {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let log = |name: &'static str, log: &Rc<RefCell<Vec<&'static str>>>| {
            let log = log.clone();
            move |_: &crate::ClickEvent, _: &mut Window, _: &mut crate::App| {
                log.borrow_mut().push(name)
            }
        };
        crate::div()
            .size_full()
            .child(
                v_stack()
                    .child(
                        button("first")
                            .label("First")
                            .on_click(log("first", &self.log)),
                    )
                    .child(
                        segmented_control("choice", self.choice.clone())
                            .item(SegmentedItem::new("a", "A"))
                            .item(SegmentedItem::new("b", "B"))
                            .item(SegmentedItem::new("c", "C"))
                            .on_change(cx.listener(|this, value: &SharedString, _, cx| {
                                this.choice = value.clone();
                                cx.notify();
                            })),
                    )
                    .child(button("open").label("Open").on_click({
                        let log = self.log.clone();
                        cx.listener(move |this, _, _, cx| {
                            log.borrow_mut().push("open");
                            this.open = true;
                            cx.notify();
                        })
                    })),
            )
            .child(
                dialog("dialog", self.open)
                    .title("Dialog")
                    .on_close(cx.listener(|this, _, _, cx| {
                        this.open = false;
                        cx.notify();
                    }))
                    .child(
                        button("inside-1")
                            .label("One")
                            .on_click(log("inside-1", &self.log)),
                    )
                    .child(
                        button("inside-2")
                            .label("Two")
                            .on_click(log("inside-2", &self.log)),
                    ),
            )
    }
}

fn setup(
    cx: &mut TestAppContext,
) -> (
    crate::Entity<Page>,
    &mut crate::VisualTestContext,
    Rc<RefCell<Vec<&'static str>>>,
) {
    cx.update(init_components);
    let log = Rc::new(RefCell::new(Vec::new()));
    let l = log.clone();
    let (view, cx) = cx.add_window_view(move |_, _| Page {
        log: l,
        choice: "a".into(),
        open: false,
    });
    cx.run_until_parked();
    (view, cx, log)
}

/// Presses and releases each key (a keyboard click happens on the release).
pub(super) fn keys(cx: &mut crate::VisualTestContext, keys: &'static str) {
    for key in keys.split(' ') {
        let keystroke = crate::Keystroke::parse(key).unwrap();
        cx.simulate_event(crate::KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        cx.simulate_event(crate::KeyUpEvent { keystroke });
        cx.run_until_parked();
    }
}

#[crate::test]
fn tab_reaches_buttons_and_enter_presses_them(cx: &mut TestAppContext) {
    let (_, cx, log) = setup(cx);
    keys(cx, "tab enter");
    assert_eq!(*log.borrow(), ["first"]);
    keys(cx, "tab tab enter");
    assert_eq!(
        *log.borrow(),
        ["first", "open"],
        "the segments are one tab stop"
    );
    // Close the dialog Open opened; focus goes back to Open.
    keys(cx, "escape shift-tab shift-tab enter");
    assert_eq!(log.borrow().last(), Some(&"first"));
}

#[crate::test]
fn arrows_move_a_choice_groups_selection(cx: &mut TestAppContext) {
    let (view, cx, log) = setup(cx);
    keys(cx, "tab tab right");
    assert_eq!(view.read_with(cx, |p, _| p.choice.clone()), "b");
    keys(cx, "right right");
    assert_eq!(
        view.read_with(cx, |p, _| p.choice.clone()),
        "a",
        "wraps around"
    );
    keys(cx, "left");
    assert_eq!(view.read_with(cx, |p, _| p.choice.clone()), "c");
    // The selected segment is the group's tab stop, so Tab leaves the group.
    keys(cx, "tab enter");
    assert_eq!(*log.borrow(), ["open"]);
}

#[crate::test]
fn a_dialog_keeps_tab_inside_and_gives_focus_back(cx: &mut TestAppContext) {
    let (view, cx, log) = setup(cx);
    keys(cx, "tab tab tab enter");
    assert!(view.read_with(cx, |p, _| p.open));
    for _ in 0..7 {
        keys(cx, "tab");
    }
    keys(cx, "shift-tab");
    // Wherever Tab went, it stayed in the dialog.
    keys(cx, "enter");
    let last = *log.borrow().last().unwrap();
    assert!(
        last != "first" && (last != "open" || log.borrow().len() == 1),
        "{:?}",
        log.borrow()
    );
    if view.read_with(cx, |p, _| p.open) {
        keys(cx, "escape");
    }
    assert!(!view.read_with(cx, |p, _| p.open));
    // Focus is back on the Open button.
    log.borrow_mut().clear();
    keys(cx, "enter");
    assert_eq!(*log.borrow(), ["open"]);
}
