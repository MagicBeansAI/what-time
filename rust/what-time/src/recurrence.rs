//! Expands recurrence rules into bounded occurrence previews.

use crate::calendar::{LocalPeriod, add_civil, resolve_dates, week_beginning};
use crate::clock::resolve_time;
use crate::exclusions::{excluded_weekdays, exclusion_filter};
use crate::occurrence::{NumericOccurrence, ResolutionContext, add_duration, resolve_occurrence};
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
    let maximum = match rule.freq {
        Frequency::weekly => 7,
        Frequency::daily => 24,
        _ => 0,
    };
    if !(1..=maximum).contains(&times_per) {
        return Err("Frequency counts need 1–7 times per week or 1–24 times per day.".into());
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
        if let Some(time) = &clause.time {
            let (start, end) = resolve_time(time, context.options)?;
            if let Some(end) = end {
                if end == start {
                    return Err(
                        "An hourly recurrence window needs distinct clock boundaries.".into(),
                    );
                }
                let length = if end > start {
                    end - start
                } else {
                    end - start + 86_400.0
                };
                if length == 86_400.0 {
                    event.duration = Some(crate::types::Duration {
                        components: None,
                        amount: 1.0,
                        unit: Unit::day,
                    });
                } else {
                    let end_civil = add_civil(date, length / 60.0, Unit::minute);
                    event.time = Some(crate::types::TimeSpec {
                        start: crate::types::ClockTime::Hm {
                            hour: date.hour as i64,
                            minute: date.minute as i64,
                            second: Some(date.second as i64),
                        },
                        end: Some(crate::types::ClockTime::Hm {
                            hour: end_civil.hour as i64,
                            minute: end_civil.minute as i64,
                            second: Some(end_civil.second as i64),
                        }),
                        open: None,
                    });
                }
            }
        }
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
    let generated_clock = rule.freq == Frequency::hourly || hours_for_rule(&rule).is_some();
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
    let steps_per_day: i64 = if generated_clock { 24 } else { 1 };
    let mut beginning = if rule.freq == Frequency::hourly {
        requested
    } else {
        requested.start_of_day()
    };
    if generated_clock && clause.time.is_some() {
        let seconds = resolve_time(clause.time.as_ref().unwrap(), options)?.0;
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
                Unit::hour
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
    let step: i64 = if !generated_clock
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
                Unit::hour
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
                Unit::hour
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
    })
}
