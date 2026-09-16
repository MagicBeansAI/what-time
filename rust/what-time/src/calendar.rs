//! Resolves date specifications to local calendar periods.

use crate::types::{
    CalendarDate, DateSpec, DayGroup, Edge, Modifier, ResolveOptions, Unit, WEEKDAYS, WeekStart,
    Weekday, weekday_index,
};
use crate::zoned::{
    Civil, add_days, add_months, day_of_week, days_in_month, from_utc, is_valid, utc,
};

pub struct LocalPeriod {
    pub start: Civil,
    /// An exclusive end, used for date ranges and whole calendar periods.
    pub end: Option<Civil>,
}

fn holiday_date(key: &str, year: i32) -> Option<(i32, i32)> {
    match holiday_entry(key) {
        Some(HolidayEntry::Fixed { month, day }) => Some((month, day)),
        Some(HolidayEntry::NthWeekday {
            month,
            ordinal,
            weekday,
        }) => nth_weekday_of_month(year, month, ordinal, weekday),
        Some(HolidayEntry::EasterOffset { offset }) => {
            let (month, day) = western_easter(year);
            add_ordinal_days(month, day, offset)
        }
        Some(HolidayEntry::Tabulated { dates }) => {
            dates.get(&year.to_string()).map(|pair| (pair[0], pair[1]))
        }
        None => None,
    }
}

#[derive(Clone, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum HolidayEntry {
    Fixed {
        month: i32,
        day: i32,
    },
    NthWeekday {
        month: i32,
        ordinal: i32,
        weekday: Weekday,
    },
    EasterOffset {
        offset: i32,
    },
    Tabulated {
        dates: std::collections::BTreeMap<String, [i32; 2]>,
    },
}

#[derive(serde::Deserialize)]
struct HolidayAsset {
    entries: Vec<HolidayHoliday>,
}

#[derive(serde::Deserialize)]
struct HolidayHoliday {
    key: String,
    #[serde(flatten)]
    entry: HolidayEntry,
}

fn holiday_entry(key: &str) -> Option<HolidayEntry> {
    use std::sync::OnceLock;
    static ASSET: OnceLock<Vec<HolidayHoliday>> = OnceLock::new();
    let entries = ASSET.get_or_init(|| {
        serde_json::from_str::<HolidayAsset>(include_str!("../assets/holidays.json"))
            .expect("holidays asset must parse")
            .entries
    });
    entries
        .iter()
        .find(|entry| entry.key == key)
        .map(|entry| entry.entry.clone())
}

/// Western (Gregorian) Easter, the anonymous algorithm; exact for the
/// proleptic Gregorian calendar.
fn western_easter(year: i32) -> (i32, i32) {
    let a = year % 19;
    let b = year / 100;
    let c = year % 100;
    let d = b / 4;
    let e = b % 4;
    let f = (b + 8) / 25;
    let g = (b - f + 1) / 3;
    let h = (19 * a + b - d - g + 15) % 30;
    let i = c / 4;
    let k = c % 4;
    let l = (32 + 2 * e + 2 * i - h - k) % 7;
    let m = (a + 11 * h + 22 * l) / 451;
    let month = (h + l - 7 * m + 114) / 31;
    let day = (h + l - 7 * m + 114) % 31 + 1;
    (month, day)
}

fn add_ordinal_days(month: i32, day: i32, offset: i32) -> Option<(i32, i32)> {
    // Offsets from Easter stay inside the same year and near the anchor;
    // step through month lengths without inventing a date library.
    let mut result = (month, day);
    let mut remaining = offset;
    while remaining != 0 {
        let step = remaining.signum();
        let (mut m, mut d) = result;
        d += step;
        let length = month_length(2026, m); // lengths are year-independent here
        if d < 1 {
            m -= 1;
            d = month_length(2026, m.max(1));
        } else if d > length {
            m += 1;
            d = 1;
        }
        result = (m, d);
        remaining -= step;
    }
    Some(result)
}

fn month_length(_year: i32, month: i32) -> i32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => 28,
        _ => 30,
    }
}

