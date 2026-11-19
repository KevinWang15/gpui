//! Compound [`TimeRangePicker`] with Quick Select (`Last N units` + Commonly Used presets),
//! Start & End date inputs with embedded [`CalendarGrid`], period steppers (`<` / `>`), and zoom out.

use crate::{
    Animation, AnimationExt, App, ClickEvent, Context, ElementId, Entity, EventEmitter,
    FocusHandle, Focusable, FontWeight, IntoElement, KeyDownEvent, ParentElement, RenderOnce,
    SharedString, StyleRefinement, Styled, Window, anchored, deferred, div, ease_out_quint,
    prelude::*, px,
};
use crate::icons::{Icon, IconSize, LucideIcon};

use super::calendar::CalendarGrid;
use super::date::{CalendarDate, DateRange, TimeRangePreset, TimeRangeUnit};
use crate::components::button::{Button, ButtonVariant};
use crate::components::theme::{ANIM_FAST, ActiveTheme, ComponentSize};

/// Events emitted by [`TimeRangePickerState`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeRangeEvent {
    /// Emitted whenever the committed [`DateRange`] or [`TimeRangePreset`] changes.
    Change {
        /// Newly active inclusive date range.
        range: DateRange,
        /// Newly active preset (or [`TimeRangePreset::Custom`]).
        preset: TimeRangePreset,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ActivePopover {
    None,
    QuickSelect,
    StartAndEnd,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CalendarTarget {
    FirstDay,
    LastDay,
}

/// Interactive state entity backing a [`TimeRangePicker`].
pub struct TimeRangePickerState {
    today: CalendarDate,
    max_span_days: u32,
    preset: TimeRangePreset,
    range: DateRange,
    active_popover: ActivePopover,
    quick_amount: u32,
    quick_amount_text: String,
    quick_unit: TimeRangeUnit,
    quick_unit_open: bool,
    draft_start: CalendarDate,
    draft_end: CalendarDate,
    calendar_target: Option<CalendarTarget>,
    calendar_month: CalendarDate,
    helper_note: Option<SharedString>,
    focus_handle: FocusHandle,
}

impl EventEmitter<TimeRangeEvent> for TimeRangePickerState {}

impl Focusable for TimeRangePickerState {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl TimeRangePickerState {
    /// Creates a new [`TimeRangePickerState`] anchored at `today` with `Last 30 days` active.
    pub fn new(today: CalendarDate, cx: &mut Context<Self>) -> Self {
        let max_span_days = 92;
        let preset = TimeRangePreset::Last30Days;
        let range = preset.resolve(today, max_span_days);
        Self {
            today,
            max_span_days,
            preset,
            range,
            active_popover: ActivePopover::None,
            quick_amount: 15,
            quick_amount_text: "15".to_string(),
            quick_unit: TimeRangeUnit::Days,
            quick_unit_open: false,
            draft_start: range.start,
            draft_end: range.end,
            calendar_target: None,
            calendar_month: range.end.first_of_month(),
            helper_note: None,
            focus_handle: cx.focus_handle(),
        }
    }

    /// Sets the maximum allowed span in whole days (default `92`).
    pub fn max_span_days(mut self, days: u32) -> Self {
        self.max_span_days = days.max(1);
        self.range = self.preset.resolve(self.today, self.max_span_days);
        self.draft_start = self.range.start;
        self.draft_end = self.range.end;
        self
    }

    /// Sets the initial [`TimeRangePreset`].
    pub fn preset(mut self, preset: TimeRangePreset) -> Self {
        self.preset = preset;
        self.range = preset.resolve(self.today, self.max_span_days);
        self.draft_start = self.range.start;
        self.draft_end = self.range.end;
        self
    }

    /// Overrides the muted footer note inside the popovers.
    pub fn helper_note(mut self, note: impl Into<SharedString>) -> Self {
        self.helper_note = Some(note.into());
        self
    }

    /// Currently active inclusive [`DateRange`].
    pub fn range(&self) -> DateRange {
        self.range
    }

    /// Currently active [`TimeRangePreset`].
    pub fn active_preset(&self) -> TimeRangePreset {
        self.preset
    }

    /// Anchor "today" / maximum selectable date.
    pub fn today(&self) -> CalendarDate {
        self.today
    }

    /// Maximum allowed span in days.
    pub fn max_span(&self) -> u32 {
        self.max_span_days
    }

    /// Whether the forward `>` stepper is enabled (`range.end < today`).
    pub fn can_step_forward(&self) -> bool {
        self.range.end < self.today
    }

    /// Whether zoom-out `🔍-` can expand the range further.
    pub fn can_zoom_out(&self) -> bool {
        self.range.span_days() < self.max_span_days
    }

    /// Applies a named [`TimeRangePreset`] and closes any open popover.
    pub fn apply_preset(&mut self, preset: TimeRangePreset, cx: &mut Context<Self>) {
        self.preset = preset;
        self.range = preset.resolve(self.today, self.max_span_days);
        self.draft_start = self.range.start;
        self.draft_end = self.range.end;
        self.active_popover = ActivePopover::None;
        self.quick_unit_open = false;
        self.calendar_target = None;
        cx.emit(TimeRangeEvent::Change {
            range: self.range,
            preset: self.preset,
        });
        cx.notify();
    }

    /// Applies the current Quick Select (`Last N unit`) draft.
    pub fn apply_quick_select(&mut self, cx: &mut Context<Self>) {
        let parsed = self
            .quick_amount_text
            .trim()
            .parse::<u32>()
            .unwrap_or(self.quick_amount)
            .clamp(1, self.max_span_days);
        self.quick_amount = parsed;
        self.quick_amount_text = parsed.to_string();
        let preset = TimeRangePreset::QuickRelative(parsed, self.quick_unit);
        self.apply_preset(preset, cx);
    }

    /// Applies an explicit `[start, end]` date range (clamped to `today` and `max_span_days`).
    pub fn apply_custom_range(
        &mut self,
        start: CalendarDate,
        end: CalendarDate,
        cx: &mut Context<Self>,
    ) {
        let mut range = DateRange::new(start.min(self.today), end.min(self.today));
        if range.span_days() > self.max_span_days {
            range = DateRange::last_days(range.end, self.max_span_days);
        }
        self.range = range;
        self.preset = TimeRangePreset::Custom;
        self.draft_start = range.start;
        self.draft_end = range.end;
        self.active_popover = ActivePopover::None;
        self.calendar_target = None;
        cx.emit(TimeRangeEvent::Change {
            range: self.range,
            preset: self.preset,
        });
        cx.notify();
    }

    /// Shifts the active window backward by its current span.
    pub fn step_backward(&mut self, cx: &mut Context<Self>) {
        self.range = self.range.shift_backward();
        self.preset = TimeRangePreset::Custom;
        self.draft_start = self.range.start;
        self.draft_end = self.range.end;
        cx.emit(TimeRangeEvent::Change {
            range: self.range,
            preset: self.preset,
        });
        cx.notify();
    }

    /// Shifts the active window forward by its current span (clamped to `today`).
    pub fn step_forward(&mut self, cx: &mut Context<Self>) {
        if !self.can_step_forward() {
            return;
        }
        self.range = self.range.shift_forward(self.today);
        if self.range.end == self.today {
            // Re-match common presets if stepping landed back on today.
            self.preset = match self.range.span_days() {
                7 => TimeRangePreset::Last7Days,
                30 => TimeRangePreset::Last30Days,
                90 => TimeRangePreset::Last90Days,
                _ => TimeRangePreset::Custom,
            };
        } else {
            self.preset = TimeRangePreset::Custom;
        }
        self.draft_start = self.range.start;
        self.draft_end = self.range.end;
        cx.emit(TimeRangeEvent::Change {
            range: self.range,
            preset: self.preset,
        });
        cx.notify();
    }

    /// Zooms out by doubling the range span, clamped to `max_span_days`.
    pub fn zoom_out(&mut self, cx: &mut Context<Self>) {
        if !self.can_zoom_out() {
            return;
        }
        self.range = self.range.zoom_out(self.today, self.max_span_days);
        self.preset = if self.range.end == self.today {
            TimeRangePreset::QuickRelative(self.range.span_days(), TimeRangeUnit::Days)
        } else {
            TimeRangePreset::Custom
        };
        self.draft_start = self.range.start;
        self.draft_end = self.range.end;
        cx.emit(TimeRangeEvent::Change {
            range: self.range,
            preset: self.preset,
        });
        cx.notify();
    }

    fn toggle_quick_select(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_popover == ActivePopover::QuickSelect {
            self.active_popover = ActivePopover::None;
            self.quick_unit_open = false;
        } else {
            self.active_popover = ActivePopover::QuickSelect;
            self.quick_unit_open = false;
            self.calendar_target = None;
            window.focus(&self.focus_handle, cx);
        }
        cx.notify();
    }

    fn toggle_start_and_end(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_popover == ActivePopover::StartAndEnd {
            self.active_popover = ActivePopover::None;
            self.calendar_target = None;
        } else {
            self.draft_start = self.range.start;
            self.draft_end = self.range.end;
            self.calendar_target = None;
            self.calendar_month = self.range.end.first_of_month();
            self.active_popover = ActivePopover::StartAndEnd;
            self.quick_unit_open = false;
            window.focus(&self.focus_handle, cx);
        }
        cx.notify();
    }

    fn close_popover(&mut self, cx: &mut Context<Self>) {
        if self.active_popover != ActivePopover::None {
            self.active_popover = ActivePopover::None;
            self.quick_unit_open = false;
            self.calendar_target = None;
            cx.notify();
        }
    }

    fn step_quick_amount(&mut self, delta: i32, cx: &mut Context<Self>) {
        let base = self
            .quick_amount_text
            .trim()
            .parse::<i32>()
            .unwrap_or(self.quick_amount as i32);
        let max_for_unit = match self.quick_unit {
            TimeRangeUnit::Days => self.max_span_days as i32,
            TimeRangeUnit::Weeks => (self.max_span_days / 7).max(1) as i32,
            TimeRangeUnit::Months => (self.max_span_days / 30).max(1) as i32,
        };
        let next = (base + delta).clamp(1, max_for_unit) as u32;
        self.quick_amount = next;
        self.quick_amount_text = next.to_string();
        cx.notify();
    }

    fn set_quick_unit(&mut self, unit: TimeRangeUnit, cx: &mut Context<Self>) {
        self.quick_unit = unit;
        self.quick_unit_open = false;
        self.step_quick_amount(0, cx);
    }

    fn handle_key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "escape" => self.close_popover(cx),
            "enter" => match self.active_popover {
                ActivePopover::QuickSelect => self.apply_quick_select(cx),
                ActivePopover::StartAndEnd => {
                    self.apply_custom_range(self.draft_start, self.draft_end, cx);
                }
                ActivePopover::None => {}
            },
            "up" if self.active_popover == ActivePopover::QuickSelect => {
                self.step_quick_amount(1, cx);
            }
            "down" if self.active_popover == ActivePopover::QuickSelect => {
                self.step_quick_amount(-1, cx);
            }
            "backspace" if self.active_popover == ActivePopover::QuickSelect => {
                self.quick_amount_text.pop();
                cx.notify();
            }
            _ => {
                if self.active_popover == ActivePopover::QuickSelect
                    && let Some(ch) = &event.keystroke.key_char
                    && ch.len() == 1
                    && ch.chars().all(|c| c.is_ascii_digit())
                    && self.quick_amount_text.len() < 3
                {
                    self.quick_amount_text.push_str(ch);
                    if let Ok(v) = self.quick_amount_text.parse::<u32>() {
                        self.quick_amount = v.max(1);
                    }
                    cx.notify();
                }
            }
        }
    }

    fn select_calendar_date(&mut self, date: CalendarDate, cx: &mut Context<Self>) {
        match self.calendar_target {
            Some(CalendarTarget::FirstDay) => {
                self.draft_start = date;
                if self.draft_start > self.draft_end {
                    self.draft_end = date;
                }
                if DateRange::new(self.draft_start, self.draft_end).span_days() > self.max_span_days
                {
                    self.draft_end = self
                        .draft_start
                        .add_days(i64::from(self.max_span_days) - 1)
                        .min(self.today);
                }
                self.calendar_target = None;
            }
            Some(CalendarTarget::LastDay) => {
                self.draft_end = date.min(self.today);
                if self.draft_end < self.draft_start {
                    self.draft_start = self.draft_end;
                }
                if DateRange::new(self.draft_start, self.draft_end).span_days() > self.max_span_days
                {
                    self.draft_start = self
                        .draft_end
                        .add_days(-(i64::from(self.max_span_days) - 1));
                }
                self.calendar_target = None;
            }
            None => {}
        }
        cx.notify();
    }

    fn footer_text(&self) -> SharedString {
        if let Some(custom) = &self.helper_note {
            custom.clone()
        } else {
            format!(
                "Whole days in your local time, at most {}.",
                self.max_span_days
            )
            .into()
        }
    }
}

/// Compound date/time range picker matching analytics toolbar ergonomics:
///
/// `[ 📅 ⌄ ] [ Last 30 days  Oct 21 – Nov 19 ] | [ < ] [ > ] [ 🔍- ]`
#[derive(IntoElement)]
pub struct TimeRangePicker {
    id: ElementId,
    state: Entity<TimeRangePickerState>,
    style: StyleRefinement,
}

/// Convenience constructor for [`TimeRangePicker::new`].
pub fn time_range_picker(
    id: impl Into<ElementId>,
    state: &Entity<TimeRangePickerState>,
) -> TimeRangePicker {
    TimeRangePicker::new(id, state)
}

impl TimeRangePicker {
    /// Creates a new [`TimeRangePicker`] bound to `state`.
    pub fn new(id: impl Into<ElementId>, state: &Entity<TimeRangePickerState>) -> Self {
        Self {
            id: id.into(),
            state: state.clone(),
            style: StyleRefinement::default(),
        }
    }
}

impl Styled for TimeRangePicker {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for TimeRangePicker {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let snap = self.state.read(cx);
        let preset = snap.preset;
        let range = snap.range;
        let today = snap.today;
        let active_popover = snap.active_popover;
        let quick_amount_text = snap.quick_amount_text.clone();
        let quick_unit = snap.quick_unit;
        let quick_unit_open = snap.quick_unit_open;
        let draft_start = snap.draft_start;
        let draft_end = snap.draft_end;
        let calendar_target = snap.calendar_target;
        let calendar_month = snap.calendar_month;
        let can_forward = snap.can_step_forward();
        let can_zoom = snap.can_zoom_out();
        let footer_text = snap.footer_text();
        let focus_handle = snap.focus_handle.clone();

        let hover_bg = theme.surface_hover;
        let quick_open = active_popover == ActivePopover::QuickSelect;
        let custom_open = active_popover == ActivePopover::StartAndEnd;

        // 1. Left trigger: [ 📅 ⌄ ] -> Quick Select popover
        let quick_btn = div()
            .id((self.id.clone(), "quick-trigger"))
            .flex()
            .items_center()
            .gap(px(4.))
            .h(px(28.))
            .px(px(8.))
            .rounded(theme.radius_md)
            .cursor_pointer()
            .when(quick_open, |el| el.bg(theme.primary_soft))
            .when(!quick_open, |el| el.hover(move |s| s.bg(hover_bg)))
            .child(
                Icon::new(LucideIcon::Calendar)
                    .size(IconSize::Sm)
                    .color(if quick_open {
                        theme.primary_soft_foreground
                    } else {
                        theme.foreground
                    }),
            )
            .child(
                Icon::new(LucideIcon::ChevronDown)
                    .size(IconSize::Xs)
                    .color(if quick_open {
                        theme.primary_soft_foreground
                    } else {
                        theme.muted_foreground
                    }),
            )
            .on_click({
                let state = self.state.clone();
                move |_: &ClickEvent, window, cx| {
                    state.update(cx, |s, cx| s.toggle_quick_select(window, cx));
                }
            });

        // 2. Center trigger: [ Last 30 days   Oct 21 – Nov 19 ] -> Start & End popover
        let range_label_btn = div()
            .id((self.id.clone(), "range-trigger"))
            .flex()
            .items_center()
            .gap(px(8.))
            .h(px(28.))
            .px(px(10.))
            .rounded(theme.radius_md)
            .cursor_pointer()
            .when(custom_open, |el| el.bg(theme.primary_soft))
            .when(!custom_open, |el| el.hover(move |s| s.bg(hover_bg)))
            .when(preset != TimeRangePreset::Custom, |el| {
                el.child(
                    div()
                        .text_size(px(12.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(if custom_open {
                            theme.primary_soft_foreground
                        } else {
                            theme.muted_foreground
                        })
                        .child(preset.label()),
                )
            })
            .child(
                div()
                    .text_size(px(12.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(if custom_open {
                        theme.primary_soft_foreground
                    } else {
                        theme.foreground
                    })
                    .child(range.format_compact()),
            )
            .on_click({
                let state = self.state.clone();
                move |_: &ClickEvent, window, cx| {
                    state.update(cx, |s, cx| s.toggle_start_and_end(window, cx));
                }
            });

        // 3. Right steppers: [ < ] [ > ] [ 🔍- ]
        let prev_btn = div()
            .id((self.id.clone(), "step-prev"))
            .flex()
            .items_center()
            .justify_center()
            .w(px(26.))
            .h(px(28.))
            .rounded(theme.radius_md)
            .cursor_pointer()
            .hover(move |s| s.bg(hover_bg))
            .child(
                Icon::new(LucideIcon::ChevronLeft)
                    .size(IconSize::Sm)
                    .color(theme.foreground),
            )
            .on_click({
                let state = self.state.clone();
                move |_: &ClickEvent, _, cx| {
                    state.update(cx, |s, cx| s.step_backward(cx));
                }
            });

        let next_btn = div()
            .id((self.id.clone(), "step-next"))
            .flex()
            .items_center()
            .justify_center()
            .w(px(26.))
            .h(px(28.))
            .rounded(theme.radius_md)
            .when(can_forward, |el| {
                let state = self.state.clone();
                el.cursor_pointer()
                    .hover(move |s| s.bg(hover_bg))
                    .on_click(move |_: &ClickEvent, _, cx| {
                        state.update(cx, |s, cx| s.step_forward(cx));
                    })
            })
            .child(
                Icon::new(LucideIcon::ChevronRight)
                    .size(IconSize::Sm)
                    .color(if can_forward {
                        theme.foreground
                    } else {
                        theme.disabled_foreground
                    }),
            );

        let zoom_btn = div()
            .id((self.id.clone(), "zoom-out"))
            .flex()
            .items_center()
            .justify_center()
            .w(px(26.))
            .h(px(28.))
            .rounded(theme.radius_md)
            .when(can_zoom, |el| {
                let state = self.state.clone();
                el.cursor_pointer()
                    .hover(move |s| s.bg(hover_bg))
                    .on_click(move |_: &ClickEvent, _, cx| {
                        state.update(cx, |s, cx| s.zoom_out(cx));
                    })
            })
            .child(
                Icon::new(LucideIcon::ZoomOut)
                    .size(IconSize::Sm)
                    .color(if can_zoom {
                        theme.foreground
                    } else {
                        theme.disabled_foreground
                    }),
            );

        let bar = div()
            .id((self.id.clone(), "bar"))
            .track_focus(&focus_handle)
            .on_key_down({
                let state = self.state.clone();
                move |event: &KeyDownEvent, _, cx| {
                    state.update(cx, |s, cx| s.handle_key_down(event, cx));
                }
            })
            .flex()
            .items_center()
            .h(px(34.))
            .px(px(3.))
            .gap(px(2.))
            .rounded(theme.radius_lg)
            .bg(theme.surface)
            .border_1()
            .border_color(theme.input_border)
            .shadow_xs()
            .child(quick_btn)
            .child(range_label_btn)
            .child(div().w(px(1.)).h(px(18.)).mx(px(2.)).bg(theme.border))
            .child(prev_btn)
            .child(next_btn)
            .child(zoom_btn);

        let mut root = div().id(self.id.clone()).relative().flex_shrink_0().child(bar);

        // Popover A: Quick Select
        if quick_open {
            let state_up = self.state.clone();
            let state_down = self.state.clone();
            let state_unit = self.state.clone();
            let state_apply = self.state.clone();
            let state_out = self.state.clone();

            let amount_box = div()
                .flex()
                .items_center()
                .justify_between()
                .w(px(116.))
                .h(px(34.))
                .px(px(10.))
                .rounded(theme.radius_md)
                .bg(theme.surface)
                .border_2()
                .border_color(theme.ring)
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(13.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.foreground)
                        .child(if quick_amount_text.is_empty() {
                            "0".to_string()
                        } else {
                            quick_amount_text
                        }),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap(px(1.))
                        .child(
                            div()
                                .id((self.id.clone(), "amt-up"))
                                .cursor_pointer()
                                .px(px(2.))
                                .rounded(px(2.))
                                .hover(move |s| s.bg(hover_bg))
                                .child(
                                    Icon::new(LucideIcon::ChevronUp)
                                        .size(IconSize::Xs)
                                        .color(theme.muted_foreground),
                                )
                                .on_click(move |_: &ClickEvent, _, cx| {
                                    state_up.update(cx, |s, cx| s.step_quick_amount(1, cx));
                                }),
                        )
                        .child(
                            div()
                                .id((self.id.clone(), "amt-down"))
                                .cursor_pointer()
                                .px(px(2.))
                                .rounded(px(2.))
                                .hover(move |s| s.bg(hover_bg))
                                .child(
                                    Icon::new(LucideIcon::ChevronDown)
                                        .size(IconSize::Xs)
                                        .color(theme.muted_foreground),
                                )
                                .on_click(move |_: &ClickEvent, _, cx| {
                                    state_down.update(cx, |s, cx| s.step_quick_amount(-1, cx));
                                }),
                        ),
                );

            let mut unit_picker = div().relative().child(
                div()
                    .id((self.id.clone(), "unit-btn"))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(6.))
                    .h(px(34.))
                    .px(px(10.))
                    .min_w(px(80.))
                    .rounded(theme.radius_md)
                    .bg(theme.surface)
                    .border_1()
                    .border_color(theme.input_border)
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover_bg))
                    .text_size(px(13.))
                    .text_color(theme.foreground)
                    .child(quick_unit.label())
                    .child(
                        Icon::new(LucideIcon::ChevronDown)
                            .size(IconSize::Xs)
                            .color(theme.muted_foreground),
                    )
                    .on_click(move |_: &ClickEvent, _, cx| {
                        state_unit.update(cx, |s, cx| {
                            s.quick_unit_open = !s.quick_unit_open;
                            cx.notify();
                        });
                    }),
            );

            if quick_unit_open {
                let mut menu = div()
                    .absolute()
                    .top(px(38.))
                    .left_0()
                    .w(px(104.))
                    .p(px(4.))
                    .rounded(theme.radius_md)
                    .bg(theme.popover)
                    .border_1()
                    .border_color(theme.border)
                    .shadow_lg()
                    .flex()
                    .flex_col()
                    .gap(px(2.));
                for unit in [TimeRangeUnit::Days, TimeRangeUnit::Weeks, TimeRangeUnit::Months] {
                    let active = unit == quick_unit;
                    let state = self.state.clone();
                    menu = menu.child(
                        div()
                            .id((self.id.clone(), format!("unit-{}", unit.label())))
                            .px(px(8.))
                            .py(px(5.))
                            .rounded(theme.radius_sm)
                            .cursor_pointer()
                            .text_size(px(12.5))
                            .text_color(if active {
                                theme.primary_soft_foreground
                            } else {
                                theme.foreground
                            })
                            .when(active, |el| el.bg(theme.primary_soft))
                            .when(!active, |el| el.hover(move |s| s.bg(hover_bg)))
                            .on_click(move |_: &ClickEvent, _, cx| {
                                state.update(cx, |s, cx| s.set_quick_unit(unit, cx));
                            })
                            .child(unit.label()),
                    );
                }
                unit_picker = unit_picker.child(menu);
            }

            let mut preset_list = div().flex().flex_col().gap(px(2.));
            for (idx, item) in TimeRangePreset::COMMON.into_iter().enumerate() {
                let active = item == preset;
                let state = self.state.clone();
                preset_list = preset_list.child(
                    div()
                        .id((self.id.clone(), format!("preset-{idx}")))
                        .px(px(10.))
                        .py(px(7.))
                        .rounded(theme.radius_md)
                        .cursor_pointer()
                        .text_size(px(13.))
                        .font_weight(if active {
                            FontWeight::SEMIBOLD
                        } else {
                            FontWeight::MEDIUM
                        })
                        .text_color(if active {
                            theme.primary_soft_foreground
                        } else {
                            theme.foreground
                        })
                        .when(active, |el| el.bg(theme.primary_soft))
                        .when(!active, |el| el.hover(move |s| s.bg(hover_bg)))
                        .on_click(move |_: &ClickEvent, _, cx| {
                            state.update(cx, |s, cx| s.apply_preset(item, cx));
                        })
                        .child(item.label()),
                );
            }

            let panel = div()
                .id((self.id.clone(), "quick-popover"))
                .w(px(340.))
                .mt(px(6.))
                .rounded(theme.radius_xl)
                .bg(theme.popover)
                .border_1()
                .border_color(theme.border)
                .shadow_lg()
                .flex()
                .flex_col()
                .on_mouse_down_out(move |_, _, cx| {
                    state_out.update(cx, |s, cx| s.close_popover(cx));
                })
                .child(
                    div()
                        .p(px(14.))
                        .flex()
                        .flex_col()
                        .gap(px(10.))
                        .child(
                            div()
                                .text_size(px(10.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.muted_foreground)
                                .child("QUICK SELECT"),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .text_color(theme.muted_foreground)
                                        .child("Last"),
                                )
                                .child(amount_box)
                                .child(unit_picker)
                                .child(
                                    Button::new((self.id.clone(), "quick-apply"))
                                        .label("Apply")
                                        .variant(ButtonVariant::Primary)
                                        .size(ComponentSize::Sm)
                                        .on_click(move |_, _, cx| {
                                            state_apply.update(cx, |s, cx| s.apply_quick_select(cx));
                                        }),
                                ),
                        ),
                )
                .child(div().w_full().h(px(1.)).bg(theme.border))
                .child(
                    div()
                        .p(px(14.))
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(
                            div()
                                .text_size(px(10.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.muted_foreground)
                                .child("COMMONLY USED"),
                        )
                        .child(preset_list),
                )
                .child(div().w_full().h(px(1.)).bg(theme.border))
                .child(
                    div()
                        .px(px(14.))
                        .py(px(10.))
                        .text_size(px(12.))
                        .text_color(theme.muted_foreground)
                        .child(footer_text.clone()),
                )
                .with_animation(
                    (self.id.clone(), "quick-enter"),
                    Animation::new(ANIM_FAST).with_easing(ease_out_quint()),
                    |el, delta| el.opacity(delta).mt(px(2. + (1. - delta) * 6.)),
                );

            root = root.child(
                deferred(
                    anchored()
                        .snap_to_window_with_margin(px(8.))
                        .child(panel),
                )
                .with_priority(2),
            );
        }

        // Popover B: Start and End + Calendar Grid
        if custom_open {
            let state_out = self.state.clone();
            let state_apply = self.state.clone();
            let first_active = calendar_target == Some(CalendarTarget::FirstDay);
            let last_active = calendar_target == Some(CalendarTarget::LastDay);
            let draft_range = DateRange::new(draft_start, draft_end);

            let first_field = div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(
                    div()
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.foreground)
                        .child("First day"),
                )
                .child(
                    div()
                        .id((self.id.clone(), "first-day-btn"))
                        .flex()
                        .items_center()
                        .justify_between()
                        .h(px(36.))
                        .px(px(12.))
                        .rounded(theme.radius_md)
                        .bg(theme.surface)
                        .border_1()
                        .border_color(if first_active {
                            theme.ring
                        } else {
                            theme.input_border
                        })
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover_bg))
                        .text_size(px(13.))
                        .text_color(theme.foreground)
                        .child(draft_start.format_long())
                        .child(
                            Icon::new(LucideIcon::Calendar)
                                .size(IconSize::Sm)
                                .color(theme.muted_foreground),
                        )
                        .on_click({
                            let state = self.state.clone();
                            move |_: &ClickEvent, _, cx| {
                                state.update(cx, |s, cx| {
                                    if s.calendar_target == Some(CalendarTarget::FirstDay) {
                                        s.calendar_target = None;
                                    } else {
                                        s.calendar_target = Some(CalendarTarget::FirstDay);
                                        s.calendar_month = s.draft_start.first_of_month();
                                    }
                                    cx.notify();
                                });
                            }
                        }),
                );

            let last_field = div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(
                    div()
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.foreground)
                        .child("Last day"),
                )
                .child(
                    div()
                        .id((self.id.clone(), "last-day-btn"))
                        .flex()
                        .items_center()
                        .justify_between()
                        .h(px(36.))
                        .px(px(12.))
                        .rounded(theme.radius_md)
                        .bg(theme.surface)
                        .border_1()
                        .border_color(if last_active {
                            theme.ring
                        } else {
                            theme.input_border
                        })
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover_bg))
                        .text_size(px(13.))
                        .text_color(theme.foreground)
                        .child(draft_end.format_long())
                        .child(
                            Icon::new(LucideIcon::Calendar)
                                .size(IconSize::Sm)
                                .color(theme.muted_foreground),
                        )
                        .on_click({
                            let state = self.state.clone();
                            move |_: &ClickEvent, _, cx| {
                                state.update(cx, |s, cx| {
                                    if s.calendar_target == Some(CalendarTarget::LastDay) {
                                        s.calendar_target = None;
                                    } else {
                                        s.calendar_target = Some(CalendarTarget::LastDay);
                                        s.calendar_month = s.draft_end.first_of_month();
                                    }
                                    cx.notify();
                                });
                            }
                        }),
                );

            let mut panel = div()
                .id((self.id.clone(), "custom-popover"))
                .w(px(320.))
                .mt(px(6.))
                .p(px(16.))
                .rounded(theme.radius_xl)
                .bg(theme.popover)
                .border_1()
                .border_color(theme.border)
                .shadow_lg()
                .flex()
                .flex_col()
                .gap(px(12.))
                .on_mouse_down_out(move |_, _, cx| {
                    state_out.update(cx, |s, cx| s.close_popover(cx));
                })
                .child(
                    div()
                        .text_size(px(10.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.muted_foreground)
                        .child("START AND END"),
                )
                .child(first_field)
                .child(last_field);

            if let Some(target) = calendar_target {
                let selected_day = match target {
                    CalendarTarget::FirstDay => draft_start,
                    CalendarTarget::LastDay => draft_end,
                };
                let state_sel = self.state.clone();
                let state_nav = self.state.clone();
                panel = panel.child(
                    CalendarGrid::new((self.id.clone(), "calendar"), calendar_month)
                        .w_full()
                        .selected(Some(selected_day))
                        .highlighted_range(Some(draft_range))
                        .max_date(Some(today))
                        .on_select(move |date, _, cx| {
                            state_sel.update(cx, |s, cx| s.select_calendar_date(date, cx));
                        })
                        .on_month_change(move |month, _, cx| {
                            state_nav.update(cx, |s, cx| {
                                s.calendar_month = month;
                                cx.notify();
                            });
                        }),
                );
            }

            panel = panel.child(
                div()
                    .pt(px(4.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(11.5))
                            .text_color(theme.muted_foreground)
                            .child(footer_text),
                    )
                    .child(
                        Button::new((self.id.clone(), "custom-apply"))
                            .label("Apply")
                            .variant(ButtonVariant::Primary)
                            .size(ComponentSize::Sm)
                            .on_click(move |_, _, cx| {
                                state_apply.update(cx, |s, cx| {
                                    s.apply_custom_range(s.draft_start, s.draft_end, cx);
                                });
                            }),
                    ),
            );

            let animated_panel = panel.with_animation(
                (self.id.clone(), "custom-enter"),
                Animation::new(ANIM_FAST).with_easing(ease_out_quint()),
                |el, delta| el.opacity(delta).mt(px(2. + (1. - delta) * 6.)),
            );

            root = root.child(
                deferred(
                    anchored()
                        .snap_to_window_with_margin(px(8.))
                        .child(animated_panel),
                )
                .with_priority(2),
            );
        }

        crate::Refineable::refine(root.style(), &self.style);
        root
    }
}
