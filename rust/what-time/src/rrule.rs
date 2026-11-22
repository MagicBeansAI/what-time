//! Exports recurrences as RFC 5545 RRULE properties, and describes each
//! exported rule as an [`ExportPlan`] that the cron exporter reads.

use crate::calendar::resolve_dates;
use crate::exclusions::excluded_weekdays;
use crate::occurrence::{NumericOccurrence, ResolutionContext};
use crate::recurrence::{
    RecurrenceExpansion, expand_recurrence, hours_for_rule, normalize_recurrence,
};
use crate::types::{Clause, DateSpec, Frequency, WEEKDAYS, Weekday};
use crate::zoned::{Civil, add_days, civil, day_of_week, iso, zoned_to_epoch};

pub struct RecurrenceExportError(pub String);

/// The exported rule in resolved form: every selector the RRULE carries,
/// plus the facts cron needs that an RRULE keeps in DTSTART.
pub struct ExportPlan {
    pub freq: Frequency,
    pub interval: i64,
    /// Weekdays after weekday exclusions; `None` leaves the day free.
    pub by_day: Option<Vec<Weekday>>,
    pub by_month: Vec<i64>,
    pub by_month_day: Vec<i64>,
    pub by_set_pos: Vec<i64>,
    /// Hours of a spread daily count ("three times a day").
    pub hours: Option<Vec<i64>>,
    /// Local wall-clock fields of the first occurrence.
    pub anchor: Civil,
    pub all_day: bool,
    /// The clause names its own clock, so sub-daily steps count from it.
    pub clocked: bool,
    /// The rule starts on a named date, or COUNT, UNTIL or a series
    /// duration ends it.
    pub bounded: bool,
    /// Single dates are removed with EXDATE.
    pub exdates: bool,
    /// Firing times of day (seconds) of a windowed sub-daily rule.
    pub slots: Option<Vec<i64>>,
}

pub struct RecurrenceExport {
    pub rrules: Vec<String>,
    pub plan: ExportPlan,
}

