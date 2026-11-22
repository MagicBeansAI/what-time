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

#[derive(Deserialize)]
struct CronCase {
    id: String,
    text: String,
    crons: Vec<String>,
    /// The `RRULE:` line of each rule, pinned where cron has no expression.
    rrules: Option<Vec<String>>,
}

/// Cron cases the model does not yet read correctly, by id; the same
/// contract as `KNOWN_GAPS`.
const KNOWN_CRON_GAPS: [&str; 0] = [];

/// Recurring phrases through the whole pipeline — model, compiler,
/// resolver, exporters — against authored cron expressions. The reference
/// is Sunday 2026-11-22, 15:30 in Kolkata.
#[test]
fn cron_corpus_matches() {
    let parser = Parser::new(Default::default());
    let context = ParseContext {
        reference: "2026-11-22T10:00:00Z".into(),
        time_zone: "Asia/Kolkata".into(),
        limit: Some(5),
        ..Default::default()
    };
    let cases: Vec<CronCase> = read_lines("cron")
        .iter()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(cases.len() >= 50, "expected the full cron corpus");
    let mut mismatches = Vec::new();
    for case in &cases {
        let result = parser.parse(&case.text, &context).unwrap();
        let rules: Vec<String> = result
            .rrules
            .iter()
            .filter_map(|rule| rule.lines().last().map(String::from))
            .collect();
        let matches = result.crons == case.crons
            && case.rrules.as_ref().is_none_or(|expected| &rules == expected)
            && !result.occurrences.is_empty()
            // No expression must come with its reason.
            && (!case.crons.is_empty()
                || result
                    .diagnostics
                    .iter()
                    .any(|value| value.code == "unsupported-cron"));
        if matches == KNOWN_CRON_GAPS.contains(&case.id.as_str()) {
            mismatches.push(format!(
                "{} {:?}: expected {:?} {:?}, got {:?} {:?} ({:?})",
                case.id,
                case.text,
                case.crons,
                case.rrules,
                result.crons,
                rules,
                result
                    .diagnostics
                    .iter()
                    .map(|value| value.code.as_str())
                    .collect::<Vec<_>>()
            ));
        }
    }
    assert!(
        mismatches.is_empty(),
        "{} mismatching cron cases:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
    eprintln!(
        "cron eval: {}/{} cases match ({} known gap(s) expected to fail)",
        cases.len() - KNOWN_CRON_GAPS.len(),
        cases.len(),
        KNOWN_CRON_GAPS.len()
    );
}

#[derive(Deserialize)]
struct CronParseCase {
    id: String,
    cron: String,
    valid: bool,
    /// The first fires as local `YYYY-MM-DDTHH:MM`, from the reference.
    first: Vec<String>,
}

/// Cron lines read back into schedules: the first four fires in Kolkata
/// from Sunday 2026-11-22 15:30, or a rejection for an invalid line. The
/// expectations were written independently of the parser, from POSIX cron
/// semantics (either-day rule included), then pinned. A preview that stops
/// at the one-year horizon matches when what it shows is right.
#[test]
fn cron_parse_corpus_matches() {
    let context = ParseContext {
        reference: "2026-11-22T10:00:00Z".into(),
        time_zone: "Asia/Kolkata".into(),
        limit: Some(4),
        ..Default::default()
    };
    let cases: Vec<CronParseCase> = read_lines("cron-parse")
        .iter()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(cases.len() >= 80, "expected the full cron-parse corpus");
    let mut mismatches = Vec::new();
    for case in &cases {
        let parsed = what_time::parse_cron(&case.cron, &context);
        let ok = match (&parsed, case.valid) {
            (Err(_), false) => true,
            (Ok(result), true) => {
                let got: Vec<String> = result
                    .occurrences
                    .iter()
                    .map(|value| value.start[..16].to_string())
                    .collect();
                got == case.first
                    || (!got.is_empty()
                        && case.first.starts_with(&got)
                        && case.first[got.len()..]
                            .iter()
                            .all(|value| value.as_str() >= "2027-11-22"))
            }
            _ => false,
        };
        if !ok {
            mismatches.push(format!(
                "{} {:?}: expected {:?}, got {:?}",
                case.id,
                case.cron,
                if case.valid {
                    format!("{:?}", case.first)
                } else {
                    "rejection".into()
                },
                parsed.map(|value| value
                    .occurrences
                    .iter()
                    .map(|o| o.start.clone())
                    .collect::<Vec<_>>())
            ));
        }
    }
    assert!(
        mismatches.is_empty(),
        "{} mismatching cron-parse cases:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
    eprintln!(
        "cron-parse eval: {}/{} cases match",
        cases.len(),
        cases.len()
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
