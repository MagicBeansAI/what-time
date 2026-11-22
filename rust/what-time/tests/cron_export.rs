//! Cron export and the recurrence grammar behind it, from explicit labels:
//! the cases hand-label tokens (the oracle interface), so they pin the
//! compiler, resolver and exporters independently of the model.

use what_time::testing::{Role, Token, compile_predictions, tokenize};
use what_time::{DateOrder, ParseContext, Resolved, Schedule, resolve};

/// Assigns `labels` to the non-whitespace tokens of `text`.
fn oracle(text: &str, labels: &[&str]) -> Vec<Token> {
    let mut index = 0;
    tokenize(text)
        .into_iter()
        .map(|raw| {
            let label = if raw.kind == 3 {
                Role::O
            } else {
                let name = labels.get(index).copied().unwrap_or("O");
                index += 1;
                Role::from_name(name).unwrap_or_else(|| panic!("unknown label {name}"))
            };
            Token {
                raw,
                label,
                clause_start: false,
                score: 1.0,
            }
        })
        .collect()
}

/// Sunday 2026-11-22, 15:30 in Kolkata.
fn context() -> ParseContext {
    ParseContext {
        reference: "2026-11-22T10:00:00Z".into(),
        time_zone: "Asia/Kolkata".into(),
        limit: Some(40),
        ..Default::default()
    }
}

fn schedule(text: &str, labels: &[&str]) -> Schedule {
    let expressions = compile_predictions(text, &oracle(text, labels), DateOrder::MDY);
    assert_eq!(expressions.len(), 1, "{text}: {expressions:?}");
    expressions[0]
        .schedule
        .clone()
        .unwrap_or_else(|| panic!("{text}: {:?}", expressions[0].diagnostics))
}

fn resolved(text: &str, labels: &[&str]) -> Resolved {
    resolve(&schedule(text, labels), &context()).unwrap_or_else(|error| panic!("{text}: {error}"))
}

fn crons(text: &str, labels: &[&str]) -> Vec<String> {
    resolved(text, labels).crons
}

fn rules(text: &str, labels: &[&str]) -> Vec<String> {
    resolved(text, labels)
        .rrules
        .iter()
        .map(|rule| rule.lines().last().unwrap().to_string())
        .collect()
}

fn starts(result: &Resolved, count: usize) -> Vec<&str> {
    result
        .occurrences
        .iter()
        .take(count)
        .map(|occurrence| &occurrence.start[..16])
        .collect()
}

fn codes(result: &Resolved) -> Vec<&str> {
    result
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.as_str())
        .collect()
}

#[test]
fn exports_daily_weekly_and_monthly_rules() {
    assert_eq!(
        crons(
            "every day at 9am",
            &["RECUR", "UNIT", "GLUE", "HOUR", "MERIDIEM"]
        ),
        ["0 9 * * *"]
    );
    assert_eq!(
        crons(
            "every weekday at 8:30am",
            &[
                "RECUR", "DAYGROUP", "GLUE", "HOUR", "GLUE", "MINUTE", "MERIDIEM"
            ]
        ),
        ["30 8 * * MON-FRI"]
    );
    assert_eq!(
        crons(
            "every mon, wed, fri at 7am",
            &[
                "RECUR", "WEEKDAY", "JOIN", "WEEKDAY", "JOIN", "WEEKDAY", "GLUE", "HOUR",
                "MERIDIEM"
            ]
        ),
        ["0 7 * * MON,WED,FRI"]
    );
    assert_eq!(
        crons(
            "1st and 15th of every month",
            &[
                "DOM", "GLUE", "GLUE", "DOM", "GLUE", "GLUE", "RECUR", "UNIT"
            ]
        ),
        ["0 0 1,15 * *"]
    );
    assert_eq!(
        crons(
            "every year on march 3",
            &["RECUR", "UNIT", "GLUE", "MONTH", "DOM"]
        ),
        ["0 0 3 3 *"]
    );
    // An all-day weekend rule fires at midnight; Sunday stays out of ranges.
    assert_eq!(
        crons("every weekend", &["RECUR", "DAYGROUP"]),
        ["0 0 * * SAT,SUN"]
    );
}

