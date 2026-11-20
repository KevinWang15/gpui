//! Standalone single-date [`DatePicker`] (powered by [`CalendarGrid`]) and scrollable
//! column [`TimePicker`] (24h or 12h AM/PM, optional seconds, `"Now"` shortcut, keyboard support).

use std::cell::Cell;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::icons::{Icon, IconSize, LucideIcon};
use crate::{
    Anchor, Animation, AnimationExt, App, Bounds, Context, Entity, EventEmitter, FocusHandle,
    Focusable, FontWeight, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, Pixels, Render,
    RenderOnce, SharedString, StyleRefinement, Styled, Window, anchored, canvas, deferred, div,
    ease_out_quint, point, prelude::*, px, transparent_black,
};

use super::button::button;
use super::theme::{ANIM_FAST, ActiveTheme, ComponentSize, focus_ring_shadow, tabular_numerals};
use super::time_range::{CalendarDate, CalendarGrid};

const TWO_DIGIT: [&str; 60] = [
    "00", "01", "02", "03", "04", "05", "06", "07", "08", "09", "10", "11", "12", "13", "14", "15",
    "16", "17", "18", "19", "20", "21", "22", "23", "24", "25", "26", "27", "28", "29", "30", "31",
    "32", "33", "34", "35", "36", "37", "38", "39", "40", "41", "42", "43", "44", "45", "46", "47",
    "48", "49", "50", "51", "52", "53", "54", "55", "56", "57", "58", "59",
];

// ============================================================================
// Single-Date DatePicker
// ============================================================================

/// Events emitted by [`DatePickerState`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DatePickerEvent {
    /// Emitted when the selected date changes (or is cleared to `None`).
    Change(Option<CalendarDate>),
}

/// Entity state backing a standalone [`DatePicker`].
pub struct DatePickerState {
    focus_handle: FocusHandle,
    selected: Option<CalendarDate>,
    visible_month: CalendarDate,
    min_date: Option<CalendarDate>,
    max_date: Option<CalendarDate>,
    placeholder: SharedString,
    clearable: bool,
    disabled: bool,
    open: bool,
    size: ComponentSize,
    trigger_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    frame_style: StyleRefinement,
}

impl EventEmitter<DatePickerEvent> for DatePickerState {}

impl Focusable for DatePickerState {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl DatePickerState {
    /// Creates a new [`DatePickerState`] anchored on today's month.
    pub fn new(cx: &mut Context<Self>) -> Self {
        let today = CalendarDate::today_utc();
        Self {
            focus_handle: cx.focus_handle(),
            selected: None,
            visible_month: today.first_of_month(),
            min_date: None,
            max_date: None,
            placeholder: "Select date...".into(),
            clearable: true,
            disabled: false,
            open: false,
            size: ComponentSize::Md,
            trigger_bounds: Rc::new(Cell::new(None)),
            frame_style: StyleRefinement::default(),
        }
    }

    fn clamp_date(&self, mut date: CalendarDate) -> CalendarDate {
        if let Some(min) = self.min_date
            && date < min
        {
            date = min;
        }
        if let Some(max) = self.max_date
            && date > max
        {
            date = max;
        }
        date
    }

    fn is_date_in_bounds(&self, date: CalendarDate) -> bool {
        if let Some(min) = self.min_date
            && date < min
        {
            return false;
        }
        if let Some(max) = self.max_date
            && date > max
        {
            return false;
        }
        true
    }

    /// Sets the initially selected date (clamped to `[min_date, max_date]` if configured).
    pub fn default_date(mut self, date: CalendarDate) -> Self {
        let d = self.clamp_date(date);
        self.selected = Some(d);
        self.visible_month = d.first_of_month();
        self
    }

    /// Sets minimum selectable date.
    pub fn min_date(mut self, min: CalendarDate) -> Self {
        self.min_date = Some(min);
        if let Some(max) = self.max_date
            && min > max
        {
            self.max_date = Some(min);
        }
        if let Some(sel) = self.selected {
            let clamped = self.clamp_date(sel);
            self.selected = Some(clamped);
            self.visible_month = clamped.first_of_month();
        } else if self.visible_month < min.first_of_month() {
            self.visible_month = min.first_of_month();
        }
        self
    }

