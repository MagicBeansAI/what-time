//! Authored end-to-end regressions for the V2 brief, including terse inputs.
//! These are development cases, not an untouched accuracy benchmark.

use serde_json::json;
use what_time::{Schedule, ScheduleParser};

#[test]
fn resolves_v2_gap_phrases_with_the_exported_model() {
    let parser = ScheduleParser::new(Default::default());
    let cases = [
        (
            "15 tareekh ko",
            json!({"date": {"kind": "calendar", "day": 15}}),
        ),
        (
            "reminder ki 15 ko",
            json!({"date": {"kind": "calendar", "day": 15}}),
        ),
        ("१५ तारीख", json!({"date": {"kind": "calendar", "day": 15}})),
        (
            "agle mahine ki 20 tareek",
            json!({"date": {"kind": "periodCalendar", "period": "month", "modifier": "next", "day": 20}}),
        ),
        (
            "har mahine ki 5 tareekh",
            json!({"recurrence": {"freq": "monthly", "interval": 1, "byMonthDay": [5]}}),
        ),
        (
            "har roz subah 7 baje",
            json!({"time": {"start": {"hour": 7, "minute": 0}}, "recurrence": {"freq": "daily", "interval": 1}}),
        ),
        (
            "rozz subah 7 baje",
            json!({"time": {"start": {"hour": 7, "minute": 0}}, "recurrence": {"freq": "daily", "interval": 1}}),
        ),
        (
            "सवा ४",
            json!({"time": {"start": {"hour": 4, "minute": 15}}}),
        ),
        (
            "call for 10 mins",
            json!({"duration": {"amount": 10, "unit": "minute"}}),
        ),
        (
            "पूरे 30 मिनट",
            json!({"duration": {"amount": 30, "unit": "minute"}}),
        ),
        (
            "shaam ko 8 baje",
            json!({"time": {"start": {"hour": 20, "minute": 0}}}),
        ),
        (
            "shaam ko sava 8 baje",
            json!({"time": {"start": {"hour": 8, "minute": 15}}}),
        ),
        (
            "कल आया था",
            json!({"date": {"kind": "relativeDay", "offset": -1}}),
        ),
        (
            "kal aaya tha",
            json!({"date": {"kind": "relativeDay", "offset": -1}}),
        ),
        (
            "parso aana hai",
            json!({"date": {"kind": "relativeDay", "offset": 2}}),
        ),
        (
            "2mrw",
            json!({"date": {"kind": "relativeDay", "offset": 1}}),
        ),
        ("evng", json!({"time": {"start": {"part": "evening"}}})),
        (
            "EOD",
            json!({"date": {"kind": "relativeUnit", "unit": "day", "modifier": "this", "edge": "end"}}),
        ),
    ];
    let mut failures = Vec::new();
    for (text, clause) in cases {
        let result = parser.parse(text).unwrap();
        let actual = result.expressions.first().and_then(|e| e.schedule.as_ref());
        let expected: Schedule = serde_json::from_value(json!({"clauses": [clause]})).unwrap();
        if actual != Some(&expected) {
            failures.push(format!("{text:?}: expected {expected:?}, got {actual:?}"));
        }
    }
    for text in [
        "invoice 2024 balance 3500 pending",
        "movie was 3 hours long tbh",
    ] {
        let result = parser.parse(text).unwrap();
        if result.expressions.iter().any(|e| e.schedule.is_some()) {
            failures.push(format!(
                "{text:?}: expected no schedule, got {:?}",
                result.expressions
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
