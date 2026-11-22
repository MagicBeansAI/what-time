//! The parser never panics: every phrase in the tracked corpora, cut at
//! every word boundary from either end, goes through the full parse
//! (model, compiler, resolver, exporters). Truncations reach the edges a
//! label repair looks past ("my flight last month" once indexed two tokens
//! beyond "last").

use serde::Deserialize;
use what_time::testing::read_jsonl;
use what_time::{ParseContext, Parser};

#[derive(Deserialize)]
struct Case {
    text: String,
}

#[test]
fn no_phrase_or_truncation_panics() {
    let parser = Parser::new(Default::default());
    let context = ParseContext {
        reference: "2026-11-22T10:00:00Z".into(),
        time_zone: "Asia/Kolkata".into(),
        limit: Some(3),
        ..Default::default()
    };
    let mut checked = 0;
    let mut panics = Vec::new();
    for corpus in [
        "user-cases",
        "negatives",
        "adversarial",
        "multilingual",
        "grammar",
        "grammar-variations",
        "cron",
        "prose",
    ] {
        for line in read_jsonl(corpus) {
            let Ok(case) = serde_json::from_str::<Case>(&line) else {
                continue;
            };
            let words: Vec<&str> = case.text.split_whitespace().collect();
            let mut variants = vec![case.text.clone()];
            for cut in 1..words.len() {
                variants.push(words[..cut].join(" "));
                variants.push(words[cut..].join(" "));
            }
            for text in variants {
                checked += 1;
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let _ = parser.parse(&text, &context);
                }));
                if outcome.is_err() {
                    panics.push(text);
                }
            }
        }
    }
    eprintln!("no-panic: {checked} phrases and truncations parsed");
    assert!(
        panics.is_empty(),
        "{} inputs panic: {panics:#?}",
        panics.len()
    );
}