#[test]
fn exports_sub_hourly_steps_on_clock_boundaries() {
    let quarter = resolved("every 15 minutes", &["RECUR", "NUM", "UNIT"]);
    assert_eq!(quarter.crons, ["*/15 * * * *"]);
    assert!(quarter.rrules[0].ends_with("RRULE:FREQ=MINUTELY;INTERVAL=15"));
    // The preview counts from the reference instant.
    assert_eq!(
        starts(&quarter, 3),
        ["2026-11-22T15:30", "2026-11-22T15:45", "2026-11-22T16:00"]
    );
    assert_eq!(crons("every minute", &["RECUR", "UNIT"]), ["* * * * *"]);
    assert_eq!(
        crons("every half hour", &["RECUR", "NUM", "UNIT"]),
        ["*/30 * * * *"]
    );
    assert_eq!(
        crons("every half an hour", &["RECUR", "NUM", "GLUE", "UNIT"]),
        ["*/30 * * * *"]
    );
    assert_eq!(
        crons("every half an hour", &["RECUR", "NUM", "NUM", "UNIT"]),
        ["*/30 * * * *"]
    );
    assert_eq!(
        crons("every 6 hours", &["RECUR", "NUM", "UNIT"]),
        ["0 */6 * * *"]
    );
    assert_eq!(crons("hourly", &["FREQ"]), ["0 * * * *"]);
    // 90 minutes divides the day, not the hour: two exact lines.
    assert_eq!(
        crons("every 90 minutes", &["RECUR", "NUM", "UNIT"]),
        ["0 */3 * * *", "30 1-22/3 * * *"]
    );
}

#[test]
fn refuses_rules_cron_cannot_state_exactly() {
    for (text, labels) in [
        (
            "every 2 weeks on friday",
            vec!["RECUR", "NUM", "UNIT", "GLUE", "WEEKDAY"],
        ),
        ("every other day", vec!["RECUR", "NUM", "UNIT"]),
        ("every 7 minutes", vec!["RECUR", "NUM", "UNIT"]),
        ("every 5 hours", vec!["RECUR", "NUM", "UNIT"]),
        (
            "first monday of every month",
            vec!["ORD", "WEEKDAY", "GLUE", "RECUR", "UNIT"],
        ),
        (
            "last day of every month",
            vec!["ORD", "UNIT", "GLUE", "RECUR", "UNIT"],
        ),
    ] {
        let result = resolved(text, &labels);
        assert!(result.crons.is_empty(), "{text}: {:?}", result.crons);
        assert_eq!(result.rrules.len(), 1, "{text}");
        assert!(codes(&result).contains(&"unsupported-cron"), "{text}");
    }
}

#[test]
fn warns_when_cron_drops_a_bound() {
    let result = resolved(
        "every day until december 31",
        &["RECUR", "UNIT", "BOUND_END", "MONTH", "DOM"],
    );
    assert_eq!(result.crons, ["0 0 * * *"]);
    assert!(codes(&result).contains(&"cron-unbounded"));
    let unbounded = resolved(
        "every day at 9am",
        &["RECUR", "UNIT", "GLUE", "HOUR", "MERIDIEM"],
    );
    assert!(!codes(&unbounded).contains(&"cron-unbounded"));
}

#[test]
fn expands_a_sub_daily_rule_inside_its_clock_window() {
    let labels = [
        "RECUR",
        "NUM",
        "UNIT",
        "RANGE_START",
        "HOUR",
        "MERIDIEM",
        "RANGE_END",
        "HOUR",
        "MERIDIEM",
    ];
    let result = resolved("every 2 hours from 9am to 5pm", &labels);
    // Both window ends fire; the day after the reference (15:30) starts over.
    assert_eq!(
        starts(&result, 4),
        [
            "2026-11-22T17:00",
            "2026-11-23T09:00",
            "2026-11-23T11:00",
            "2026-11-23T13:00"
        ]
    );
    assert!(result.occurrences.iter().all(|value| value.end.is_none()));
    assert_eq!(result.crons, ["0 9-17/2 * * *"]);
    assert_eq!(
        rules("every 2 hours from 9am to 5pm", &labels),
        ["RRULE:FREQ=DAILY;INTERVAL=1;BYHOUR=9,11,13,15,17;BYMINUTE=0"]
    );
}

#[test]
fn splits_a_window_whose_last_slot_stands_alone() {
    let labels = [
        "RECUR",
        "NUM",
        "UNIT",
        "RANGE_START",
        "HOUR",
        "MERIDIEM",
        "RANGE_END",
        "HOUR",
        "MERIDIEM",
        "GLUE",
        "DAYGROUP",
    ];
    let text = "every 15 minutes between 9am and 6pm on weekdays";
    let result = resolved(text, &labels);
    assert_eq!(result.crons, ["*/15 9-17 * * MON-FRI", "0 18 * * MON-FRI"]);
    assert_eq!(
        rules(text, &labels),
        [
            "RRULE:FREQ=DAILY;INTERVAL=1;BYDAY=MO,TU,WE,TH,FR;BYHOUR=9,10,11,12,13,14,15,16,17;BYMINUTE=0,15,30,45",
            "RRULE:FREQ=DAILY;INTERVAL=1;BYDAY=MO,TU,WE,TH,FR;BYHOUR=18;BYMINUTE=0",
        ]
    );
    // Sunday is skipped: the first slot is Monday 09:00.
    assert_eq!(starts(&result, 2), ["2026-11-23T09:00", "2026-11-23T09:15"]);
    let monday: Vec<&str> = result
        .occurrences
        .iter()
        .map(|value| &value.start[..16])
        .filter(|start| start.starts_with("2026-11-23"))
        .collect();
    assert_eq!(monday.len(), 37);
    assert_eq!(monday.last(), Some(&"2026-11-23T18:00"));
}

