//! Calendar resolution cases. Schedules are expressed in the JSON encoding
//! and compared through the same encoding (structural equality).

use serde_json::{Value, json};
use what_time::{BareWeekdaysPolicy, NextWeekday, ParseContext, Schedule, WeekStart, resolve};

fn dhaka() -> ParseContext {
    ParseContext {
        reference: "2026-09-09T12:00:00+06:00".into(),
        time_zone: "Asia/Dhaka".into(),
        ..Default::default()
    }
}

fn schedule(source: Value) -> Schedule {
    serde_json::from_value(source).unwrap()
}

fn occurrences(result: &what_time::Resolved) -> Value {
    serde_json::to_value(&result.occurrences).unwrap()
}

fn starts(result: &what_time::Resolved) -> Vec<String> {
    result
        .occurrences
        .iter()
        .map(|value| value.start.clone())
        .collect()
}

fn start_dates(result: &what_time::Resolved) -> Vec<String> {
    result
        .occurrences
        .iter()
        .map(|value| value.start[..10].to_string())
        .collect()
}

#[test]
fn preserves_a_range_ending_at_the_unix_epoch() {
    let result = resolve(
        &schedule(json!({"clauses": [{
            "date": {"kind": "calendar", "year": 1969, "month": 12, "day": 31},
            "time": {"start": {"hour": 22, "minute": 0}, "end": {"hour": 0, "minute": 0}},
        }]})),
        &ParseContext {
            reference: "1969-12-30T12:00:00Z".into(),
            time_zone: "UTC".into(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        occurrences(&result),
        json!([{
            "start": "1969-12-31T22:00:00+00:00",
            "end": "1970-01-01T00:00:00+00:00",
            "allDay": false,
            "clause": 0,
        }]),
    );
}

#[test]
fn retains_second_precision_duration_behavior_for_fractional_references() {
    let result = resolve(
        &schedule(json!({"clauses": [{"duration": {"amount": 2, "unit": "hour"}}]})),
        &ParseContext {
            reference: "2026-09-09T12:00:00.500Z".into(),
            time_zone: "UTC".into(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        occurrences(&result),
        json!([{
            "start": "2026-09-09T12:00:00+00:00",
            "end": "2026-09-09T14:00:00+00:00",
            "allDay": false,
            "clause": 0,
        }]),
    );
}

#[test]
fn reports_a_recurrence_search_limit_instead_of_a_silent_partial_series() {
    let error = resolve(
        &schedule(json!({"clauses": [{"recurrence": {"freq": "yearly", "interval": 100}}]})),
        &ParseContext {
            reference: "2026-01-01T00:00:00Z".into(),
            time_zone: "UTC".into(),
            until: Some("2500-01-01".into()),
            limit: Some(10),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(error.contains("100000-day search limit"), "{error}");
}

#[test]
fn finishes_a_finite_count_reached_on_the_final_permitted_search_day() {
    let result = resolve(
        &schedule(
            json!({"clauses": [{"recurrence": {"freq": "daily", "interval": 99999, "count": 2}}]}),
        ),
        &ParseContext {
            reference: "2026-01-01T00:00:00Z".into(),
            time_zone: "UTC".into(),
            until: Some("2500-01-01".into()),
            limit: Some(10),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(result.occurrences.len(), 2);
    assert!(!result.truncated);
    assert!(result.rrules[0].contains("COUNT=2"));
}

#[test]
fn makes_bare_weekday_clauses_weekly_only_when_the_caller_requests_it() {
    let source = json!({"clauses": [{
        "date": {"kind": "weekday", "days": ["SA", "SU"]},
        "time": {"start": {"hour": 13, "minute": 0}, "end": {"hour": 20, "minute": 0}},
    }]});
    let once = resolve(
        &schedule(source.clone()),
        &ParseContext {
            bare_weekdays: Some(BareWeekdaysPolicy::once),
            ..dhaka()
        },
    )
    .unwrap();
    assert_eq!(once.occurrences.len(), 2);

    let recurring = resolve(
        &schedule(source),
        &ParseContext {
            bare_weekdays: Some(BareWeekdaysPolicy::weekly),
            limit: Some(4),
            ..dhaka()
        },
    )
    .unwrap();
    assert_eq!(
        starts(&recurring),
        [
            "2026-09-12T13:00:00+06:00",
            "2026-09-13T13:00:00+06:00",
            "2026-09-19T13:00:00+06:00",
            "2026-09-20T13:00:00+06:00",
        ]
    );
    assert!(recurring.rrules[0].contains("FREQ=WEEKLY;INTERVAL=1;BYDAY=SA,SU"));

    let modified = resolve(
        &schedule(json!({"clauses": [
            {"date": {"kind": "weekday", "days": ["MO"], "modifier": "next"}}
        ]})),
        &ParseContext {
            bare_weekdays: Some(BareWeekdaysPolicy::weekly),
            ..dhaka()
        },
    )
    .unwrap();
    assert!(modified.rrules.is_empty());
}

#[test]
fn lets_an_explicit_duration_replace_the_implied_end_of_a_day_part() {
    let source = json!({"clauses": [{
        "date": {"kind": "relativeDay", "offset": 1},
        "time": {"start": {"part": "morning"}},
        "duration": {"amount": 30, "unit": "minute"},
    }]});
    let result = resolve(&schedule(source.clone()), &dhaka()).unwrap();
    assert_eq!(
        occurrences(&result),
        json!([{
            "start": "2026-09-10T06:00:00+06:00",
            "end": "2026-09-10T06:30:00+06:00",
            "allDay": false,
            "clause": 0,
        }]),
    );
    let conflicting = json!({"clauses": [{
        "date": {"kind": "relativeDay", "offset": 1},
        "time": {"start": {"part": "morning"}, "end": {"named": "noon"}},
        "duration": {"amount": 30, "unit": "minute"},
    }]});
    let error = resolve(&schedule(conflicting), &dhaka()).unwrap_err();
    assert!(error.contains("duration or an explicit end"), "{error}");
}

#[test]
fn keeps_modified_day_groups_in_one_period_instead_of_mixing_weeks() {
    for week_start in [Some(WeekStart::MO), Some(WeekStart::SU)] {
        let result = resolve(
            &schedule(json!({"clauses": [
                {"date": {"kind": "dayGroup", "group": "weekend", "modifier": "this"}}
            ]})),
            &ParseContext {
                week_start,
                ..dhaka()
            },
        )
        .unwrap();
        assert_eq!(start_dates(&result), ["2026-09-12", "2026-09-13"]);
    }
    let weekdays = resolve(
        &schedule(json!({"clauses": [
            {"date": {"kind": "dayGroup", "group": "weekday", "modifier": "last"}}
        ]})),
        &dhaka(),
    )
    .unwrap();
    assert_eq!(
        start_dates(&weekdays),
        [
            "2026-08-31",
            "2026-09-01",
            "2026-09-02",
            "2026-09-03",
            "2026-09-04"
        ]
    );
    let next = resolve(
        &schedule(json!({"clauses": [
            {"date": {"kind": "dayGroup", "group": "weekend", "modifier": "next"}}
        ]})),
        &ParseContext {
            reference: "2026-09-12T12:00:00+06:00".into(),
            next_weekday: Some(NextWeekday::immediate),
            ..dhaka()
        },
    )
    .unwrap();
    assert_eq!(start_dates(&next), ["2026-09-19", "2026-09-20"]);
}

#[test]
fn resolves_known_one_off_dates_beyond_the_default_recurrence_preview_horizon() {
    let relative = json!({"clauses": [
        {"shift": {"amount": 1, "unit": "year", "direction": "after"}}
    ]});
    assert_eq!(
        resolve(&schedule(relative), &dhaka()).unwrap().occurrences[0].start,
        "2027-09-09T12:00:00+06:00"
    );
    let explicit = json!({"clauses": [{
        "date": {"kind": "calendar", "year": 2027, "month": 10, "day": 1},
        "time": {"start": {"named": "noon"}},
    }]});
    assert_eq!(
        resolve(&schedule(explicit.clone()), &dhaka())
            .unwrap()
            .occurrences[0]
            .start,
        "2027-10-01T12:00:00+06:00"
    );
    assert!(
        resolve(
            &schedule(explicit),
            &ParseContext {
                until: Some("2027-09-01".into()),
                ..dhaka()
            },
        )
        .unwrap()
        .occurrences
        .is_empty()
    );
}

#[test]
fn resolves_shared_weekend_clocks_and_the_following_midnight_with_explicit_offsets() {
    let result = resolve(
        &schedule(json!({"clauses": [
            {
                "date": {"kind": "weekday", "days": ["SA", "SU"]},
                "time": {"start": {"hour": 13, "minute": 0}, "end": {"hour": 20, "minute": 0}},
            },
            {
                "date": {"kind": "weekday", "days": ["MO"]},
                "time": {"start": {"hour": 22, "minute": 0}, "end": {"hour": 0, "minute": 0}},
            },
        ]})),
        &dhaka(),
    )
    .unwrap();
    assert_eq!(
        occurrences(&result),
        json!([
            {
                "start": "2026-09-12T13:00:00+06:00",
                "end": "2026-09-12T20:00:00+06:00",
                "allDay": false,
                "clause": 0,
            },
            {
                "start": "2026-09-13T13:00:00+06:00",
                "end": "2026-09-13T20:00:00+06:00",
                "allDay": false,
                "clause": 0,
            },
            {
                "start": "2026-09-14T22:00:00+06:00",
                "end": "2026-09-15T00:00:00+06:00",
                "allDay": false,
                "clause": 1,
            },
        ]),
    );
}

#[test]
fn applies_an_elapsed_shift_to_its_explicit_local_date_and_clock_anchor() {
    let result = resolve(
        &schedule(json!({"clauses": [{
            "date": {"kind": "relativeDay", "offset": 1},
            "time": {"start": {"named": "noon"}},
            "shift": {"amount": 2, "unit": "hour", "direction": "before"},
        }]})),
        &dhaka(),
    )
    .unwrap();
    assert_eq!(
        occurrences(&result),
        json!([{
            "start": "2026-09-10T10:00:00+06:00",
            "allDay": false,
            "clause": 0,
        }]),
    );
}

#[test]
fn preserves_the_reference_clock_for_unanchored_relative_quantities() {
    let result = resolve(
        &schedule(json!({"clauses": [
            {"shift": {"amount": 1, "unit": "day", "direction": "before"}}
        ]})),
        &dhaka(),
    )
    .unwrap();
    assert_eq!(
        occurrences(&result),
        json!([{"start": "2026-09-08T12:00:00+06:00", "allDay": false, "clause": 0}]),
    );
}

#[test]
fn resolves_an_inclusive_calendar_range_with_an_exclusive_all_day_end() {
    let result = resolve(
        &schedule(json!({"clauses": [{
            "date": {
                "kind": "calendarRange",
                "from": {"year": 2026, "month": 6, "day": 11},
                "to": {"year": 2026, "month": 6, "day": 16},
            },
        }]})),
        &dhaka(),
    )
    .unwrap();
    assert_eq!(
        occurrences(&result),
        json!([{
            "start": "2026-06-11T00:00:00+06:00",
            "end": "2026-06-17T00:00:00+06:00",
            "allDay": true,
            "clause": 0,
        }]),
    );
}

#[test]
fn distinguishes_immediate_next_weekdays_from_next_calendar_week() {
    let source = json!({"clauses": [{
        "date": {"kind": "weekday", "days": ["FR"], "modifier": "next"},
        "time": {"start": {"hour": 14, "minute": 0}},
    }]});
    assert_eq!(
        resolve(&schedule(source.clone()), &dhaka())
            .unwrap()
            .occurrences[0]
            .start,
        "2026-09-18T14:00:00+06:00"
    );
    assert_eq!(
        resolve(
            &schedule(source),
            &ParseContext {
                next_weekday: Some(NextWeekday::immediate),
                ..dhaka()
            },
        )
        .unwrap()
        .occurrences[0]
            .start,
        "2026-09-11T14:00:00+06:00"
    );
}

#[test]
fn resolves_calendar_periods_and_their_final_dates_not_fixed_elapsed_days() {
    let next_week = resolve(
        &schedule(json!({"clauses": [
            {"date": {"kind": "relativeUnit", "unit": "week", "modifier": "next"}}
        ]})),
        &dhaka(),
    )
    .unwrap();
    assert_eq!(
        occurrences(&next_week),
        json!([{
            "start": "2026-09-14T00:00:00+06:00",
            "end": "2026-09-21T00:00:00+06:00",
            "allDay": true,
            "clause": 0,
        }]),
    );

    let end_of_next_month = resolve(
        &schedule(json!({"clauses": [{
            "date": {
                "kind": "relativeUnit",
                "unit": "month",
                "modifier": "next",
                "edge": "end",
            },
        }]})),
        &dhaka(),
    )
    .unwrap();
    assert_eq!(
        occurrences(&end_of_next_month),
        json!([{"start": "2026-10-31T00:00:00+06:00", "allDay": true, "clause": 0}]),
    );
}

#[test]
fn finds_an_ordinal_weekday_inside_a_relative_month() {
    let result = resolve(
        &schedule(json!({"clauses": [{
            "date": {
                "kind": "ordinalWeekday",
                "ordinal": 1,
                "day": "MO",
                "of": {"kind": "relativeUnit", "unit": "month", "modifier": "next"},
            },
            "time": {"start": {"named": "noon"}},
        }]})),
        &dhaka(),
    )
    .unwrap();
    assert_eq!(result.occurrences[0].start, "2026-10-05T12:00:00+06:00");
}

#[test]
fn resolves_fixed_holidays_before_applying_a_relative_calendar_shift() {
    let result = resolve(
        &schedule(json!({"clauses": [{
            "date": {"kind": "holiday", "name": "christmas"},
            "shift": {"amount": 3, "unit": "day", "direction": "before"},
        }]})),
        &dhaka(),
    )
    .unwrap();
    assert_eq!(
        occurrences(&result),
        json!([{"start": "2026-12-22T00:00:00+06:00", "allDay": true, "clause": 0}]),
    );
}

#[test]
fn wraps_weekday_ranges_across_a_week_boundary_and_distributes_day_groups() {
    let range = resolve(
        &schedule(json!({"clauses": [
            {"date": {"kind": "weekdayRange", "from": "FR", "to": "MO"}}
        ]})),
        &dhaka(),
    )
    .unwrap();
    assert_eq!(starts(&range), ["2026-09-11T00:00:00+06:00"]);
    assert_eq!(
        range.occurrences[0].end.as_deref(),
        Some("2026-09-15T00:00:00+06:00")
    );

    let weekend = resolve(
        &schedule(json!({"clauses": [{
            "date": {"kind": "dayGroup", "group": "weekend", "modifier": "this"},
            "time": {"start": {"hour": 13, "minute": 0}},
        }]})),
        &dhaka(),
    )
    .unwrap();
    assert_eq!(
        starts(&weekend),
        ["2026-09-12T13:00:00+06:00", "2026-09-13T13:00:00+06:00"]
    );
}

#[test]
fn keeps_explicit_seconds_and_resolves_a_named_day_part_to_its_configured_window() {
    let precise = resolve(
        &schedule(json!({"clauses": [{
            "date": {"kind": "relativeDay", "offset": 1},
            "time": {"start": {"hour": 14, "minute": 30, "second": 15}},
        }]})),
        &dhaka(),
    )
    .unwrap();
    assert_eq!(precise.occurrences[0].start, "2026-09-10T14:30:15+06:00");

    let mut day_parts = std::collections::HashMap::new();
    day_parts.insert(
        what_time::DayPart::morning,
        ("07:30".to_string(), "11:00".to_string()),
    );
    let morning = resolve(
        &schedule(json!({"clauses": [{
            "date": {"kind": "relativeDay", "offset": 1},
            "time": {"start": {"part": "morning"}},
        }]})),
        &ParseContext {
            day_parts: Some(day_parts),
            ..dhaka()
        },
    )
    .unwrap();
    assert_eq!(morning.occurrences[0].start, "2026-09-10T07:30:00+06:00");
    assert_eq!(
        morning.occurrences[0].end.as_deref(),
        Some("2026-09-10T11:00:00+06:00")
    );
}

#[test]
fn rejects_a_nonexistent_explicit_time_and_selects_the_earlier_offset_when_repeated() {
    let context = ParseContext {
        time_zone: "America/New_York".into(),
        ..Default::default()
    };
    let gap = json!({"clauses": [{
        "date": {"kind": "calendar", "year": 2026, "month": 3, "day": 8},
        "time": {"start": {"hour": 2, "minute": 30}},
    }]});
    let error = resolve(
        &schedule(gap),
        &ParseContext {
            reference: "2026-03-07T12:00:00-05:00".into(),
            ..context.clone()
        },
    )
    .unwrap_err();
    assert!(error.contains("does not exist"), "{error}");

    let overlap = json!({"clauses": [{
        "date": {"kind": "calendar", "year": 2026, "month": 11, "day": 1},
        "time": {"start": {"hour": 1, "minute": 30}},
    }]});
    assert_eq!(
        resolve(
            &schedule(overlap),
            &ParseContext {
                reference: "2026-10-31T12:00:00-04:00".into(),
                ..context
            },
        )
        .unwrap()
        .occurrences[0]
            .start,
        "2026-11-01T01:30:00-04:00"
    );
}

#[test]
fn resolves_durations_and_relative_quantity_windows_from_the_reference() {
    let duration = resolve(
        &schedule(json!({"clauses": [{
            "date": {"kind": "relativeDay", "offset": 1},
            "time": {"start": {"hour": 9, "minute": 0}},
            "duration": {"amount": 90, "unit": "minute"},
        }]})),
        &dhaka(),
    )
    .unwrap();
    assert_eq!(duration.occurrences[0].start, "2026-09-10T09:00:00+06:00");
    assert_eq!(
        duration.occurrences[0].end.as_deref(),
        Some("2026-09-10T10:30:00+06:00")
    );

    let window = resolve(
        &schedule(json!({"clauses": [{
            "shift": {"amount": 5, "endAmount": 10, "unit": "minute", "direction": "after"},
        }]})),
        &dhaka(),
    )
    .unwrap();
    assert_eq!(
        occurrences(&window),
        json!([{
            "start": "2026-09-09T12:05:00+06:00",
            "end": "2026-09-09T12:10:00+06:00",
            "allDay": false,
            "clause": 0,
        }]),
    );
}

#[test]
fn expands_a_bounded_weekly_rule_preserving_its_local_clock_and_interval() {
    let result = resolve(
        &schedule(json!({"clauses": [{
            "time": {"start": {"hour": 20, "minute": 0}, "end": {"hour": 22, "minute": 0}},
            "recurrence": {"freq": "weekly", "interval": 2, "byDay": ["MO"], "count": 3},
        }]})),
        &dhaka(),
    )
    .unwrap();
    assert_eq!(
        starts(&result),
        [
            "2026-09-14T20:00:00+06:00",
            "2026-09-28T20:00:00+06:00",
            "2026-10-12T20:00:00+06:00",
        ]
    );
}

#[test]
fn applies_recurring_weekday_exclusions_and_an_inclusive_local_end_date() {
    let result = resolve(
        &schedule(json!({"clauses": [{
            "time": {"start": {"hour": 9, "minute": 0}},
            "recurrence": {
                "freq": "daily",
                "interval": 1,
                "start": {"kind": "calendar", "year": 2026, "month": 9, "day": 10},
                "until": {"kind": "calendar", "year": 2026, "month": 9, "day": 14},
                "except": [{"kind": "dayGroup", "group": "weekend"}],
            },
        }]})),
        &dhaka(),
    )
    .unwrap();
    assert_eq!(
        starts(&result),
        [
            "2026-09-10T09:00:00+06:00",
            "2026-09-11T09:00:00+06:00",
            "2026-09-14T09:00:00+06:00",
        ]
    );
}

#[test]
fn preserves_count_across_past_starts_and_skips_dst_gaps_without_consuming_it() {
    let result = resolve(
        &schedule(json!({"clauses": [{
            "time": {"start": {"hour": 2, "minute": 30}},
            "recurrence": {
                "freq": "daily",
                "interval": 1,
                "count": 2,
                "start": {"kind": "calendar", "year": 2026, "month": 3, "day": 7},
            },
        }]})),
        &ParseContext {
            reference: "2026-03-07T12:00:00-05:00".into(),
            time_zone: "America/New_York".into(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(starts(&result), ["2026-03-09T02:30:00-04:00"]);
}

#[test]
fn distinguishes_calendar_days_from_elapsed_hours_across_dst_and_clamps_months() {
    let context = ParseContext {
        reference: "2026-03-07T12:00:00-05:00".into(),
        time_zone: "America/New_York".into(),
        ..Default::default()
    };
    let day = resolve(
        &schedule(json!({"clauses": [
            {"shift": {"amount": 1, "unit": "day", "direction": "after"}}
        ]})),
        &context,
    )
    .unwrap();
    assert_eq!(day.occurrences[0].start, "2026-03-08T12:00:00-04:00");
    let hours = resolve(
        &schedule(json!({"clauses": [
            {"shift": {"amount": 24, "unit": "hour", "direction": "after"}}
        ]})),
        &context,
    )
    .unwrap();
    assert_eq!(hours.occurrences[0].start, "2026-03-08T13:00:00-04:00");

    let month = resolve(
        &schedule(json!({"clauses": [
            {"shift": {"amount": 1, "unit": "month", "direction": "after"}}
        ]})),
        &ParseContext {
            reference: "2026-01-31T12:00:00+06:00".into(),
            ..dhaka()
        },
    )
    .unwrap();
    assert_eq!(month.occurrences[0].start, "2026-02-28T12:00:00+06:00");
}

#[test]
fn skips_missing_monthly_dates_and_selects_ordinal_weekdays_within_each_month() {
    let month_end = resolve(
        &schedule(json!({"clauses": [{
            "time": {"start": {"named": "noon"}},
            "recurrence": {"freq": "monthly", "interval": 1, "byMonthDay": [31], "count": 3},
        }]})),
        &dhaka(),
    )
    .unwrap();
    assert_eq!(
        start_dates(&month_end),
        ["2026-10-31", "2026-12-31", "2027-01-31"]
    );

    let last_friday = resolve(
        &schedule(json!({"clauses": [{
            "time": {"start": {"named": "noon"}},
            "recurrence": {
                "freq": "monthly",
                "interval": 1,
                "byDay": ["FR"],
                "bySetPos": [-1],
                "count": 3,
            },
        }]})),
        &dhaka(),
    )
    .unwrap();
    assert_eq!(
        start_dates(&last_friday),
        ["2026-09-25", "2026-10-30", "2026-11-27"]
    );
}

#[test]
fn uses_month_day_fields_for_yearly_rules_and_skips_non_leap_years() {
    let march = resolve(
        &schedule(json!({"clauses": [{
            "time": {"start": {"named": "noon"}},
            "recurrence": {
                "freq": "yearly",
                "interval": 2,
                "byMonth": [3],
                "byMonthDay": [26],
                "count": 2,
            },
        }]})),
        &ParseContext {
            until: Some("2031-01-01T00:00:00Z".into()),
            ..dhaka()
        },
    )
    .unwrap();
    assert_eq!(start_dates(&march), ["2027-03-26", "2029-03-26"]);

    let leap = resolve(
        &schedule(json!({"clauses": [{
            "recurrence": {
                "freq": "yearly",
                "interval": 1,
                "byMonth": [2],
                "byMonthDay": [29],
                "count": 2,
            },
        }]})),
        &ParseContext {
            until: Some("2035-01-01T00:00:00Z".into()),
            ..dhaka()
        },
    )
    .unwrap();
    assert_eq!(start_dates(&leap), ["2028-02-29", "2032-02-29"]);
}

#[test]
fn expands_hourly_recurrence_as_timed_events_across_a_dst_gap() {
    let result = resolve(
        &schedule(json!({"clauses": [
            {"recurrence": {"freq": "hourly", "interval": 1, "count": 4}}
        ]})),
        &ParseContext {
            reference: "2026-03-08T00:30:00-05:00".into(),
            time_zone: "America/New_York".into(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        starts(&result),
        [
            "2026-03-08T00:30:00-05:00",
            "2026-03-08T01:30:00-05:00",
            "2026-03-08T03:30:00-04:00",
            "2026-03-08T04:30:00-04:00",
        ]
    );
    assert!(result.occurrences.iter().all(|value| !value.all_day));
}

#[test]
fn spreads_unspecified_frequency_counts_with_an_approximation_diagnostic() {
    let weekly = resolve(
        &schedule(json!({"clauses": [
            {"recurrence": {"freq": "weekly", "interval": 1, "timesPer": 2, "count": 4}}
        ]})),
        &dhaka(),
    )
    .unwrap();
    assert_eq!(
        start_dates(&weekly),
        ["2026-09-10", "2026-09-14", "2026-09-17", "2026-09-21"]
    );
    assert!(
        weekly
            .diagnostics
            .iter()
            .any(|value| value.code == "times-per-approximated")
    );

    let daily = resolve(
        &schedule(json!({"clauses": [
            {"recurrence": {"freq": "daily", "interval": 1, "timesPer": 3, "count": 4}}
        ]})),
        &dhaka(),
    )
    .unwrap();
    assert_eq!(
        starts(&daily),
        [
            "2026-09-09T16:00:00+06:00",
            "2026-09-10T00:00:00+06:00",
            "2026-09-10T08:00:00+06:00",
            "2026-09-10T16:00:00+06:00",
        ]
    );
}

#[test]
fn honors_explicit_preview_bounds_for_both_recurrence_and_one_off_dates() {
    let source = json!({"clauses": [{
        "recurrence": {"freq": "daily", "interval": 1},
        "time": {"start": {"named": "noon"}},
    }]});
    let result = resolve(
        &schedule(source.clone()),
        &ParseContext {
            until: Some("2026-09-11".into()),
            limit: Some(10),
            ..dhaka()
        },
    )
    .unwrap();
    assert_eq!(result.occurrences.len(), 2);
    assert!(!result.truncated);
    assert!(
        resolve(
            &schedule(source.clone()),
            &ParseContext {
                limit: Some(2),
                ..dhaka()
            },
        )
        .unwrap()
        .truncated
    );
    for bad_limit in [0, 1001] {
        assert!(
            resolve(
                &schedule(source.clone()),
                &ParseContext {
                    limit: Some(bad_limit),
                    ..dhaka()
                },
            )
            .unwrap_err()
            .contains("limit"),
            "limit {bad_limit}"
        );
    }

    let future = resolve(
        &schedule(json!({"clauses": [
            {"date": {"kind": "calendar", "year": 2040, "month": 1, "day": 1}}
        ]})),
        &ParseContext {
            until: Some("2027-09-09".into()),
            ..dhaka()
        },
    )
    .unwrap();
    assert!(future.occurrences.is_empty());
}

#[test]
fn limits_a_recurrence_by_a_calendar_duration_and_warns_about_a_month_only_end() {
    let bounded = resolve(
        &schedule(json!({"clauses": [{
            "recurrence": {
                "freq": "weekly",
                "interval": 1,
                "byDay": ["MO"],
                "span": {"amount": 3, "unit": "week"},
            },
            "time": {"start": {"named": "noon"}},
        }]})),
        &dhaka(),
    )
    .unwrap();
    assert_eq!(
        start_dates(&bounded),
        ["2026-09-14", "2026-09-21", "2026-09-28"]
    );

    let month_end = resolve(
        &schedule(json!({"clauses": [{
            "recurrence": {
                "freq": "weekly",
                "interval": 2,
                "byDay": ["TU"],
                "until": {"kind": "calendar", "month": 12},
            },
        }]})),
        &dhaka(),
    )
    .unwrap();
    assert!(
        month_end
            .diagnostics
            .iter()
            .any(|value| value.code == "until-month-only")
    );
}

#[test]
fn resolves_standalone_clocks_in_the_future_and_relative_hour_periods_precisely() {
    let clock = resolve(
        &schedule(json!({"clauses": [{"time": {"start": {"hour": 9, "minute": 0}}}]})),
        &dhaka(),
    )
    .unwrap();
    assert_eq!(clock.occurrences[0].start, "2026-09-10T09:00:00+06:00");

    let hour = resolve(
        &schedule(json!({"clauses": [
            {"date": {"kind": "relativeUnit", "unit": "hour", "modifier": "next"}}
        ]})),
        &dhaka(),
    )
    .unwrap();
    assert_eq!(
        occurrences(&hour),
        json!([{
            "start": "2026-09-09T13:00:00+06:00",
            "end": "2026-09-09T14:00:00+06:00",
            "allDay": false,
            "clause": 0,
        }]),
    );
}

#[test]
fn still_rejects_an_empty_hourly_recurrence_clock_window() {
    assert!(
        resolve(
            &schedule(json!({"clauses": [{
                "recurrence": {"freq": "hourly", "interval": 1},
                "time": {"start": {"hour": 14, "minute": 0}, "end": {"hour": 14, "minute": 0}},
            }]})),
            &dhaka(),
        )
        .is_err()
    );
}

#[test]
fn leaves_an_open_upper_bound_without_an_end() {
    let result = resolve(
        &schedule(json!({"clauses": [
            {"time": {"start": {"hour": 18, "minute": 0}, "open": "end"}}
        ]})),
        &ParseContext {
            reference: "2026-09-11T10:00:00Z".into(),
            time_zone: "UTC".into(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        occurrences(&result),
        json!([{
            "start": "2026-09-11T18:00:00+00:00",
            "open": "end",
            "allDay": false,
            "clause": 0,
        }]),
    );
}

#[test]
fn anchors_an_open_lower_bound_on_the_day_its_named_edge_falls_in() {
    let result = resolve(
        &schedule(json!({"clauses": [{
            "time": {
                "start": {"hour": 0, "minute": 0},
                "end": {"hour": 18, "minute": 0},
                "open": "start",
            },
        }]})),
        &ParseContext {
            reference: "2026-09-11T10:00:00Z".into(),
            time_zone: "UTC".into(),
            ..Default::default()
        },
    )
    .unwrap();
    // Midnight is always behind the reference; searching on it lands tomorrow.
    assert_eq!(
        occurrences(&result),
        json!([{
            "start": "2026-09-11T00:00:00+00:00",
            "end": "2026-09-11T18:00:00+00:00",
            "open": "start",
            "allDay": false,
            "clause": 0,
        }]),
    );
}

#[test]
fn omits_the_open_field_for_a_fully_bounded_occurrence() {
    let result = resolve(
        &schedule(json!({"clauses": [
            {"time": {"start": {"hour": 8, "minute": 0}, "end": {"hour": 10, "minute": 0}}}
        ]})),
        &ParseContext {
            reference: "2026-09-11T10:00:00Z".into(),
            time_zone: "UTC".into(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        occurrences(&result),
        json!([{
            "start": "2026-09-12T08:00:00+00:00",
            "end": "2026-09-12T10:00:00+00:00",
            "allDay": false,
            "clause": 0,
        }]),
    );
}

#[test]
fn anchors_a_calendar_day_to_a_deictic_year_or_month() {
    // Reference 2026-09-09 (Wednesday), Asia/Dhaka.
    let ctx = dhaka();
    let year_anchored = resolve(
        &schedule(json!({"clauses": [{
            "date": {"kind": "periodCalendar", "period": "year", "modifier": "last", "month": 8, "day": 24},
        }]})),
        &ctx,
    )
    .unwrap();
    assert_eq!(start_dates(&year_anchored), vec!["2025-08-24"]);

    let month_anchored = resolve(
        &schedule(json!({"clauses": [{
            "date": {"kind": "periodCalendar", "period": "month", "modifier": "last", "day": 15},
        }]})),
        &ctx,
    )
    .unwrap();
    assert_eq!(start_dates(&month_anchored), vec!["2026-08-15"]);

    // A named month must agree with the anchored month.
    let mismatch = resolve(
        &schedule(json!({"clauses": [{
            "date": {"kind": "periodCalendar", "period": "month", "modifier": "last", "month": 3, "day": 15},
        }]})),
        &ctx,
    );
    assert!(mismatch.is_err());
}

#[test]
fn anchors_a_weekday_to_a_deictic_week() {
    // Reference 2026-09-09 (Wednesday); last week is Aug 31 – Sep 6.
    let ctx = dhaka();
    let result = resolve(
        &schedule(json!({"clauses": [{
            "date": {"kind": "periodWeekday", "modifier": "last", "days": ["FR"]},
        }]})),
        &ctx,
    )
    .unwrap();
    assert_eq!(start_dates(&result), vec!["2026-09-04"]);

    let next = resolve(
        &schedule(json!({"clauses": [{
            "date": {"kind": "periodWeekday", "modifier": "next", "days": ["MO"]},
        }]})),
        &ctx,
    )
    .unwrap();
    assert_eq!(start_dates(&next), vec!["2026-09-14"]);
}
