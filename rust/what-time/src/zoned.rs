//! Civil calendar arithmetic and timezone conversion.
//!
//! Wall-clock fields live in a plain `Civil`
//! struct, performs day/month arithmetic in a proleptic-Gregorian UTC space
//! (with `Date.UTC`-style month/day overflow normalization), and resolves
//! local-to-instant through a candidate-offset search that handles DST gaps,
//! overlaps, half-hour offsets, and whole skipped days. This port preserves
//! those algorithms exactly; `jiff` replaces `Intl.DateTimeFormat` only for
//! offset lookups against the IANA database.

use jiff::Timestamp;
use jiff::tz::TimeZone;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Civil {
    pub year: i32,
    pub month: i32,
    pub day: i32,
    pub hour: i32,
    pub minute: i32,
    pub second: i32,
}

impl Civil {
    #[cfg(test)]
    pub fn date(year: i32, month: i32, day: i32) -> Civil {
        Civil {
            year,
            month,
            day,
            hour: 0,
            minute: 0,
            second: 0,
        }
    }

    pub fn start_of_day(mut self) -> Civil {
        self.hour = 0;
        self.minute = 0;
        self.second = 0;
        self
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ZonedKind {
    Exact,
    Gap,
    Overlap,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ZonedInstant {
    pub epoch_ms: f64,
    pub kind: ZonedKind,
}

const MS_PER_DAY: f64 = 86_400_000.0;

/// Days since 1970-01-01 in the proleptic Gregorian calendar
/// (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = (month + 9) % 12; // [0, 11]
    let doy = (153 * mp + 2) / 5 + day - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146097 + doe - 719468
}

fn civil_from_days(days: i64) -> (i32, i32, i32) {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let year = if m <= 2 { y + 1 } else { y };
    (year as i32, m as i32, d as i32)
}

/// Normalizes month and day overflow the way `Date.UTC` does
/// (month 13 rolls into the next year, day 0 is the last day of
/// the previous month, and so on).
pub fn utc(fields: &Civil) -> f64 {
    let year = fields.year as i64;
    let month0 = fields.month as i64 - 1;
    let year = year + month0.div_euclid(12);
    let month = month0.rem_euclid(12) + 1; // [1, 12]
    let days = days_from_civil(year, month, 1) + (fields.day as i64 - 1);
    (days * 86_400 + fields.hour as i64 * 3600 + fields.minute as i64 * 60 + fields.second as i64)
        as f64
        * 1000.0
}

pub fn from_utc(epoch: f64) -> Civil {
    let epoch = epoch.floor() as i64;
    let days = epoch.div_euclid(86_400_000);
    let seconds = epoch.rem_euclid(86_400_000) / 1000;
    let (year, month, day) = civil_from_days(days);
    Civil {
        year,
        month,
        day,
        hour: (seconds / 3600) as i32,
        minute: (seconds / 60 % 60) as i32,
        second: (seconds % 60) as i32,
    }
}

fn time_zone(name: &str) -> Result<TimeZone, String> {
    TimeZone::get(name).map_err(|_| format!("Invalid time zone: {name}"))
}

fn timestamp(epoch_ms: f64) -> Timestamp {
    Timestamp::from_millisecond(epoch_ms.floor() as i64)
        .expect("epochs stay within the timestamp range")
}

/// Local civil fields for an instant, exactly as the reference's
/// `Intl.DateTimeFormat` projection.
pub fn civil(epoch: f64, time_zone_name: &str) -> Result<Civil, String> {
    let zone = time_zone(time_zone_name)?;
    let datetime = timestamp(epoch).to_zoned(zone).datetime();
    Ok(Civil {
        year: datetime.year() as i32,
        month: datetime.month() as i32,
        day: datetime.day() as i32,
        hour: datetime.hour() as i32,
        minute: datetime.minute() as i32,
        second: datetime.second() as i32,
    })
}

/// Resolves local fields to an instant. Overlaps pick the earliest match,
/// gaps pick the first instant after the gap (the standard interpretation;
/// Temporal's "compatible" disambiguation).
pub fn zoned_to_epoch(fields: &Civil, time_zone_name: &str) -> Result<ZonedInstant, String> {
    let zone = time_zone(time_zone_name)?;
    let local = utc(fields);

    // Both sides of a transition matter, including half-hour and skipped-day changes.
    let mut offsets: Vec<i64> = Vec::new();
    for days_away in [-2i64, -1, 0, 1, 2] {
        let offset = timestamp(local + days_away as f64 * MS_PER_DAY)
            .to_zoned(zone.clone())
            .offset()
            .seconds() as i64
            * 1000;
        if !offsets.contains(&offset) {
            offsets.push(offset);
        }
    }

    let mut candidates: Vec<i64> = offsets
        .iter()
        .map(|offset| local as i64 - *offset)
        .collect();
    candidates.sort_unstable();

    fn to_civil(epoch: i64, zone: &TimeZone) -> Civil {
        let datetime = Timestamp::from_millisecond(epoch)
            .expect("candidates stay within the timestamp range")
            .to_zoned(zone.clone())
            .datetime();
        Civil {
            year: datetime.year() as i32,
            month: datetime.month() as i32,
            day: datetime.day() as i32,
            hour: datetime.hour() as i32,
            minute: datetime.minute() as i32,
            second: datetime.second() as i32,
        }
    }

    let matches: Vec<i64> = candidates
        .iter()
        .copied()
        .filter(|epoch| utc(&to_civil(*epoch, &zone)) as i64 == local as i64)
        .collect();

    if !matches.is_empty() {
        return Ok(ZonedInstant {
            epoch_ms: matches[0] as f64,
            kind: if matches.len() > 1 {
                ZonedKind::Overlap
            } else {
                ZonedKind::Exact
            },
        });
    }
    let after_gap = candidates
        .iter()
        .copied()
        .find(|epoch| utc(&to_civil(*epoch, &zone)) as i64 > local as i64);
    Ok(ZonedInstant {
        epoch_ms: after_gap.unwrap_or(*candidates.last().unwrap()) as f64,
        kind: ZonedKind::Gap,
    })
}

pub fn day_number(date: &Civil) -> i64 {
    (utc(&date.start_of_day()) / MS_PER_DAY).floor() as i64
}

pub fn day_of_week(date: &Civil) -> usize {
    (day_number(date) + 3).rem_euclid(7) as usize
}

pub fn days_in_month(year: i32, month: i32) -> i32 {
    (days_from_civil(year as i64, month as i64 + 1, 1)
        - days_from_civil(year as i64, month as i64, 1)) as i32
}

pub fn is_valid(fields: &Civil) -> bool {
    let in_range = |value: i32, minimum: i32, maximum: i32| (minimum..=maximum).contains(&value);
    in_range(fields.year, 1, 9999)
        && in_range(fields.month, 1, 12)
        && in_range(fields.day, 1, days_in_month(fields.year, fields.month))
        && in_range(fields.hour, 0, 23)
        && in_range(fields.minute, 0, 59)
        && in_range(fields.second, 0, 59)
}

pub fn add_days(date: &Civil, days: f64) -> Civil {
    from_utc(utc(date) + days * MS_PER_DAY)
}

pub fn add_months(date: &Civil, months: f64) -> Civil {
    let total = months.floor() as i64;
    let mut shifted = *date;
    shifted.month += total as i32;
    let target = from_utc(utc(&Civil { day: 1, ..shifted }));
    let day = date.day.min(days_in_month(target.year, target.month));
    Civil { day, ..target }
}

fn two_digits(value: i64) -> String {
    format!("{value:02}")
}

pub fn iso(epoch: f64, time_zone_name: &str) -> Result<String, String> {
    let local = civil(epoch, time_zone_name)?;
    let whole_seconds = (epoch / 1000.0).floor() * 1000.0;
    let offset_minutes = ((utc(&local) - whole_seconds) / 60_000.0).floor();
    let sign = if offset_minutes < 0.0 { '-' } else { '+' };
    let magnitude = offset_minutes.abs() as i64;
    let date = format!(
        "{:04}-{}-{}",
        local.year,
        two_digits(local.month as i64),
        two_digits(local.day as i64)
    );
    let time = format!(
        "{}:{}:{}",
        two_digits(local.hour as i64),
        two_digits(local.minute as i64),
        two_digits(local.second as i64)
    );
    Ok(format!(
        "{date}T{time}{}{}:{}",
        sign,
        two_digits(magnitude / 60),
        two_digits(magnitude % 60)
    ))
}

/// Parses an ISO instant that carries `Z` or an explicit numeric offset.
pub fn instant(text: &str) -> Result<f64, String> {
    let bytes = text.as_bytes();
    let offset_suffix = bytes.len() >= 6
        && (bytes[bytes.len() - 6] == b'+' || bytes[bytes.len() - 6] == b'-')
        && bytes[bytes.len() - 5].is_ascii_digit()
        && bytes[bytes.len() - 4].is_ascii_digit()
        && bytes[bytes.len() - 3] == b':'
        && bytes[bytes.len() - 2].is_ascii_digit()
        && bytes[bytes.len() - 1].is_ascii_digit();
    if !text.ends_with('Z') && !offset_suffix {
        return Err("reference must be an ISO instant with Z or an explicit offset.".into());
    }
    let parsed: Timestamp = text
        .parse()
        .map_err(|_| "Invalid reference instant.".to_string())?;
    Ok(parsed.as_millisecond() as f64)
}

/// Epoch milliseconds for an already-formatted local ISO string
/// (the `Date.parse` used to order the public result).
pub fn iso_epoch(text: &str) -> Result<f64, String> {
    let parsed: Timestamp = text
        .parse()
        .map_err(|_| format!("Invalid ISO instant: {text}"))?;
    Ok(parsed.as_millisecond() as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn epoch_of(date: &str) -> f64 {
        format!("{date}T00:00:00Z")
            .parse::<jiff::Timestamp>()
            .unwrap()
            .as_millisecond() as f64
    }

    #[test]
    fn preserves_gregorian_date_arithmetic() {
        let cases = [
            ((1, 1, 1), "0001-01-01"),
            ((5, 3, 0), "0005-02-28"),
            ((99, 13, 1), "0100-01-01"),
            ((100, 2, 29), "0100-03-01"),
            ((1969, 12, 31), "1969-12-31"),
            ((2000, 2, 29), "2000-02-29"),
        ];
        for ((year, month, day), date) in cases {
            let fields = Civil::date(year, month, day);
            assert_eq!(utc(&fields), epoch_of(date), "{date}");
            let weekday = epoch_of(date) as i64 / 86_400_000;
            assert_eq!(
                day_of_week(&fields) as i64,
                (weekday + 3).rem_euclid(7),
                "{date}"
            );
        }
    }

    #[test]
    fn roundtrips_through_from_utc() {
        let fields = Civil {
            year: 2026,
            month: 9,
            day: 9,
            hour: 18,
            minute: 0,
            second: 0,
        };
        assert_eq!(from_utc(utc(&fields)), fields);
    }

    #[test]
    fn resolves_dst_boundaries() {
        // New York spring gap 2026: 02:30 does not exist.
        let gap = zoned_to_epoch(&Civil::date(2026, 3, 8).at(2, 30), "America/New_York").unwrap();
        assert_eq!(gap.kind, ZonedKind::Gap);
        assert_eq!(
            gap.epoch_ms,
            epoch_at("2026-03-08T03:30:00-04:00", "America/New_York")
        );
        // New York fall overlap 2026: 01:30 exists twice; earliest wins.
        let overlap =
            zoned_to_epoch(&Civil::date(2026, 11, 1).at(1, 30), "America/New_York").unwrap();
        assert_eq!(overlap.kind, ZonedKind::Overlap);
        assert_eq!(
            overlap.epoch_ms,
            epoch_at("2026-11-01T01:30:00-04:00", "America/New_York")
        );
    }

    /// Test helper: set the clock fields of a date.
    trait AtClock {
        fn at(self, hour: i32, minute: i32) -> Civil;
    }
    impl AtClock for Civil {
        fn at(self, hour: i32, minute: i32) -> Civil {
            Civil {
                hour,
                minute,
                second: 0,
                ..self
            }
        }
    }

    fn epoch_at(local: &str, _zone: &str) -> f64 {
        local.parse::<jiff::Timestamp>().unwrap().as_millisecond() as f64
    }

    #[test]
    fn civil_projection_matches_dhaka() {
        let epoch = instant("2026-09-09T12:00:00+06:00").unwrap();
        let local = civil(epoch, "Asia/Dhaka").unwrap();
        assert_eq!(
            local,
            Civil {
                year: 2026,
                month: 9,
                day: 9,
                hour: 12,
                minute: 0,
                second: 0
            }
        );
        let horizon = add_months(&local, 12.0);
        assert_eq!((horizon.year, horizon.month, horizon.day), (2027, 9, 9));
        zoned_to_epoch(&horizon, "Asia/Dhaka").unwrap();
    }

    #[test]
    fn formats_iso_with_offset() {
        let before = epoch_at("2026-11-01T01:30:00-04:00", "America/New_York");
        let after = epoch_at("2026-11-01T01:30:00-05:00", "America/New_York");
        assert_eq!(
            iso(before, "America/New_York").unwrap(),
            "2026-11-01T01:30:00-04:00"
        );
        assert_eq!(
            iso(after, "America/New_York").unwrap(),
            "2026-11-01T01:30:00-05:00"
        );
    }
}
