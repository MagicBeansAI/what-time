//! Expands recurrence rules into bounded occurrence previews.

use crate::calendar::{LocalPeriod, add_civil, resolve_dates, week_beginning};
use crate::clock::resolve_time;
use crate::exclusions::{excluded_weekdays, exclusion_filter};
use crate::occurrence::{
    NumericOccurrence, ResolutionContext, add_duration, at_time, resolve_occurrence,
};
use crate::types::{Clause, Frequency, Recurrence, Unit, WEEKDAYS, WeekStart};
use crate::zoned::{
    Civil, add_days, civil, day_number, day_of_week, days_in_month, utc, zoned_to_epoch,
};

pub struct RecurrenceExpansion {
    pub occurrences: Vec<NumericOccurrence>,
    pub first: Option<NumericOccurrence>,
    pub truncated: bool,
    /// Exclusive end of the actual rule, independent of the preview horizon.
    pub until: Option<f64>,
    #[allow(dead_code)]
    pub excluded: Vec<NumericOccurrence>,
    pub date_excluded: Vec<NumericOccurrence>,
    /// For a sub-daily rule confined to a daily clock window ("every 15
    /// minutes between 9am and 6pm"): each firing time of day, in seconds
    /// from the window day's midnight, with its first occurrence. Exports
    /// build one rule per group of slots.
    pub slots: Option<Vec<(i64, NumericOccurrence)>>,
}

/// The step of a sub-daily rule, in minutes.
pub fn sub_daily_step(rule: &Recurrence) -> Option<i64> {
    match rule.freq {
        Frequency::minutely => Some(rule.interval),
        Frequency::hourly => Some(rule.interval * 60),
        _ => None,
    }
}

/// The daily firing times of a sub-daily rule with a clock window, as
/// seconds from the window day's midnight (past 86 400 when the window
/// crosses midnight). The window includes both of its boundaries: "every 2
/// hours from 9am to 5pm" fires at 9, 11, 1, 3 and 5.
pub fn window_times(
    clause: &Clause,
    rule: &Recurrence,
    options: &crate::types::ResolveOptions,
) -> Result<Option<Vec<f64>>, String> {
    let Some(step) = sub_daily_step(rule) else {
        return Ok(None);
    };
    let Some(time) = &clause.time else {
        return Ok(None);
    };
    let (start, end) = resolve_time(time, options)?;
    let open_end = time.open == Some(crate::types::OpenBound::end);
    let (end, inclusive) = match end {
        // "every hour after 6pm" runs to the end of that day.
        None if open_end => (86_400.0, false),
        None => return Ok(None),
        Some(end) if end == start => {
            return Err("An hourly recurrence window needs distinct clock boundaries.".into());
        }
        Some(end) if end > start => (end, true),
        Some(end) => (end + 86_400.0, true),
    };
    let step = step as f64 * 60.0;
    let mut times = Vec::new();
    let mut value = start;
    while (value < end || (inclusive && value == end)) && value < start + 86_400.0 {
        times.push(value);
        value += step;
    }
    Ok(Some(times))
}

pub fn hours_for_rule(rule: &Recurrence) -> Option<Vec<i64>> {
    if rule.freq != Frequency::daily {
        return None;
    }
    rule.times_per.map(|times_per| {
        (0..times_per)
            .map(|index| (index * 24) / times_per.max(1))
            .collect()
    })
}

