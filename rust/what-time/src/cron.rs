//! Exports recurrences as five-field cron expressions.
//!
//! The dialect is the common denominator: `minute hour day-of-month month
//! day-of-week`, weekday names instead of numbers (numbering differs
//! between cron implementations), no seconds, no `L`/`#`/`W` extensions.
//! A rule cron cannot state exactly is refused with a reason rather than
//! approximated; the RRULE stays the complete export.
//!
//! Cron fires on the wall clock of the zone it runs in, so expressions are
//! in the caller's time zone, and steps that the phrase left unanchored
//! ("every 15 minutes") sit on clock boundaries.

use crate::rrule::ExportPlan;
use crate::types::{Frequency, WEEKDAYS};
use crate::zoned::day_of_week;

/// One cron line as value sets. An empty day, month or weekday set is `*`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CronSpec {
    minutes: Vec<u32>,
    hours: Vec<u32>,
    days: Vec<u32>,
    months: Vec<u32>,
    /// Indexes into `WEEKDAYS` (Monday first).
    weekdays: Vec<u32>,
}

fn sorted(mut values: Vec<u32>) -> Vec<u32> {
    values.sort_unstable();
    values.dedup();
    values
}

/// Groups minutes of the day into `(minutes, hours)` sets whose product is
/// exactly the given times.
fn time_groups(times: &[u32]) -> Vec<(Vec<u32>, Vec<u32>)> {
    let mut by_hour: Vec<(u32, Vec<u32>)> = Vec::new();
    for time in times {
        let (hour, minute) = (time / 60, time % 60);
        match by_hour.iter_mut().find(|entry| entry.0 == hour) {
            Some(entry) => entry.1.push(minute),
            None => by_hour.push((hour, vec![minute])),
        }
    }
    let mut groups: Vec<(Vec<u32>, Vec<u32>)> = Vec::new();
    for (hour, minutes) in by_hour {
        let minutes = sorted(minutes);
        match groups.iter_mut().find(|group| group.0 == minutes) {
            Some(group) => group.1.push(hour),
            None => groups.push((minutes, vec![hour])),
        }
    }
    groups
        .into_iter()
        .map(|(minutes, hours)| (minutes, sorted(hours)))
        .collect()
}

/// The cron lines for one exported rule, or why cron cannot state it.
pub fn cron_specs(plan: &ExportPlan) -> Result<Vec<CronSpec>, String> {
    if !plan.by_set_pos.is_empty() {
        return Err(
            "Cron has no field for the nth weekday of a month; use the RRULE for this rule.".into(),
        );
    }
    if plan.by_month_day.iter().any(|day| *day < 1) {
        return Err(
            "Cron has no field for the last day of a month; use the RRULE for this rule.".into(),
        );
    }
    let mut days: Vec<u32> = plan.by_month_day.iter().map(|day| *day as u32).collect();
    let mut months: Vec<u32> = plan.by_month.iter().map(|month| *month as u32).collect();
    let mut weekdays: Vec<u32> = plan
        .by_day
        .iter()
        .flatten()
        .filter_map(|day| WEEKDAYS.iter().position(|candidate| candidate == day))
        .map(|index| index as u32)
        .collect();

    match plan.freq {
        Frequency::minutely | Frequency::hourly => {}
        Frequency::daily => {
            if plan.interval != 1 {
                return Err(format!(
                    "Cron cannot count every {} days across month ends; use the RRULE for this rule.",
                    plan.interval
                ));
            }
        }
        Frequency::weekly => {
            if plan.interval != 1 {
                return Err(format!(
                    "Cron cannot skip weeks, so every {} weeks needs the RRULE.",
                    plan.interval
                ));
            }
            if weekdays.is_empty() {
                weekdays.push(day_of_week(&plan.anchor) as u32);
            }
        }
        Frequency::monthly | Frequency::quarterly => {
            let step = plan.interval
                * if plan.freq == Frequency::quarterly {
                    3
                } else {
                    1
                };
            if step != 1 {
                if 12 % step != 0 || !months.is_empty() {
                    return Err(format!(
                        "Every {step} months does not repeat within a calendar year, so cron cannot state it; use the RRULE."
                    ));
                }
                let first = (plan.anchor.month as i64 - 1) % step;
                months = (0..12 / step)
                    .map(|index| (first + index * step + 1) as u32)
                    .collect();
            }
            if days.is_empty() && weekdays.is_empty() {
                days.push(plan.anchor.day as u32);
            }
        }
        Frequency::yearly => {
            if plan.interval != 1 {
                return Err(format!(
                    "Cron has no year field, so every {} years needs the RRULE.",
                    plan.interval
                ));
            }
            if months.is_empty() {
                months.push(plan.anchor.month as u32);
            }
            if days.is_empty() && weekdays.is_empty() {
                days.push(plan.anchor.day as u32);
            }
        }
    }
    if !days.is_empty() && !weekdays.is_empty() {
        return Err(
            "Cron runs when either the day of the month or the weekday matches, so it cannot require both; use the RRULE."
                .into(),
        );
    }

    let no_seconds = "Cron has no seconds field; use the RRULE for this rule.";
    let sub_daily = match plan.freq {
        Frequency::minutely => Some(plan.interval),
        Frequency::hourly => Some(plan.interval * 60),
        _ => None,
    };
    let groups: Vec<(Vec<u32>, Vec<u32>)> = if let Some(slots) = &plan.slots {
        if slots.iter().any(|seconds| seconds % 60 != 0) {
            return Err(no_seconds.into());
        }
        let times: Vec<u32> = slots.iter().map(|seconds| (seconds / 60) as u32).collect();
        time_groups(&times)
    } else if let Some(step) = sub_daily {
        if step < 1 || 1440 % step != 0 {
            return Err(format!(
                "Every {step} minutes does not divide a day evenly, so cron cannot keep the rhythm past midnight; use the RRULE."
            ));
        }
        // A step the phrase did not anchor to a clock sits on clock
        // boundaries, which is what a cron reader expects of "*/15".
        let phase = if plan.clocked {
            if plan.anchor.second != 0 {
                return Err(no_seconds.into());
            }
            (plan.anchor.hour as i64 * 60 + plan.anchor.minute as i64) % step
        } else {
            0
        };
        let times: Vec<u32> = (0..1440 / step)
            .map(|index| (phase + index * step) as u32)
            .collect();
        time_groups(&times)
    } else if let Some(hours) = &plan.hours {
        vec![(
            vec![plan.anchor.minute as u32],
            sorted(hours.iter().map(|hour| *hour as u32).collect()),
        )]
    } else if plan.all_day {
        vec![(vec![0], vec![0])]
    } else {
        if plan.anchor.second != 0 {
            return Err(no_seconds.into());
        }
        vec![(
            vec![plan.anchor.minute as u32],
            vec![plan.anchor.hour as u32],
        )]
    };

    Ok(groups
        .into_iter()
        .map(|(minutes, hours)| CronSpec {
            minutes,
            hours,
            days: sorted(days.clone()),
            months: sorted(months.clone()),
            weekdays: sorted(weekdays.clone()),
        })
        .collect())
}

