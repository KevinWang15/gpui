//! Keyboard focus tests for the fields (see `keyboard_tests.rs` for the harness).

use std::cell::RefCell;
use std::rc::Rc;

use super::keyboard_tests::keys;
use crate::components::*;
use crate::{
    Context, Entity, Focusable, IntoElement, Modifiers, Render, TestAppContext, VisualTestContext,
    Window, point, prelude::*, px,
};

struct Fields {
    log: Rc<RefCell<Vec<&'static str>>>,
    input: Entity<InputState>,
    select: Entity<SelectState<u32>>,
    slider: Entity<SliderState>,
    range: Entity<RangeSliderState>,
    number: Entity<NumberInputState>,
    combo: Entity<ComboBoxState<u32>>,
    drawer_open: bool,
}

impl Render for Fields {
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
                    .child(input(&self.input))
                    .child(
                        crate::div()
                            .debug_selector(|| "select".into())
                            .child(select(&self.select)),
                    )
                    .child(slider(&self.slider))
                    .child(range_slider(&self.range))
                    .child(number_input(&self.number))
                    .child(combo_box(&self.combo))
                    .child(
                        button("after")
                            .label("After")
                            .on_click(log("after", &self.log)),
                    )
                    .child(button("open").label("Open").on_click({
                        let log = self.log.clone();
                        cx.listener(move |this, _, _, cx| {
                            log.borrow_mut().push("open");
                            this.drawer_open = true;
                            cx.notify();
                        })
                    })),
            )
            .child(
                drawer("drawer", self.drawer_open)
                    .title("Drawer")
                    .on_close(cx.listener(|this, _, _, cx| {
                        this.drawer_open = false;
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
    Entity<Fields>,
    &mut VisualTestContext,
    Rc<RefCell<Vec<&'static str>>>,
) {
    cx.update(init_components);
    let log = Rc::new(RefCell::new(Vec::new()));
    let l = log.clone();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let options = (0..3)
            .map(|i| SelectOption::new(i, format!("Option {i}")))
            .collect();
        let combo_options = (0..3)
            .map(|i| ComboBoxOption::new(i, format!("Choice {i}")))
            .collect();
        Fields {
            log: l,
            input: cx.new(InputState::new),
            select: cx.new(|cx| SelectState::new(options, cx)),
            slider: cx.new(SliderState::new),
            range: cx.new(RangeSliderState::new),
            number: cx.new(NumberInputState::new),
            combo: cx.new(|cx| ComboBoxState::new(combo_options, cx)),
            drawer_open: false,
        }
    });
    cx.run_until_parked();
    (view, cx, log)
}

fn focused<T: Focusable>(cx: &mut VisualTestContext, entity: &Entity<T>) -> bool {
    cx.update(|window, cx| entity.read(cx).focus_handle(cx).is_focused(window))
}

fn input_focused(cx: &mut VisualTestContext, view: &Entity<Fields>) -> bool {
    let input = view.read_with(cx, |p, _| p.input.clone());
    focused(cx, &input)
}

fn select_state(cx: &mut VisualTestContext, view: &Entity<Fields>) -> Entity<SelectState<u32>> {
    view.read_with(cx, |p, _| p.select.clone())
}

#[crate::test]
fn tab_visits_each_field_once(cx: &mut TestAppContext) {
    let (view, cx, log) = setup(cx);
    let (select, slider, range, number) = view.read_with(cx, |p, _| {
        (
            p.select.clone(),
            p.slider.clone(),
            p.range.clone(),
            p.number.clone(),
        )
    });
    keys(cx, "tab");
    assert!(input_focused(cx, &view));
    keys(cx, "tab");
    assert!(focused(cx, &select));
    keys(cx, "tab");
    assert!(focused(cx, &slider));
    keys(cx, "right");
    assert_eq!(slider.read_with(cx, |s, _| s.value()), 51.0);
    // The range slider's thumbs are two stops: start, then end.
    keys(cx, "tab right");
    assert_eq!(range.read_with(cx, |s, _| s.values()), (21.0, 80.0));
    keys(cx, "tab right");
    assert_eq!(range.read_with(cx, |s, _| s.values()), (21.0, 81.0));
    // The number input is one stop (its text field); Up steps it.
    keys(cx, "tab up");
    assert_eq!(number.read_with(cx, |s, _| s.value()), 1.0);
    // The combobox is one stop, then the button after it.
    keys(cx, "tab tab enter");
    assert_eq!(*log.borrow(), ["after"]);
    // And back: Shift+Tab lands on the range slider's end thumb first.
    keys(cx, "shift-tab shift-tab shift-tab left");
    assert_eq!(range.read_with(cx, |s, _| s.values()), (21.0, 80.0));
    keys(cx, "shift-tab left");
    assert_eq!(range.read_with(cx, |s, _| s.values()), (20.0, 80.0));
    keys(cx, "shift-tab");
    assert!(focused(cx, &slider));
}

#[crate::test]
fn a_combobox_opens_from_the_keyboard_and_tab_leaves_it(cx: &mut TestAppContext) {
    let (view, cx, log) = setup(cx);
    let combo = view.read_with(cx, |p, _| p.combo.clone());
    // Input, select, slider, two thumbs, number input, combobox.
    keys(cx, "tab tab tab tab tab tab tab");
    assert!(focused(cx, &combo));
    keys(cx, "down");
    assert!(combo.read_with(cx, |c, _| c.is_open()));
    keys(cx, "tab");
    assert!(!combo.read_with(cx, |c, _| c.is_open()));
    keys(cx, "enter");
    assert_eq!(*log.borrow(), ["after"]);
}

#[crate::test]
fn escape_closes_a_select_and_keeps_focus_on_it(cx: &mut TestAppContext) {
    let (view, cx, _) = setup(cx);
    let select = select_state(cx, &view);
    keys(cx, "tab tab down");
    assert!(select.read_with(cx, |s, _| s.is_open()));
    keys(cx, "escape");
    assert!(!select.read_with(cx, |s, _| s.is_open()));
    assert!(focused(cx, &select));
    // Enter opens it again (and the key-up click doesn't close it).
    keys(cx, "enter");
    assert!(select.read_with(cx, |s, _| s.is_open()));
    keys(cx, "down enter");
    assert!(!select.read_with(cx, |s, _| s.is_open()));
    assert_eq!(select.read_with(cx, |s, _| s.selected_index()), Some(1));
}

#[crate::test]
fn tab_from_an_open_select_closes_it_and_moves_on(cx: &mut TestAppContext) {
    let (view, cx, _) = setup(cx);
    let (select, slider) = view.read_with(cx, |p, _| (p.select.clone(), p.slider.clone()));
    keys(cx, "tab tab down");
    assert!(select.read_with(cx, |s, _| s.is_open()));
    keys(cx, "tab");
    assert!(!select.read_with(cx, |s, _| s.is_open()));
    assert!(focused(cx, &slider));
    keys(cx, "shift-tab down shift-tab");
    assert!(!select.read_with(cx, |s, _| s.is_open()));
    assert!(input_focused(cx, &view));
}

#[crate::test]
fn clicking_an_open_selects_option_selects_it(cx: &mut TestAppContext) {
    let (view, cx, _) = setup(cx);
    let select = select_state(cx, &view);
    keys(cx, "tab tab down");
    assert!(select.read_with(cx, |s, _| s.is_open()));
    let trigger = cx.debug_bounds("select").expect("select bounds");
    // The menu opens 4px under the trigger, with 4px of padding.
    let first_option = point(trigger.left() + px(20.), trigger.bottom() + px(16.));
    cx.simulate_click(first_option, Modifiers::none());
    assert!(!select.read_with(cx, |s, _| s.is_open()));
    assert_eq!(select.read_with(cx, |s, _| s.selected_index()), Some(0));
    // The click kept focus on the select.
    assert!(focused(cx, &select));
}

#[crate::test]
fn pointer_open_select_does_not_prehighlight_and_hover_tracks_single_row(cx: &mut TestAppContext) {
    let (view, cx, _) = setup(cx);
    let select = select_state(cx, &view);
    let trigger = cx.debug_bounds("select").expect("select bounds");
    let trigger_center = point(
        trigger.left() + trigger.size.width / 2.,
        trigger.top() + trigger.size.height / 2.,
    );
    // Opening via mouse click leaves no option falsely pre-highlighted.
    cx.simulate_click(trigger_center, Modifiers::none());
    assert!(select.read_with(cx, |s, _| s.is_open()));
    assert_eq!(select.read_with(cx, |s, _| s.highlighted_index()), None);

    // Moving the pointer over the second option highlights only that option.
    let second_option = point(trigger.left() + px(20.), trigger.bottom() + px(48.));
    cx.simulate_mouse_move(second_option, None, Modifiers::none());
    assert_eq!(select.read_with(cx, |s, _| s.highlighted_index()), Some(1));

    // Moving away from the menu clears the highlight cleanly.
    cx.simulate_mouse_move(point(px(2.), px(2.)), None, Modifiers::none());
    assert_eq!(select.read_with(cx, |s, _| s.highlighted_index()), None);

    // Arrow key from None steps to the first enabled option.
    keys(cx, "down");
    assert_eq!(select.read_with(cx, |s, _| s.highlighted_index()), Some(0));
}

#[crate::test]
fn a_drawer_keeps_tab_inside_and_gives_focus_back(cx: &mut TestAppContext) {
    let (view, cx, log) = setup(cx);
    // Input, select, slider, two thumbs, number input, combobox, After, Open.
    for _ in 0..9 {
        keys(cx, "tab");
    }
    keys(cx, "enter");
    assert!(view.read_with(cx, |p, _| p.drawer_open));
    for _ in 0..7 {
        keys(cx, "tab");
        keys(cx, "enter");
        if !view.read_with(cx, |p, _| p.drawer_open) {
            // That was the close button; reopen and carry on.
            keys(cx, "enter");
        }
    }
    // Every press landed inside the drawer (or on Open, reopening it).
    assert!(
        log.borrow()
            .iter()
            .all(|name| ["open", "inside-1", "inside-2"].contains(name)),
        "{:?}",
        log.borrow()
    );
    keys(cx, "escape");
    assert!(!view.read_with(cx, |p, _| p.drawer_open));
    log.borrow_mut().clear();
    keys(cx, "enter");
    assert_eq!(*log.borrow(), ["open"]);
}

struct Menus {
    log: Rc<RefCell<Vec<&'static str>>>,
    dropdown_open: bool,
    bar: Entity<MenuState>,
}

impl Render for Menus {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pick = |name: &'static str, log: &Rc<RefCell<Vec<&'static str>>>| {
            let log = log.clone();
            move |_: &crate::ClickEvent, _: &mut Window, _: &mut crate::App| {
                log.borrow_mut().push(name)
            }
        };
        v_stack()
            .child(
                dropdown_menu("menu", self.dropdown_open)
                    .trigger(button("trigger").label("Actions").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.dropdown_open = !this.dropdown_open;
                            cx.notify();
                        },
                    )))
                    .on_close(cx.listener(|this, _, _, cx| {
                        this.dropdown_open = false;
                        cx.notify();
                    }))
                    .entry(MenuEntry::item("Copy", pick("copy", &self.log)))
                    .entry(MenuEntry::item("Paste", pick("paste", &self.log))),
            )
            .child(nav_menu(&self.bar))
            .child(
                button("after")
                    .label("After")
                    .on_click(pick("after", &self.log)),
            )
    }
}

