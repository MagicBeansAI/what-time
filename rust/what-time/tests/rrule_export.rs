//! RRULE export behavior: exported fragments and occurrence dates are
//! asserted directly against recorded expectations.

use what_time::{
    Clause, ClockTime, Frequency, NamedClock, Recurrence, Schedule, TimeSpec, Weekday,
};
use what_time::{ParseContext, resolve};

fn dhaka() -> ParseContext {
    ParseContext {
        reference: "2026-09-09T12:00:00+06:00".into(),
        time_zone: "Asia/Dhaka".into(),
        ..Default::default()
    }
}

fn noon_clause(recurrence: Recurrence) -> Clause {
    Clause {
        time: Some(TimeSpec {
            start: ClockTime::Named {
                named: NamedClock::noon,
            },
            end: None,
            open: None,
        }),
        recurrence: Some(recurrence),
        ..Default::default()
    }
}

fn rule(freq: Frequency, interval: i64) -> Recurrence {
    Recurrence {
        freq,
        interval,
        by_day: None,
        by_month_day: None,
        by_set_pos: None,
        by_month: None,
        times_per: None,
        count: None,
        until: None,
        start: None,
        except: None,
        span: None,
    }
}

#[test]
fn exports_a_valid_dtstart_and_rule() {
    let schedule = Schedule {
        clauses: vec![Clause {
            time: Some(TimeSpec {
                start: ClockTime::hm(20, 0),
                end: Some(ClockTime::hm(22, 0)),
                open: None,
            }),
            recurrence: Some(Recurrence {
                by_day: Some(vec![Weekday::MO]),
                count: Some(3),
                ..rule(Frequency::weekly, 2)
            }),
            ..Default::default()
        }],
    };
    let result = resolve(&schedule, &dhaka()).unwrap();
    assert_eq!(result.rrules.len(), 1);
    assert!(
        result.rrules[0].contains("DTSTART;TZID=Asia/Dhaka:20260914T200000"),
        "{}",
        result.rrules[0]
    );
    assert!(
        result.rrules[0].contains("DTEND;TZID=Asia/Dhaka:20260914T220000"),
        "{}",
        result.rrules[0]
    );
    // Count 3, every second Monday from September 14.
    assert_eq!(result.occurrences.len(), 3);
    assert_eq!(result.occurrences[0].start, "2026-09-14T20:00:00+06:00");
    assert_eq!(result.occurrences[1].start, "2026-09-28T20:00:00+06:00");
    assert_eq!(result.occurrences[2].start, "2026-10-12T20:00:00+06:00");
}

#[test]
fn exports_the_derived_hours_for_a_daily_frequency_count() {
    let schedule = Schedule {
        clauses: vec![Clause {
            recurrence: Some(Recurrence {
                times_per: Some(3),
                count: Some(6),
                ..rule(Frequency::daily, 1)
            }),
            ..Default::default()
        }],
    };
    let result = resolve(&schedule, &dhaka()).unwrap();
    assert!(
        result.rrules[0].contains("BYHOUR=0,8,16"),
        "{}",
        result.rrules[0]
    );
    // The reference instant is 12:00 local, so the same-day 00:00 and 08:00
    // slots are in the past; the preview starts at the 16:00 slot.
    assert_eq!(result.occurrences.len(), 6);
    assert_eq!(
        result
            .occurrences
            .iter()
            .map(|value| value.start.as_str())
            .collect::<Vec<_>>(),
        [
            "2026-09-09T16:00:00+06:00",
            "2026-09-10T00:00:00+06:00",
            "2026-09-10T08:00:00+06:00",
            "2026-09-10T16:00:00+06:00",
            "2026-09-11T00:00:00+06:00",
            "2026-09-11T08:00:00+06:00",
        ]
    );
}

#[test]
fn rejects_conflicting_recurrence_bounds() {
    let schedule = Schedule {
        clauses: vec![Clause {
            recurrence: Some(Recurrence {
                count: Some(3),
                until: Some(Box::new(what_time::DateSpec::Calendar {
                    year: None,
                    month: Some(12),
                    day: Some(31),
                })),
                ..rule(Frequency::daily, 1)
            }),
            ..Default::default()
        }],
    };
    let error = resolve(&schedule, &dhaka()).unwrap_err();
    assert!(error.contains("count or an end bound"), "{error}");
}

#[test]
fn exports_explicit_date_exceptions() {
    let schedule = Schedule {
        clauses: vec![noon_clause(Recurrence {
            count: Some(4),
            except: Some(vec![what_time::DateSpec::Calendar {
                year: Some(2026),
                month: Some(9),
                day: Some(10),
            }]),
            ..rule(Frequency::daily, 1)
        })],
    };
    let result = resolve(&schedule, &dhaka()).unwrap();
    assert_eq!(result.rrules.len(), 1);
    assert!(
        result.rrules[0].contains("EXDATE;TZID=Asia/Dhaka:20260910T120000"),
        "{}",
        result.rrules[0]
    );
    // The exception is excluded from the preview but the rule still counts it.
    assert_eq!(result.occurrences.len(), 4);
    let starts: Vec<&str> = result
        .occurrences
        .iter()
        .map(|value| value.start.as_str())
        .collect();
    assert_eq!(
        starts,
        [
            "2026-09-09T12:00:00+06:00",
            "2026-09-11T12:00:00+06:00",
            "2026-09-12T12:00:00+06:00",
            "2026-09-13T12:00:00+06:00",
        ]
    );
}

#[test]
fn keeps_exception_export_independent_of_the_preview_limit() {
    let schedule = Schedule {
        clauses: vec![noon_clause(Recurrence {
            count: Some(6),
            except: Some(vec![what_time::DateSpec::Calendar {
                year: Some(2026),
                month: Some(9),
                day: Some(12),
            }]),
            ..rule(Frequency::daily, 1)
        })],
    };
    let result = resolve(
        &schedule,
        &ParseContext {
            limit: Some(1),
            ..dhaka()
        },
    )
    .unwrap();
    assert_eq!(result.occurrences.len(), 1);
    assert_eq!(result.rrules.len(), 1);
    // The exported rule covers the whole series even though the preview shows
    // one occurrence: the EXDATE lists every exception date.
    assert!(
        result.rrules[0].contains("EXDATE;TZID=Asia/Dhaka:20260912T120000"),
        "{}",
        result.rrules[0]
    );
    assert!(result.rrules[0].contains("COUNT=7"), "{}", result.rrules[0]);
}