    /// Sets maximum selectable date.
    pub fn max_date(mut self, max: CalendarDate) -> Self {
        self.max_date = Some(max);
        if let Some(min) = self.min_date
            && max < min
        {
            self.min_date = Some(max);
        }
        if let Some(sel) = self.selected {
            let clamped = self.clamp_date(sel);
            self.selected = Some(clamped);
            self.visible_month = clamped.first_of_month();
        } else if self.visible_month > max.first_of_month() {
            self.visible_month = max.first_of_month();
        }
        self
    }

    /// Sets placeholder text when no date is selected.
    pub fn placeholder(mut self, text: impl Into<SharedString>) -> Self {
        self.placeholder = text.into();
        self
    }

    /// Enables or disables the clear (`×`) button (default `true`).
    pub fn clearable(mut self, clearable: bool) -> Self {
        self.clearable = clearable;
        self
    }

    /// Sets trigger size.
    pub fn size(mut self, size: ComponentSize) -> Self {
        self.size = size;
        self
    }

    /// Disables user interaction.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Dynamically updates disabled state.
    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        if self.disabled != disabled {
            self.disabled = disabled;
            if disabled {
                self.open = false;
            }
            cx.notify();
        }
    }

    /// Returns whether the calendar popover is currently open.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Opens the calendar popover if not disabled.
    pub fn open(&mut self, cx: &mut Context<Self>) {
        if !self.disabled && !self.open {
            self.open = true;
            cx.notify();
        }
    }

    /// Closes the calendar popover.
    pub fn close(&mut self, cx: &mut Context<Self>) {
        if self.open {
            self.open = false;
            cx.notify();
        }
    }

    /// Returns the currently selected date.
    pub fn selected(&self) -> Option<CalendarDate> {
        self.selected
    }

    /// Programmatically sets the selected date (clamped to bounds) and emits [`DatePickerEvent::Change`].
    pub fn set_date(&mut self, date: Option<CalendarDate>, cx: &mut Context<Self>) {
        let next = date.map(|d| self.clamp_date(d));
        if self.selected != next {
            self.selected = next;
            if let Some(d) = next {
                self.visible_month = d.first_of_month();
            }
            cx.emit(DatePickerEvent::Change(next));
            cx.notify();
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        let mods = event.keystroke.modifiers;
        if mods.control || mods.platform || mods.alt {
            return;
        }
        match event.keystroke.key.as_str() {
            "escape" if self.open => {
                self.close(cx);
                cx.stop_propagation();
            }
            "enter" | "space" => {
                self.open = !self.open;
                cx.notify();
                cx.stop_propagation();
            }
            "down" if !self.open => {
                self.open(cx);
                cx.stop_propagation();
            }
            _ => {}
        }
    }
}

impl Render for DatePickerState {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let reduce_motion = cx.reduce_motion();
        let hover_bg = theme.surface_hover;
        let entity_id = cx.entity_id();
        let entity_weak_sel = cx.entity().downgrade();
        let entity_weak_mon = cx.entity().downgrade();
        let is_focused = self.focus_handle.is_focused(window);
        let bounds_cell = self.trigger_bounds.clone();
        let bounds_for_out = self.trigger_bounds.clone();
        let captured_bounds = self.trigger_bounds.get();