pub fn normalize_recurrence(
    rule: &Recurrence,
    week_start: WeekStart,
) -> Result<Recurrence, String> {
    let Some(times_per) = rule.times_per else {
        return Ok(rule.clone());
    };
    if times_per == 1 {
        // "once a month" is the plain rule.
        let mut normalized = rule.clone();
        normalized.times_per = None;
        return Ok(normalized);
    }
    let maximum = match rule.freq {
        Frequency::weekly => 7,
        Frequency::daily => 24,
        Frequency::monthly => 4,
        Frequency::yearly => 12,
        Frequency::hourly => 60,
        _ => 0,
    };
    if !(1..=maximum).contains(&times_per)
        || (rule.freq == Frequency::yearly && 12 % times_per != 0)
        || (rule.freq == Frequency::hourly && (60 % times_per != 0 || rule.interval != 1))
    {
        return Err(
            "Frequency counts need 1–7 times per week, 1–24 per day, 1–4 per month, an even split of the year, or an even split of the hour."
                .into(),
        );
    }
    if rule.freq == Frequency::hourly {
        // "twice an hour" is every 30 minutes.
        let mut normalized = rule.clone();
        normalized.freq = Frequency::minutely;
        normalized.interval = 60 / times_per;
        normalized.times_per = None;
        return Ok(normalized);
    }
    if rule.freq == Frequency::monthly {
        let mut normalized = rule.clone();
        if rule.by_month_day.is_none() && rule.by_day.is_none() {
            normalized.by_month_day = Some(
                (0..times_per)
                    .map(|index| 1 + index * 28 / times_per)
                    .collect(),
            );
        }
        normalized.times_per = None;
        return Ok(normalized);
    }
    if rule.freq == Frequency::yearly {
        let mut normalized = rule.clone();
        if rule.by_month.is_none() {
            normalized.by_month = Some(
                (0..times_per)
                    .map(|index| 1 + index * 12 / times_per)
                    .collect(),
            );
            if rule.by_month_day.is_none() && rule.by_day.is_none() {
                normalized.by_month_day = Some(vec![1]);
            }
        }
        normalized.times_per = None;
        return Ok(normalized);
    }
    if rule.freq == Frequency::weekly && rule.by_day.is_none() {
        let first_day = if week_start == WeekStart::SU { 6 } else { 0 };
        let by_day = (0..times_per)
            .map(|index| WEEKDAYS[((first_day + (index * 7) / times_per) % 7) as usize])
            .collect();
        let mut normalized = rule.clone();
        normalized.by_day = Some(by_day);
        return Ok(normalized);
    }
    Ok(rule.clone())
}

fn matches_month_day(date: &Civil, days: &[i64]) -> bool {
    let last = days_in_month(date.year, date.month) as i64;
    days.iter()
        .any(|day| date.day as i64 == if *day < 0 { last + day + 1 } else { *day })
}

fn matches_position(date: &Civil, rule: &Recurrence) -> bool {
    let months: Vec<i64> = if rule.freq == Frequency::yearly {
        rule.by_month.clone().unwrap_or_else(|| (1..=12).collect())
    } else {
        vec![date.month as i64]
    };
    let mut candidates: Vec<i64> = Vec::new();

    for month in months.iter().copied() {
        let last = days_in_month(date.year, month as i32) as i64;
        for day in 1..=last {
            let candidate = Civil {
                month: month as i32,
                day: day as i32,
                ..*date
            };
            if rule
                .by_day
                .as_ref()
                .is_some_and(|days| !days.contains(&WEEKDAYS[day_of_week(&candidate)]))
            {
                continue;
            }
            if rule
                .by_month_day
                .as_ref()
                .is_some_and(|days| !matches_month_day(&candidate, days))
            {
                continue;
            }
            candidates.push(day_number(&candidate));
        }
    }

    rule.by_set_pos.as_ref().unwrap().iter().any(|position| {
        let index = if *position > 0 {
            (*position - 1) as usize
        } else {
            (candidates.len() as i64 + position) as usize
        };
        candidates.get(index).copied() == Some(day_number(date))
    })
}

