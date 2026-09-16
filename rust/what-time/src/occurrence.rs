//! Turns clauses plus calendar periods into concrete numeric occurrences.

use crate::calendar::{LocalPeriod, resolve_dates};
use crate::clock::resolve_time;
use crate::types::{Clause, Direction, Duration, OpenBound, ResolveOptions, Shift};
use crate::zoned::{Civil, ZonedKind, add_days, add_months, civil, zoned_to_epoch};

#[derive(Clone, PartialEq, Debug)]
pub struct NumericOccurrence {
    pub start: f64,
    pub end: Option<f64>,
    pub open: Option<OpenBound>,
    pub all_day: bool,
    pub clause: usize,
}

/// Public timestamps have second precision. Keep the same precision internally.
fn seconds(epoch: f64) -> f64 {
    (epoch / 1000.0).floor() * 1000.0
}

#[derive(Clone)]
pub struct ResolutionContext<'a> {
    pub reference: f64,
    pub options: &'a ResolveOptions,
    pub clause_index: usize,
    pub recurring: bool,
}

#[derive(Debug)]
pub struct NonexistentTimeError(pub String);

impl std::fmt::Display for NonexistentTimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

pub fn add_duration(epoch: f64, duration: &Duration, time_zone: &str) -> Result<f64, String> {
    if let Some(components) = &duration.components {
        let mut result = add_duration(
            epoch,
            &Duration {
                components: None,
                amount: duration.amount,
                unit: duration.unit,
            },
            time_zone,
        )?;
        for component in components {
            result = add_duration(
                result,
                &Duration {
                    components: None,
                    amount: component.amount,
                    unit: component.unit,
                },
                time_zone,
            )?;
        }
        return Ok(result);
    }
    let Duration { amount, unit, .. } = duration;
    match unit {
        crate::types::Unit::minute => Ok(epoch + amount * 60_000.0),
        crate::types::Unit::hour => Ok(epoch + amount * 3_600_000.0),
        _ => {
            let local = civil(epoch, time_zone)?;
            let is_month_unit = matches!(
                *unit,
                crate::types::Unit::month | crate::types::Unit::quarter | crate::types::Unit::year
            );
            let shifted = if is_month_unit {
                add_months(
                    &local,
                    amount
                        * match *unit {
                            crate::types::Unit::year => 12.0,
                            crate::types::Unit::quarter => 3.0,
                            _ => 1.0,
                        },
                )
            } else {
                add_days(
                    &local,
                    amount
                        * if *unit == crate::types::Unit::week {
                            7.0
                        } else {
                            1.0
                        },
                )
            };

            Ok(zoned_to_epoch(&shifted, time_zone)?.epoch_ms)
        }
    }
}

fn apply_shift(epoch: f64, shift: Option<&Shift>, time_zone: &str) -> Result<f64, String> {
    let Some(shift) = shift else {
        return Ok(epoch);
    };
    let sign = if shift.direction == Direction::before {
        -1.0
    } else {
        1.0
    };
    let duration = Duration {
        components: shift.components.as_ref().map(|components| {
            components
                .iter()
                .map(|quantity| crate::types::Quantity {
                    amount: quantity.amount * sign,
                    unit: quantity.unit,
                })
                .collect()
        }),
        amount: shift.amount * sign,
        unit: shift.unit,
    };
    add_duration(epoch, &duration, time_zone)
}

fn at_time(date: &Civil, seconds: f64, time_zone: &str) -> Result<f64, NonexistentTimeError> {
    let day_offset = (seconds / 86_400.0).floor();
    let local = if seconds == seconds_of_day(date) {
        *date
    } else {
        let shifted = if day_offset != 0.0 {
            add_days(date, day_offset)
        } else {
            *date
        };
        Civil {
            hour: ((seconds / 3600.0).floor() % 24.0) as i32,
            minute: ((seconds / 60.0).floor() % 60.0) as i32,
            second: (seconds % 60.0) as i32,
            ..shifted
        }
    };

    match zoned_to_epoch(&local, time_zone) {
        Ok(result) if result.kind == ZonedKind::Gap => Err(NonexistentTimeError(format!(
            "The requested local time does not exist in {time_zone}."
        ))),
        Ok(result) => Ok(result.epoch_ms),
        Err(error) => Err(NonexistentTimeError(error)),
    }
}

fn seconds_of_day(date: &Civil) -> f64 {
    (date.hour * 3600 + date.minute * 60 + date.second) as f64
}

fn future_start(
    mut date: Civil,
    seconds: f64,
    step: f64,
    context: &ResolutionContext,
) -> Result<(Civil, f64), String> {
    for _ in 0..8 {
        let attempt = at_time(&date, seconds, &context.options.time_zone);
        match attempt {
            Ok(start) => {
                if step == 0.0 || start >= context.reference {
                    return Ok((date, start));
                }
            }
            Err(error) => {
                if step == 0.0 {
                    return Err(error.0);
                }
            }
        }
        date = add_days(&date, step);
    }
    Err("No eligible upcoming clock time was found.".into())
}

