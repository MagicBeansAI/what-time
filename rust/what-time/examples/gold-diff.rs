//! One-off comparison: which gold-corpus schedule cases each model misses.
use serde::Deserialize;
use serde_json::Value;
use what_time::testing::read_jsonl;
use what_time::{Schedule, ScheduleParser};

#[derive(Deserialize)]
struct Case {
    id: String,
    text: String,
    schedule: Option<Value>,
}

fn main() {
    let mut cases: Vec<Case> = Vec::new();
    for name in ["grammar", "grammar-variations", "adversarial", "prose"] {
        cases.extend(
            read_jsonl(name)
                .iter()
                .map(|line| serde_json::from_str(line).unwrap()),
        );
    }
    {
        let parser = ScheduleParser::new(Default::default());
        let misses: Vec<&Case> = cases
            .iter()
            .filter(|case| {
                let expected: Option<Schedule> = case
                    .schedule
                    .clone()
                    .and_then(|value| serde_json::from_value(value).ok());
                let actual = parser
                    .parse(&case.text)
                    .ok()
                    .and_then(|parsed| parsed.expressions.into_iter().next())
                    .and_then(|expression| expression.schedule);
                actual != expected
            })
            .collect();
        println!("== {} misses of {}", misses.len(), cases.len());
        for case in misses.iter() {
            println!("  {} {:?}", case.id, case.text);
        }
    }
}