fn matches(date: &Civil, anchor: &Civil, rule: &Recurrence, anchor_week: i64) -> bool {
    let day = WEEKDAYS[day_of_week(date)];
    if rule
        .by_day
        .as_ref()
        .is_some_and(|days| !days.contains(&day))
    {
        return false;
    }
    if rule
        .by_month
        .as_ref()
        .is_some_and(|months| !months.contains(&(date.month as i64)))
    {
        return false;
    }
    if rule
        .by_month_day
        .as_ref()
        .is_some_and(|days| !matches_month_day(date, days))
    {
        return false;
    }
    if let Some(hours) = hours_for_rule(rule)
        && !hours.contains(&(date.hour as i64))
    {
        return false;
    }

    match rule.freq {
        Frequency::minutely => {
            let minutes = ((utc(date) - utc(anchor)) / 60_000.0).round();
            minutes as i64 % rule.interval == 0
        }
        Frequency::hourly => {
            let hours = ((utc(date) - utc(anchor)) / 3_600_000.0).round();
            hours as i64 % rule.interval == 0
        }
        Frequency::daily => (day_number(date) - day_number(anchor)) % rule.interval == 0,
        Frequency::weekly => {
            let distance = (day_number(date) - anchor_week) / 7;
            distance % rule.interval == 0
                && (rule.by_day.is_some() || day_of_week(date) == day_of_week(anchor))
        }
        Frequency::monthly => {
            let months = (date.year - anchor.year) as i64 * 12 + (date.month - anchor.month) as i64;
            if months % rule.interval != 0 {
                return false;
            }
            if rule.by_set_pos.is_some() {
                return matches_position(date, rule);
            }
            rule.by_month_day.is_some() || rule.by_day.is_some() || date.day == anchor.day
        }
        // A quarter is three months on the calendar; everything else
        // follows the monthly rules.
        Frequency::quarterly => {
            let months = (date.year - anchor.year) as i64 * 12 + (date.month - anchor.month) as i64;
            if months % (rule.interval * 3) != 0 {
                return false;
            }
            if rule.by_set_pos.is_some() {
                return matches_position(date, rule);
            }
            rule.by_month_day.is_some() || rule.by_day.is_some() || date.day == anchor.day
        }
        Frequency::yearly => {
            if (date.year - anchor.year) as i64 % rule.interval != 0 {
                return false;
            }
            if rule.by_set_pos.is_some() {
                return matches_position(date, rule);
            }
            let expands_months =
                rule.by_month.is_some() || rule.by_month_day.is_some() || rule.by_day.is_some();
            if !expands_months && date.month != anchor.month {
                return false;
            }
            rule.by_month_day.is_some() || rule.by_day.is_some() || date.day == anchor.day
        }
    }
}

fn candidate(
    clause: &Clause,
    date: &Civil,
    context: &ResolutionContext,
    generated_clock: bool,
) -> Result<Option<NumericOccurrence>, String> {
    let mut event = clause.clone();
    if generated_clock {
        event.time = Some(crate::types::TimeSpec {
            start: crate::types::ClockTime::Hm {
                hour: date.hour as i64,
                minute: date.minute as i64,
                second: Some(date.second as i64),
            },
            end: None,
            open: None,
        });
    }
    // The recurrence chooses the date; one-off weekday roll-forward must not
    // run again.
    let resolved = resolve_occurrence(
        &event,
        &LocalPeriod {
            start: *date,
            end: None,
        },
        context,
    );
    match resolved {
        Ok(occurrence) => Ok(Some(occurrence)),
        Err(message) if is_nonexistent(&message) => Ok(None),
        Err(message) => Err(message),
    }
}

fn is_nonexistent(message: &str) -> bool {
    message.starts_with("The requested local time does not exist")
}