fn nth_weekday_of_month(
    year: i32,
    month: i32,
    ordinal: i32,
    weekday: Weekday,
) -> Option<(i32, i32)> {
    let first = Civil {
        year,
        month,
        day: 1,
        hour: 0,
        minute: 0,
        second: 0,
    };
    let first_dow = weekday_index(weekday) as i32;
    let actual_dow = day_of_week(&first) as i32;
    let offset = (first_dow - actual_dow + 7) % 7;
    let length = month_length(year, month);
    if ordinal > 0 {
        let day = 1 + offset + (ordinal - 1) * 7;
        if day > length {
            return None;
        }
        Some((month, day))
    } else {
        // Negative ordinal counts from the end (Memorial Day: -1 MO of May).
        let last = Civil {
            year,
            month,
            day: length,
            hour: 0,
            minute: 0,
            second: 0,
        };
        let last_dow = day_of_week(&last) as i32;
        let back = (last_dow - first_dow + 7) % 7;
        let day = length - back - ((-ordinal - 1) * 7);
        if day < 1 {
            return None;
        }
        Some((month, day))
    }
}

fn su_week(options: &ResolveOptions) -> bool {
    options.week_start == Some(WeekStart::SU)
}

pub fn week_beginning(date: &Civil, week_start: WeekStart) -> Civil {
    let first_day = if week_start == WeekStart::SU { 6 } else { 0 };
    add_days(
        &date.start_of_day(),
        -(((day_of_week(date) as i32 - first_day + 7) % 7) as f64),
    )
}

fn weekday_date(
    day: crate::types::Weekday,
    modifier: Option<Modifier>,
    reference: &Civil,
    options: &ResolveOptions,
) -> Civil {
    use crate::types::{BareWeekday, NextWeekday};
    let target = weekday_index(day);
    let current = day_of_week(reference);
    let future = (target as i32 - current as i32 + 7) % 7;
    let week_start = if su_week(options) { 6 } else { 0 };
    let position = (target as i32 - week_start + 7) % 7;
    let start = options.week_start.unwrap_or(WeekStart::MO);

    if modifier == Some(Modifier::this)
        || modifier.is_none() && options.bare_weekday == Some(BareWeekday::thisWeek)
    {
        return add_days(&week_beginning(reference, start), position as f64);
    }
    if modifier == Some(Modifier::last) {
        let back = if future_back(current, target) == 0 {
            7
        } else {
            future_back(current, target)
        };
        return add_days(&reference.start_of_day(), -(back as f64));
    }
    if modifier == Some(Modifier::next) {
        if options.next_weekday == Some(NextWeekday::immediate) {
            return add_days(
                &reference.start_of_day(),
                if future == 0 { 7 } else { future } as f64,
            );
        }
        return add_days(&week_beginning(reference, start), (7 + position) as f64);
    }
    if options.bare_weekday == Some(BareWeekday::nearest) {
        return add_days(
            &reference.start_of_day(),
            if future > 3 { future - 7 } else { future } as f64,
        );
    }
    add_days(&reference.start_of_day(), future as f64)
}

fn future_back(current: usize, target: usize) -> i32 {
    (current as i32 - target as i32 + 7) % 7
}

fn calendar_date(spec: &CalendarDate, reference: &Civil) -> Result<Civil, String> {
    let base = reference.start_of_day();
    let date = Civil {
        year: spec.year.map(|value| value as i32).unwrap_or(base.year),
        month: spec.month.map(|value| value as i32).unwrap_or(base.month),
        day: spec.day.map(|value| value as i32).unwrap_or(1),
        ..base
    };

    if !is_valid(&date) {
        return Err("The expression names an invalid calendar date.".into());
    }
    Ok(date)
}

pub fn add_civil(date: &Civil, amount: f64, unit: Unit) -> Civil {
    match unit {
        Unit::minute => from_utc(utc(date) + amount * 60_000.0),
        Unit::hour => from_utc(utc(date) + amount * 3_600_000.0),
        Unit::day => add_days(date, amount),
        Unit::week => add_days(date, amount * 7.0),
        Unit::month => add_months(date, amount),
        Unit::quarter => add_months(date, amount * 3.0),
        Unit::year => add_months(date, amount * 12.0),
    }
}

