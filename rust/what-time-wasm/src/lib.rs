//! Browser bindings: parse English, Hindi, and Hinglish schedule phrases
//! entirely client-side. Both entry points return the same JSON the HTTP
//! API produced, so the playground's rendering code is unchanged.

use wasm_bindgen::prelude::*;

/// The library version embedded in this wasm module.
#[wasm_bindgen]
pub fn version() -> String {
    what_time::VERSION.to_string()
}

fn context(reference: &str, time_zone: &str, limit: i64) -> what_time::ParseContext {
    what_time::ParseContext {
        reference: reference.to_string(),
        time_zone: time_zone.to_string(),
        limit: Some(limit.clamp(1, 1000)),
        ..Default::default()
    }
}

/// Full parse: occurrences, diagnostics, rrules, timings.
#[wasm_bindgen]
pub fn parse(text: &str, reference: &str, time_zone: &str, limit: i64, date_order: &str) -> String {
    let order = if date_order == "DMY" {
        what_time::DateOrder::DMY
    } else {
        what_time::DateOrder::MDY
    };
    let parser = what_time::Parser::new(what_time::ParserOptions {
        date_order: Some(order),
        ..Default::default()
    });
    match parser.parse(text, &context(reference, time_zone, limit)) {
        Ok(result) => serde_json::to_string(&result).unwrap_or_else(|_| "{}".into()),
        Err(error) => serde_json::json!({"error": error.to_string()}).to_string(),
    }
}

/// Model-level schedules: expression spans, confidence, schedule JSON.
#[wasm_bindgen]
pub fn parse_expressions(text: &str) -> String {
    match what_time::ScheduleParser::new(Default::default()).parse(text) {
        Ok(parsed) => serde_json::to_string(&parsed.expressions).unwrap_or_else(|_| "[]".into()),
        Err(error) => serde_json::json!({"error": error.to_string()}).to_string(),
    }
}