        let popover = (self.open && !self.disabled).then(|| {
            let today = CalendarDate::today_utc();
            let today_allowed = self.is_date_in_bounds(today);

            let panel = div()
                .id(("dp-popover", entity_id))
                .flex()
                .flex_col()
                .gap(px(10.))
                .w(px(286.))
                .p(px(12.))
                .rounded(theme.radius_xl)
                .bg(theme.popover)
                .text_color(theme.popover_foreground)
                .border_1()
                .border_color(theme.border)
                .when(theme.elevation_shadows, |el| el.shadow_xl())
                .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
                .on_mouse_down_out(cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                    if bounds_for_out
                        .get()
                        .is_some_and(|b| b.contains(&ev.position))
                    {
                        return;
                    }
                    this.close(cx);
                }))
                .on_key_down(cx.listener(Self::on_key_down))
                .child(
                    CalendarGrid::new(("dp-cal", entity_id), self.visible_month)
                        .selected(self.selected)
                        .min_date(self.min_date)
                        .max_date(self.max_date)
                        .w_full()
                        .p_0()
                        .border_0()
                        .shadow_none()
                        .bg(transparent_black())
                        .on_select(move |date, _, cx| {
                            if let Some(ent) = entity_weak_sel.upgrade() {
                                ent.update(cx, |this, cx| {
                                    this.open = false;
                                    this.set_date(Some(date), cx);
                                    cx.notify();
                                });
                            }
                        })
                        .on_month_change(move |next_month, _, cx| {
                            if let Some(ent) = entity_weak_mon.upgrade() {
                                ent.update(cx, |this, cx| {
                                    this.visible_month = next_month.first_of_month();
                                    cx.notify();
                                });
                            }
                        }),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .pt(px(6.))
                        .border_t_1()
                        .border_color(theme.border)
                        .child(
                            button(("dp-today", entity_id))
                                .ghost()
                                .size(ComponentSize::Xs)
                                .label("Today")
                                .disabled(!today_allowed)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if today_allowed {
                                        this.open = false;
                                        this.set_date(Some(today), cx);
                                        cx.notify();
                                    }
                                })),
                        )
                        .when(self.clearable && self.selected.is_some(), |el| {
                            el.child(
                                button(("dp-clear-btn", entity_id))
                                    .ghost()
                                    .size(ComponentSize::Xs)
                                    .label("Clear")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.open = false;
                                        this.set_date(None, cx);
                                        cx.notify();
                                    })),
                            )
                        }),
                );

            let anim = if reduce_motion {
                panel.into_any_element()
            } else {
                panel
                    .with_animation(
                        ("dp-anim", entity_id),
                        Animation::new(ANIM_FAST).with_easing(ease_out_quint()),
                        |el, delta| el.opacity(delta),
                    )
                    .into_any_element()
            };

            let mut anchor_el = anchored()
                .anchor(Anchor::TopLeft)
                .snap_to_window_with_margin(px(8.));
            if let Some(bounds) = captured_bounds {
                anchor_el = anchor_el
                    .attach(bounds, Anchor::BottomLeft)
                    .offset(point(px(0.), px(6.)));
            } else {
                anchor_el = anchor_el.offset(point(px(0.), px(6.)));
            }

            deferred(anchor_el.child(anim)).priority(35)
        });

        let display_text: SharedString = self
            .selected
            .map(|d| SharedString::from(format!("{:04}-{:02}-{:02}", d.year, d.month, d.day)))
            .unwrap_or_else(|| self.placeholder.clone());
        let is_placeholder = self.selected.is_none();

        let mut trigger = div()
            .id(("dp-trigger", entity_id))
            .relative()
            .when(!self.disabled, |el| {
                el.track_focus(&self.focus_handle)
                    .on_key_down(cx.listener(Self::on_key_down))
            })
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .w_full()
            .h(self.size.height())
            .px(self.size.px_x())
            .rounded(theme.radius_md)
            .border_1()
            .border_color(if (self.open || is_focused) && !self.disabled {
                theme.ring
            } else {
                theme.input_border
            })
            .when((self.open || is_focused) && !self.disabled, |el| {
                el.shadow(focus_ring_shadow(&theme))
            })
            .bg(if self.disabled {
                theme.disabled_bg
            } else {
                theme.input_bg
            })
            .text_size(self.size.font_size())
            .font_features(tabular_numerals())
            .when(!self.disabled, |el| {
                el.cursor_pointer()
                    .hover(move |s| s.bg(hover_bg))
                    .on_click(cx.listener(|this, _, window, cx| {
                        window.focus(&this.focus_handle, cx);
                        this.open = !this.open;
                        cx.notify();
                    }))
            })
            .when(self.disabled, |el| el.opacity(0.55).cursor_not_allowed())
            .child(
                canvas(
                    move |bounds, _, _| bounds_cell.set(Some(bounds)),
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .min_w_0()
                    .child(
                        Icon::new(LucideIcon::Calendar)
                            .size(IconSize::Sm)
                            .color(theme.muted_foreground),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_color(if is_placeholder {
                                theme.placeholder
                            } else {
                                theme.foreground
                            })
                            .child(display_text),
                    ),
            )
            .when(
                self.clearable && self.selected.is_some() && !self.disabled,
                |el| {
                    el.child(
                        div()
                            .id(("dp-inline-clear", entity_id))
                            .p(px(2.))
                            .rounded(theme.radius_sm)
                            .hover(move |s| s.bg(hover_bg))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.set_date(None, cx);
                                cx.stop_propagation();
                            }))
                            .child(
                                Icon::new(LucideIcon::X)
                                    .size(IconSize::Xs)
                                    .color(theme.muted_foreground),
                            ),
                    )
                },
            );

        crate::Refineable::refine(trigger.style(), &self.frame_style);
        let has_explicit_width = self.frame_style.size.width.is_some();

        div()
            .flex()
            .flex_col()
            .when(!has_explicit_width, |el| el.w_full())
            .child(trigger)
            .when_some(popover, |el, pop| el.child(div().h_0().child(pop)))
    }
}

