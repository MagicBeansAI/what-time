//! The gold corpora are a hard gate: every recorded occurrence and schedule
//! expectation must match exactly.

use serde::Deserialize;
use serde_json::Value;
use what_time::testing::read_jsonl;
use what_time::testing::transformer;
use what_time::{ParseContext, Parser, Schedule, ScheduleParser};

#[derive(Deserialize)]
struct ResultCase {
    text: String,
    context: ParseContext,
    occurrences: Vec<what_time::TimeRange>,
}

#[derive(Deserialize)]
struct ScheduleCase {
    text: String,
    schedule: Option<Value>,
}

fn occurrence_matches(cases: &[ResultCase]) -> usize {
    let parser = Parser::new(Default::default());
    cases
        .iter()
        .filter(|case| {
            parser
                .parse(&case.text, &case.context)
                .is_ok_and(|result| result.occurrences == case.occurrences)
        })
        .count()
}

fn schedule_matches(cases: &[ScheduleCase]) -> usize {
    let parser = ScheduleParser::new(Default::default());
    cases
        .iter()
        .filter(|case| {
            let actual = parser
                .parse(&case.text)
                .ok()
                .and_then(|parsed| parsed.expressions.into_iter().next())
                .and_then(|expression| expression.schedule);
            match &case.schedule {
                // "schedule: null" expects no expression at all.
                None => actual.is_none(),
                Some(expected) => {
                    let expected: Option<Schedule> = serde_json::from_value(expected.clone()).ok();
                    actual == expected
                }
            }
        })
        .count()
}

#[test]
fn gold_corpora_match_recorded_expectations() {
    if !transformer::available() {
        eprintln!("skipping: transformer weights are a placeholder");
        return;
    }
    let result_cases: Vec<ResultCase> = read_jsonl("results")
        .iter()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let results = occurrence_matches(&result_cases);

    let mut schedule_cases: Vec<ScheduleCase> = Vec::new();
    for name in ["grammar", "grammar-variations", "adversarial", "prose"] {
        schedule_cases.extend(
            read_jsonl(name)
                .iter()
                .map(|line| serde_json::from_str(line).unwrap()),
        );
    }
    let schedules = schedule_matches(&schedule_cases);

    eprintln!(
        "gold corpora — occurrences: {}/{}; schedules: {}/{}",
        results,
        result_cases.len(),
        schedules,
        schedule_cases.len(),
    );

    assert_eq!(results, result_cases.len(), "occurrence mismatches");
    assert_eq!(schedules, schedule_cases.len(), "schedule mismatches");
}
