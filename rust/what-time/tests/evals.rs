//! End-to-end evaluation over the tracked gold corpora (`rust/evals/data`).

use serde::Deserialize;
use what_time::testing::read_jsonl;
use what_time::{ParseContext, Parser, Schedule, ScheduleParser};

fn read_lines(name: &str) -> Vec<String> {
    read_jsonl(name)
}

#[derive(Deserialize)]
struct ResultCase {
    #[allow(dead_code)]
    id: String,
    text: String,
    context: ParseContext,
    occurrences: Vec<what_time::TimeRange>,
    error: Option<String>,
}

#[test]
fn results_corpus_matches() {
    let parser = Parser::new(Default::default());
    let cases: Vec<ResultCase> = read_lines("results")
        .iter()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(!cases.is_empty());
    for case in &cases {
        let actual = parser.parse(&case.text, &case.context).unwrap();
        assert_eq!(
            actual.occurrences, case.occurrences,
            "occurrences mismatch for {:?}",
            case.text
        );
        if let Some(expected_error) = &case.error {
            assert!(
                actual
                    .diagnostics
                    .iter()
                    .any(|value| &value.code == expected_error),
                "expected diagnostic {expected_error} for {:?}, got {:?}",
                case.text,
                actual.diagnostics
            );
        } else {
            assert!(
                !actual
                    .diagnostics
                    .iter()
                    .any(|value| value.severity == what_time::Severity::error),
                "unexpected error diagnostics for {:?}: {:?}",
                case.text,
                actual.diagnostics
            );
        }
    }
    eprintln!(
        "results eval: {}/{} cases match the recorded occurrences",
        cases.len(),
        cases.len()
    );
}

#[derive(Deserialize)]
struct ScheduleCase {
    id: String,
    text: String,
    schedule: Option<Schedule>,
}

/// Known-gapped corpus cases, by id: the pinned expectation is what the
/// case SHOULD produce, but the model is not there yet. The suite fails
/// if a gap starts passing (remove it from this list) and still fails if
/// it regresses further. Gaps close through training, never by editing
/// the expectation. Currently empty.
const KNOWN_GAPS: [&str; 0] = [];

#[test]
fn schedule_corpora_match() {
    let parser = ScheduleParser::new(Default::default());
    let mut cases: Vec<ScheduleCase> = Vec::new();
    for corpus in [
        "adversarial",
        "grammar",
        "negatives",
        "grammar-variations",
        "prose",
        "user-cases",
    ] {
        for line in read_lines(corpus) {
            cases.push(serde_json::from_str(&line).unwrap());
        }
    }
    assert!(
        cases.len() > 500,
        "expected the full corpora, got {}",
        cases.len()
    );

    let mut mismatches = Vec::new();
    for case in &cases {
        let result = parser.parse(&case.text).unwrap();
        let is_gap = KNOWN_GAPS.contains(&case.id.as_str());
        let actual = result
            .expressions
            .first()
            .and_then(|expression| expression.schedule.clone());
        let matches = actual == case.schedule;
        if matches == is_gap {
            mismatches.push(format!(
                "{} {:?}: expected match={}, got schedule {:?}",
                case.id, case.text, !is_gap, actual
            ));
        }
    }
    assert!(
        mismatches.is_empty(),
        "{} mismatching cases:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
    eprintln!(
        "schedule eval: {}/{} cases match the recorded expectations ({} known model gap(s) expected to fail)",
        cases.len() - KNOWN_GAPS.len(),
        cases.len(),
        KNOWN_GAPS.len()
    );
}

#[test]
fn rejects_malformed_or_ambiguous_input() {
    let parser = ScheduleParser::new(Default::default());
    for (text, code) in [
        ("27pm", "invalid-time"),
        ("2:99pm", "invalid-time"),
        ("2026-13-01", "invalid-date"),
        ("every Monday until", "invalid-bound"),
    ] {
        let result = parser.parse(text).unwrap();
        assert_eq!(result.expressions.len(), 1, "{text}");
        assert!(result.expressions[0].schedule.is_none(), "{text}");
        assert!(
            result.expressions[0]
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == code),
            "{text}: expected {code}, got {:?}",
            result.expressions[0].diagnostics
        );
    }
}