#[test]
fn treats_a_day_part_as_the_window_of_a_sub_daily_rule() {
    let result = resolved(
        "every hour in the evening",
        &["RECUR", "UNIT", "GLUE", "GLUE", "DAYPART"],
    );
    assert_eq!(result.crons, ["0 17-21 * * *"]);
    assert_eq!(
        starts(&result, 3),
        ["2026-11-22T17:00", "2026-11-22T18:00", "2026-11-22T19:00"]
    );
}

#[test]
fn bounds_a_sub_daily_series_by_its_duration() {
    let result = resolved(
        "every 15 minutes for 1 hour",
        &["RECUR", "NUM", "UNIT", "DUR", "NUM", "UNIT"],
    );
    assert_eq!(
        starts(&result, 9),
        [
            "2026-11-22T15:30",
            "2026-11-22T15:45",
            "2026-11-22T16:00",
            "2026-11-22T16:15"
        ]
    );
}

#[test]
fn merges_listed_clocks_and_keeps_independent_rules_apart() {
    assert_eq!(
        crons(
            "every day at 8am, 1pm and 8pm",
            &[
                "RECUR", "UNIT", "GLUE", "HOUR", "MERIDIEM", "GLUE", "HOUR", "MERIDIEM", "JOIN",
                "HOUR", "MERIDIEM"
            ]
        ),
        ["0 8,13,20 * * *"]
    );
    let text = "every tuesday at 3pm and every friday at 5:30pm";
    let labels = [
        "RECUR", "WEEKDAY", "GLUE", "HOUR", "MERIDIEM", "JOIN", "RECUR", "WEEKDAY", "GLUE", "HOUR",
        "GLUE", "MINUTE", "MERIDIEM",
    ];
    assert_eq!(schedule(text, &labels).clauses.len(), 2);
    assert_eq!(crons(text, &labels), ["0 15 * * TUE", "30 17 * * FRI"]);
    // The listed clocks are the count.
    assert_eq!(
        crons(
            "twice a day at 9am and 9pm",
            &[
                "TIMES", "GLUE", "UNIT", "GLUE", "HOUR", "MERIDIEM", "JOIN", "HOUR", "MERIDIEM"
            ]
        ),
        ["0 9,21 * * *"]
    );
}

#[test]
fn reads_ordinals_before_a_period_as_a_day_or_a_step() {
    assert_eq!(
        rules(
            "last day of every month",
            &["ORD", "UNIT", "GLUE", "RECUR", "UNIT"]
        ),
        ["RRULE:FREQ=MONTHLY;INTERVAL=1;BYMONTHDAY=-1"]
    );
    assert_eq!(
        rules(
            "last working day of every month",
            &["ORD", "DAYGROUP", "DAYGROUP", "GLUE", "RECUR", "UNIT"]
        ),
        ["RRULE:FREQ=MONTHLY;INTERVAL=1;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1"]
    );
    assert_eq!(
        rules("every third day", &["RECUR", "ORD", "UNIT"]),
        ["RRULE:FREQ=DAILY;INTERVAL=3"]
    );
    assert_eq!(
        rules("har doosre hafte", &["RECUR", "ORD", "UNIT"]),
        ["RRULE:FREQ=WEEKLY;INTERVAL=2"]
    );
    // A small ordinal before a weekday counts within the month.
    for labels in [
        ["RECUR", "DOM", "GLUE", "WEEKDAY"],
        ["RECUR", "ORD", "GLUE", "WEEKDAY"],
    ] {
        assert_eq!(
            rules("every 2nd saturday", &labels),
            ["RRULE:FREQ=MONTHLY;INTERVAL=1;BYDAY=SA;BYSETPOS=2"]
        );
    }
    // The briefs label "other" GLUE, the generators NUM: both step by two.
    for labels in [["RECUR", "GLUE", "WEEKDAY"], ["RECUR", "NUM", "WEEKDAY"]] {
        assert_eq!(
            rules("every other friday", &labels),
            ["RRULE:FREQ=WEEKLY;INTERVAL=2;BYDAY=FR"]
        );
    }
    assert_eq!(
        rules("every other week", &["RECUR", "GLUE", "UNIT"]),
        ["RRULE:FREQ=WEEKLY;INTERVAL=2"]
    );
    assert_eq!(
        rules("alternate saturdays", &["RECUR", "WEEKDAY"]),
        ["RRULE:FREQ=WEEKLY;INTERVAL=2;BYDAY=SA"]
    );
    assert_eq!(
        rules("every alternate day", &["RECUR", "NUM", "UNIT"]),
        ["RRULE:FREQ=DAILY;INTERVAL=2"]
    );
}