pub fn resolve_occurrence(
    clause: &Clause,
    period: &LocalPeriod,
    context: &ResolutionContext,
) -> Result<NumericOccurrence, String> {
    let options = context.options;
    let uses_reference_clock = clause.time.is_none()
        && ((clause.date.is_none() && (clause.shift.is_some() || clause.duration.is_some()))
            || matches!(clause.date, Some(crate::types::DateSpec::Now)));
    let implied_clock = matches!(
        clause.date,
        Some(crate::types::DateSpec::RelativeUnit {
            unit: crate::types::Unit::hour | crate::types::Unit::minute,
            ..
        })
    );
    let time = match &clause.time {
        Some(time) => Some(resolve_time(time, options)?),
        None => None,
    };
    let start_seconds = time
        .as_ref()
        .map(|(start, _)| *start)
        .unwrap_or_else(|| seconds_of_day(&period.start));
    let end_seconds = if clause.duration.is_some()
        && clause.time.as_ref().and_then(|t| t.end.as_ref()).is_none()
    {
        None
    } else {
        time.as_ref().and_then(|(_, end)| *end)
    };

    let is_timed_weekday = clause.time.is_some()
        && matches!(
            clause.date,
            Some(crate::types::DateSpec::Weekday { .. })
                | Some(crate::types::DateSpec::DayGroup { .. })
        )
        && !matches!(
            clause.date,
            Some(crate::types::DateSpec::Weekday {
                modifier: Some(_),
                ..
            }) | Some(crate::types::DateSpec::DayGroup {
                modifier: Some(_),
                ..
            })
        )
        && options
            .bare_weekday
            .unwrap_or(crate::types::BareWeekday::future)
            == crate::types::BareWeekday::future;

    let standalone_clock = clause.time.is_some() && clause.date.is_none() && clause.shift.is_none();
    let step = if context.recurring {
        0.0
    } else if is_timed_weekday {
        7.0
    } else if standalone_clock {
        1.0
    } else {
        0.0
    };
    // "before 6pm" floors the start at midnight, which is always in the past,
    // so search on the edge the phrase actually named or every one lands
    // tomorrow.
    let search_seconds = if clause.time.as_ref().and_then(|t| t.open) == Some(OpenBound::start) {
        end_seconds.unwrap_or(start_seconds)
    } else {
        start_seconds
    };
    let (date, searched) = if uses_reference_clock {
        (period.start, context.reference)
    } else {
        future_start(period.start, search_seconds, step, context)?
    };
    let start = if search_seconds == start_seconds {
        searched
    } else {
        at_time(&date, start_seconds, &options.time_zone).map_err(|error| error.0)?
    };

    let mut occurrence = NumericOccurrence {
        start: seconds(apply_shift(
            start,
            clause.shift.as_ref(),
            &options.time_zone,
        )?),
        end: None,
        open: clause.time.as_ref().and_then(|t| t.open),
        all_day: clause.time.is_none() && !uses_reference_clock && !implied_clock,
        clause: context.clause_index,
    };

    if let Some(end_seconds) = end_seconds {
        let crosses_midnight = end_seconds < start_seconds;
        let end_date = if let Some(end_date_spec) = &clause.end_date {
            resolve_dates(Some(end_date_spec), &date, options)?[0].start
        } else if crosses_midnight {
            add_days(&date, 1.0)
        } else {
            date
        };
        let end = at_time(&end_date, end_seconds, &options.time_zone).map_err(|e| e.0)?;
        occurrence.end = Some(seconds(apply_shift(
            end,
            clause.shift.as_ref(),
            &options.time_zone,
        )?));
    }

    if end_seconds.is_none()
        && period.end.is_some()
        && clause.time.as_ref().and_then(|t| t.open) != Some(OpenBound::end)
    {
        let period_end = period.end.as_ref().unwrap();
        let end =
            at_time(period_end, seconds_of_day(period_end), &options.time_zone).map_err(|e| e.0)?;
        occurrence.end = Some(seconds(apply_shift(
            end,
            clause.shift.as_ref(),
            &options.time_zone,
        )?));
    }

    if let Some(end_amount) = clause.shift.as_ref().and_then(|shift| shift.end_amount) {
        let shift = clause.shift.as_ref().unwrap();
        let mut end_shift = shift.clone();
        end_shift.amount = end_amount;
        let shifted_end = apply_shift(start, Some(&end_shift), &options.time_zone)?;
        let shifted_start = occurrence.start;
        occurrence.start = seconds(shifted_start.min(shifted_end));
        occurrence.end = Some(seconds(shifted_start.max(shifted_end)));
    }

    if let Some(duration) = &clause.duration {
        if occurrence.end.is_some() {
            return Err("Use a duration or an explicit end, not both.".into());
        }
        let end = add_duration(occurrence.start, duration, &options.time_zone)?;
        occurrence.end = Some(seconds(end));
    }

    if occurrence.end.is_some_and(|end| end <= occurrence.start) {
        return Err("The resolved end must be after the start.".into());
    }
    Ok(occurrence)
}
