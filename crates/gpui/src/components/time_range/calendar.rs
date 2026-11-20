//! Interactive month [`CalendarGrid`] component used standalone or inside [`TimeRangePicker`](super::TimeRangePicker).

use std::rc::Rc;

use crate::{
    App, ClickEvent, ElementId, FocusHandle, FontWeight, IntoElement, KeyDownEvent, ParentElement,
    RenderOnce,
    StyleRefinement, Styled, Window, div, prelude::*, px,
};
use crate::icons::{Icon, IconSize, LucideIcon};

use super::date::{CalendarDate, DateRange, days_in_month};
use crate::components::theme::ActiveTheme;

const WEEKDAYS_MON_FIRST: [&str; 7] = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"];

const DAY_LABELS: [&str; 31] = [
    "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16", "17",
    "18", "19", "20", "21", "22", "23", "24", "25", "26", "27", "28", "29", "30", "31",
];

/// Interactive single-month calendar grid (`Mo..Su`) with month navigation, `min_date`/`max_date`
/// bounds, single-day highlight, and optional range tinting.
#[derive(IntoElement)]
pub struct CalendarGrid {
    id: ElementId,
    /// First day of the month being displayed.
    visible_month: CalendarDate,
    /// Currently focused / active selected date (highlighted in solid primary).
    selected: Option<CalendarDate>,
    /// Optional active date range for subtle in-range background tinting.
    highlighted_range: Option<DateRange>,
    /// Optional inclusive lower bound; earlier dates are disabled.
    min_date: Option<CalendarDate>,
    /// Optional inclusive upper bound (e.g. today); later dates are disabled.
    max_date: Option<CalendarDate>,
    style: StyleRefinement,
    on_select: Option<Rc<dyn Fn(CalendarDate, &mut Window, &mut App) + 'static>>,
    on_month_change: Option<Rc<dyn Fn(CalendarDate, &mut Window, &mut App) + 'static>>,
    focus_active_day: bool,
}

/// Convenience constructor for [`CalendarGrid::new`].
pub fn calendar_grid(id: impl Into<ElementId>, visible_month: CalendarDate) -> CalendarGrid {
    CalendarGrid::new(id, visible_month)
}

impl CalendarGrid {
    /// Creates a new [`CalendarGrid`] displaying `visible_month`'s month & year.
    pub fn new(id: impl Into<ElementId>, visible_month: CalendarDate) -> Self {
        Self {
            id: id.into(),
            visible_month: visible_month.first_of_month(),
            selected: None,
            highlighted_range: None,
            min_date: None,
            max_date: None,
            style: StyleRefinement::default(),
            on_select: None,
            on_month_change: None,
            focus_active_day: false,
        }
    }

    /// Sets the primary selected date cell.
    pub fn selected(mut self, date: Option<CalendarDate>) -> Self {
        self.selected = date;
        self
    }

    /// Sets an inclusive date range whose interior cells receive a subtle tint.
    pub fn highlighted_range(mut self, range: Option<DateRange>) -> Self {
        self.highlighted_range = range;
        self
    }

    /// Sets the minimum selectable date.
    pub fn min_date(mut self, min_date: Option<CalendarDate>) -> Self {
        self.min_date = min_date;
        self
    }

    /// Sets the maximum selectable date (dates beyond this render muted and disabled).
    pub fn max_date(mut self, max_date: Option<CalendarDate>) -> Self {
        self.max_date = max_date;
        self
    }