#[test]
fn infers_the_frequency_a_bare_selector_implies() {
    assert_eq!(
        rules("every january", &["RECUR", "MONTH"]),
        ["RRULE:FREQ=YEARLY;INTERVAL=1;BYMONTH=1"]
    );
    assert_eq!(
        rules("every 15th", &["RECUR", "DOM", "GLUE"]),
        ["RRULE:FREQ=MONTHLY;INTERVAL=1;BYMONTHDAY=15"]
    );
    // A weekday keeps the weekly reading.
    assert_eq!(
        rules(
            "every monday in january",
            &["RECUR", "WEEKDAY", "GLUE", "MONTH"]
        ),
        ["RRULE:FREQ=WEEKLY;INTERVAL=1;BYDAY=MO;BYMONTH=1"]
    );
}

#[test]
fn reads_frequency_counts_in_either_order() {
    let weekly = ["RRULE:FREQ=WEEKLY;INTERVAL=1;BYDAY=MO,WE,FR"];
    assert_eq!(
        rules("three times a week", &["NUM", "TIMES", "GLUE", "UNIT"]),
        weekly
    );
    assert_eq!(
        rules("hafte mein teen baar", &["UNIT", "GLUE", "NUM", "TIMES"]),
        weekly
    );
    assert_eq!(
        rules("twice daily", &["TIMES", "FREQ"]),
        ["RRULE:FREQ=DAILY;INTERVAL=1;BYHOUR=0,12"]
    );
    assert_eq!(
        rules("once a month", &["TIMES", "GLUE", "UNIT"]),
        ["RRULE:FREQ=MONTHLY;INTERVAL=1"]
    );
    assert_eq!(
        rules("twice a month", &["TIMES", "GLUE", "UNIT"]),
        ["RRULE:FREQ=MONTHLY;INTERVAL=1;BYMONTHDAY=1,15"]
    );
    assert_eq!(
        rules("twice a year", &["TIMES", "GLUE", "UNIT"]),
        ["RRULE:FREQ=YEARLY;INTERVAL=1;BYMONTH=1,7;BYMONTHDAY=1"]
    );
}

#[test]
fn keeps_a_clock_that_follows_an_exception() {
    let result = resolved(
        "weekdays except wednesday at 9am",
        &["DAYGROUP", "EXCEPT", "WEEKDAY", "GLUE", "HOUR", "MERIDIEM"],
    );
    assert_eq!(result.crons, ["0 9 * * MON,TUE,THU,FRI"]);
    assert_eq!(starts(&result, 1), ["2026-11-23T09:00"]);
}

#[test]
fn reads_night_clocks_past_midnight() {
    assert_eq!(
        crons(
            "run at 2:30 every night",
            &["O", "GLUE", "HOUR", "GLUE", "MINUTE", "RECUR", "DAYPART"]
        ),
        ["30 2 * * *"]
    );
    assert_eq!(
        crons("every night at 10", &["RECUR", "DAYPART", "GLUE", "HOUR"]),
        ["0 22 * * *"]
    );
    assert_eq!(
        crons("nightly at 2", &["FREQ", "GLUE", "HOUR"]),
        ["0 2 * * *"]
    );
    assert_eq!(
        crons(
            "har raat 12 baje",
            &["RECUR", "DAYPART", "HOUR", "MERIDIEM"]
        ),
        ["0 0 * * *"]
    );
}

#[test]
fn recurs_weekly_for_bare_plural_weekdays() {
    assert_eq!(
        crons(
            "mondays and thursdays at 7pm",
            &["WEEKDAY", "JOIN", "WEEKDAY", "GLUE", "HOUR", "MERIDIEM"]
        ),
        ["0 19 * * MON,THU"]
    );
    // The singular still names one date.
    let once = resolved("monday at 7pm", &["WEEKDAY", "GLUE", "HOUR", "MERIDIEM"]);
    assert!(once.crons.is_empty() && once.rrules.is_empty());
    assert_eq!(once.occurrences.len(), 1);
}
