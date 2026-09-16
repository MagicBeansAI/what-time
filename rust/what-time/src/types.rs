//! Shared data model: schedules, clauses, tokens, diagnostics, and results.
//!
//! The serde representations are the public JSON encoding of
//! the same structures (internally tagged by `kind`, camelCase fields,
//! optional fields omitted), so the gold corpora compare directly.

use crate::labels::Role;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Weekday {
    MO,
    TU,
    WE,
    TH,
    FR,
    SA,
    SU,
}

pub const WEEKDAYS: [Weekday; 7] = [
    Weekday::MO,
    Weekday::TU,
    Weekday::WE,
    Weekday::TH,
    Weekday::FR,
    Weekday::SA,
    Weekday::SU,
];

pub fn weekday_index(day: Weekday) -> usize {
    WEEKDAYS
        .iter()
        .position(|candidate| *candidate == day)
        .unwrap()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[allow(non_camel_case_types)]
pub enum Unit {
    minute,
    hour,
    day,
    week,
    month,
    quarter,
    year,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[allow(non_camel_case_types)]
pub enum Modifier {
    this,
    next,
    last,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[allow(non_camel_case_types)]
pub enum OpenBound {
    start,
    end,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[allow(non_camel_case_types)]
pub enum DayGroup {
    weekday,
    weekend,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[allow(non_camel_case_types)]
pub enum Direction {
    before,
    after,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[allow(non_camel_case_types)]
pub enum Edge {
    start,
    end,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[allow(non_camel_case_types)]
pub enum HolidayName {
    christmas,
    #[serde(rename = "christmas-eve")]
    ChristmasEve,
    #[serde(rename = "new-year")]
    NewYear,
    #[serde(rename = "new-years-eve")]
    NewYearsEve,
    halloween,
    valentines,
    diwali,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[allow(non_camel_case_types)]
pub enum NamedClock {
    noon,
    midnight,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[allow(non_camel_case_types)]
pub enum DayPart {
    morning,
    afternoon,
    evening,
    night,
}

#[derive(Clone, Default, PartialEq, Debug, Serialize, Deserialize)]
pub struct CalendarDate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub month: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub day: Option<i64>,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum MonthRef {
    Calendar {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        year: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        month: Option<i64>,
    },
    RelativeUnit {
        unit: RelativeUnitKind,
        modifier: Modifier,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[allow(non_camel_case_types)]
pub enum RelativeUnitKind {
    month,
    year,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DateSpec {
    Now,
    RelativeDay {
        offset: i64,
    },
    Weekday {
        days: Vec<Weekday>,
        #[serde(skip_serializing_if = "Option::is_none")]
        modifier: Option<Modifier>,
    },
    WeekdayRange {
        from: Weekday,
        to: Weekday,
    },
    DayGroup {
        group: DayGroup,
        #[serde(skip_serializing_if = "Option::is_none")]
        modifier: Option<Modifier>,
    },
    Calendar {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        year: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        month: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        day: Option<i64>,
    },
    CalendarRange {
        from: CalendarDate,
        to: CalendarDate,
    },
    CalendarPeriod {
        month: i64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        year: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        modifier: Option<Modifier>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        week: Option<i64>,
    },
    RelativeUnit {
        unit: Unit,
        modifier: Modifier,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        edge: Option<Edge>,
    },
    /// A calendar day anchored to a deictic period: "24th august last year"
    /// (period picks the year), "15th last month" (period picks month and
    /// year), "august next year" (period picks the year, no day).
    PeriodCalendar {
        period: RelativeUnitKind,
        modifier: Modifier,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        month: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        day: Option<i64>,
    },
    /// A weekday anchored to a deictic week: "friday last week",
    /// "monday next week".
    PeriodWeekday {
        modifier: Modifier,
        days: Vec<Weekday>,
    },
    OrdinalWeekday {
        ordinal: i64,
        day: Weekday,
        of: Box<MonthRef>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        recurring: Option<bool>,
    },
    Holiday {
        name: HolidayName,
    },
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ClockTime {
    Hm {
        hour: i64,
        minute: i64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        second: Option<i64>,
    },
    Named {
        named: NamedClock,
    },
    Part {
        part: DayPart,
    },
}

impl ClockTime {
    pub fn hm(hour: i64, minute: i64) -> ClockTime {
        ClockTime::Hm {
            hour,
            minute,
            second: None,
        }
    }

    pub fn as_hm(&self) -> Option<(i64, i64, Option<i64>)> {
        match self {
            ClockTime::Hm {
                hour,
                minute,
                second,
            } => Some((*hour, *minute, *second)),
            _ => None,
        }
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeSpec {
    pub start: ClockTime,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<ClockTime>,
    /// The bound is open in this direction. The opposite edge is a floor, not a
    /// real edge: "after 6pm" is `{ start: 18:00, open: "end" }`, which a reader
    /// can tell apart from "at 6pm", `{ start: 18:00 }`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open: Option<OpenBound>,
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct Quantity {
    pub amount: f64,
    pub unit: Unit,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Shift {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub components: Option<Vec<Quantity>>,
    pub amount: f64,
    pub unit: Unit,
    pub direction: Direction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_amount: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approximate: Option<bool>,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Duration {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub components: Option<Vec<Quantity>>,
    pub amount: f64,
    pub unit: Unit,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[allow(non_camel_case_types)]
pub enum Frequency {
    hourly,
    daily,
    weekly,
    monthly,
    quarterly,
    yearly,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Recurrence {
    pub freq: Frequency,
    pub interval: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by_day: Option<Vec<Weekday>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by_month_day: Option<Vec<i64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by_set_pos: Option<Vec<i64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by_month: Option<Vec<i64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub times_per: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<Box<DateSpec>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<Box<DateSpec>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub except: Option<Vec<DateSpec>>,
    /// A bound on the entire series, distinct from the duration of each occurrence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<Duration>,
}

#[derive(Clone, Default, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Clause {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_date: Option<DateSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<DateSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<TimeSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shift: Option<Shift>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<Duration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recurrence: Option<Recurrence>,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Schedule {
    pub clauses: Vec<Clause>,
}

#[derive(Clone, PartialEq, Debug)]
pub struct RawToken {
    pub start: usize,
    pub end: usize,
    pub text: String,
    pub kind: u8,
    pub features: (u32, u32),
}

#[derive(Clone, PartialEq, Debug)]
pub struct Token {
    pub raw: RawToken,
    pub label: Role,
    pub clause_start: bool,
    pub score: f32,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub code: String,
    pub message: String,
    pub start: usize,
    pub end: usize,
    pub severity: Severity,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[allow(non_camel_case_types)]
pub enum Severity {
    error,
    warning,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum WeekStart {
    MO,
    SU,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[allow(non_camel_case_types)]
pub enum BareWeekday {
    future,
    nearest,
    thisWeek,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[allow(non_camel_case_types)]
pub enum BareWeekdaysPolicy {
    once,
    weekly,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[allow(non_camel_case_types)]
pub enum NextWeekday {
    immediate,
    nextWeek,
}

pub type DayParts = std::collections::HashMap<DayPart, (String, String)>;

/// Caller context for calendar resolution. Never enters the model.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ResolveOptions {
    pub reference: String,
    #[serde(rename = "timeZone")]
    pub time_zone: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub week_start: Option<WeekStart>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bare_weekday: Option<BareWeekday>,
    /// Convert unmodified weekday clauses to weekly recurrence; defaults to once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bare_weekdays: Option<BareWeekdaysPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_weekday: Option<NextWeekday>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub day_parts: Option<DayParts>,
    /// Explicit preview filter. Without this option, the one-year horizon
    /// applies only to recurrence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
}

pub type ParseContext = ResolveOptions;

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Occurrence {
    pub start: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<String>,
    /// Set when the expression bounded only one side, as in "after 6pm".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open: Option<OpenBound>,
    pub all_day: bool,
    pub clause: usize,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Resolved {
    pub occurrences: Vec<Occurrence>,
    pub rrules: Vec<String>,
    pub truncated: bool,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeRange {
    pub start: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open: Option<OpenBound>,
    pub all_day: bool,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicTimings {
    pub tokenize_ms: f64,
    pub infer_ms: f64,
    pub resolve_ms: f64,
}

/// The published parse result: occurrences, recurrence rules, diagnostics.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParseResult {
    pub occurrences: Vec<TimeRange>,
    pub rrules: Vec<String>,
    pub truncated: bool,
    pub diagnostics: Vec<Diagnostic>,
    pub backend: &'static str,
    pub timings: PublicTimings,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<String>,
}

/// Options for constructing a reusable parser instance.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ParserOptions {
    pub backend: Option<Backend>,
    pub date_order: Option<DateOrder>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum DateOrder {
    #[default]
    MDY,
    DMY,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Expression {
    pub start: usize,
    pub end: usize,
    pub text: String,
    pub confidence: f64,
    pub schedule: Option<Schedule>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Internal model-level parse result, before calendar resolution.
#[derive(Clone, PartialEq, Debug)]
pub struct ScheduleResult {
    pub expressions: Vec<Expression>,
    pub backend: &'static str,
    pub timings: ScheduleTimings,
    pub tokens: Option<Vec<PublicToken>>,
    pub fallback_reason: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ScheduleTimings {
    pub tokenize_ms: f64,
    pub infer_ms: f64,
    pub compile_ms: f64,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicToken {
    pub start: usize,
    pub end: usize,
    pub text: String,
    pub kind: u8,
    pub label: String,
    pub clause_start: bool,
    pub score: f32,
}

/// Inference backend selection. what-time ships one scalar CPU path:
/// native inference on this model size already outperforms device dispatch,
/// so `Auto` and `Cpu` are equivalent.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Backend {
    #[default]
    Auto,
    Cpu,
}

#[derive(Clone, PartialEq, Debug)]
pub struct TagResult {
    pub tokens: Vec<Token>,
    pub unknown_labels: bool,
    pub backend: &'static str,
    pub timings: TagTimings,
    pub fallback_reason: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct TagTimings {
    pub tokenize_ms: f64,
    pub infer_ms: f64,
}