    /// Invoked when the user clicks an enabled calendar day.
    pub fn on_select(
        mut self,
        handler: impl Fn(CalendarDate, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_select = Some(Rc::new(handler));
        self
    }

    /// Invoked when the user navigates `<` or `>` to a different month.
    pub fn on_month_change(
        mut self,
        handler: impl Fn(CalendarDate, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_month_change = Some(Rc::new(handler));
        self
    }
}

impl CalendarGrid {
    /// Moves focus to the active day (the calendar's tab stop) when this
    /// renders: for a picker opened from the keyboard. Pass `true` for one
    /// render only.
    pub fn focus_active_day(mut self, focus: bool) -> Self {
        self.focus_active_day = focus;
        self
    }
}

impl Styled for CalendarGrid {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

/// Where an arrow key moves the calendar's active day (see the module
/// rules in `components::focus`): a day or a week, to the week's ends, or a
/// month with Page Up / Page Down.
fn day_step(key: &str, from: CalendarDate) -> Option<CalendarDate> {
    let shift_month = |date: CalendarDate, forward: bool| {
        let first = if forward {
            date.first_of_month().next_month()
        } else {
            date.first_of_month().prev_month()
        };
        let last_day = days_in_month(first.year, first.month);
        CalendarDate::new(first.year, first.month, date.day.min(last_day))
    };
    Some(match key {
        "left" => from.add_days(-1),
        "right" => from.add_days(1),
        "up" => from.add_days(-7),
        "down" => from.add_days(7),
        "home" => from.add_days(-(from.weekday_mon0() as i64)),
        "end" => from.add_days(6 - from.weekday_mon0() as i64),
        "pageup" => shift_month(from, false),
        "pagedown" => shift_month(from, true),
        _ => return None,
    })
}

/// The calendar's keyboard state: the day that's its tab stop, and a focus
/// handle per grid cell.
struct DayFocus {
    active: Option<CalendarDate>,
    cells: Vec<FocusHandle>,
}

impl RenderOnce for CalendarGrid {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let month_first = self.visible_month.first_of_month();
        let leading_days = month_first.weekday_mon0() as i64;
        let grid_start = month_first.add_days(-leading_days);
        let month_days = days_in_month(month_first.year, month_first.month) as usize;
        let total_cells = if (leading_days as usize) + month_days > 35 {
            42
        } else {
            35
        };

        let prev_month = month_first.prev_month();
        let next_month = month_first.next_month();
        let can_next = self
            .max_date
            .is_none_or(|max| next_month <= max.first_of_month());
        let can_prev = self
            .min_date
            .is_none_or(|min| prev_month.last_of_month() >= min);

        let on_month_prev = self.on_month_change.clone();
        let on_month_next = self.on_month_change.clone();
        let hover_bg = theme.surface_hover;

        let prev_btn = div()
            .id((self.id.clone(), "cal-prev"))
            .map(|el| super::super::focus::tab_stop(el, can_prev, &theme))
            .flex()
            .items_center()
            .justify_center()
            .w(px(28.))
            .h(px(28.))
            .rounded(theme.radius_md)
            .when(can_prev, |el| {
                el.cursor_pointer()
                    .hover(move |s| s.bg(hover_bg))
                    .when_some(on_month_prev, |el, cb| {
                        el.on_click(move |_: &ClickEvent, window, cx| {
                            cb(prev_month, window, cx);
                        })
                    })
            })
            .child(Icon::new(LucideIcon::ChevronLeft).size(IconSize::Sm).color(
                if can_prev {
                    theme.foreground
                } else {
                    theme.disabled_foreground
                },
            ));

        let next_btn = div()
            .id((self.id.clone(), "cal-next"))
            .map(|el| super::super::focus::tab_stop(el, can_next, &theme))
            .flex()
            .items_center()
            .justify_center()
            .w(px(28.))
            .h(px(28.))
            .rounded(theme.radius_md)
            .when(can_next, |el| {
                el.cursor_pointer()
                    .hover(move |s| s.bg(hover_bg))
                    .when_some(on_month_next, |el, cb| {
                        el.on_click(move |_: &ClickEvent, window, cx| {
                            cb(next_month, window, cx);
                        })
                    })
            })
            .child(
                Icon::new(LucideIcon::ChevronRight)
                    .size(IconSize::Sm)
                    .color(if can_next {
                        theme.foreground
                    } else {
                        theme.disabled_foreground
                    }),
            );

        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .px(px(4.))
            .pb(px(10.))
            .child(prev_btn)
            .child(
                div()
                    .text_size(px(13.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.foreground)
                    .child(month_first.format_month_year()),
            )
            .child(next_btn);

        let mut weekday_row = div().flex().items_center().pb(px(6.));
        for day_name in WEEKDAYS_MON_FIRST {
            weekday_row = weekday_row.child(
                div()
                    .w(px(34.))
                    .h(px(22.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(11.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.muted_foreground)
                    .child(day_name),
            );
        }

        // One tab stop, the active day: the one last moved to in this month,
        // else the selected day, else today, else the 1st. The arrow keys
        // move it, and Enter or Space picks it.
        let keyboard = window.use_keyed_state((self.id.clone(), "days"), cx, |_, cx| DayFocus {
            active: None,
            cells: (0..42).map(|_| cx.focus_handle()).collect(),
        });
        let in_view = |d: &CalendarDate| d.year == month_first.year && d.month == month_first.month;
        let today = CalendarDate::today_utc();
        let active = {
            let k = keyboard.read(cx);
            k.active
                .filter(in_view)
                .or(self.selected.filter(in_view))
                .or(Some(today).filter(in_view))
                .unwrap_or(month_first)
        };
        let cells = keyboard.read(cx).cells.clone();
        if self.focus_active_day {
            let handle = cells[active.days_since(grid_start).clamp(0, 41) as usize].clone();
            window.defer(cx, move |window, cx| window.focus(&handle, cx));
        }
        let mut weeks = div().flex().flex_col().gap(px(2.));
        {
            let cells = cells.clone();
            let (min, max) = (self.min_date, self.max_date);
            let on_month_change = self.on_month_change.clone();
            weeks = weeks.on_key_down(move |event: &KeyDownEvent, window, cx| {
                if event.keystroke.modifiers.modified() {
                    return;
                }
                let Some(from) = cells
                    .iter()
                    .position(|h| h.is_focused(window))
                    .map(|i| grid_start.add_days(i as i64))
                else {
                    return;
                };
                let Some(mut to) = day_step(&event.keystroke.key, from) else {
                    return;
                };
                cx.stop_propagation();
                if let Some(min) = min {
                    to = to.max(min);
                }
                if let Some(max) = max {
                    to = to.min(max);
                }
                let mut start = grid_start;
                if to.first_of_month() != month_first {
                    // Another month: turn the page, if the calendar can.
                    let Some(cb) = on_month_change.as_ref() else {
                        return;
                    };
                    cb(to.first_of_month(), window, cx);
                    let first = to.first_of_month();
                    start = first.add_days(-(first.weekday_mon0() as i64));
                }
                keyboard.update(cx, |k, _| k.active = Some(to));
                let cell = to.days_since(start).clamp(0, 41) as usize;
                window.focus(&cells[cell], cx);
            });
        }
        for week in 0..(total_cells / 7) {
            let mut row = div().flex().items_center();
            for col in 0..7 {
                let offset = (week * 7 + col) as i64;
                let date = grid_start.add_days(offset);
                let in_month = date.month == month_first.month && date.year == month_first.year;
                let is_selected = self.selected == Some(date);
                let in_range = self
                    .highlighted_range
                    .is_some_and(|range| range.contains(date));
                let is_endpoint = self
                    .highlighted_range
                    .is_some_and(|range| range.start == date || range.end == date);
                let is_disabled = self.min_date.is_some_and(|min| date < min)
                    || self.max_date.is_some_and(|max| date > max);

                let text_color = if is_selected {
                    theme.primary_foreground
                } else if is_disabled {
                    theme.disabled_foreground
                } else if !in_month {
                    theme.muted_foreground.opacity(0.55)
                } else if is_endpoint {
                    theme.primary_soft_foreground
                } else {
                    theme.foreground
                };

                let bg_color = if is_selected {
                    Some(theme.primary)
                } else if in_range && !is_disabled && in_month {
                    Some(theme.primary_soft)
                } else {
                    None
                };

                let day_label = DAY_LABELS[(date.day.clamp(1, 31) - 1) as usize];
                let handle = &cells[offset as usize];
                let focus_outline = super::super::focus::focus_visible_outline(handle, window, &theme);
                let cell = div()
                    .id(("day", offset as usize))
                    .track_focus(&handle.clone().tab_stop(date == active && !is_disabled))
                    .when_some(focus_outline, |el, outline| el.outline(outline))
                    .w(px(34.))
                    .h(px(32.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(theme.radius_md)
                    .text_size(px(12.5))
                    .font_weight(if is_selected || is_endpoint {
                        FontWeight::SEMIBOLD
                    } else {
                        FontWeight::MEDIUM
                    })
                    .text_color(text_color)
                    .when_some(bg_color, |el, bg| el.bg(bg))
                    .when(!is_disabled && !is_selected, |el| {
                        el.cursor_pointer().hover(move |s| s.bg(hover_bg))
                    })
                    .when(!is_disabled, |el| {
                        let on_select = self.on_select.clone();
                        el.cursor_pointer().when_some(on_select, |el, cb| {
                            el.on_click(move |_: &ClickEvent, window, cx| {
                                cb(date, window, cx);
                            })
                        })
                    })
                    .child(day_label);

                row = row.child(cell);
            }
            weeks = weeks.child(row);
        }

        let mut card = div()
            .id(self.id)
            .w(px(262.))
            .p(px(12.))
            .rounded(theme.radius_xl)
            .bg(theme.popover)
            .border_1()
            .border_color(theme.border)
            .shadow_lg()
            .flex()
            .flex_col()
            .child(header)
            .child(weekday_row)
            .child(weeks);

        crate::Refineable::refine(card.style(), &self.style);
        card
    }
}
