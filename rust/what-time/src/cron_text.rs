//! Reads five-field cron expressions: an English description, and the same
//! typed [`Schedule`] the text parser builds, so a cron line gets exact
//! occurrences, RRULE export and a normalized cron from the shared resolver.
//!
//! Accepted: `minute hour day-of-month month day-of-week` with `*`, `?`,
//! numbers, ranges, lists, steps (`*/15`, `1-10/3`, `5/10` = `5-59/10`),
//! month and weekday names (`JAN`, `mon-fri`), Sunday as 0 or 7, the macros
//! `@hourly @daily @midnight @weekly @monthly @yearly @annually`, and the
//! common extensions `L` (last day of the month) in day-of-month and `5#2`
//! / `5L` (second / last Friday) in day-of-week. Fields are read in the
//! caller's time zone, as cron runs.
//!
//! As POSIX cron does, when day-of-month and day-of-week are both
//! restricted (neither is exactly `*` or `?`), a day matches when EITHER
//! does; the schedule then carries one rule per field.
//!
//! No model is involved: cron is a formal grammar, so reading it is exact.

use crate::types::{
    Clause, ClockTime, Frequency, Recurrence, Schedule, TimeSpec, WEEKDAYS, Weekday,
};

/// A parsed cron line.
#[derive(Clone, PartialEq, Debug)]
pub struct CronExpression {
    minutes: Vec<u32>,
    hours: Vec<u32>,
    days: DayField,
    months: Vec<u32>,
    weekdays: WeekdayField,
}

#[derive(Clone, PartialEq, Debug)]
enum DayField {
    Any,
    Days(Vec<u32>),
    /// `L`: the last day of the month.
    Last,
}

#[derive(Clone, PartialEq, Debug)]
enum WeekdayField {
    Any,
    /// Indexes into `WEEKDAYS` (Monday first).
    Days(Vec<u32>),
    /// `5#2`: the second Friday; `5L`: the last Friday (ordinal -1).
    Nth {
        day: u32,
        ordinal: i64,
    },
}

const MONTHS: [&str; 12] = [
    "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
];
/// Cron numbering: 0 and 7 are Sunday.
const CRON_DAYS: [&str; 7] = ["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"];

fn macro_fields(expression: &str) -> Option<&'static str> {
    Some(match expression.to_ascii_lowercase().as_str() {
        "@yearly" | "@annually" => "0 0 1 1 *",
        "@monthly" => "0 0 1 * *",
        "@weekly" => "0 0 * * 0",
        "@daily" | "@midnight" => "0 0 * * *",
        "@hourly" => "0 * * * *",
        _ => return None,
    })
}

fn number(text: &str, names: &[&str], minimum: u32, field: &str) -> Result<u32, String> {
    let upper = text.to_ascii_uppercase();
    if let Some(index) = names.iter().position(|name| *name == upper) {
        return Ok(index as u32 + minimum);
    }
    upper
        .parse::<u32>()
        .map_err(|_| format!("\"{text}\" is not a valid {field} value."))
}

/// Expands one numeric field into sorted unique values within
/// `minimum..=maximum`. Returns `None` for an unrestricted `*` / `?`.
fn field(
    text: &str,
    minimum: u32,
    maximum: u32,
    names: &[&str],
    name: &str,
) -> Result<Vec<u32>, String> {
    let mut values = Vec::new();
    for part in text.split(',') {
        if part.is_empty() {
            return Err(format!("The {name} field has an empty list item."));
        }
        let (base, step) = match part.split_once('/') {
            Some((base, step)) => {
                let step: u32 = step
                    .parse()
                    .map_err(|_| format!("\"{step}\" is not a valid step in the {name} field."))?;
                if step == 0 {
                    return Err(format!("A step in the {name} field must be at least 1."));
                }
                (base, step)
            }
            None => (part, 1),
        };
        let (start, end) = if base == "*" || base == "?" {
            (minimum, maximum)
        } else if let Some((start, end)) = base.split_once('-') {
            (
                number(start, names, minimum, name)?,
                number(end, names, minimum, name)?,
            )
        } else {
            let start = number(base, names, minimum, name)?;
            // "5/10" runs from 5 to the end of the range.
            (start, if part.contains('/') { maximum } else { start })
        };
        for value in [start, end] {
            if value < minimum || value > maximum {
                return Err(format!(
                    "{value} is outside the {name} range {minimum}–{maximum}."
                ));
            }
        }
        if start > end {
            return Err(format!(
                "The range {start}-{end} in the {name} field runs backwards."
            ));
        }
        let mut value = start;
        while value <= end {
            values.push(value);
            value += step;
        }
    }
    values.sort_unstable();
    values.dedup();
    Ok(values)
}

