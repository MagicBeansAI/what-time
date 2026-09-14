//! Turns schedules into instants, recurrence previews, and RRULE exports.

use crate::calendar::resolve_dates;
use crate::occurrence::{NumericOccurrence, ResolutionContext, resolve_occurrence};
use crate::recurrence::expand_recurrence;
use crate::rrule::{RecurrenceExportError, recurrence_rule};
use crate::types::{
    Clause, DateSpec, Diagnostic, Frequency, Occurrence, ResolveOptions, Resolved, Schedule,
    Severity,
};
use crate::zoned::{add_months, civil, instant, iso, zoned_to_epoch};

fn preview_horizon(
    options: &ResolveOptions,
    local_reference: &crate::zoned::Civil,
) -> Result<f64, String> {
    let Some(until_text) = &options.until else {
        return Ok(
            zoned_to_epoch(&add_months(local_reference, 12.0), &options.time_zone)?.epoch_ms,
        );
    };

    let parse_calendar = |text: &str| -> Option<(i64, i64, i64)> {
        let bytes = text.as_bytes();
        if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
            return None;
        }
        let digits =
            |range: std::ops::Range<usize>| bytes[range].iter().all(|b| b.is_ascii_digit());
        if !digits(0..4) || !digits(5..7) || !digits(8..10) {
            return None;
        }
        Some((
            text[0..4].parse().ok()?,
            text[5..7].parse().ok()?,
            text[8..10].parse().ok()?,
        ))
    };

    if let Some((year, month, day)) = parse_calendar(until_text) {
        let period = &resolve_dates(
            Some(&DateSpec::Calendar {
                year: Some(year),
                month: Some(month),
                day: Some(day),
            }),
            local_reference,
            options,
        )?[0];
        return Ok(zoned_to_epoch(&period.start, &options.time_zone)?.epoch_ms);
    }
    instant(until_text)
}

fn weekday_policy(clause: &Clause, options: &ResolveOptions) -> Clause {
    if options.bare_weekdays != Some(crate::types::BareWeekdaysPolicy::weekly)
        || clause.recurrence.is_some()
        || !matches!(clause.date, Some(DateSpec::Weekday { modifier: None, .. }))
    {
        return clause.clone();
    }
    let days = match &clause.date {
        Some(DateSpec::Weekday { days, .. }) => days.clone(),
        _ => unreachable!(),
    };
    Clause {
        date: None,
        recurrence: Some(crate::types::Recurrence {
            freq: Frequency::weekly,
            interval: 1,
            by_day: Some(days),
            by_month_day: None,
            by_set_pos: None,
            by_month: None,
            times_per: None,
            count: None,
            until: None,
            start: None,
            except: None,
            span: None,
        }),
        ..clause.clone()
    }
}

pub struct Prepared {
    pub reference: f64,
    pub local_reference: crate::zoned::Civil,
    pub limit: usize,
    pub horizon: f64,
}

pub fn prepare(options: &ResolveOptions) -> Result<Prepared, String> {
    let reference = instant(&options.reference)?;
    let local_reference = civil(reference, &options.time_zone)?;
    let limit = options.limit.unwrap_or(30);
    if !(1..=1000).contains(&limit) {
        return Err("limit must be an integer from 1 through 1000.".into());
    }
    let horizon = preview_horizon(options, &local_reference)?;
    if horizon <= reference {
        return Err("The preview horizon must be after the reference instant.".into());
    }
    Ok(Prepared {
        reference,
        local_reference,
        limit: limit as usize,
        horizon,
    })
}

/// Share calendar context across a batch, without caching any schedule results.
pub fn resolve_with(options: &ResolveOptions, schedule: &Schedule) -> Result<Resolved, String> {
    resolve_prepared(schedule, options, &prepare(options)?)
}

pub(crate) fn resolve_prepared(
    schedule: &Schedule,
    options: &ResolveOptions,
    prepared: &Prepared,
) -> Result<Resolved, String> {
    let Prepared {
        reference,
        local_reference,
        limit,
        horizon,
    } = prepared;
    let mut occurrences: Vec<NumericOccurrence> = Vec::new();
    let mut rrules: Vec<String> = Vec::new();
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut truncated = false;

    for (clause_index, original) in schedule.clauses.iter().enumerate() {
        let clause = weekday_policy(original, options);
        let context = ResolutionContext {
            reference: *reference,
            options,
            clause_index,
            recurring: false,
        };
        if let Some(rule) = &clause.recurrence {
            if matches!(
                rule.until.as_deref(),
                Some(DateSpec::Calendar { day: None, .. })
            ) {
                diagnostics.push(Diagnostic {
                    code: "until-month-only".into(),
                    severity: Severity::warning,
                    start: 0,
                    end: 0,
                    message: format!(
                        "Clause {}: a month-only end means the first day of that month, inclusive.",
                        clause_index + 1
                    ),
                });
            }
            if rule.times_per.is_some() && (rule.freq == Frequency::daily || rule.by_day.is_none())
            {
                diagnostics.push(Diagnostic {
                    code: "times-per-approximated".into(),
                    severity: Severity::warning,
                    start: 0,
                    end: 0,
                    message: format!(
                        "Clause {}: unspecified frequency times were spread across whole hours or weekdays.",
                        clause_index + 1
                    ),
                });
            }
            let series = expand_recurrence(&clause, &context, *horizon, *limit)?;
            if series.first.is_some() {
                match recurrence_rule(&clause, &series, &context) {
                    Ok(rrule) => rrules.push(rrule),
                    Err(RecurrenceExportError(message)) => {
                        diagnostics.push(Diagnostic {
                            code: "unsupported-export".into(),
                            severity: Severity::warning,
                            start: 0,
                            end: 0,
                            message,
                        });
                    }
                }
            }
            occurrences.extend(series.occurrences);
            truncated = truncated || series.truncated;
            continue;
        }
        let periods = resolve_dates(clause.date.as_ref(), local_reference, options)?;
        for period in &periods {
            let occurrence = resolve_occurrence(&clause, period, &context)?;
            if options.until.is_none() || occurrence.start < *horizon {
                occurrences.push(occurrence);
            }
        }
    }

    if schedule.clauses.len() != 1 || schedule.clauses[0].recurrence.is_none() {
        occurrences.sort_by(|left, right| {
            left.start
                .partial_cmp(&right.start)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }
    let mut resolved: Vec<Occurrence> = Vec::with_capacity(occurrences.len().min(*limit));
    for value in occurrences.iter().take(*limit) {
        resolved.push(Occurrence {
            start: iso(value.start, &options.time_zone)?,
            end: match value.end {
                Some(end) => Some(iso(end, &options.time_zone)?),
                None => None,
            },
            open: value.open,
            all_day: value.all_day,
            clause: value.clause,
        });
    }
    Ok(Resolved {
        occurrences: resolved,
        rrules,
        truncated: truncated || occurrences.len() > *limit,
        diagnostics,
    })
}