fn relative_period(
    unit: Unit,
    modifier: Modifier,
    edge: Option<Edge>,
    reference: &Civil,
    options: &ResolveOptions,
) -> LocalPeriod {
    let mut beginning = reference.start_of_day();
    if unit == Unit::week {
        beginning = week_beginning(reference, options.week_start.unwrap_or(WeekStart::MO));
    }
    if unit == Unit::month {
        beginning.day = 1;
    }
    if unit == Unit::quarter {
        beginning.month = (beginning.month - 1) / 3 * 3 + 1;
        beginning.day = 1;
    }
    if unit == Unit::year {
        beginning.month = 1;
        beginning.day = 1;
    }
    if unit == Unit::hour {
        beginning = Civil {
            minute: 0,
            second: 0,
            ..*reference
        };
    }
    if unit == Unit::minute {
        beginning.second = 0;
    }

    let offset = match modifier {
        Modifier::next => 1.0,
        Modifier::last => -1.0,
        Modifier::this => 0.0,
    };
    let start = add_civil(&beginning, offset, unit);
    let end = add_civil(&start, 1.0, unit);

    if edge == Some(Edge::start) {
        return LocalPeriod { start, end: None };
    }
    if edge == Some(Edge::end) {
        let is_clock_unit = unit == Unit::minute || unit == Unit::hour;
        return LocalPeriod {
            start: if is_clock_unit {
                from_utc(utc(&end) - 1000.0)
            } else {
                add_days(&end, -1.0)
            },
            end: None,
        };
    }
    LocalPeriod {
        start,
        end: Some(end),
    }
}