fn unrestricted(text: &str) -> bool {
    text == "*" || text == "?"
}

/// Parses a five-field cron line or one of the `@` macros.
pub fn parse(expression: &str) -> Result<CronExpression, String> {
    let trimmed = expression.trim();
    if trimmed.starts_with('@') {
        let Some(expanded) = macro_fields(trimmed) else {
            return Err(format!(
                "Unknown cron macro {trimmed}; use @hourly, @daily, @midnight, @weekly, @monthly, @yearly or @annually."
            ));
        };
        return parse(expanded);
    }
    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    if parts.len() != 5 {
        return Err(format!(
            "A cron expression has five fields (minute hour day-of-month month day-of-week); this one has {}.",
            parts.len()
        ));
    }
    let minutes = field(parts[0], 0, 59, &[], "minute")?;
    let hours = field(parts[1], 0, 23, &[], "hour")?;
    let days = match parts[2].to_ascii_uppercase().as_str() {
        "L" => DayField::Last,
        text if unrestricted(text) => DayField::Any,
        text if text.contains('W') || text.contains('L') => {
            return Err("Only a bare L is supported in the day-of-month field.".into());
        }
        text => DayField::Days(field(text, 1, 31, &[], "day-of-month")?),
    };
    let months = if unrestricted(parts[3]) {
        Vec::new()
    } else {
        field(parts[3], 1, 12, &MONTHS, "month")?
    };
    let weekdays = weekday_field(parts[4])?;
    if !matches!(days, DayField::Any) && matches!(weekdays, WeekdayField::Nth { .. }) {
        return Err(
            "A # or L weekday cannot be combined with a day-of-month; use * in day-of-month."
                .into(),
        );
    }
    Ok(CronExpression {
        minutes,
        hours,
        days,
        months,
        weekdays,
    })
}

fn weekday_field(text: &str) -> Result<WeekdayField, String> {
    if unrestricted(text) {
        return Ok(WeekdayField::Any);
    }
    let upper = text.to_ascii_uppercase();
    let to_index = |cron: u32| (cron + 6) % 7; // cron Sunday 0/7 -> 6, Monday 1 -> 0
    if let Some((day, ordinal)) = upper.split_once('#') {
        let day = number(day, &CRON_DAYS, 0, "day-of-week")?;
        let ordinal: i64 = ordinal
            .parse()
            .map_err(|_| format!("\"{ordinal}\" is not a valid week number."))?;
        if day > 7 || !(1..=5).contains(&ordinal) {
            return Err("Use DAY#N with a weekday 0–7 and N from 1 through 5.".into());
        }
        return Ok(WeekdayField::Nth {
            day: to_index(day),
            ordinal,
        });
    }
    if let Some(day) = upper.strip_suffix('L')
        && !day.is_empty()
        && !day.contains([',', '-', '/'])
    {
        let day = number(day, &CRON_DAYS, 0, "day-of-week")?;
        if day > 7 {
            return Err(format!("{day} is outside the day-of-week range 0–7."));
        }
        return Ok(WeekdayField::Nth {
            day: to_index(day),
            ordinal: -1,
        });
    }
    // 7 is Sunday too; ranges may end on it ("5-7").
    let values = field(&upper, 0, 7, &CRON_DAYS, "day-of-week")?;
    let mut days: Vec<u32> = values.into_iter().map(to_index).collect();
    days.sort_unstable();
    days.dedup();
    Ok(WeekdayField::Days(days))
}

// ---------------------------------------------------------------- schedule

/// One day filter of the schedule (two under the POSIX either-day rule).
#[derive(Clone)]
struct DayFilter {
    by_day: Option<Vec<Weekday>>,
    by_month_day: Option<Vec<i64>>,
    by_set_pos: Option<Vec<i64>>,
    by_month: Option<Vec<i64>>,
}

impl CronExpression {
    /// Minutes of the day the line fires at, ascending.
    fn times(&self) -> Vec<u32> {
        let mut times: Vec<u32> = self
            .hours
            .iter()
            .flat_map(|hour| self.minutes.iter().map(move |minute| hour * 60 + minute))
            .collect();
        times.sort_unstable();
        times
    }

