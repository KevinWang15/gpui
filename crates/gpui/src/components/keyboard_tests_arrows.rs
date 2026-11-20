//! The arrow keys inside components (`focus` module docs): lists, toolbars
//! and the sidebar move focus; calendars move the day; time columns and
//! color presets are roving lists.

use std::cell::RefCell;
use std::rc::Rc;

use super::keyboard_tests::keys;
use crate::components::*;
use crate::{
    AppContext as _, Context, Entity, IntoElement, Render, TestAppContext, Window, prelude::*,
};

type Log = Rc<RefCell<Vec<String>>>;

fn logger(
    log: &Log,
    name: &'static str,
) -> impl Fn(&crate::ClickEvent, &mut Window, &mut crate::App) + 'static {
    let log = log.clone();
    move |_, _, _| log.borrow_mut().push(name.to_string())
}

struct Toolbar {
    log: Log,
}

impl Render for Toolbar {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        crate::div()
            .child(
                button_group()
                    .child(
                        button("one")
                            .label("One")
                            .on_click(logger(&self.log, "one")),
                    )
                    .child(
                        button("two")
                            .label("Two")
                            .on_click(logger(&self.log, "two")),
                    )
                    .child(
                        button("three")
                            .label("Three")
                            .on_click(logger(&self.log, "three")),
                    ),
            )
            .child(
                Sidebar::new("side")
                    .child(SidebarItem::new("s1", "First").on_click(logger(&self.log, "s1")))
                    .child(SidebarItem::new("s2", "Second").on_click(logger(&self.log, "s2"))),
            )
    }
}

#[crate::test]
fn arrows_move_along_a_button_group_and_the_sidebar(cx: &mut TestAppContext) {
    cx.update(init_components);
    let log: Log = Rc::default();
    let l = log.clone();
    let (_, cx) = cx.add_window_view(move |_, _| Toolbar { log: l });
    cx.run_until_parked();
    keys(cx, "tab right right enter");
    assert_eq!(log.borrow().last().map(String::as_str), Some("three"));
    keys(cx, "right enter");
    assert_eq!(
        log.borrow().last().map(String::as_str),
        Some("one"),
        "wraps around"
    );
    keys(cx, "end enter");
    assert_eq!(log.borrow().last().map(String::as_str), Some("three"));
    // Into the sidebar, then Down.
    keys(cx, "tab down enter");
    assert_eq!(log.borrow().last().map(String::as_str), Some("s2"));
}

struct Calendar {
    month: CalendarDate,
    picked: Option<CalendarDate>,
}

impl Render for Calendar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (pick, turn) = (cx.entity(), cx.entity());
        calendar_grid("cal", self.month)
            .selected(self.picked)
            .on_select(move |date, _, cx| {
                pick.update(cx, |this, cx| {
                    this.picked = Some(date);
                    cx.notify();
                })
            })
            .on_month_change(move |month, _, cx| {
                turn.update(cx, |this, cx| {
                    this.month = month;
                    cx.notify();
                })
            })
    }
}

#[crate::test]
fn arrows_move_a_calendars_day_and_turn_the_month(cx: &mut TestAppContext) {
    cx.update(init_components);
    let start = CalendarDate::new(2026, 3, 10);
    let (view, cx) = cx.add_window_view(move |_, _| Calendar {
        month: start.first_of_month(),
        picked: Some(start),
    });
    cx.run_until_parked();
    // The previous and next month buttons come first, then the day.
    keys(cx, "tab tab tab right down enter");
    assert_eq!(
        view.read_with(cx, |c, _| c.picked),
        Some(CalendarDate::new(2026, 3, 18))
    );
    // Three weeks on is in April: the page turns.
    keys(cx, "down down enter");
    view.read_with(cx, |c, _| {
        assert_eq!(c.picked, Some(CalendarDate::new(2026, 4, 1)));
        assert_eq!(c.month, CalendarDate::new(2026, 4, 1));
    });
    keys(cx, "pageup enter");
    assert_eq!(
        view.read_with(cx, |c, _| c.picked),
        Some(CalendarDate::new(2026, 3, 1))
    );
}

struct Pickers {
    date: Entity<DatePickerState>,
    time: Entity<TimePickerState>,
    color: Entity<ColorPickerState>,
}

impl Render for Pickers {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        v_stack()
            .child(date_picker(&self.date))
            .child(time_picker(&self.time))
            .child(color_picker(&self.color))
    }
}

fn pickers(cx: &mut TestAppContext) -> (Entity<Pickers>, &mut crate::VisualTestContext) {
    cx.update(init_components);
    let (view, cx) = cx.add_window_view(|_, cx| Pickers {
        date: cx.new(|cx| DatePickerState::new(cx).default_date(CalendarDate::new(2026, 3, 10))),
        time: cx.new(TimePickerState::new),
        color: cx.new(|cx| ColorPickerState::new(cx).inline(true)),
    });
    cx.run_until_parked();
    (view, cx)
}

#[crate::test]
fn a_date_picker_opened_by_keyboard_moves_focus_into_its_calendar(cx: &mut TestAppContext) {
    let (view, cx) = pickers(cx);
    keys(cx, "tab enter right enter");
    let date = view.read_with(cx, |p, cx| p.date.read(cx).selected());
    assert_eq!(date, Some(CalendarDate::new(2026, 3, 11)));
    // Closed, and focus is back on the trigger: Enter opens it again.
    keys(cx, "enter escape");
    keys(cx, "tab");
    // The time picker's trigger is next.
    keys(cx, "enter down enter");
    let hour = view.read_with(cx, |p, cx| p.time.read(cx).value().hour);
    assert_eq!(hour, 10, "09:30 moved down one hour");
}

#[crate::test]
fn color_presets_are_one_tab_stop_with_arrows(cx: &mut TestAppContext) {
    let (view, cx) = pickers(cx);
    let before = view.read_with(cx, |p, cx| p.color.read(cx).color());
    // Date and time triggers, then into the color picker until a preset
    // takes Right: press Tab until the color changes on Right + Enter.
    keys(cx, "tab tab");
    let mut changed = false;
    for _ in 0..8 {
        keys(cx, "tab right enter");
        if view.read_with(cx, |p, cx| p.color.read(cx).color()) != before {
            changed = true;
            break;
        }
    }
    assert!(changed, "a preset was reached and picked");
}