fn setup_menus(
    cx: &mut TestAppContext,
) -> (
    Entity<Menus>,
    &mut VisualTestContext,
    Rc<RefCell<Vec<&'static str>>>,
) {
    cx.update(init_components);
    let log = Rc::new(RefCell::new(Vec::new()));
    let l = log.clone();
    let (view, cx) = cx.add_window_view(move |_, cx| Menus {
        log: l,
        dropdown_open: false,
        bar: cx.new(|cx| {
            MenuState::new(
                vec![
                    MenuNode::item("home", "Home"),
                    MenuNode::submenu(
                        "more",
                        "More",
                        [MenuNode::item("a", "A"), MenuNode::item("b", "B")],
                    ),
                    MenuNode::item("about", "About"),
                ],
                cx,
            )
            .mode(MenuMode::Horizontal)
        }),
    });
    cx.run_until_parked();
    (view, cx, log)
}

#[crate::test]
fn a_dropdown_menu_takes_the_arrow_keys_and_gives_focus_back(cx: &mut TestAppContext) {
    let (view, cx, log) = setup_menus(cx);
    keys(cx, "tab enter");
    assert!(view.read_with(cx, |m, _| m.dropdown_open));
    keys(cx, "down down enter");
    assert_eq!(*log.borrow(), ["paste"]);
    assert!(!view.read_with(cx, |m, _| m.dropdown_open));
    // Focus is back on the trigger: Enter opens the menu again.
    keys(cx, "enter");
    assert!(view.read_with(cx, |m, _| m.dropdown_open));
    keys(cx, "escape");
    assert!(!view.read_with(cx, |m, _| m.dropdown_open));
    // Tab from an open menu closes it and moves on, to the menubar.
    keys(cx, "enter tab");
    assert!(!view.read_with(cx, |m, _| m.dropdown_open));
    keys(cx, "enter");
    let bar = view.read_with(cx, |m, _| m.bar.clone());
    assert_eq!(
        bar.read_with(cx, |b, _| b.active_key().cloned()),
        Some("home".into())
    );
}

