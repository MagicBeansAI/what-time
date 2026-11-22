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
    // The preview is on the same wall-clock grid cron fires on.
    assert_eq!(
        starts(&quarter, 3),
        ["2026-11-22T15:30", "2026-11-22T15:45", "2026-11-22T16:00"]
    );
    let six = resolved("every 6 hours", &["RECUR", "NUM", "UNIT"]);
    assert_eq!(six.crons, ["0 */6 * * *"]);
    assert!(six.rrules[0].ends_with("RRULE:FREQ=HOURLY;INTERVAL=6"));
    assert_eq!(
        starts(&six, 3),
        ["2026-11-22T18:00", "2026-11-23T00:00", "2026-11-23T06:00"]
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

// ---------------------------------------------------------------- cron in

fn read_cron(expression: &str) -> what_time::CronResult {
    what_time::parse_cron(expression, &context())
        .unwrap_or_else(|error| panic!("{expression}: {error}"))
}

#[test]
fn reads_cron_back_into_a_described_schedule() {
    let window = read_cron("*/15 9-17 * * 1-5");
    assert_eq!(
        window.description,
        "Every 15 minutes from 9:00 AM to 5:45 PM on weekdays"
    );
    // Normalized by the exporter: numbers become names.
    assert_eq!(window.crons, ["*/15 9-17 * * MON-FRI"]);
    // Sunday 15:30: the next weekday window opens Monday 09:00.
    assert_eq!(window.occurrences[0].start, "2026-11-23T09:00:00+05:30");
    assert!(window.rrules[0].contains("BYDAY=MO,TU,WE,TH,FR"));

    let macro_line = read_cron("@weekly");
    assert_eq!(macro_line.description, "At midnight every Sunday");
    assert_eq!(macro_line.crons, ["0 0 * * SUN"]);
}

#[test]
fn applies_the_either_day_rule_without_duplicates() {
    // The 1st of the month OR any Monday; Mon 2027-02-01 matches both.
    let either = what_time::parse_cron(
        "0 9 1 * MON",
        &ParseContext {
            reference: "2027-01-24T00:00:00Z".into(),
            time_zone: "Asia/Kolkata".into(),
            limit: Some(4),
            ..Default::default()
        },
    )
    .unwrap();
    let starts: Vec<&str> = either.occurrences.iter().map(|o| &o.start[..10]).collect();
    assert_eq!(
        starts,
        ["2027-01-25", "2027-02-01", "2027-02-08", "2027-02-15"]
    );
    assert_eq!(either.schedule.clauses.len(), 2);
}

#[test]
fn reports_a_line_that_never_fires() {
    let never = read_cron("0 0 30 2 *");
    assert!(never.occurrences.is_empty());
    assert_eq!(never.description, "At midnight on the 30th of February");
    assert!(never.diagnostics.iter().any(|d| d.code == "never-fires"));
}

#[test]
fn rejects_invalid_cron_with_a_reason() {
    let error = what_time::parse_cron("0 0 9 * * *", &context()).unwrap_err();
    assert!(error.to_string().contains("five fields"), "{error}");
    assert_eq!(
        what_time::describe_cron("*/15 * * * *").unwrap(),
        "Every 15 minutes"
    );
}

#[test]
fn round_trips_what_the_exporter_writes() {
    for line in [
        "0 9 * * MON-FRI",
        "*/15 9-17 * * MON-FRI",
        "0 18 * * MON,WED,FRI",
        "0 0 1,15 * *",
        "30 2 * * *",
        "0 */6 * * *",
        "0 8,13,20 * * *",
        "0 0 25 12 *",
        "0 9-17/2 * * *",
        "0 0 1 2-11/3 *",
    ] {
        assert_eq!(read_cron(line).crons, [line], "{line}");
    }
}

// ------------------------------------------------- gaps closed in 0.4.1

#[test]
fn removes_excepted_clocks_by_splitting_the_window() {
    let result = resolved(
        "every hour except 1pm",
        &["RECUR", "UNIT", "EXCEPT", "HOUR", "MERIDIEM"],
    );
    assert_eq!(result.crons, ["0 0-12,14-23 * * *"]);
    assert!(!result.occurrences.iter().any(|o| o.start.contains("T13:")));
    // An excepted range is half-open: 2pm itself fires again.
    let lunch = resolved(
        "every hour on weekdays except between 12 and 2pm",
        &[
            "RECUR",
            "UNIT",
            "GLUE",
            "DAYGROUP",
            "EXCEPT",
            "RANGE_START",
            "HOUR",
            "RANGE_END",
            "HOUR",
            "MERIDIEM",
        ],
    );
    assert_eq!(lunch.crons, ["0 0-11,14-23 * * MON-FRI"]);
    // Days after the exception select the rule's days, not the exception's.
    let trailing = resolved(
        "every hour except between 12 and 2pm on weekdays",
        &[
            "RECUR",
            "UNIT",
            "EXCEPT",
            "RANGE_START",
            "HOUR",
            "RANGE_END",
            "HOUR",
            "MERIDIEM",
            "GLUE",
            "DAYGROUP",
        ],
    );
    assert_eq!(trailing.crons, lunch.crons);
}

#[test]
fn ends_a_series_at_the_bound_clock() {
    let result = resolved(
        "every 2 hours until 8pm today",
        &[
            "RECUR",
            "NUM",
            "UNIT",
            "BOUND_END",
            "HOUR",
            "MERIDIEM",
            "REL_DAY",
        ],
    );
    assert_eq!(
        starts(&result, 9),
        ["2026-11-22T16:00", "2026-11-22T18:00", "2026-11-22T20:00"]
    );
    assert!(codes(&result).contains(&"cron-unbounded"));
    assert!(result.rrules[0].contains("UNTIL=20261122T143000Z"));
    // Without a date the bound is today's clock.
    let today = resolved(
        "every 15 minutes until 6pm",
        &["RECUR", "NUM", "UNIT", "BOUND_END", "HOUR", "MERIDIEM"],
    );
    assert_eq!(
        today.occurrences.last().map(|o| &o.start[..16]),
        Some("2026-11-22T18:00")
    );
}

#[test]
fn resolves_a_one_off_first_or_last_day_of_a_period() {
    let last = resolved(
        "on the last day of the month",
        &["GLUE", "GLUE", "ORD", "UNIT", "GLUE", "GLUE", "UNIT"],
    );
    assert_eq!(starts(&last, 2), ["2026-11-30T00:00"]);
    let first = resolved(
        "the first day of next month at 9am",
        &[
            "GLUE", "ORD", "UNIT", "GLUE", "DEICTIC", "UNIT", "GLUE", "HOUR", "MERIDIEM",
        ],
    );
    assert_eq!(starts(&first, 2), ["2026-12-01T09:00"]);
}

#[test]
fn rejoins_a_clock_split_off_by_an_event_word() {
    let text = "every night before bed at 10:30";
    let tokens = oracle(
        text,
        &[
            "RECUR",
            "DAYPART",
            "DIR_BEFORE",
            "O",
            "GLUE",
            "HOUR",
            "GLUE",
            "MINUTE",
        ],
    );
    let expressions = compile_predictions(text, &tokens, DateOrder::MDY);
    assert_eq!(expressions.len(), 1, "{expressions:?}");
    let result = resolve(expressions[0].schedule.as_ref().unwrap(), &context()).unwrap();
    assert_eq!(result.crons, ["30 22 * * *"]);
}

#[test]
fn reads_this_weekday_as_the_coming_one() {
    // Sunday 2026-11-22: "this Saturday" is the 28th, not yesterday.
    let saturday = resolved(
        "this Saturday at 11am",
        &["DEICTIC", "WEEKDAY", "GLUE", "HOUR", "MERIDIEM"],
    );
    assert_eq!(starts(&saturday, 1), ["2026-11-28T11:00"]);
    let monday = resolved("this Monday", &["DEICTIC", "WEEKDAY"]);
    assert_eq!(starts(&monday, 1), ["2026-11-23T00:00"]);
}