/// Renderable single-date [`DatePicker`] wrapping [`Entity<DatePickerState>`].
#[derive(IntoElement)]
pub struct DatePicker {
    state: Entity<DatePickerState>,
    style: StyleRefinement,
}

/// Convenience constructor for [`DatePicker::new`].
pub fn date_picker(state: &Entity<DatePickerState>) -> DatePicker {
    DatePicker::new(state)
}

impl DatePicker {
    /// Creates a [`DatePicker`] bound to `state`.
    pub fn new(state: &Entity<DatePickerState>) -> Self {
        Self {
            state: state.clone(),
            style: StyleRefinement::default(),
        }
    }
}

impl Styled for DatePicker {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for DatePicker {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        if self.state.read(cx).frame_style != self.style {
            let style = self.style;
            self.state.update(cx, |s, _| {
                s.frame_style = style;
            });
        }
        self.state
    }
}

// ============================================================================
// TimePicker (24h / 12h Scrollable Columns)
// ============================================================================

/// Wall-clock time (`hour: 0..24`, `minute: 0..60`, `second: 0..60`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TimeValue {
    /// Hour in 24-hour clock (`0..=23`).
    pub hour: u8,
    /// Minute (`0..=59`).
    pub minute: u8,
    /// Second (`0..=59`).
    pub second: u8,
}

impl TimeValue {
    /// Creates a clamped [`TimeValue`].
    pub const fn new(hour: u8, minute: u8, second: u8) -> Self {
        Self {
            hour: if hour > 23 { 23 } else { hour },
            minute: if minute > 59 { 59 } else { minute },
            second: if second > 59 { 59 } else { second },
        }
    }

    /// Returns a clamped copy of this [`TimeValue`] (defending against direct field writes).
    pub const fn clamped(self) -> Self {
        Self::new(self.hour, self.minute, self.second)
    }

    /// Returns the current UTC wall-clock time.
    pub fn now_utc() -> Self {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let day_secs = secs % 86_400;
        let hour = (day_secs / 3_600) as u8;
        let minute = ((day_secs % 3_600) / 60) as u8;
        let second = (day_secs % 60) as u8;
        Self::new(hour, minute, second)
    }

    /// Formats this time in 24h (`"14:30"` / `"14:30:00"`) or 12h (`"02:30 PM"`).
    pub fn format(self, use_12h: bool, show_seconds: bool) -> String {
        let v = self.clamped();
        if use_12h {
            let suffix = if v.hour < 12 { "AM" } else { "PM" };
            let h12 = match v.hour % 12 {
                0 => 12,
                n => n,
            };
            if show_seconds {
                format!("{h12:02}:{:02}:{:02} {suffix}", v.minute, v.second)
            } else {
                format!("{h12:02}:{:02} {suffix}", v.minute)
            }
        } else if show_seconds {
            format!("{:02}:{:02}:{:02}", v.hour, v.minute, v.second)
        } else {
            format!("{:02}:{:02}", v.hour, v.minute)
        }
    }
}

