//! Clock-time resolution: named times, day parts, and seconds-of-day.

use crate::types::{ClockTime, DayPart, NamedClock, ResolveOptions, TimeSpec};

fn default_parts(part: DayPart) -> [&'static str; 2] {
    match part {
        DayPart::morning => ["06:00", "12:00"],
        DayPart::afternoon => ["12:00", "17:00"],
        DayPart::evening => ["17:00", "21:00"],
        DayPart::night => ["21:00", "24:00"],
    }
}

fn configured_clock(value: &str) -> Result<ClockTime, String> {
    let invalid = || "A day-part boundary must use HH:MM or HH:MM:SS.".to_string();
    let mut parts = value.split(':');
    let hour = parts.next().ok_or_else(invalid)?;
    let minute = parts.next().ok_or_else(invalid)?;
    let second = parts.next();
    if parts.next().is_some() {
        return Err(invalid());
    }
    let digits = |text: &str| text.len() == 2 && text.bytes().all(|b| b.is_ascii_digit());
    if !digits(hour) || !digits(minute) || second.is_some_and(|value| !digits(value)) {
        return Err(invalid());
    }
    Ok(ClockTime::Hm {
        hour: hour.parse().map_err(|_| invalid())?,
        minute: minute.parse().map_err(|_| invalid())?,
        second: match second {
            Some(value) => Some(value.parse().map_err(|_| invalid())?),
            None => None,
        },
    })
}

pub fn clock_seconds(
    clock: &ClockTime,
    options: &ResolveOptions,
    end: bool,
) -> Result<f64, String> {
    match clock {
        ClockTime::Named { named } => Ok(if *named == NamedClock::noon {
            43_200.0
        } else {
            0.0
        }),
        ClockTime::Part { part } => {
            let window = options
                .day_parts
                .as_ref()
                .and_then(|parts| parts.get(part))
                .map(|pair| [pair.0.as_str(), pair.1.as_str()])
                .unwrap_or(default_parts(*part));
            clock_seconds(
                &configured_clock(window[if end { 1 } else { 0 }])?,
                options,
                end,
            )
        }
        ClockTime::Hm {
            hour,
            minute,
            second,
        } => {
            let second = second.unwrap_or(0);
            let valid_hour = (*hour as f64).fract() == 0.0
                && *hour >= 0
                && (*hour < 24 || (end && *hour == 24 && *minute == 0 && second == 0));
            let valid_minute = (*minute as f64).fract() == 0.0 && *minute >= 0 && *minute < 60;
            let valid_second = (second as f64).fract() == 0.0 && (0..60).contains(&second);
            if !valid_hour || !valid_minute || !valid_second {
                return Err("Clock components are out of range.".into());
            }
            Ok((*hour * 3600 + *minute * 60 + second) as f64)
        }
    }
}

pub fn resolve_time(
    time: &TimeSpec,
    options: &ResolveOptions,
) -> Result<(f64, Option<f64>), String> {
    let start = clock_seconds(&time.start, options, false)?;
    let mut end: Option<f64> = None;
    if let Some(end_clock) = &time.end {
        end = Some(clock_seconds(end_clock, options, true)?);
    } else if time.open != Some(crate::types::OpenBound::end)
        && let ClockTime::Part { .. } = time.start
    {
        end = Some(clock_seconds(&time.start, options, true)?);
    }

    // Ordering is validated after the endpoint dates and timezone are resolved.
    Ok((start, end))
}
