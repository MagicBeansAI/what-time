//! Corpus validation: read JSONL lines {text, labels: ["O", "HOUR", ...]}
//! (one label per non-whitespace token, in order), compile each with the
//! oracle path, and report lines whose labels do not produce a schedule.
//! Exits non-zero if more than the allowed fraction fails.

use what_time::testing::{LABELS, Role, Token, compile_predictions, tokenize};

fn main() {
    let path = match std::env::args().nth(1) {
        Some(path) => path,
        None => {
            eprintln!("usage: validate-corpus <corpus.jsonl>");
            std::process::exit(2);
        }
    };
    let mut total = 0usize;
    let mut failures = Vec::new();
    for (number, line) in std::fs::read_to_string(&path).unwrap().lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let value: serde_json::Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(error) => {
                failures.push(format!("line {}: bad json: {error}", number + 1));
                continue;
            }
        };
        let text = value["text"].as_str().unwrap_or_default();
        let labels: Vec<String> = value["labels"]
            .as_array()
            .map(|array| {
                array
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let mut index = 0usize;
        let tokens: Vec<_> = tokenize(text)
            .into_iter()
            .map(|raw| {
                let label = if raw.kind == 3 {
                    Role::O
                } else {
                    let name = labels
                        .get(index)
                        .map(String::as_str)
                        .unwrap_or("__missing__");
                    index += 1;
                    Role::from_name(name).unwrap_or(Role::O)
                };
                Token {
                    raw,
                    label,
                    clause_start: false,
                    score: 1.0,
                }
            })
            .collect();
        if index != labels.len() {
            failures.push(format!(
                "line {}: {} labels for {} non-space tokens: {:?}",
                number + 1,
                labels.len(),
                index,
                text
            ));
            continue;
        }
        let expressions = compile_predictions(text, &tokens, what_time::DateOrder::MDY);
        let has_schedule = expressions.first().is_some_and(|e| e.schedule.is_some());
        let expects_time = labels.iter().any(|l| l != "O" && l != "GLUE");
        if expects_time && !has_schedule {
            let codes: Vec<&str> = expressions
                .first()
                .map(|e| e.diagnostics.iter().map(|d| d.code.as_str()).collect())
                .unwrap_or_default();
            failures.push(format!(
                "line {}: no schedule from labels ({:?}): {:?}",
                number + 1,
                codes,
                text
            ));
        }
        total += 1;
        let _ = LABELS; // referenced so the import documents the taxonomy source
    }
    for failure in &failures {
        eprintln!("{failure}");
    }
    println!(
        "validated {total} lines, {} failures ({:.1}%)",
        failures.len(),
        failures.len() as f64 / total.max(1) as f64 * 100.0
    );
    if failures.len() * 10 > total.max(1) {
        std::process::exit(1);
    }
}
