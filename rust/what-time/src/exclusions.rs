//! Exception filters for recurrence expansion.

use crate::calendar::resolve_dates;
use crate::occurrence::ResolutionContext;
use crate::types::{DateSpec, WEEKDAYS, Weekday, weekday_index};
use crate::zoned::{Civil, add_days, civil, day_number, day_of_week};

pub fn excluded_weekdays(spec: &DateSpec) -> Option<Vec<Weekday>> {
    match spec {
        DateSpec::Weekday {
            modifier: None,
            days,
        } => Some(days.clone()),
        DateSpec::DayGroup {
            modifier: None,
            group,
        } => Some(if *group == crate::types::DayGroup::weekend {
            WEEKDAYS[5..].to_vec()
        } else {
            WEEKDAYS[..5].to_vec()
        }),
        DateSpec::WeekdayRange { from, to } => {
            let start = weekday_index(*from);
            let length = (weekday_index(*to) as i32 - start as i32 + 7) as usize % 7 + 1;
            Some(
                (0..length)
                    .map(|index| WEEKDAYS[(start + index) % 7])
                    .collect(),
            )
        }
        _ => None,
    }
}

pub struct ExclusionFilter {
    patterns: Vec<Weekday>,
    periods: Vec<(i64, i64)>,
    monthly: Vec<MonthlyPattern>,
}

struct MonthlyPattern {
    ordinal: i64,
    day: Weekday,
}

impl ExclusionFilter {
    pub fn excludes(&self, date: &Civil) -> bool {
        if self.patterns.contains(&WEEKDAYS[day_of_week(date)]) {
            return true;
        }
        if self.monthly.iter().any(|pattern| {
            if WEEKDAYS[day_of_week(date)] != pattern.day {
                return false;
            }
            let ordinal = ((date.day - 1) / 7 + 1) as i64;
            if pattern.ordinal > 0 {
                return ordinal == pattern.ordinal;
            }
            add_days(date, (7 * -pattern.ordinal) as f64).month != date.month
                && (pattern.ordinal == -1
                    || add_days(date, (7 * (-pattern.ordinal - 1)) as f64).month == date.month)
        }) {
            return true;
        }
        let day = day_number(date);
        self.periods
            .iter()
            .any(|period| day >= period.0 && day < period.1)
    }
}

pub fn exclusion_filter(
    exceptions: &[DateSpec],
    context: &ResolutionContext,
) -> Result<ExclusionFilter, String> {
    let mut filter = ExclusionFilter {
        patterns: Vec::new(),
        periods: Vec::new(),
        monthly: Vec::new(),
    };
    if exceptions.is_empty() {
        return Ok(filter);
    }
    let reference = civil(context.reference, &context.options.time_zone)?;
    for exception in exceptions {
        if let DateSpec::OrdinalWeekday {
            ordinal,
            day,
            recurring: Some(true),
            ..
        } = exception
        {
            filter.monthly.push(MonthlyPattern {
                ordinal: *ordinal,
                day: *day,
            });
            continue;
        }
        if let Some(days) = excluded_weekdays(exception) {
            for day in days {
                if !filter.patterns.contains(&day) {
                    filter.patterns.push(day);
                }
            }
            continue;
        }

        for period in resolve_dates(Some(exception), &reference, context.options)? {
            filter.periods.push((
                day_number(&period.start),
                day_number(period.end.as_ref().unwrap_or(&add_days(&period.start, 1.0))),
            ));
        }
    }
    Ok(filter)
}