    fn filters(&self) -> Vec<DayFilter> {
        let by_month = (!self.months.is_empty())
            .then(|| self.months.iter().map(|month| *month as i64).collect());
        let base = DayFilter {
            by_day: None,
            by_month_day: None,
            by_set_pos: None,
            by_month,
        };
        let weekday_filter = match &self.weekdays {
            WeekdayField::Any => None,
            WeekdayField::Days(days) => Some(DayFilter {
                by_day: Some(days.iter().map(|day| WEEKDAYS[*day as usize]).collect()),
                ..base.clone()
            }),
            WeekdayField::Nth { day, ordinal } => Some(DayFilter {
                by_day: Some(vec![WEEKDAYS[*day as usize]]),
                by_set_pos: Some(vec![*ordinal]),
                ..base.clone()
            }),
        };
        let day_filter = match &self.days {
            DayField::Any => None,
            DayField::Last => Some(DayFilter {
                by_month_day: Some(vec![-1]),
                ..base.clone()
            }),
            DayField::Days(days) => Some(DayFilter {
                by_month_day: Some(days.iter().map(|day| *day as i64).collect()),
                ..base.clone()
            }),
        };
        match (day_filter, weekday_filter) {
            (None, None) => vec![base],
            (Some(only), None) | (None, Some(only)) => vec![only],
            (Some(days), Some(weekdays)) => vec![days, weekdays],
        }
    }

    /// The typed schedule: one clause per (time group × day filter).
    pub fn schedule(&self) -> Result<Schedule, String> {
        let groups = time_groups(&self.times())?;
        let mut clauses = Vec::new();
        for filter in self.filters() {
            for group in &groups {
                if filter.by_set_pos.is_some() && !matches!(group, TimeGroup::At(_)) {
                    return Err(
                        "An nth-weekday cron line with many times a day is not supported.".into(),
                    );
                }
                clauses.push(clause(group, &filter));
            }
        }
        Ok(Schedule { clauses })
    }
}

enum TimeGroup {
    /// A single clock time.
    At(u32),
    /// Every `step` minutes from `first` through `last`, both included.
    Window { first: u32, last: u32, step: u32 },
}

fn progression(values: &[u32]) -> Option<u32> {
    let step = values.get(1)? - values[0];
    (step > 0 && values.windows(2).all(|pair| pair[1] - pair[0] == step)).then_some(step)
}

/// The fewest exact groups that cover every firing time.
fn time_groups(times: &[u32]) -> Result<Vec<TimeGroup>, String> {
    if times.len() == 1 {
        return Ok(vec![TimeGroup::At(times[0])]);
    }
    if times.len() >= 3
        && let Some(step) = progression(times)
    {
        return Ok(vec![TimeGroup::Window {
            first: times[0],
            last: times[times.len() - 1],
            step,
        }]);
    }
    if times.len() <= 12 {
        return Ok(times.iter().map(|time| TimeGroup::At(*time)).collect());
    }
    // Group by minute (hours in step) or by hour (minutes in step),
    // whichever needs fewer groups.
    let split = |key: fn(u32) -> u32| -> Option<Vec<TimeGroup>> {
        let mut buckets: Vec<(u32, Vec<u32>)> = Vec::new();
        for time in times {
            match buckets.iter_mut().find(|bucket| bucket.0 == key(*time)) {
                Some(bucket) => bucket.1.push(*time),
                None => buckets.push((key(*time), vec![*time])),
            }
        }
        buckets
            .into_iter()
            .map(|(_, values)| match values.len() {
                1 => Some(TimeGroup::At(values[0])),
                _ => progression(&values).map(|step| TimeGroup::Window {
                    first: values[0],
                    last: values[values.len() - 1],
                    step,
                }),
            })
            .collect()
    };
    let by_minute = split(|time| time % 60);
    let by_hour = split(|time| time / 60);
    match (by_minute, by_hour) {
        (Some(a), Some(b)) => Ok(if a.len() <= b.len() { a } else { b }),
        (Some(a), None) => Ok(a),
        (None, Some(b)) => Ok(b),
        (None, None) => Err("This cron line's times are too irregular to read.".into()),
    }
}

fn hm(minutes: u32) -> ClockTime {
    ClockTime::hm((minutes / 60) as i64, (minutes % 60) as i64)
}