/// Events emitted by [`TimePickerState`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimePickerEvent {
    /// Emitted whenever the selected [`TimeValue`] changes.
    Change(TimeValue),
}

/// Entity state backing an interactive scrollable-column [`TimePicker`].
pub struct TimePickerState {
    focus_handle: FocusHandle,
    value: TimeValue,
    use_12h: bool,
    show_seconds: bool,
    minute_step: u8,
    second_step: u8,
    disabled: bool,
    open: bool,
    size: ComponentSize,
    trigger_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    frame_style: StyleRefinement,
}

impl EventEmitter<TimePickerEvent> for TimePickerState {}

impl Focusable for TimePickerState {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl TimePickerState {
    /// Creates a 24-hour [`TimePickerState`] defaulting to `09:30:00`.
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            value: TimeValue::new(9, 30, 0),
            use_12h: false,
            show_seconds: false,
            minute_step: 5,
            second_step: 1,
            disabled: false,
            open: false,
            size: ComponentSize::Md,
            trigger_bounds: Rc::new(Cell::new(None)),
            frame_style: StyleRefinement::default(),
        }
    }

    /// Sets the initial [`TimeValue`] (clamped to valid clock bounds).
    pub fn default_time(mut self, value: TimeValue) -> Self {
        self.value = value.clamped();
        self
    }

    /// Enables 12-hour AM/PM mode instead of 24-hour mode.
    pub fn use_12h(mut self, use_12h: bool) -> Self {
        self.use_12h = use_12h;
        self
    }

    /// Shows or hides the seconds column (default `false`).
    pub fn show_seconds(mut self, show: bool) -> Self {
        self.show_seconds = show;
        self
    }

    /// Sets the minute increment step in the dropdown column (default `5`, e.g. `1`, `5`, `15`).
    pub fn minute_step(mut self, step: u8) -> Self {
        self.minute_step = step.clamp(1, 30);
        self
    }

    /// Sets the second increment step in the seconds column (default `1`).
    pub fn second_step(mut self, step: u8) -> Self {
        self.second_step = step.clamp(1, 30);
        self
    }

    /// Sets trigger size.
    pub fn size(mut self, size: ComponentSize) -> Self {
        self.size = size;
        self
    }

    /// Disables user interaction.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Dynamically updates disabled state.
    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        if self.disabled != disabled {
            self.disabled = disabled;
            if disabled {
                self.open = false;
            }
            cx.notify();
        }
    }

    /// Returns whether the time popover is currently open.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Opens the time popover if not disabled.
    pub fn open(&mut self, cx: &mut Context<Self>) {
        if !self.disabled && !self.open {
            self.open = true;
            cx.notify();
        }
    }

    /// Closes the time popover.
    pub fn close(&mut self, cx: &mut Context<Self>) {
        if self.open {
            self.open = false;
            cx.notify();
        }
    }

    /// Returns the currently selected [`TimeValue`].
    pub fn value(&self) -> TimeValue {
        self.value
    }

    /// Updates the time value (clamped) and emits [`TimePickerEvent::Change`].
    pub fn set_time(&mut self, value: TimeValue, cx: &mut Context<Self>) {
        let clamped = value.clamped();
        if self.value != clamped {
            self.value = clamped;
            cx.emit(TimePickerEvent::Change(clamped));
            cx.notify();
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        let mods = event.keystroke.modifiers;
        if mods.control || mods.platform || mods.alt {
            return;
        }
        match event.keystroke.key.as_str() {
            "escape" if self.open => {
                self.close(cx);
                cx.stop_propagation();
            }
            "enter" | "space" => {
                self.open = !self.open;
                cx.notify();
                cx.stop_propagation();
            }
            "down" if !self.open => {
                self.open(cx);
                cx.stop_propagation();
            }
            _ => {}
        }
    }
}