pub fn expand_recurrence(
    clause: &Clause,
    context: &ResolutionContext,
    horizon: f64,
    limit: usize,
) -> Result<RecurrenceExpansion, String> {
    let week_start = context.options.week_start.unwrap_or(WeekStart::MO);
    let rule = normalize_recurrence(clause.recurrence.as_ref().unwrap(), week_start)?;
    if rule.count.is_some() && (rule.until.is_some() || rule.span.is_some()) {
        return Err("Use a count or an end bound, not both.".into());
    }
    if rule.interval < 1 {
        return Err("Recurrence interval must be a positive integer.".into());
    }
    if rule.count.is_some_and(|count| count < 1) {
        return Err("Recurrence count must be a positive integer.".into());
    }

    // Normalize these once for the whole series, rather than once per occurrence.
    let mut clause = clause.clone();
    clause.date = None;
    let context = ResolutionContext {
        reference: context.reference,
        options: context.options,
        clause_index: context.clause_index,
        recurring: true,
    };
    let options = context.options;
    let local_reference = civil(context.reference, &options.time_zone)?;
    let requested = match &rule.start {
        Some(start) => resolve_dates(Some(start), &local_reference, options)?[0].start,
        None => local_reference,
    };
    let sub_daily = sub_daily_step(&rule).is_some();
    let generated_clock = sub_daily || hours_for_rule(&rule).is_some();
    let clock_unit = if rule.freq == Frequency::minutely {
        Unit::minute
    } else {
        Unit::hour
    };
    // A step that divides the day and names no clock ("every 15 minutes",
    // "every 6 hours") sits on the wall-clock grid from midnight, the way
    // cron runs it, rather than counting from the reference instant: the
    // preview then shows the instants a scheduler will actually fire at,
    // and DST shifts move the step with the wall clock.
    if let Some(step) = sub_daily_step(&rule)
        && clause.time.is_none()
        && step > 0
        && 1440 % step == 0
    {
        let last = 1440 - step;
        clause.time = Some(crate::types::TimeSpec {
            start: crate::types::ClockTime::hm(0, 0),
            end: Some(crate::types::ClockTime::hm(last / 60, last % 60)),
            open: None,
        });
    }
    let window = window_times(&clause, &rule, options)?;
    if rule.freq == Frequency::daily && rule.times_per.is_some() && clause.time.is_some() {
        return Err(
            "A daily frequency count needs distinct times; it cannot share one fixed clock.".into(),
        );
    }
    let requested_instant = if rule.start.is_some() || (clause.time.is_none() && !generated_clock) {
        zoned_to_epoch(&requested.start_of_day(), &options.time_zone)?.epoch_ms
    } else {
        context.reference
    };
    let steps_per_day: i64 = if !generated_clock {
        1
    } else if clock_unit == Unit::minute {
        1440
    } else {
        24
    };
    let mut beginning = if sub_daily {
        requested
    } else {
        requested.start_of_day()
    };
    if generated_clock && let Some(time) = &clause.time {
        let seconds = resolve_time(time, options)?.0;
        beginning = Civil {
            hour: (seconds / 3600.0).floor() as i32,
            minute: ((seconds / 60.0).floor() % 60.0) as i32,
            second: (seconds % 60.0) as i32,
            ..beginning
        };
    }
    let exceptions = rule.except.clone().unwrap_or_default();
    let is_excluded = exclusion_filter(&exceptions, &context)?;
    let is_pattern_excluded = exclusion_filter(
        &exceptions
            .iter()
            .filter(|exception| excluded_weekdays(exception).is_some())
            .cloned()
            .collect::<Vec<_>>(),
        &context,
    )?;
    let mut until = f64::INFINITY;
    if let Some(rule_until) = &rule.until {
        let period = &resolve_dates(Some(rule_until), &local_reference, options)?[0];
        until = zoned_to_epoch(
            period.end.as_ref().unwrap_or(&add_days(&period.start, 1.0)),
            &options.time_zone,
        )?
        .epoch_ms;
    }
    if let Some(span) = &rule.span {
        if span.amount.fract() != 0.0 || span.amount <= 0.0 {
            return Err("A recurrence duration must be positive.".into());
        }
        until = until.min(add_duration(requested_instant, span, &options.time_zone)?);
    }
    if let Some(times) = &window {
        return expand_window(
            &clause,
            &rule,
            times,
            &context,
            WindowBounds {
                requested,
                requested_instant,
                until,
                horizon,
                limit,
            },
            &is_excluded,
            &is_pattern_excluded,
        );
    }
    let mut seed_rule = rule.clone();
    seed_rule.interval = 1;
    let mut first: Option<NumericOccurrence> = None;
    let mut anchor: Option<Civil> = None;
    let beginning_week = if rule.freq == Frequency::weekly {
        day_number(&week_beginning(&beginning, week_start))
    } else {
        0
    };

    for offset in 0..366 * 8 * steps_per_day {
        let date = add_civil(
            &beginning,
            offset as f64,
            if generated_clock {
                clock_unit
            } else {
                Unit::day
            },
        );
        if !matches(&date, &beginning, &seed_rule, beginning_week) || is_excluded.excludes(&date) {
            continue;
        }
        let occurrence = candidate(&clause, &date, &context, generated_clock)?;
        let Some(occurrence) =
            occurrence.filter(|occurrence| occurrence.start >= requested_instant)
        else {
            continue;
        };
        first = Some(occurrence);
        anchor = Some(date);
        break;
    }
    let (Some(anchor), Some(first)) = (anchor, first) else {
        return Err("No eligible recurrence start within eight years.".into());
    };

    if first.start >= until {
        return Ok(RecurrenceExpansion {
            occurrences: Vec::new(),
            first: Some(first),
            truncated: false,
            until: None,
            excluded: Vec::new(),
            date_excluded: Vec::new(),
            slots: None,
        });
    }
    let last_day = day_number(&civil(horizon.min(until), &options.time_zone)?);
    let mut occurrences: Vec<NumericOccurrence> = Vec::new();
    let mut excluded: Vec<NumericOccurrence> = Vec::new();
    let mut date_excluded: Vec<NumericOccurrence> = Vec::new();
    let cutoff = if first.all_day {
        zoned_to_epoch(&local_reference.start_of_day(), &options.time_zone)?.epoch_ms
    } else {
        context.reference
    };
    let mut count: i64 = 0;
    let anchor_week = if rule.freq == Frequency::weekly {
        day_number(&week_beginning(&anchor, week_start))
    } else {
        0
    };

    let scan_limit: i64 = 100_000 * steps_per_day;
    // With one weekday, the intervening days cannot satisfy the rule.
    let step: i64 = if sub_daily {
        // Every occurrence sits a whole number of intervals from the anchor.
        rule.interval
    } else if !generated_clock
        && rule.freq == Frequency::weekly
        && rule.by_day.as_ref().is_none_or(|days| days.len() == 1)
    {
        7 * rule.interval
    } else {
        1
    };
    let mut offset: i64 = 0;
    while offset < scan_limit {
        let date = add_civil(
            &anchor,
            offset as f64,
            if generated_clock {
                clock_unit
            } else {
                Unit::day
            },
        );
        if day_number(&date) > last_day {
            break;
        }
        if !matches(&date, &anchor, &rule, anchor_week) {
            offset += step;
            continue;
        }

        let occurrence = if offset == 0 {
            Some(first.clone())
        } else {
            candidate(&clause, &date, &context, generated_clock)?
        };
        let Some(occurrence) = occurrence else {
            offset += step;
            continue;
        };
        let start = occurrence.start;
        if start >= horizon.min(until) || rule.count.is_some_and(|rule_count| count >= rule_count) {
            break;
        }
        if is_excluded.excludes(&date) {
            excluded.push(occurrence);
            if !is_pattern_excluded.excludes(&date) {
                date_excluded.push(excluded.last().unwrap().clone());
            }
            offset += step;
            continue;
        }
        count += 1;
        if start >= cutoff {
            occurrences.push(occurrence);
        }
        if occurrences.len() > limit {
            return Ok(RecurrenceExpansion {
                occurrences,
                first: Some(first),
                until: if until.is_finite() { Some(until) } else { None },
                excluded,
                date_excluded,
                truncated: true,
                slots: None,
            });
        }
        if rule.count.is_some_and(|rule_count| count >= rule_count) {
            break;
        }
        offset += step;
    }

    if offset >= scan_limit
        && day_number(&add_civil(
            &anchor,
            offset as f64,
            if generated_clock {
                clock_unit
            } else {
                Unit::day
            },
        )) <= last_day
    {
        return Err(
            "The recurrence exceeded the 100000-day search limit. Use a narrower horizon or a later start."
                .into(),
        );
    }

    Ok(RecurrenceExpansion {
        occurrences,
        first: Some(first),
        until: if until.is_finite() { Some(until) } else { None },
        excluded,
        date_excluded,
        truncated: false,
        slots: None,
    })
}