/// Folds lines that differ in exactly one field into one line: "at 8am,
/// 1pm and 8pm" is one `0 8,13,20 * * *`, not three lines.
pub fn merge(specs: Vec<CronSpec>) -> Vec<CronSpec> {
    let mut merged: Vec<CronSpec> = Vec::new();
    for spec in specs {
        let mut current = spec;
        loop {
            let partner = merged
                .iter()
                .position(|other| differing_field(other, &current).is_some());
            let Some(index) = partner else {
                break;
            };
            let other = merged.remove(index);
            let field = differing_field(&other, &current).unwrap();
            current = union(&other, &current, field);
        }
        if !merged.contains(&current) {
            merged.push(current);
        }
    }
    merged
}

/// The index of the single field two lines differ in, when the union of
/// that field is still exact. Day and weekday sets only merge between two
/// restricted sets: an empty set already means every day.
fn differing_field(left: &CronSpec, right: &CronSpec) -> Option<usize> {
    let pairs = [
        (&left.minutes, &right.minutes),
        (&left.hours, &right.hours),
        (&left.days, &right.days),
        (&left.months, &right.months),
        (&left.weekdays, &right.weekdays),
    ];
    let mut differing = pairs
        .iter()
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(index, _)| index);
    let field = differing.next()?;
    if differing.next().is_some() {
        return None;
    }
    let (a, b) = pairs[field];
    if field >= 2 && (a.is_empty() || b.is_empty()) {
        return None;
    }
    Some(field)
}

fn union(left: &CronSpec, right: &CronSpec, field: usize) -> CronSpec {
    let join = |a: &Vec<u32>, b: &Vec<u32>| sorted(a.iter().chain(b).copied().collect());
    let mut result = left.clone();
    match field {
        0 => result.minutes = join(&left.minutes, &right.minutes),
        1 => result.hours = join(&left.hours, &right.hours),
        2 => result.days = join(&left.days, &right.days),
        3 => result.months = join(&left.months, &right.months),
        _ => result.weekdays = join(&left.weekdays, &right.weekdays),
    }
    result
}