fn clause(group: &TimeGroup, filter: &DayFilter) -> Clause {
    let (time, freq, interval) = match group {
        TimeGroup::At(time) => {
            // The plainest frequency each filter shape reads as; months
            // alone stay daily ("every day in January and July").
            let freq = match (
                filter.by_set_pos.is_some(),
                filter.by_day.is_some(),
                filter.by_month_day.is_some(),
                filter.by_month.is_some(),
            ) {
                (true, ..) => Frequency::monthly,
                (false, true, _, _) => Frequency::weekly,
                (false, false, true, false) => Frequency::monthly,
                (false, false, true, true) => Frequency::yearly,
                _ => Frequency::daily,
            };
            (
                TimeSpec {
                    start: hm(*time),
                    end: None,
                    open: None,
                },
                freq,
                1,
            )
        }
        TimeGroup::Window { first, last, step } => {
            let (freq, interval) = if step % 60 == 0 {
                (Frequency::hourly, (step / 60) as i64)
            } else {
                (Frequency::minutely, *step as i64)
            };
            (
                TimeSpec {
                    start: hm(*first),
                    end: Some(hm(*last)),
                    open: None,
                },
                freq,
                interval,
            )
        }
    };
    Clause {
        time: Some(time),
        recurrence: Some(Recurrence {
            freq,
            interval,
            by_day: filter.by_day.clone(),
            by_month_day: filter.by_month_day.clone(),
            by_set_pos: filter.by_set_pos.clone(),
            by_month: filter.by_month.clone(),
            times_per: None,
            count: None,
            until: None,
            start: None,
            except: None,
            span: None,
        }),
        ..Default::default()
    }
}

// ------------------------------------------------------------- description

