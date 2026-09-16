//! Exports recurrences as RFC 5545 RRULE properties.

use crate::calendar::resolve_dates;
use crate::exclusions::excluded_weekdays;
use crate::occurrence::{NumericOccurrence, ResolutionContext};
use crate::recurrence::{
    RecurrenceExpansion, expand_recurrence, hours_for_rule, normalize_recurrence,
};
use crate::types::{Clause, DateSpec, Frequency, WEEKDAYS, Weekday};
use crate::zoned::{add_days, civil, day_of_week, iso, zoned_to_epoch};

pub struct RecurrenceExportError(pub String);

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
) -> Result<String, RecurrenceExportError> {
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
        }
        if rule.freq == Frequency::yearly && rule.by_month.is_none() {
            parts.push(format!("BYMONTH={}", anchor.month));
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
    Ok(lines.join("\n"))
}

fn frequency_text(freq: Frequency) -> &'static str {
    match freq {
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