#[crate::test]
fn a_menubar_is_one_tab_stop_with_arrow_keys(cx: &mut TestAppContext) {
    let (view, cx, log) = setup_menus(cx);
    let bar = view.read_with(cx, |m, _| m.bar.clone());
    // Trigger, then the menubar's first item.
    keys(cx, "tab tab right right enter");
    assert_eq!(
        bar.read_with(cx, |b, _| b.active_key().cloned()),
        Some("about".into())
    );
    // Down on the submenu opens its dropdown; Down again reaches its first item.
    keys(cx, "left down down enter");
    assert_eq!(
        bar.read_with(cx, |b, _| b.active_key().cloned()),
        Some("a".into())
    );
    // The menubar is one stop: Tab goes on to the button after it.
    keys(cx, "tab enter");
    assert_eq!(*log.borrow(), ["after"]);
}

struct PopupPage {
    log: Rc<RefCell<Vec<&'static str>>>,
    menu_pos: Option<crate::Point<crate::Pixels>>,
    target_focus: crate::FocusHandle,
}

impl Render for PopupPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pick = |name: &'static str, log: &Rc<RefCell<Vec<&'static str>>>| {
            let log = log.clone();
            move |_: &crate::ClickEvent, _: &mut Window, _: &mut crate::App| {
                log.borrow_mut().push(name)
            }
        };
        let target_focus = self.target_focus.clone();
        v_stack()
            .child(
                button("popup-trigger")
                    .label("Open")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.menu_pos = Some(crate::point(crate::px(40.), crate::px(40.)));
                        cx.notify();
                    })),
            )
            .child(
                button("popup-after")
                    .label("After")
                    .on_click(pick("after", &self.log)),
            )
            .child(
                crate::div()
                    .id("popup-target")
                    .track_focus(&self.target_focus),
            )
            .child(
                popup_menu("popup", self.menu_pos)
                    .on_close(cx.listener(|this, _, _, cx| {
                        this.menu_pos = None;
                        cx.notify();
                    }))
                    .entry(MenuEntry::item("Disabled", pick("disabled", &self.log)).with_disabled(true))
                    .entry(MenuEntry::item("Copy", pick("copy", &self.log)))
                    .entry(MenuEntry::item("Focus target", {
                        let log = self.log.clone();
                        move |_, window, cx| {
                            log.borrow_mut().push("focus-target");
                            window.focus(&target_focus, cx);
                        }
                    })),
            )
    }
}

