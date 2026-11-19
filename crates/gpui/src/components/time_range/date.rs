//! Zero-dependency civil calendar date arithmetic, ranges, units, and presets.

use crate::SharedString;

/// A proleptic Gregorian calendar date (`YYYY-MM-DD`) with zero external crate dependencies.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CalendarDate {
    /// Full year (e.g. `2026`).
    pub year: i32,
    /// 1-based month (`1..=12`).
    pub month: u8,
    /// 1-based day of month (`1..=31`).
    pub day: u8,
}

impl CalendarDate {
    /// Creates a validated [`CalendarDate`], clamping month to `1..=12` and day to the month's length.
    pub fn new(year: i32, month: u8, day: u8) -> Self {
        let month = month.clamp(1, 12);
        let max_day = days_in_month(year, month);
        let day = day.clamp(1, max_day);
        Self { year, month, day }
    }

    /// Converts this date to days since Unix epoch (`1970-01-01`) using Howard Hinnant's algorithm.
    pub fn to_days_since_epoch(self) -> i64 {
        let y = i64::from(self.year) - if self.month <= 2 { 1 } else { 0 };
        let era = y.div_euclid(400);
        let yoe = (y - era * 400) as u64;
        let m = u64::from(self.month);
        let d = u64::from(self.day);
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe as i64 - 719_468
    }

    /// Constructs a [`CalendarDate`] from days since Unix epoch (`1970-01-01`).
    pub fn from_days_since_epoch(days: i64) -> Self {
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = (z - era * 146_097) as u64;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let y = yoe as i64 + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = (y + if m <= 2 { 1 } else { 0 }) as i32;
        Self {
            year,
            month: m as u8,
            day: d as u8,
        }
    }

    /// Returns the local/UTC current date derived from [`std::time::SystemTime::now`].
    pub fn today_utc() -> Self {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        Self::from_days_since_epoch(secs.div_euclid(86_400))
    }

    /// Returns a date offset by `delta_days` (positive or negative).
    pub fn add_days(self, delta_days: i64) -> Self {
        Self::from_days_since_epoch(self.to_days_since_epoch() + delta_days)
    }

    /// Returns the signed number of days from `other` to `self` (`self - other`).
    pub fn days_since(self, other: Self) -> i64 {
        self.to_days_since_epoch() - other.to_days_since_epoch()
    }

    /// Returns the Monday-based weekday index (`0 = Mon`, `1 = Tue`, ..., `6 = Sun`).
    pub fn weekday_mon0(self) -> u8 {
        // 1970-01-01 was a Thursday (index 3 when Mon = 0).
        (self.to_days_since_epoch() + 3).rem_euclid(7) as u8
    }

    /// First day of this date's month.
    pub fn first_of_month(self) -> Self {
        Self::new(self.year, self.month, 1)
    }

    /// Last day of this date's month.
    pub fn last_of_month(self) -> Self {
        Self::new(self.year, self.month, days_in_month(self.year, self.month))
    }

    /// Returns the first day of the previous month.
    pub fn prev_month(self) -> Self {
        if self.month == 1 {
            Self::new(self.year - 1, 12, 1)
        } else {
            Self::new(self.year, self.month - 1, 1)
        }
    }

    /// Returns the first day of the next month.
    pub fn next_month(self) -> Self {
        if self.month == 12 {
            Self::new(self.year + 1, 1, 1)
        } else {
            Self::new(self.year, self.month + 1, 1)
        }
    }

    /// Short 3-letter English month name (`"Jan"`..`"Dec"`).
    pub fn month_short(self) -> &'static str {
        MONTH_SHORT[(self.month.clamp(1, 12) - 1) as usize]
    }

    /// Full English month name (`"January"`..`"December"`).
    pub fn month_long(self) -> &'static str {
        MONTH_LONG[(self.month.clamp(1, 12) - 1) as usize]
    }

    /// Formats as `"Oct 21"`.
    pub fn format_short(self) -> String {
        format!("{} {}", self.month_short(), self.day)
    }

    /// Formats as `"Oct 21, 2026"`.
    pub fn format_long(self) -> String {
        format!("{} {}, {}", self.month_short(), self.day, self.year)
    }

    /// Formats month header as `"November 2026"`.
    pub fn format_month_year(self) -> String {
        format!("{} {}", self.month_long(), self.year)
    }
}

