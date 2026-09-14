//! Trilingual (English / Hindi / Hinglish) schedule parsing. Expectations
//! are frozen in evals/data/multilingual.jsonl after eyeball verification of
//! the trained transformer's output.

use serde::Deserialize;
use serde_json::Value;
use what_time::testing::read_jsonl;
use what_time::{ParseContext, Parser, Schedule, ScheduleParser};

#[derive(Deserialize)]
struct Case {
    #[allow(dead_code)]
    id: String,
    language: String,
    text: String,
    schedule: Option<Value>,
}

fn context() -> ParseContext {
    ParseContext {
        reference: "2026-09-13T10:00:00Z".into(),
        time_zone: "Asia/Kolkata".into(),
        limit: Some(3),
        ..Default::default()
    }
}

#[test]
fn multilingual_phrases_resolve() {
    let cases: Vec<Case> = read_jsonl("multilingual")
        .iter()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(
        cases.len() >= 12,
        "expected the trilingual corpus, got {}",
        cases.len()
    );

    let parser = ScheduleParser::new(Default::default());
    let runtime = Parser::new(Default::default());
    let ctx = context();
    for case in &cases {
        let expected: Option<Schedule> = case
            .schedule
            .clone()
            .and_then(|value| serde_json::from_value(value).ok());
        let actual = parser
            .parse(&case.text)
            .ok()
            .and_then(|parsed| parsed.expressions.into_iter().next())
            .and_then(|expression| expression.schedule);
        assert_eq!(
            actual, expected,
            "[{}/{}] schedule mismatch",
            case.language, case.text
        );
        // Every case must also resolve to at least one occurrence.
        let resolved = runtime.parse(&case.text, &ctx).unwrap();
        assert!(
            !resolved.occurrences.is_empty(),
            "[{}/{}] produced no occurrences",
            case.language,
            case.text
        );
    }
}
