//! End-to-end phrases drawn from the training families, so they must
//! resolve through the trained labels directly.

use what_time::testing::transformer;
use what_time::{ParseContext, Parser};

fn context() -> ParseContext {
    ParseContext {
        reference: "2026-09-12T15:26:00Z".into(),
        time_zone: "Asia/Kolkata".into(),
        limit: Some(3),
        ..Default::default()
    }
}

fn parser() -> Parser {
    Parser::new(Default::default())
}

fn starts(text: &str) -> Vec<String> {
    parser()
        .parse(text, &context())
        .unwrap()
        .occurrences
        .iter()
        .map(|occurrence| occurrence.start[..10].to_string())
        .collect()
}

#[test]
fn resolves_the_phrase_families_it_was_trained_on() {
    if !transformer::available() {
        eprintln!("skipping: transformer weights are a placeholder");
        return;
    }
    assert_eq!(
        starts("day after tomorrow at 8 pm"),
        vec!["2026-09-14"],
        "article-less day after tomorrow"
    );
    assert_eq!(
        starts("book it for day after tomorrow"),
        vec!["2026-09-14"],
        "mid-sentence article-less form"
    );
    assert_eq!(
        starts("24th august last year"),
        vec!["2025-08-24"],
        "period-qualified calendar date"
    );
    assert_eq!(
        starts("15th last month"),
        vec!["2026-08-15"],
        "day anchored to last month"
    );
    assert_eq!(
        starts("friday last week"),
        vec!["2026-09-04"],
        "weekday anchored to last week"
    );
}

#[test]
fn keeps_standard_phrases_working() {
    if !transformer::available() {
        eprintln!("skipping: transformer weights are a placeholder");
        return;
    }
    assert_eq!(
        starts("tomorrow at 9am"),
        vec!["2026-09-13"],
        "plain relative day with clock"
    );
    assert_eq!(
        starts("every Friday at 7:30pm").len(),
        3,
        "weekly recurrence"
    );
}