/// Returns `true` if `year` is a Gregorian leap year.
pub fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// Returns the number of days in `(year, month)`.
pub fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

const MONTH_SHORT: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

const MONTH_LONG: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// Inclusive civil date interval `[start, end]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DateRange {
    /// First included day.
    pub start: CalendarDate,
    /// Last included day (`>= start`).
    pub end: CalendarDate,
}

impl DateRange {
    /// Creates a normalized inclusive range where `start <= end`.
    pub fn new(a: CalendarDate, b: CalendarDate) -> Self {
        if a <= b {
            Self { start: a, end: b }
        } else {
            Self { start: b, end: a }
        }
    }

    /// Creates an inclusive range of `count` days ending on `anchor` (e.g. "Last 30 days").
    pub fn last_days(anchor: CalendarDate, count: u32) -> Self {
        let count = count.max(1) as i64;
        Self {
            start: anchor.add_days(-(count - 1)),
            end: anchor,
        }
    }

    /// Number of inclusive days covered by this range (`>= 1`).
    pub fn span_days(self) -> u32 {
        (self.end.days_since(self.start).max(0) + 1) as u32
    }

    /// Returns `true` if `date` falls within `[self.start, self.end]`.
    pub fn contains(self, date: CalendarDate) -> bool {
        date >= self.start && date <= self.end
    }

    /// Shifts the entire window backward by its own span.
    pub fn shift_backward(self) -> Self {
        let span = i64::from(self.span_days());
        Self {
            start: self.start.add_days(-span),
            end: self.end.add_days(-span),
        }
    }

    /// Shifts the entire window forward by its own span, clamped so `end <= max_date`.
    pub fn shift_forward(self, max_date: CalendarDate) -> Self {
        let span = i64::from(self.span_days());
        let shifted_end = self.end.add_days(span);
        if shifted_end >= max_date {
            Self::last_days(max_date, self.span_days())
        } else {
            Self {
                start: self.start.add_days(span),
                end: shifted_end,
            }
        }
    }

    /// Zooms out by doubling the window span (centered/anchored up to `max_date`), clamped to `max_span_days`.
    pub fn zoom_out(self, max_date: CalendarDate, max_span_days: u32) -> Self {
        let target_span = (self.span_days().saturating_mul(2)).min(max_span_days.max(1));
        let half_extra = i64::from(target_span.saturating_sub(self.span_days())) / 2;
        let mut new_end = self.end.add_days(half_extra);
        if new_end > max_date {
            new_end = max_date;
        }
        Self::last_days(new_end, target_span)
    }

    /// Formats the range compactly as `"Oct 21 – Nov 19"` (including years if they differ).
    pub fn format_compact(self) -> String {
        if self.start == self.end {
            self.start.format_long()
        } else if self.start.year == self.end.year {
            format!("{} – {}", self.start.format_short(), self.end.format_short())
        } else {
            format!("{} – {}", self.start.format_long(), self.end.format_long())
        }
    }
}

/// Duration unit used in the Quick Select row (`Last [N] [days|weeks|months]`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TimeRangeUnit {
    /// Whole calendar days.
    #[default]
    Days,
    /// 7-day weeks.
    Weeks,
    /// Approximate 30-day months.
    Months,
}