/// Renders a numeric field as `*`, a step, ranges, or a list.
fn field(values: &[u32], minimum: u32, maximum: u32) -> String {
    if values.is_empty() || values.len() as u32 == maximum - minimum + 1 {
        return "*".into();
    }
    if values.len() >= 2 {
        let step = values[1] - values[0];
        let even = step >= 2 && values.windows(2).all(|pair| pair[1] - pair[0] == step);
        let last = values[values.len() - 1];
        if even && values[0] == minimum && last + step > maximum {
            return format!("*/{step}");
        }
        if even && values.len() >= 3 {
            return format!("{}-{last}/{step}", values[0]);
        }
    }
    runs(values)
        .into_iter()
        .map(|(start, end)| {
            if end - start >= 2 {
                format!("{start}-{end}")
            } else if end > start {
                format!("{start},{end}")
            } else {
                start.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// Maximal runs of consecutive values.
fn runs(values: &[u32]) -> Vec<(u32, u32)> {
    let mut result: Vec<(u32, u32)> = Vec::new();
    for value in values {
        match result.last_mut() {
            Some(run) if run.1 + 1 == *value => run.1 = *value,
            _ => result.push((*value, *value)),
        }
    }
    result
}

const NAMES: [&str; 7] = ["MON", "TUE", "WED", "THU", "FRI", "SAT", "SUN"];

/// Weekday names, Monday first. Ranges stay inside Monday–Saturday so the
/// line reads the same whether an implementation numbers Sunday 0 or 7.
fn weekday_field(values: &[u32]) -> String {
    if values.is_empty() || values.len() == 7 {
        return "*".into();
    }
    let weekdays: Vec<u32> = values.iter().copied().filter(|value| *value < 6).collect();
    let mut parts: Vec<String> = runs(&weekdays)
        .into_iter()
        .map(|(start, end)| {
            if end - start >= 2 {
                format!("{}-{}", NAMES[start as usize], NAMES[end as usize])
            } else if end > start {
                format!("{},{}", NAMES[start as usize], NAMES[end as usize])
            } else {
                NAMES[start as usize].to_string()
            }
        })
        .collect();
    if values.contains(&6) {
        parts.push("SUN".into());
    }
    parts.join(",")
}

pub fn render(spec: &CronSpec) -> String {
    format!(
        "{} {} {} {} {}",
        field(&spec.minutes, 0, 59),
        field(&spec.hours, 0, 23),
        field(&spec.days, 1, 31),
        field(&spec.months, 1, 12),
        weekday_field(&spec.weekdays)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(minutes: &[u32], hours: &[u32], weekdays: &[u32]) -> CronSpec {
        CronSpec {
            minutes: minutes.to_vec(),
            hours: hours.to_vec(),
            days: Vec::new(),
            months: Vec::new(),
            weekdays: weekdays.to_vec(),
        }
    }

    #[test]
    fn renders_steps_ranges_and_lists() {
        assert_eq!(field(&[0, 15, 30, 45], 0, 59), "*/15");
        assert_eq!(field(&[0, 30], 0, 59), "*/30");
        assert_eq!(field(&[9, 11, 13, 15, 17], 0, 23), "9-17/2");
        assert_eq!(field(&[9, 10, 11, 12], 0, 23), "9-12");
        assert_eq!(field(&[8, 13, 20], 0, 23), "8,13,20");
        assert_eq!(field(&[1, 15], 1, 31), "1,15");
        assert_eq!(field(&(0..60).collect::<Vec<_>>(), 0, 59), "*");
        assert_eq!(field(&[0, 15, 30], 0, 59), "0-30/15");
    }

    #[test]
    fn names_weekdays_without_wrapping_through_sunday() {
        assert_eq!(weekday_field(&[0, 1, 2, 3, 4]), "MON-FRI");
        assert_eq!(weekday_field(&[5, 6]), "SAT,SUN");
        assert_eq!(weekday_field(&[0, 2, 4]), "MON,WED,FRI");
        assert_eq!(weekday_field(&[0, 1, 2, 3, 4, 5]), "MON-SAT");
        assert_eq!(weekday_field(&[0, 1, 2, 3, 4, 5, 6]), "*");
    }

    #[test]
    fn merges_lines_that_differ_in_one_field() {
        let merged = merge(vec![
            spec(&[0], &[8], &[]),
            spec(&[0], &[13], &[]),
            spec(&[0], &[20], &[]),
        ]);
        assert_eq!(merged.len(), 1);
        assert_eq!(render(&merged[0]), "0 8,13,20 * * *");

        let weekdays = merge(vec![spec(&[0], &[9], &[0]), spec(&[0], &[9], &[4])]);
        assert_eq!(render(&weekdays[0]), "0 9 * * MON,FRI");

        // Different minute and hour: two real lines.
        let apart = merge(vec![spec(&[0], &[15], &[1]), spec(&[30], &[17], &[4])]);
        assert_eq!(apart.len(), 2);

        // Every day plus Mondays is not a weekday union.
        let mixed = merge(vec![spec(&[0], &[9], &[]), spec(&[0], &[9], &[0])]);
        assert_eq!(mixed.len(), 2);
    }

    #[test]
    fn groups_a_window_into_exact_lines() {
        // 9:00–18:00 every 15 minutes, both ends included.
        let times: Vec<u32> = (0..=36).map(|index| 540 + index * 15).collect();
        let groups = time_groups(&times);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0], (vec![0, 15, 30, 45], (9..=17).collect()));
        assert_eq!(groups[1], (vec![0], vec![18]));
    }
}