pub fn resolve_dates(
    spec: Option<&DateSpec>,
    reference: &Civil,
    options: &ResolveOptions,
) -> Result<Vec<LocalPeriod>, String> {
    use crate::types::{MonthRef, RelativeUnitKind};

    let today = reference.start_of_day();
    let Some(spec) = spec else {
        return Ok(vec![LocalPeriod {
            start: today,
            end: None,
        }]);
    };

    match spec {
        DateSpec::Now => Ok(vec![LocalPeriod {
            start: *reference,
            end: None,
        }]),

        DateSpec::RelativeDay { offset } => Ok(vec![LocalPeriod {
            start: add_days(&today, *offset as f64),
            end: None,
        }]),

        DateSpec::Weekday { days, modifier } => Ok(days
            .iter()
            .map(|day| LocalPeriod {
                start: weekday_date(*day, *modifier, reference, options),
                end: None,
            })
            .collect()),

        DateSpec::WeekdayRange { from, to } => {
            let start = weekday_date(*from, None, reference, options);
            let length =
                ((weekday_index(*to) as i32 - weekday_index(*from) as i32 + 7) % 7 + 1) as f64;
            Ok(vec![LocalPeriod {
                start,
                end: Some(add_days(&start, length)),
            }])
        }

        DateSpec::DayGroup { group, modifier } => {
            let selected_days: Vec<crate::types::Weekday> = if *group == DayGroup::weekend {
                WEEKDAYS[5..].to_vec()
            } else {
                WEEKDAYS[..5].to_vec()
            };
            if let Some(group_modifier) = modifier {
                let mut start =
                    weekday_date(selected_days[0], Some(Modifier::this), reference, options);
                if *group_modifier == Modifier::last {
                    start = add_days(&start, -7.0);
                }
                if *group_modifier == Modifier::next {
                    let upcoming = options.next_weekday
                        == Some(crate::types::NextWeekday::immediate)
                        && utc(&start) > utc(&today);
                    if !upcoming {
                        start = add_days(&start, 7.0);
                    }
                }
                return Ok((0..selected_days.len())
                    .map(|index| LocalPeriod {
                        start: add_days(&start, index as f64),
                        end: None,
                    })
                    .collect());
            }
            Ok(selected_days
                .iter()
                .map(|day| LocalPeriod {
                    start: weekday_date(*day, None, reference, options),
                    end: None,
                })
                .collect())
        }

        DateSpec::Calendar { year, month, day } => Ok(vec![LocalPeriod {
            start: calendar_date(
                &CalendarDate {
                    year: *year,
                    month: *month,
                    day: *day,
                },
                reference,
            )?,
            end: None,
        }]),

        DateSpec::CalendarPeriod {
            month,
            year,
            modifier,
            week,
        } => {
            let mut resolved_year = year.unwrap_or(reference.year as i64) as i32;
            if year.is_none() && *modifier == Some(Modifier::next) {
                resolved_year += i32::from(*month <= reference.month as i64);
            }
            if year.is_none() && *modifier == Some(Modifier::last) {
                resolved_year -= i32::from(*month >= reference.month as i64);
            }
            let beginning = calendar_date(
                &CalendarDate {
                    year: Some(resolved_year as i64),
                    month: Some(*month),
                    day: Some(1),
                },
                reference,
            )?;
            let end = add_months(&beginning, 1.0);
            if let Some(week) = week {
                let start = add_days(&beginning, ((*week - 1) * 7) as f64);
                if *week < 1 || *week > 5 || start.month != beginning.month {
                    return Err("The requested week does not exist in that month.".into());
                }
                let next = add_days(&start, 7.0);
                return Ok(vec![LocalPeriod {
                    start,
                    end: Some(if utc(&next) < utc(&end) { next } else { end }),
                }]);
            }
            Ok(vec![LocalPeriod {
                start: beginning,
                end: Some(end),
            }])
        }

        DateSpec::RelativeUnit {
            unit,
            modifier,
            edge,
        } => Ok(vec![relative_period(
            *unit, *modifier, *edge, reference, options,
        )]),

        DateSpec::PeriodCalendar {
            period,
            modifier,
            month,
            day,
        } => {
            let anchor = relative_period(
                match period {
                    RelativeUnitKind::month => Unit::month,
                    RelativeUnitKind::year => Unit::year,
                },
                *modifier,
                None,
                reference,
                options,
            )
            .start;
            if period == &RelativeUnitKind::month
                && month.is_some_and(|named| named != anchor.month as i64)
            {
                return Err("The named month conflicts with the anchored month.".into());
            }
            let date = calendar_date(
                &CalendarDate {
                    year: Some(anchor.year as i64),
                    month: month.or(Some(anchor.month as i64)),
                    day: *day,
                },
                reference,
            )?;
            Ok(vec![LocalPeriod {
                start: date,
                end: None,
            }])
        }

        DateSpec::PeriodWeekday { modifier, days } => {
            let week = relative_period(Unit::week, *modifier, None, reference, options).start;
            let mut earliest: Option<Civil> = None;
            for day in days {
                let target = weekday_index(*day);
                let candidate = add_days(
                    &week,
                    ((target as i32 - day_of_week(&week) as i32 + 7) % 7) as f64,
                );
                if earliest.is_none_or(|known| utc(&candidate) < utc(&known)) {
                    earliest = Some(candidate);
                }
            }
            Ok(earliest
                .map(|date| {
                    vec![LocalPeriod {
                        start: date,
                        end: None,
                    }]
                })
                .unwrap_or_default())
        }

        DateSpec::Holiday { name } => {
            let Some((month, day)) = holiday_date(name, reference.year) else {
                return Err("Diwali dates are tabulated for 2025 through 2030.".into());
            };
            let mut date = calendar_date(
                &CalendarDate {
                    year: None,
                    month: Some(month as i64),
                    day: Some(day as i64),
                },
                reference,
            )?;
            if utc(&date) < utc(&today) {
                if let Some((month, day)) = holiday_date(name, reference.year + 1) {
                    date.month = month;
                    date.day = day;
                }
                date.year += 1;
            }
            Ok(vec![LocalPeriod {
                start: date,
                end: None,
            }])
        }

        DateSpec::OrdinalWeekday {
            ordinal, day, of, ..
        } => {
            let beginning = match of.as_ref() {
                MonthRef::Calendar { year, month } => calendar_date(
                    &CalendarDate {
                        year: *year,
                        month: *month,
                        day: Some(1),
                    },
                    reference,
                )?,
                MonthRef::RelativeUnit { unit, modifier } => {
                    relative_period(
                        match unit {
                            RelativeUnitKind::month => Unit::month,
                            RelativeUnitKind::year => Unit::year,
                        },
                        *modifier,
                        None,
                        reference,
                        options,
                    )
                    .start
                }
            };
            let target = weekday_index(*day);
            let date = if *ordinal > 0 {
                add_days(
                    &beginning,
                    (((target as i32 - day_of_week(&beginning) as i32 + 7) % 7) as i64
                        + (*ordinal - 1) * 7) as f64,
                )
            } else {
                let mut last = beginning;
                last.day = days_in_month(beginning.year, beginning.month);
                add_days(
                    &last,
                    -((day_of_week(&last) as i32 - target as i32 + 7) % 7) as f64
                        + ((*ordinal + 1) * 7) as f64,
                )
            };

            if date.month != beginning.month {
                return Err("The requested ordinal weekday does not exist in that month.".into());
            }
            Ok(vec![LocalPeriod {
                start: date,
                end: None,
            }])
        }

        DateSpec::CalendarRange { from, to } => {
            let start = calendar_date(from, reference)?;
            let mut end = calendar_date(
                &CalendarDate {
                    year: to.year,
                    month: to.month.or(Some(start.month as i64)),
                    day: to.day,
                },
                &start,
            )?;

            if utc(&end) < utc(&start) && to.year.is_none() && to.month.is_some() {
                end = calendar_date(
                    &CalendarDate {
                        year: Some(start.year as i64 + 1),
                        month: to.month,
                        day: to.day,
                    },
                    &start,
                )?;
            }
            if utc(&end) < utc(&start) {
                return Err("A date range must end on or after its start date.".into());
            }

            Ok(vec![LocalPeriod {
                start,
                end: Some(add_days(&end, 1.0)),
            }])
        }
    }
}