impl TimeRangeUnit {
    /// Display label in the unit dropdown.
    pub fn label(self) -> &'static str {
        match self {
            Self::Days => "days",
            Self::Weeks => "weeks",
            Self::Months => "months",
        }
    }

    /// Cycles to the next unit (`days -> weeks -> months -> days`).
    pub fn next(self) -> Self {
        match self {
            Self::Days => Self::Weeks,
            Self::Weeks => Self::Months,
            Self::Months => Self::Days,
        }
    }

    /// Converts `amount` of this unit to days, clamped to `1..=max_span_days`.
    pub fn to_days(self, amount: u32, max_span_days: u32) -> u32 {
        let raw = match self {
            Self::Days => amount,
            Self::Weeks => amount.saturating_mul(7),
            Self::Months => amount.saturating_mul(30),
        };
        raw.clamp(1, max_span_days.max(1))
    }
}

/// Named relative time-range preset displayed under **COMMONLY USED**.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TimeRangePreset {
    /// Last 7 whole days ending on `anchor`.
    Last7Days,
    /// Last 30 whole days ending on `anchor`.
    Last30Days,
    /// Last 90 whole days ending on `anchor`.
    Last90Days,
    /// First day of `anchor`'s month through `anchor`.
    MonthToDate,
    /// Full calendar month preceding `anchor`'s month.
    LastMonth,
    /// Custom relative "Last N units".
    QuickRelative(u32, TimeRangeUnit),
    /// Explicit user-selected start & end calendar range.
    Custom,
}

impl TimeRangePreset {
    /// Standard list of commonly used presets.
    pub const COMMON: [Self; 5] = [
        Self::Last7Days,
        Self::Last30Days,
        Self::Last90Days,
        Self::MonthToDate,
        Self::LastMonth,
    ];

    /// Human-readable label shown in the picker trigger and preset menu.
    pub fn label(self) -> SharedString {
        match self {
            Self::Last7Days => "Last 7 days".into(),
            Self::Last30Days => "Last 30 days".into(),
            Self::Last90Days => "Last 90 days".into(),
            Self::MonthToDate => "Month to date".into(),
            Self::LastMonth => "Last month".into(),
            Self::QuickRelative(n, unit) => {
                if n == 1 {
                    let singular = match unit {
                        TimeRangeUnit::Days => "day",
                        TimeRangeUnit::Weeks => "week",
                        TimeRangeUnit::Months => "month",
                    };
                    format!("Last 1 {singular}").into()
                } else {
                    format!("Last {n} {}", unit.label()).into()
                }
            }
            Self::Custom => "Custom".into(),
        }
    }

    /// Maps a day span ending on `today` back to a named preset (`Last7Days`, `Last30Days`,
    /// `Last90Days`) when it matches one, or [`Self::QuickRelative`] otherwise.
    pub fn from_days_ending_today(span_days: u32) -> Self {
        match span_days {
            7 => Self::Last7Days,
            30 => Self::Last30Days,
            90 => Self::Last90Days,
            n => Self::QuickRelative(n.max(1), TimeRangeUnit::Days),
        }
    }

    /// Resolves this preset into a concrete [`DateRange`] anchored at `today`, clamped to `max_span_days`.
    pub fn resolve(self, today: CalendarDate, max_span_days: u32) -> DateRange {
        let max_span = max_span_days.max(1);
        match self {
            Self::Last7Days => DateRange::last_days(today, 7.min(max_span)),
            Self::Last30Days => DateRange::last_days(today, 30.min(max_span)),
            Self::Last90Days => DateRange::last_days(today, 90.min(max_span)),
            Self::MonthToDate => {
                let start = today.first_of_month();
                let range = DateRange::new(start, today);
                if range.span_days() > max_span {
                    DateRange::last_days(today, max_span)
                } else {
                    range
                }
            }
            Self::LastMonth => {
                let prev_first = today.prev_month();
                let prev_last = prev_first.last_of_month();
                DateRange::new(prev_first, prev_last)
            }
            Self::QuickRelative(n, unit) => {
                let days = unit.to_days(n, max_span);
                DateRange::last_days(today, days)
            }
            Self::Custom => DateRange::last_days(today, 30.min(max_span)),
        }
    }
}