#[crate::test]
fn a_popup_menu_takes_focus_handles_keys_and_restores_focus(cx: &mut TestAppContext) {
    cx.update(init_components);
    let log = Rc::new(RefCell::new(Vec::new()));
    let l = log.clone();
    let (view, cx) = cx.add_window_view(move |_, cx| PopupPage {
        log: l,
        menu_pos: None,
        target_focus: cx.focus_handle(),
    });
    cx.run_until_parked();

    // Open from the trigger button; Down skips the disabled item and selects Copy.
    keys(cx, "tab enter");
    assert!(view.read_with(cx, |p, _| p.menu_pos.is_some()));
    keys(cx, "down enter");
    assert_eq!(*log.borrow(), ["copy"]);
    assert!(view.read_with(cx, |p, _| p.menu_pos.is_none()));

    // Focus returned to the trigger: Enter opens again, Escape closes and keeps focus on trigger.
    keys(cx, "enter");
    assert!(view.read_with(cx, |p, _| p.menu_pos.is_some()));
    keys(cx, "escape");
    assert!(view.read_with(cx, |p, _| p.menu_pos.is_none()));

    // Tab from an open popup menu closes it and advances from the trigger to After.
    keys(cx, "enter tab enter");
    assert!(view.read_with(cx, |p, _| p.menu_pos.is_none()));
    assert_eq!(*log.borrow(), ["copy", "after"]);

    // Selecting an item that moves focus explicitly does not have its focus stolen back.
    keys(cx, "shift-tab enter down down enter");
    assert_eq!(*log.borrow(), ["copy", "after", "focus-target"]);
    assert!(view.read_with(cx, |p, _| p.menu_pos.is_none()));
    let target_focused = cx.update(|window, cx| {
        view.read(cx).target_focus.is_focused(window)
    });
    assert!(target_focused, "explicit focus change in on_select must persist");
}
