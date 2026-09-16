//! Failure mining: read unlabeled phrases (one per line, plain text or
//! {"text": ...} JSON), tag them with the live model, and rank them by
//! uncertainty so the hardest cases can seed the next corpus batch.
//!
//! Two signals per phrase:
//! - margin: Token.score is the top1−top2 probability gap; the phrase's
//!   value is the minimum over non-whitespace tokens (lowest = least
//!   certain).
//! - compile: whether the tagged labels compile into a schedule, judged
//!   against whether any non-O label predicted a time expression. The
//!   real pipeline path (schedule.rs) is reused verbatim.
//!
//! Output: JSONL on stdout, most-uncertain first:
//!   {"text", "min_margin", "mean_margin", "status", "diagnostics",
//!    "tokens": [[text, label, score], ...]}
//! status is "compile-fail" (labels looked temporal but nothing compiled),
//! "spurious" (a schedule compiled from an all-O tag), or "low-margin".
//! Exit code is always 0 unless reading input fails; mining is a report.

use what_time::testing::{LABELS, Role, compile_predictions, tagged_tokens};

#[derive(Clone)]
struct Mined {
    text: String,
    min_margin: f32,
    mean_margin: f32,
    status: &'static str,
    diagnostics: Vec<String>,
    tokens: Vec<(String, &'static str, f32)>,
}

fn main() {
    let mut args = std::env::args().skip(1);
    let path = match args.next() {
        Some(path) => path,
        None => {
            eprintln!("usage: mine <phrases.txt|phrases.jsonl> [min-margin]");
            eprintln!("  min-margin: threshold below which a phrase counts as");
            eprintln!("  low-margin (default 0.35)");
            std::process::exit(2);
        }
    };
    let threshold: f32 = args
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0.35);

    let source = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        eprintln!("cannot read {path}: {error}");
        std::process::exit(2);
    });

    let mut mined = Vec::new();
    for line in source.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let text = if line.starts_with('{') {
            serde_json::from_str::<serde_json::Value>(line)
                .ok()
                .and_then(|value| value["text"].as_str().map(String::from))
                .unwrap_or_else(|| line.to_string())
        } else {
            line.to_string()
        };

        let result = tagged_tokens(&text);
        if result.is_empty() {
            continue;
        }

        let mut min_margin = f32::INFINITY;
        let mut margin_sum = 0f32;
        let mut counted = 0usize;
        let mut expects_time = false;
        let mut tokens = Vec::new();
        for token in &result {
            let label = LABELS
                .get(token.label as usize)
                .copied()
                .unwrap_or("?");
            tokens.push((token.raw.text.clone(), label, token.score));
            if token.raw.kind == 3 {
                continue;
            }
            min_margin = min_margin.min(token.score);
            margin_sum += token.score;
            counted += 1;
            if token.label != Role::O {
                expects_time = true;
            }
        }
        let (min_margin, mean_margin) = if counted == 0 {
            (1.0, 1.0)
        } else {
            (min_margin, margin_sum / counted as f32)
        };

        let expressions = compile_predictions(&text, &result, what_time::DateOrder::MDY);
        let has_schedule = expressions
            .first()
            .is_some_and(|expression| expression.schedule.is_some());
        let diagnostics: Vec<String> = expressions
            .first()
            .map(|expression| {
                expression
                    .diagnostics
                    .iter()
                    .map(|d| d.code.clone())
                    .collect()
            })
            .unwrap_or_default();

        let status: &'static str = if expects_time && !has_schedule {
            "compile-fail"
        } else if !expects_time && has_schedule {
            "spurious"
        } else if min_margin < threshold {
            "low-margin"
        } else {
            "confident"
        };
        if status != "confident" {
            mined.push(Mined {
                text,
                min_margin,
                mean_margin,
                status,
                diagnostics,
                tokens,
            });
        }
    }

    mined.sort_by(|a, b| {
        b.min_margin
            .partial_cmp(&a.min_margin)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut counts = std::collections::BTreeMap::new();
    for entry in &mined {
        *counts.entry(entry.status).or_insert(0usize) += 1;
        println!(
            "{}",
            serde_json::json!({
                "text": entry.text,
                "min_margin": entry.min_margin,
                "mean_margin": entry.mean_margin,
                "status": entry.status,
                "diagnostics": entry.diagnostics,
                "tokens": entry.tokens,
            })
        );
    }
    let summary: String = counts
        .iter()
        .map(|(status, count)| format!("{status}: {count}"))
        .collect::<Vec<_>>()
        .join(", ");
    eprintln!("mined {} phrases ({summary})", mined.len());
}