struct WindowBounds {
    requested: Civil,
    requested_instant: f64,
    until: f64,
    horizon: f64,
    limit: usize,
}

/// Expands a sub-daily rule confined to a daily clock window. The window
/// restarts on every eligible day, so each day fires at the same clock
/// times whatever the step.
fn expand_window(
    clause: &Clause,
    rule: &Recurrence,
    times: &[f64],
    context: &ResolutionContext,
    bounds: WindowBounds,
    is_excluded: &crate::exclusions::ExclusionFilter,
    is_pattern_excluded: &crate::exclusions::ExclusionFilter,
) -> Result<RecurrenceExpansion, String> {
    let options = context.options;
    let WindowBounds {
        requested,
        requested_instant,
        until,
        horizon,
        limit,
    } = bounds;
    // The step is the sub-daily interval; days are selected by the BY* parts.
    let mut day_rule = rule.clone();
    day_rule.freq = Frequency::daily;
    day_rule.interval = 1;
    let overnight = times.iter().any(|value| *value >= 86_400.0);
    let first_day = add_days(
        &requested.start_of_day(),
        if overnight { -1.0 } else { 0.0 },
    );
    let instant = |day: &Civil, seconds: f64| -> Result<Option<NumericOccurrence>, String> {
        let start = match at_time(day, seconds, &options.time_zone) {
            Ok(start) => start,
            // A clock time inside a DST gap does not occur that day.
            Err(_) => return Ok(None),
        };
        let end = match &clause.duration {
            Some(duration) => Some(add_duration(start, duration, &options.time_zone)?),
            None => None,
        };
        Ok(Some(NumericOccurrence {
            start,
            end,
            open: None,
            all_day: false,
            clause: context.clause_index,
        }))
    };

    let mut occurrences: Vec<NumericOccurrence> = Vec::new();
    let mut excluded: Vec<NumericOccurrence> = Vec::new();
    let mut date_excluded: Vec<NumericOccurrence> = Vec::new();
    let mut slots: Vec<Option<NumericOccurrence>> = vec![None; times.len()];
    let mut first: Option<NumericOccurrence> = None;
    let mut count: i64 = 0;
    let mut truncated = false;
    let mut finished = false;
    let end = horizon.min(until);

    for offset in 0..366 * 8 {
        let day = add_days(&first_day, offset as f64);
        if !matches(&day, &day, &day_rule, 0) {
            continue;
        }
        let skipped = is_excluded.excludes(&day);
        for (index, seconds) in times.iter().enumerate() {
            let Some(occurrence) = instant(&day, *seconds)? else {
                continue;
            };
            if occurrence.start < requested_instant {
                continue;
            }
            if skipped {
                if !finished && occurrence.start < end {
                    excluded.push(occurrence.clone());
                    if !is_pattern_excluded.excludes(&day) {
                        date_excluded.push(occurrence);
                    }
                }
                continue;
            }
            // Every slot is recorded, even past the rule's end, so exports
            // describe the whole window rather than the days the bound left.
            if slots[index].is_none() {
                slots[index] = Some(occurrence.clone());
            }
            if occurrence.start >= until {
                finished = true;
                continue;
            }
            if first.is_none() {
                first = Some(occurrence.clone());
            }
            if finished {
                continue;
            }
            if occurrence.start >= end || rule.count.is_some_and(|limit| count >= limit) {
                finished = true;
                continue;
            }
            count += 1;
            if occurrences.len() >= limit {
                truncated = true;
                finished = true;
                // One past the limit, as the caller's truncation check expects.
                occurrences.push(occurrence);
                continue;
            }
            occurrences.push(occurrence);
        }
        if finished && slots.iter().all(Option::is_some) {
            break;
        }
    }
    if first.is_none() {
        return Err("No eligible recurrence start within eight years.".into());
    }
    let slots = times
        .iter()
        .zip(slots)
        .filter_map(|(seconds, occurrence)| {
            occurrence.map(|occurrence| ((*seconds as i64).rem_euclid(86_400), occurrence))
        })
        .collect();
    Ok(RecurrenceExpansion {
        occurrences,
        first,
        truncated,
        until: if until.is_finite() { Some(until) } else { None },
        excluded,
        date_excluded,
        slots: Some(slots),
    })
}