fn join<T: ToString>(values: &[T]) -> String {
    values
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

fn local_value(value: &str, all_day: bool) -> String {
    let prefix = if all_day { 10 } else { 19 };
    value
        .chars()
        .take(prefix)
        .collect::<String>()
        .replace(['-', ':'], "")
}

fn exception_occurrences(
    clause: &Clause,
    exceptions: &[DateSpec],
    context: &ResolutionContext,
) -> Result<Vec<NumericOccurrence>, String> {
    if exceptions.is_empty() {
        return Ok(Vec::new());
    }
    let reference = civil(context.reference, &context.options.time_zone)?;
    let mut horizon = f64::NEG_INFINITY;

    for exception in exceptions {
        for period in resolve_dates(Some(exception), &reference, context.options)? {
            let end = period.end.unwrap_or_else(|| add_days(&period.start, 1.0));
            horizon = horizon.max(zoned_to_epoch(&end, &context.options.time_zone)?.epoch_ms);
        }
    }

    // Export must include exceptions after the visible preview and retain
    // COUNT semantics.
    let expansion = expand_recurrence(clause, context, horizon, usize::MAX)?;
    Ok(expansion.date_excluded)
}

pub fn recurrence_rule(
    clause: &Clause,
    series: &RecurrenceExpansion,
    context: &ResolutionContext,
) -> Result<RecurrenceExport, RecurrenceExportError> {
    let week_start = context
        .options
        .week_start
        .unwrap_or(crate::types::WeekStart::MO);
    let first = series
        .first
        .as_ref()
        .expect("recurrence export requires a first occurrence");
    let mut rule = normalize_recurrence(clause.recurrence.as_ref().unwrap(), week_start)
        .map_err(RecurrenceExportError)?;
    let time_zone = &context.options.time_zone;
    let anchor = civil(first.start, time_zone).map_err(RecurrenceExportError)?;
    let property = |name: &str, epoch: f64| -> Result<String, RecurrenceExportError> {
        let formatted = iso(epoch, time_zone).map_err(RecurrenceExportError)?;
        if first.all_day {
            Ok(format!(
                "{name};VALUE=DATE:{}",
                local_value(&formatted, true)
            ))
        } else {
            Ok(format!(
                "{name};TZID={time_zone}:{}",
                local_value(&formatted, false)
            ))
        }
    };

    if clause.shift.is_some() {
        return Err(RecurrenceExportError(
            "Shifted recurrences need an explicit calendar transformation before RRULE export."
                .into(),
        ));
    }

    let monthly_exception = rule
        .except
        .as_ref()
        .and_then(|exceptions| (exceptions.len() == 1).then(|| exceptions[0].clone()));
    if let Some(DateSpec::OrdinalWeekday {
        ordinal,
        day,
        recurring: Some(true),
        ..
    }) = monthly_exception
        && rule.freq == Frequency::weekly
        && rule.interval == 1
        && rule
            .by_day
            .as_ref()
            .is_some_and(|days| days.len() == 1 && days[0] == day)
        && rule.by_month_day.is_none()
        && rule.by_set_pos.is_none()
    {
        // All Mondays except the first is exactly the remaining Mondays
        // each month.
        let positions: Vec<i64> = if ordinal > 0 {
            vec![1, 2, 3, 4, 5]
        } else {
            vec![-1, -2, -3, -4, -5]
        };
        rule.freq = Frequency::monthly;
        rule.by_set_pos = Some(
            positions
                .into_iter()
                .filter(|value| *value != ordinal)
                .collect(),
        );
        rule.except = None;
    }

    let mut by_day = rule.by_day.clone();
    let mut excluded: Vec<Weekday> = Vec::new();
    let mut date_exceptions: Vec<DateSpec> = Vec::new();
    for exception in rule.except.iter().flatten() {
        if matches!(
            exception,
            DateSpec::OrdinalWeekday {
                recurring: Some(true),
                ..
            }
        ) {
            return Err(RecurrenceExportError(
                "Repeating monthly exceptions cannot be represented by a single RRULE; occurrence previews still apply them."
                    .into(),
            ));
        }
        let Some(days) = excluded_weekdays(exception) else {
            date_exceptions.push(exception.clone());
            continue;
        };
        for day in days {
            if !excluded.contains(&day) {
                excluded.push(day);
            }
        }
    }

    if !excluded.is_empty() {
        let effective_days: Vec<Weekday> = by_day.clone().unwrap_or_else(|| WEEKDAYS.to_vec());
        if rule.by_set_pos.is_some() && effective_days.iter().any(|day| excluded.contains(day)) {
            return Err(RecurrenceExportError(
                "An ordinal recurrence with weekday exclusions cannot be represented by removing BYDAY values."
                    .into(),
            ));
        }
        let defaults: Vec<Weekday> = if rule.freq == Frequency::weekly {
            vec![WEEKDAYS[day_of_week(&anchor)]]
        } else {
            WEEKDAYS.to_vec()
        };
        let source = by_day.clone().unwrap_or(defaults);
        by_day = Some(
            source
                .into_iter()
                .filter(|day| !excluded.contains(day))
                .collect(),
        );
    }

    let unique_days = |days: &Option<Vec<Weekday>>| -> Option<Vec<Weekday>> {
        let days = days.as_ref().filter(|days| !days.is_empty())?;
        let mut unique: Vec<Weekday> = Vec::new();
        for day in days {
            if !unique.contains(day) {
                unique.push(*day);
            }
        }
        Some(unique)
    };

    if let Some(slots) = &series.slots {
        return windowed_rule(
            &rule,
            unique_days(&by_day),
            slots,
            series,
            context,
            clause,
            !date_exceptions.is_empty(),
        );
    }
    let plan_days = unique_days(&by_day);
    let mut plan_month_day: Vec<i64> = rule.by_month_day.clone().unwrap_or_default();
    let mut plan_month: Vec<i64> = rule.by_month.clone().unwrap_or_default();

    // RFC 5545 has no QUARTERLY frequency; a quarter is three months.
    let rrule_interval = if rule.freq == Frequency::quarterly {
        rule.interval * 3
    } else {
        rule.interval
    };
    let mut parts = vec![
        format!("FREQ={}", frequency_text(rule.freq)),
        format!("INTERVAL={}", rrule_interval),
    ];
    if context.options.week_start == Some(crate::types::WeekStart::SU) {
        parts.push("WKST=SU".into());
    }
    if by_day.as_ref().is_some_and(|days| !days.is_empty()) {
        let mut unique: Vec<Weekday> = Vec::new();
        for day in by_day.unwrap() {
            if !unique.contains(&day) {
                unique.push(day);
            }
        }
        let names: Vec<&str> = unique.iter().copied().map(weekday_text).collect();
        parts.push(format!("BYDAY={}", names.join(",")));
    }
    if rule
        .by_month
        .as_ref()
        .is_some_and(|months| !months.is_empty())
    {
        let values: Vec<String> = rule
            .by_month
            .as_ref()
            .unwrap()
            .iter()
            .map(|v| v.to_string())
            .collect();
        parts.push(format!("BYMONTH={}", values.join(",")));
    }
    if rule
        .by_month_day
        .as_ref()
        .is_some_and(|days| !days.is_empty())
    {
        let values: Vec<String> = rule
            .by_month_day
            .as_ref()
            .unwrap()
            .iter()
            .map(|v| v.to_string())
            .collect();
        parts.push(format!("BYMONTHDAY={}", values.join(",")));
    }
    if rule
        .by_set_pos
        .as_ref()
        .is_some_and(|positions| !positions.is_empty())
    {
        let values: Vec<String> = rule
            .by_set_pos
            .as_ref()
            .unwrap()
            .iter()
            .map(|v| v.to_string())
            .collect();
        parts.push(format!("BYSETPOS={}", values.join(",")));
    }

    // BYDAY filtering must not turn an implicit month/day into a whole-month
    // expansion.
    if !excluded.is_empty()
        && rule.by_day.is_none()
        && rule.by_month_day.is_none()
        && rule.by_set_pos.is_none()
    {
        if matches!(rule.freq, Frequency::monthly | Frequency::yearly) {
            parts.push(format!("BYMONTHDAY={}", anchor.day));
            plan_month_day.push(anchor.day as i64);
        }
        if rule.freq == Frequency::yearly && rule.by_month.is_none() {
            parts.push(format!("BYMONTH={}", anchor.month));
            plan_month.push(anchor.month as i64);
        }
    }

    if let Some(hours) = hours_for_rule(&rule) {
        let values: Vec<String> = hours.iter().map(|v| v.to_string()).collect();
        parts.push(format!("BYHOUR={}", values.join(",")));
    }
    let exceptions =
        exception_occurrences(clause, &date_exceptions, context).map_err(RecurrenceExportError)?;
    if let Some(count) = rule.count {
        parts.push(format!("COUNT={}", count + exceptions.len() as i64));
    }
    if let Some(series_until) = series.until {
        let last = series_until - 1.0;
        let until = if first.all_day {
            let formatted = iso(last, time_zone).map_err(RecurrenceExportError)?;
            local_value(&formatted, true)
        } else {
            utc_iso_basic(last)
        };
        parts.push(format!("UNTIL={until}"));
    }

    let mut lines = vec![property("DTSTART", first.start)?];
    if let Some(end) = first.end {
        lines.push(property("DTEND", end)?);
    }
    lines.push(format!("RRULE:{}", parts.join(";")));
    if !exceptions.is_empty() {
        let values: Vec<String> = exceptions
            .iter()
            .map(|occurrence| {
                iso(occurrence.start, time_zone)
                    .map(|value| local_value(&value, first.all_day))
                    .map_err(RecurrenceExportError)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let kind = if first.all_day {
            "VALUE=DATE".to_string()
        } else {
            format!("TZID={time_zone}")
        };
        lines.push(format!("EXDATE;{kind}:{}", values.join(",")));
    }
    Ok(RecurrenceExport {
        rrules: vec![lines.join("\n")],
        plan: ExportPlan {
            freq: rule.freq,
            interval: rule.interval,
            by_day: plan_days,
            by_month: plan_month,
            by_month_day: plan_month_day,
            by_set_pos: rule.by_set_pos.clone().unwrap_or_default(),
            hours: hours_for_rule(&rule),
            anchor,
            all_day: first.all_day,
            clocked: clause.time.is_some(),
            bounded: rule.count.is_some() || series.until.is_some() || rule.start.is_some(),
            exdates: !exceptions.is_empty(),
            slots: None,
        },
    })
}

type SlotGroup = (Vec<i64>, Vec<(i64, i64)>);

/// A sub-daily rule inside a daily clock window fires at fixed times of
/// day, so it exports as FREQ=DAILY with BYHOUR/BYMINUTE. Hours that share
/// a minute set form one rule: "every 15 minutes from 9am to 6pm" is the
/// quarter hours of 9–17 plus 18:00 itself.
fn windowed_rule(
    rule: &crate::types::Recurrence,
    by_day: Option<Vec<Weekday>>,
    slots: &[(i64, crate::occurrence::NumericOccurrence)],
    series: &RecurrenceExpansion,
    context: &ResolutionContext,
    clause: &Clause,
    date_exceptions: bool,
) -> Result<RecurrenceExport, RecurrenceExportError> {
    let time_zone = &context.options.time_zone;
    let first = series
        .first
        .as_ref()
        .expect("recurrence export requires a first occurrence");
    let anchor = civil(first.start, time_zone).map_err(RecurrenceExportError)?;
    let selects_days = by_day.is_some() || rule.by_month_day.is_some() || rule.by_month.is_some();
    let window_start = slots.first().map(|slot| slot.0).unwrap_or(0);
    let overnight = slots.iter().any(|slot| slot.0 < window_start);
    if overnight && selects_days {
        return Err(RecurrenceExportError(
            "A window that crosses midnight cannot carry day selectors in one RRULE; occurrence previews still apply them."
                .into(),
        ));
    }
    if date_exceptions {
        return Err(RecurrenceExportError(
            "Date exceptions on a windowed sub-daily rule are not exported; occurrence previews still apply them."
                .into(),
        ));
    }

    // (hour) -> sorted [(minute, second)], then hours grouped by that set.
    let mut by_hour: Vec<(i64, Vec<(i64, i64)>)> = Vec::new();
    for (seconds, _) in slots {
        let hour = seconds / 3600;
        let rest = ((seconds / 60) % 60, seconds % 60);
        match by_hour.iter_mut().find(|entry| entry.0 == hour) {
            Some(entry) => entry.1.push(rest),
            None => by_hour.push((hour, vec![rest])),
        }
    }
    for entry in by_hour.iter_mut() {
        entry.1.sort();
    }
    // (hours, [(minute, second)]) pairs whose product is exact.
    let mut groups: Vec<SlotGroup> = Vec::new();
    for (hour, rest) in by_hour {
        match groups.iter_mut().find(|group| group.1 == rest) {
            Some(group) => group.0.push(hour),
            None => groups.push((vec![hour], rest)),
        }
    }
    if rule.count.is_some() && groups.len() > 1 {
        return Err(RecurrenceExportError(
            "An occurrence count on this window needs more than one RRULE; occurrence previews still apply it."
                .into(),
        ));
    }

    let mut rrules = Vec::new();
    for (hours, rest) in &mut groups {
        hours.sort();
        // A BYMINUTE × BYSECOND product only equals the slot list when the
        // seconds agree across the group.
        let second = rest[0].1;
        if rest.iter().any(|value| value.1 != second) {
            return Err(RecurrenceExportError(
                "This window's seconds differ between slots and cannot be one RRULE.".into(),
            ));
        }
        let start = slots
            .iter()
            .filter(|(seconds, _)| {
                hours.contains(&(seconds / 3600))
                    && rest.contains(&((seconds / 60) % 60, seconds % 60))
            })
            .map(|(_, occurrence)| occurrence.start)
            .fold(f64::INFINITY, f64::min);
        let formatted = iso(start, time_zone).map_err(RecurrenceExportError)?;
        let mut lines = vec![format!(
            "DTSTART;TZID={time_zone}:{}",
            local_value(&formatted, false)
        )];
        if let Some(duration) = &clause.duration {
            let end = crate::occurrence::add_duration(start, duration, time_zone)
                .map_err(RecurrenceExportError)?;
            let formatted = iso(end, time_zone).map_err(RecurrenceExportError)?;
            lines.push(format!(
                "DTEND;TZID={time_zone}:{}",
                local_value(&formatted, false)
            ));
        }
        let mut parts = vec!["FREQ=DAILY".to_string(), "INTERVAL=1".to_string()];
        if context.options.week_start == Some(crate::types::WeekStart::SU) {
            parts.push("WKST=SU".into());
        }
        if let Some(days) = &by_day {
            let names: Vec<&str> = days.iter().copied().map(weekday_text).collect();
            parts.push(format!("BYDAY={}", names.join(",")));
        }
        if let Some(months) = rule.by_month.as_ref().filter(|months| !months.is_empty()) {
            parts.push(format!("BYMONTH={}", join(months)));
        }
        if let Some(days) = rule.by_month_day.as_ref().filter(|days| !days.is_empty()) {
            parts.push(format!("BYMONTHDAY={}", join(days)));
        }
        parts.push(format!("BYHOUR={}", join(hours)));
        let minutes: Vec<i64> = rest.iter().map(|value| value.0).collect();
        parts.push(format!("BYMINUTE={}", join(&minutes)));
        if second != 0 {
            parts.push(format!("BYSECOND={second}"));
        }
        if let Some(count) = rule.count {
            parts.push(format!("COUNT={count}"));
        }
        if let Some(series_until) = series.until {
            parts.push(format!("UNTIL={}", utc_iso_basic(series_until - 1.0)));
        }
        lines.push(format!("RRULE:{}", parts.join(";")));
        rrules.push((start, lines.join("\n")));
    }
    rrules.sort_by(|left, right| left.0.total_cmp(&right.0));
    Ok(RecurrenceExport {
        rrules: rrules.into_iter().map(|rule| rule.1).collect(),
        plan: ExportPlan {
            freq: rule.freq,
            interval: rule.interval,
            by_day,
            by_month: rule.by_month.clone().unwrap_or_default(),
            by_month_day: rule.by_month_day.clone().unwrap_or_default(),
            by_set_pos: Vec::new(),
            hours: None,
            anchor,
            all_day: false,
            clocked: true,
            bounded: rule.count.is_some() || series.until.is_some() || rule.start.is_some(),
            exdates: false,
            slots: Some(slots.iter().map(|slot| slot.0).collect()),
        },
    })
}

fn frequency_text(freq: Frequency) -> &'static str {
    match freq {
        Frequency::minutely => "MINUTELY",
        Frequency::hourly => "HOURLY",
        Frequency::daily => "DAILY",
        Frequency::weekly => "WEEKLY",
        Frequency::monthly => "MONTHLY",
        Frequency::quarterly => "MONTHLY",
        Frequency::yearly => "YEARLY",
    }
}

fn weekday_text(day: Weekday) -> &'static str {
    match day {
        Weekday::MO => "MO",
        Weekday::TU => "TU",
        Weekday::WE => "WE",
        Weekday::TH => "TH",
        Weekday::FR => "FR",
        Weekday::SA => "SA",
        Weekday::SU => "SU",
    }
}

/// `new Date(last).toISOString().slice(0, 19).replace(/[-:]/g, "") + "Z"`.
fn utc_iso_basic(epoch: f64) -> String {
    let utc_value = crate::zoned::civil(epoch, "UTC").expect("UTC is always valid");
    format!(
        "{:04}{}{}T{}{}{}Z",
        utc_value.year,
        two(utc_value.month as i64),
        two(utc_value.day as i64),
        two(utc_value.hour as i64),
        two(utc_value.minute as i64),
        two(utc_value.second as i64)
    )
}

fn two(value: i64) -> String {
    format!("{value:02}")
}
