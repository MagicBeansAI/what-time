//! Runtime-level cases: raw tagging behavior, schedule encoding round-trips,
//! and batch behavior for the scenarios that apply to the synchronous Rust
//! runtime (async batching and disposal are exercised
//! infrastructure this port does not have).

use serde_json::Value;
use what_time::testing::{read_jsonl, tokenize};
use what_time::{Parser, Schedule, ScheduleParser};

#[test]
fn returns_source_aligned_predictions_without_interpreting_calendar_values() {
    let parser = ScheduleParser::new(Default::default()); // default model
    let result = parser.parse("27pm").unwrap();
    let tokens = result.tokens.as_ref().unwrap();
    let observed: Vec<(&str, &str)> = tokens
        .iter()
        .map(|token| (token.text.as_str(), token.label.as_str()))
        .collect();
    assert_eq!(
        observed,
        [("27", "HOUR"), ("pm", "MERIDIEM")],
        "the tagger reports labels before any calendar validation"
    );
    // The invalid clock value surfaces as a diagnostic, not a tagger error.
    assert!(
        result.expressions[0]
            .diagnostics
            .iter()
            .any(|value| value.code == "invalid-time")
    );
}

#[test]
fn roundtrips_every_corpus_schedule_through_the_json_encoding() {
    // Gold schedules validate against a generated JSON schema;
    // the equivalent guarantee here is a lossless serde round-trip.
    let mut schedules = 0;
    for corpus in ["grammar", "grammar-variations", "adversarial", "prose"] {
        for line in read_jsonl(corpus) {
            let value: Value = serde_json::from_str(&line).expect("corpus lines are valid JSON");
            let Some(Value::Object(fields)) = Some(value.clone()) else {
                continue;
            };
            let Some(expected) = fields.get("schedule").cloned() else {
                continue;
            };
            if expected.is_null() {
                continue;
            }
            let schedule: Schedule = serde_json::from_value(expected.clone()).unwrap();
            let encoded = serde_json::to_string(&schedule).unwrap();
            let reparsed: Schedule = serde_json::from_str(&encoded).unwrap();
            assert_eq!(reparsed, schedule, "{corpus}: {line}");
            schedules += 1;
        }
    }
    assert!(schedules > 400, "expected the corpora, got {schedules}");
}

#[test]
fn does_not_initialize_special_handling_for_batches_of_empty_strings() {
    let parser = Parser::new(Default::default());
    assert!(parser.parse_many(&[], &context()).unwrap().is_empty());
    let empty: Vec<&str> = vec![""; 40];
    let results = parser.parse_many(&empty, &context()).unwrap();
    assert!(
        results
            .iter()
            .all(|result| result.backend == "cpu" && result.occurrences.is_empty())
    );
}

#[test]
fn preserves_every_source_offset_on_repeated_long_inputs() {
    let text = "Tomorrow at noon. ".repeat(80);
    let parser = ScheduleParser::new(Default::default()); // default model
    let result = parser.parse(&text).unwrap();
    let tokens = result.tokens.as_ref().unwrap();
    let joined: String = tokens.iter().map(|token| token.text.as_str()).collect();
    assert_eq!(joined, text);
    for token in tokens {
        assert_eq!(&text[token.start..token.end], token.text);
    }
}

#[test]
fn keeps_the_tokenizer_lossless_over_the_corpus_texts() {
    let mut checked = 0;
    for corpus in ["grammar", "negatives", "prose", "results", "user-cases"] {
        for line in read_jsonl(corpus) {
            let value: Value = serde_json::from_str(&line).unwrap();
            let Some(text) = value.get("text").and_then(Value::as_str) else {
                continue;
            };
            let tokens = tokenize(text);
            let joined: String = tokens.iter().map(|token| token.text.as_str()).collect();
            assert_eq!(joined, text, "{corpus}");
            let mut offset = 0;
            for token in &tokens {
                assert_eq!(token.start, offset);
                assert_eq!(token.end, token.start + token.text.len());
                offset = token.end;
            }
            assert_eq!(offset, text.len());
            checked += 1;
        }
    }
    assert!(checked > 200, "expected corpus texts, got {checked}");
}

fn context() -> what_time::ParseContext {
    what_time::ParseContext {
        reference: "2026-09-09T12:00:00+06:00".into(),
        time_zone: "Asia/Dhaka".into(),
        ..Default::default()
    }
}