#[cfg(test)]
mod holiday_tests {
    use super::*;

    #[test]
    fn western_easter_anchors() {
        assert_eq!(western_easter(2026), (4, 5));
        assert_eq!(western_easter(2027), (3, 28));
        assert_eq!(western_easter(2028), (4, 16));
        assert_eq!(western_easter(2024), (3, 31));
    }

    #[test]
    fn holiday_asset_resolves_each_kind() {
        // fixed
        assert_eq!(holiday_date("christmas", 2026), Some((12, 25)));
        assert_eq!(holiday_date("boxing-day", 2026), Some((12, 26)));
        // nth weekday: Thanksgiving is the 4th Thursday of November
        assert_eq!(holiday_date("thanksgiving", 2026), Some((11, 26)));
        assert_eq!(holiday_date("thanksgiving", 2027), Some((11, 25)));
        // negative ordinal: Memorial Day is the last Monday of May
        assert_eq!(holiday_date("memorial-day", 2026), Some((5, 25)));
        // Easter offsets
        assert_eq!(holiday_date("good-friday", 2026), Some((4, 3)));
        assert_eq!(holiday_date("easter-monday", 2027), Some((3, 29)));
        // tabulated lunar calendar
        assert_eq!(holiday_date("diwali", 2026), Some((11, 8)));
        assert_eq!(holiday_date("diwali", 2031), None);
        // festival tables from the curated reference
        assert_eq!(holiday_date("holi", 2026), Some((3, 4)));
        assert_eq!(holiday_date("holi", 2030), Some((3, 20)));
        assert_eq!(holiday_date("karwa-chauth", 2028), Some((10, 7)));
        assert_eq!(holiday_date("dussehra", 2027), Some((10, 9)));
        assert_eq!(holiday_date("eid-ul-fitr", 2026), Some((3, 21)));
        // UK Mothering Sunday is the fourth Sunday of Lent
        assert_eq!(holiday_date("mothering-sunday", 2026), Some((3, 15)));
        // English-speaking additions
        assert_eq!(holiday_date("columbus-day", 2026), Some((10, 12)));
        assert_eq!(holiday_date("canadian-thanksgiving", 2026), Some((10, 12)));
        assert_eq!(holiday_date("anzac-day", 2027), Some((4, 25)));
        // unknown key
        assert_eq!(holiday_date("nobody", 2026), None);
    }
}