const DAY_NAMES: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];
const MONTH_NAMES: [&str; 12] = [
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

fn clock(minutes: u32) -> String {
    let (hour, minute) = (minutes / 60, minutes % 60);
    match (hour, minute) {
        (0, 0) => "midnight".into(),
        (12, 0) => "noon".into(),
        _ => format!(
            "{}:{minute:02} {}",
            if hour % 12 == 0 { 12 } else { hour % 12 },
            if hour < 12 { "AM" } else { "PM" }
        ),
    }
}

fn list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [first, second] => format!("{first} and {second}"),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

fn ordinal(value: i64) -> String {
    if value == -1 {
        return "last".into();
    }
    let suffix = match (value % 10, value % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{value}{suffix}")
}

/// Runs of consecutive values, each as "a" or "a through b".
fn spans(values: &[u32], name: impl Fn(u32) -> String) -> Vec<String> {
    let mut runs: Vec<(u32, u32)> = Vec::new();
    for value in values {
        match runs.last_mut() {
            Some(run) if run.1 + 1 == *value => run.1 = *value,
            _ => runs.push((*value, *value)),
        }
    }
    runs.into_iter()
        .flat_map(|(start, end)| match end - start {
            0 => vec![name(start)],
            1 => vec![name(start), name(end)],
            _ => vec![format!("{} through {}", name(start), name(end))],
        })
        .collect()
}

fn duration(minutes: u32) -> String {
    match minutes {
        60 => "hour".into(),
        minutes if minutes % 60 == 0 => format!("{} hours", minutes / 60),
        1 => "minute".into(),
        minutes => format!("{minutes} minutes"),
    }
}

impl CronExpression {
    fn time_phrase(&self) -> String {
        let times = self.times();
        let all_minutes = self.minutes.len() == 60;
        let all_hours = self.hours.len() == 24;
        if all_minutes && all_hours {
            return "Every minute".into();
        }
        if times.len() <= 2 || (times.len() <= 6 && progression(&times).is_none()) {
            let names: Vec<String> = times.iter().map(|time| clock(*time)).collect();
            return format!("At {}", list(&names));
        }
        if let Some(step) = progression(&times) {
            let whole_day = times[0] < step && times[times.len() - 1] + step >= 1440;
            if whole_day && times[0] == 0 {
                return format!("Every {}", duration(step));
            }
            if whole_day && step < 60 && 60 % step == 0 {
                let marks: Vec<String> = self.minutes.iter().map(|m| m.to_string()).collect();
                return format!(
                    "Every {} at {} minutes past the hour",
                    duration(step),
                    list(&marks)
                );
            }
            return format!(
                "Every {} from {} to {}",
                duration(step),
                clock(times[0]),
                clock(times[times.len() - 1])
            );
        }
        // Minutes repeat across hours.
        let minute_phrase = if all_minutes {
            "Every minute".to_string()
        } else if let (Some(step), true) = (progression(&self.minutes), self.minutes.len() >= 3) {
            if all_hours {
                // */7 restarts every hour: :00 … :56, then :00 again.
                return format!(
                    "Every {} past the hour (minutes {} through {})",
                    duration(step),
                    self.minutes[0],
                    self.minutes[self.minutes.len() - 1]
                );
            }
            if self.minutes[0] == 0 && self.minutes[self.minutes.len() - 1] + step >= 60 {
                format!("Every {}", duration(step))
            } else {
                format!(
                    "Every {} from minute {} to {}",
                    duration(step),
                    self.minutes[0],
                    self.minutes[self.minutes.len() - 1]
                )
            }
        } else if self.minutes == [0] {
            "On the hour".to_string()
        } else {
            let marks: Vec<String> = self.minutes.iter().map(|m| m.to_string()).collect();
            format!("At {} minutes past the hour", list(&marks))
        };
        let hour_phrase = if all_hours {
            "every hour".to_string()
        } else if let (Some(step), true) = (progression(&self.hours), self.hours.len() >= 3) {
            let first = self.hours[0];
            let last = self.hours[self.hours.len() - 1];
            if step == 1 {
                format!(
                    "from {} to {}",
                    clock(first * 60),
                    clock(last * 60 + self.minutes[self.minutes.len() - 1])
                )
            } else {
                format!(
                    "every {} hours from {} to {}",
                    step,
                    clock(first * 60),
                    clock(last * 60)
                )
            }
        } else {
            let names: Vec<String> = self.hours.iter().map(|hour| clock(hour * 60)).collect();
            format!("in the hours starting {}", list(&names))
        };
        format!("{minute_phrase}, {hour_phrase}")
    }

    fn weekday_phrase(&self) -> Option<String> {
        match &self.weekdays {
            WeekdayField::Any => None,
            WeekdayField::Nth { day, ordinal: o } => Some(format!(
                "on the {} {} of",
                if *o == -1 {
                    "last".to_string()
                } else {
                    ["first", "second", "third", "fourth", "fifth"][*o as usize - 1].to_string()
                },
                DAY_NAMES[*day as usize]
            )),
            WeekdayField::Days(days) => Some(match days.as_slice() {
                [0, 1, 2, 3, 4] => "on weekdays".into(),
                [5, 6] => "on weekends".into(),
                [0, 1, 2, 3, 4, 5, 6] => "every day".into(),
                [one] => format!("every {}", DAY_NAMES[*one as usize]),
                days => format!(
                    "on {}",
                    list(&spans(days, |day| DAY_NAMES[day as usize].to_string()))
                ),
            }),
        }
    }

    fn day_phrase(&self) -> Option<String> {
        match &self.days {
            DayField::Any => None,
            DayField::Last => Some("on the last day of".into()),
            DayField::Days(days) if days.len() == 31 => None,
            DayField::Days(days) => Some(format!(
                "on the {} of",
                list(&spans(days, |day| ordinal(day as i64)))
            )),
        }
    }

    fn month_phrase(&self) -> Option<String> {
        (!self.months.is_empty() && self.months.len() < 12).then(|| {
            list(&spans(&self.months, |month| {
                MONTH_NAMES[month as usize - 1].to_string()
            }))
        })
    }

    /// An English description: "Every 15 minutes from 9:00 AM to 5:45 PM on
    /// weekdays", "At 9:00 AM on the 1st of every month".
    pub fn describe(&self) -> String {
        let time = self.time_phrase();
        let months = self.month_phrase();
        // "of every month" / "of January and July"
        let of_months = || match &months {
            Some(months) => months.clone(),
            None => "every month".to_string(),
        };
        let day = self
            .day_phrase()
            .map(|day| format!("{day} {}", of_months()));
        let weekday = match (&self.weekdays, self.weekday_phrase()) {
            (WeekdayField::Nth { .. }, Some(phrase)) => Some(format!("{phrase} {}", of_months())),
            (_, phrase) => phrase,
        };
        let months_alone = matches!(self.days, DayField::Any)
            && !matches!(self.weekdays, WeekdayField::Nth { .. });
        let mut parts = vec![time];
        match (day, weekday) {
            (Some(day), Some(weekday)) => {
                parts.push(format!("{day}, and also {weekday}"));
            }
            (Some(day), None) => parts.push(day),
            (None, Some(weekday)) => parts.push(weekday),
            (None, None) => {
                if months.is_none() && !parts[0].starts_with("Every") {
                    parts.push("every day".into());
                }
            }
        }
        if months_alone && let Some(months) = &months {
            parts.push(format!("in {months}"));
        }
        parts.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn describe(expression: &str) -> String {
        parse(expression).unwrap().describe()
    }

    #[test]
    fn describes_common_lines() {
        assert_eq!(describe("* * * * *"), "Every minute");
        assert_eq!(describe("*/15 * * * *"), "Every 15 minutes");
        assert_eq!(describe("0 * * * *"), "Every hour");
        assert_eq!(describe("0 */6 * * *"), "Every 6 hours");
        assert_eq!(describe("0 9 * * *"), "At 9:00 AM every day");
        assert_eq!(describe("30 2 * * *"), "At 2:30 AM every day");
        assert_eq!(describe("0 0 * * *"), "At midnight every day");
        assert_eq!(describe("0 9 * * 1-5"), "At 9:00 AM on weekdays");
        assert_eq!(describe("0 10 * * SAT,SUN"), "At 10:00 AM on weekends");
        assert_eq!(
            describe("0 18 * * MON,WED,FRI"),
            "At 6:00 PM on Monday, Wednesday and Friday"
        );
        assert_eq!(describe("0 7 * * 0"), "At 7:00 AM every Sunday");
        assert_eq!(describe("0 7 * * 7"), "At 7:00 AM every Sunday");
        assert_eq!(
            describe("0 8,13,20 * * *"),
            "At 8:00 AM, 1:00 PM and 8:00 PM every day"
        );
        assert_eq!(
            describe("*/15 9-17 * * MON-FRI"),
            "Every 15 minutes from 9:00 AM to 5:45 PM on weekdays"
        );
        assert_eq!(
            describe("0 9-17/2 * * *"),
            "Every 2 hours from 9:00 AM to 5:00 PM"
        );
        assert_eq!(
            describe("0 0 1 * *"),
            "At midnight on the 1st of every month"
        );
        assert_eq!(
            describe("0 9 1,15 * *"),
            "At 9:00 AM on the 1st and 15th of every month"
        );
        assert_eq!(
            describe("0 0 L * *"),
            "At midnight on the last day of every month"
        );
        assert_eq!(
            describe("0 0 25 12 *"),
            "At midnight on the 25th of December"
        );
        assert_eq!(
            describe("0 10 * * 1#1"),
            "At 10:00 AM on the first Monday of every month"
        );
        assert_eq!(
            describe("0 17 * * 5L"),
            "At 5:00 PM on the last Friday of every month"
        );
        assert_eq!(
            describe("0 9 1 * MON"),
            "At 9:00 AM on the 1st of every month, and also every Monday"
        );
        assert_eq!(
            describe("0 0 * JAN,JUL *"),
            "At midnight in January and July"
        );
        assert_eq!(
            describe("5/15 * * * *"),
            "Every 15 minutes at 5, 20, 35 and 50 minutes past the hour"
        );
        assert_eq!(
            describe("*/7 * * * *"),
            "Every 7 minutes past the hour (minutes 0 through 56)"
        );
        assert_eq!(describe("@hourly"), "Every hour");
        assert_eq!(describe("@weekly"), "At midnight every Sunday");
        assert_eq!(describe("@yearly"), "At midnight on the 1st of January");
    }

    #[test]
    fn rejects_malformed_lines_with_a_reason() {
        for (expression, fragment) in [
            ("* * * *", "five fields"),
            ("0 0 9 * * *", "five fields"),
            ("60 * * * *", "outside the minute range"),
            ("0 24 * * *", "outside the hour range"),
            ("0 0 32 * *", "outside the day-of-month range"),
            ("0 0 * 13 *", "outside the month range"),
            ("0 0 * * 8", "outside the day-of-week range"),
            ("*/0 * * * *", "at least 1"),
            ("5-1 * * * *", "runs backwards"),
            ("0 0 * FOO *", "not a valid month"),
            ("@reboot", "Unknown cron macro"),
            ("0 0 15W * *", "bare L"),
        ] {
            let error = parse(expression).unwrap_err();
            assert!(error.contains(fragment), "{expression}: {error}");
        }
    }
}