impl Render for TimePickerState {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let reduce_motion = cx.reduce_motion();
        let hover_bg = theme.surface_hover;
        let entity_id = cx.entity_id();
        let cur = self.value.clamped();
        let use_12h = self.use_12h;
        let show_seconds = self.show_seconds;
        let is_focused = self.focus_handle.is_focused(window);
        let bounds_cell = self.trigger_bounds.clone();
        let bounds_for_out = self.trigger_bounds.clone();
        let captured_bounds = self.trigger_bounds.get();

        let popover = (self.open && !self.disabled).then(|| {
            let cur_h_display = if use_12h {
                match cur.hour % 12 {
                    0 => 12,
                    n => n,
                }
            } else {
                cur.hour
            };
            let is_pm = cur.hour >= 12;

            let col_header = |title: &'static str| {
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .pb(px(4.))
                    .text_size(px(11.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.muted_foreground)
                    .child(title)
            };

            let hour_range = if use_12h { 1u8..=12 } else { 0u8..=23 };
            let hour_col = div()
                .flex()
                .flex_col()
                .w(px(56.))
                .pr(px(4.))
                .child(col_header("Hr"))
                .child(
                    div()
                        .id(("tp-hours", entity_id))
                        .flex()
                        .flex_col()
                        .h(px(184.))
                        .overflow_y_scroll()
                        .children(hour_range.map(|h| {
                            let active = h == cur_h_display;
                            div()
                                .id(("tp-h", h as usize))
                                .flex()
                                .items_center()
                                .justify_center()
                                .h(px(28.))
                                .flex_shrink_0()
                                .rounded(theme.radius_sm)
                                .text_size(px(13.))
                                .font_features(tabular_numerals())
                                .font_weight(if active {
                                    FontWeight::SEMIBOLD
                                } else {
                                    FontWeight::NORMAL
                                })
                                .bg(if active {
                                    theme.primary_soft
                                } else {
                                    transparent_black()
                                })
                                .text_color(if active {
                                    theme.primary_soft_foreground
                                } else {
                                    theme.popover_foreground
                                })
                                .cursor_pointer()
                                .when(!active, |el| el.hover(move |s| s.bg(hover_bg)))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    let next_h = if use_12h {
                                        let base = h % 12;
                                        if this.value.hour >= 12 {
                                            base + 12
                                        } else {
                                            base
                                        }
                                    } else {
                                        h
                                    };
                                    this.set_time(
                                        TimeValue::new(
                                            next_h,
                                            this.value.minute,
                                            this.value.second,
                                        ),
                                        cx,
                                    );
                                }))
                                .child(TWO_DIGIT[h as usize])
                        })),
                );

            let min_step = self.minute_step as usize;
            let min_col = div()
                .flex()
                .flex_col()
                .w(px(56.))
                .px(px(4.))
                .border_l_1()
                .border_color(theme.border)
                .child(col_header("Min"))
                .child(
                    div()
                        .id(("tp-mins", entity_id))
                        .flex()
                        .flex_col()
                        .h(px(184.))
                        .overflow_y_scroll()
                        .children((0usize..60).step_by(min_step).map(|m| {
                            let m_u8 = m as u8;
                            let active = m_u8 == cur.minute;
                            div()
                                .id(("tp-m", m))
                                .flex()
                                .items_center()
                                .justify_center()
                                .h(px(28.))
                                .flex_shrink_0()
                                .rounded(theme.radius_sm)
                                .text_size(px(13.))
                                .font_features(tabular_numerals())
                                .font_weight(if active {
                                    FontWeight::SEMIBOLD
                                } else {
                                    FontWeight::NORMAL
                                })
                                .bg(if active {
                                    theme.primary_soft
                                } else {
                                    transparent_black()
                                })
                                .text_color(if active {
                                    theme.primary_soft_foreground
                                } else {
                                    theme.popover_foreground
                                })
                                .cursor_pointer()
                                .when(!active, |el| el.hover(move |s| s.bg(hover_bg)))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.set_time(
                                        TimeValue::new(
                                            this.value.hour,
                                            m_u8,
                                            this.value.second,
                                        ),
                                        cx,
                                    );
                                }))
                                .child(TWO_DIGIT[m])
                        })),
                );

            let sec_step = self.second_step as usize;
            let sec_col = show_seconds.then(|| {
                div()
                    .flex()
                    .flex_col()
                    .w(px(56.))
                    .px(px(4.))
                    .border_l_1()
                    .border_color(theme.border)
                    .child(col_header("Sec"))
                    .child(
                        div()
                            .id(("tp-secs", entity_id))
                            .flex()
                            .flex_col()
                            .h(px(184.))
                            .overflow_y_scroll()
                            .children((0usize..60).step_by(sec_step).map(|s| {
                                let s_u8 = s as u8;
                                let active = s_u8 == cur.second;
                                div()
                                    .id(("tp-s", s))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .h(px(28.))
                                    .flex_shrink_0()
                                    .rounded(theme.radius_sm)
                                    .text_size(px(13.))
                                    .font_features(tabular_numerals())
                                    .font_weight(if active {
                                        FontWeight::SEMIBOLD
                                    } else {
                                        FontWeight::NORMAL
                                    })
                                    .bg(if active {
                                        theme.primary_soft
                                    } else {
                                        transparent_black()
                                    })
                                    .text_color(if active {
                                        theme.primary_soft_foreground
                                    } else {
                                        theme.popover_foreground
                                    })
                                    .cursor_pointer()
                                    .when(!active, |el| el.hover(move |s| s.bg(hover_bg)))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.set_time(
                                            TimeValue::new(
                                                this.value.hour,
                                                this.value.minute,
                                                s_u8,
                                            ),
                                            cx,
                                        );
                                    }))
                                    .child(TWO_DIGIT[s])
                            })),
                    )
            });

            let ampm_col = use_12h.then(|| {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .w(px(52.))
                    .pl(px(6.))
                    .border_l_1()
                    .border_color(theme.border)
                    .child(col_header(""))
                    .children([(false, "AM"), (true, "PM")].into_iter().map(|(pm, lbl)| {
                        let active = is_pm == pm;
                        div()
                            .id(("tp-ap", usize::from(pm)))
                            .flex()
                            .items_center()
                            .justify_center()
                            .h(px(28.))
                            .rounded(theme.radius_sm)
                            .text_size(px(12.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .bg(if active {
                                theme.primary_soft
                            } else {
                                transparent_black()
                            })
                            .text_color(if active {
                                theme.primary_soft_foreground
                            } else {
                                theme.popover_foreground
                            })
                            .cursor_pointer()
                            .when(!active, |el| el.hover(move |s| s.bg(hover_bg)))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let base = this.value.hour % 12;
                                let next_h = if pm { base + 12 } else { base };
                                this.set_time(
                                    TimeValue::new(next_h, this.value.minute, this.value.second),
                                    cx,
                                );
                            }))
                            .child(lbl)
                    }))
            });

            let panel = div()
                .id(("tp-popover", entity_id))
                .flex()
                .flex_col()
                .gap(px(8.))
                .p(px(10.))
                .rounded(theme.radius_xl)
                .bg(theme.popover)
                .text_color(theme.popover_foreground)
                .border_1()
                .border_color(theme.border)
                .when(theme.elevation_shadows, |el| el.shadow_xl())
                .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
                .on_mouse_down_out(cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                    if bounds_for_out
                        .get()
                        .is_some_and(|b| b.contains(&ev.position))
                    {
                        return;
                    }
                    this.close(cx);
                }))
                .on_key_down(cx.listener(Self::on_key_down))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .child(hour_col)
                        .child(min_col)
                        .children(sec_col)
                        .children(ampm_col),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .pt(px(6.))
                        .border_t_1()
                        .border_color(theme.border)
                        .child(
                            button(("tp-now", entity_id))
                                .ghost()
                                .size(ComponentSize::Xs)
                                .label("Now")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.set_time(TimeValue::now_utc(), cx);
                                })),
                        )
                        .child(
                            button(("tp-ok", entity_id))
                                .primary()
                                .size(ComponentSize::Xs)
                                .label("Done")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.close(cx);
                                })),
                        ),
                );

            let anim = if reduce_motion {
                panel.into_any_element()
            } else {
                panel
                    .with_animation(
                        ("tp-anim", entity_id),
                        Animation::new(ANIM_FAST).with_easing(ease_out_quint()),
                        |el, delta| el.opacity(delta),
                    )
                    .into_any_element()
            };

            let mut anchor_el = anchored()
                .anchor(Anchor::TopLeft)
                .snap_to_window_with_margin(px(8.));
            if let Some(bounds) = captured_bounds {
                anchor_el = anchor_el
                    .attach(bounds, Anchor::BottomLeft)
                    .offset(point(px(0.), px(6.)));
            } else {
                anchor_el = anchor_el.offset(point(px(0.), px(6.)));
            }

            deferred(anchor_el.child(anim)).priority(35)
        });

        let mut trigger = div()
            .id(("tp-trigger", entity_id))
            .relative()
            .when(!self.disabled, |el| {
                el.track_focus(&self.focus_handle)
                    .on_key_down(cx.listener(Self::on_key_down))
            })
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .w_full()
            .h(self.size.height())
            .px(self.size.px_x())
            .rounded(theme.radius_md)
            .border_1()
            .border_color(if (self.open || is_focused) && !self.disabled {
                theme.ring
            } else {
                theme.input_border
            })
            .when((self.open || is_focused) && !self.disabled, |el| {
                el.shadow(focus_ring_shadow(&theme))
            })
            .bg(if self.disabled {
                theme.disabled_bg
            } else {
                theme.input_bg
            })
            .text_size(self.size.font_size())
            .font_features(tabular_numerals())
            .text_color(theme.foreground)
            .when(!self.disabled, |el| {
                el.cursor_pointer()
                    .hover(move |s| s.bg(hover_bg))
                    .on_click(cx.listener(|this, _, window, cx| {
                        window.focus(&this.focus_handle, cx);
                        this.open = !this.open;
                        cx.notify();
                    }))
            })
            .when(self.disabled, |el| el.opacity(0.55).cursor_not_allowed())
            .child(
                canvas(
                    move |bounds, _, _| bounds_cell.set(Some(bounds)),
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        Icon::new(LucideIcon::Clock)
                            .size(IconSize::Sm)
                            .color(theme.muted_foreground),
                    )
                    .child(cur.format(use_12h, show_seconds)),
            )
            .child(
                Icon::new(LucideIcon::ChevronDown)
                    .size(IconSize::Sm)
                    .color(theme.muted_foreground),
            );

        crate::Refineable::refine(trigger.style(), &self.frame_style);
        let has_explicit_width = self.frame_style.size.width.is_some();

        div()
            .flex()
            .flex_col()
            .when(!has_explicit_width, |el| el.w_full())
            .child(trigger)
            .when_some(popover, |el, pop| el.child(div().h_0().child(pop)))
    }
}

/// Renderable [`TimePicker`] element wrapping [`Entity<TimePickerState>`].
#[derive(IntoElement)]
pub struct TimePicker {
    state: Entity<TimePickerState>,
    style: StyleRefinement,
}

/// Convenience constructor for [`TimePicker::new`].
pub fn time_picker(state: &Entity<TimePickerState>) -> TimePicker {
    TimePicker::new(state)
}

impl TimePicker {
    /// Creates a [`TimePicker`] bound to `state`.
    pub fn new(state: &Entity<TimePickerState>) -> Self {
        Self {
            state: state.clone(),
            style: StyleRefinement::default(),
        }
    }
}

impl Styled for TimePicker {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for TimePicker {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        if self.state.read(cx).frame_style != self.style {
            let style = self.style;
            self.state.update(cx, |s, _| {
                s.frame_style = style;
            });
        }
        self.state
    }
}
