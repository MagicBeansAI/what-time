//! Oracle compilation from hand-labeled tokens (`labels.jsonl`): each case
//! supplies the exact token labels and clause starts, isolating the compiler
//! from the neural model.

use serde::Deserialize;
use what_time::DateOrder;
use what_time::testing::{Role, Token, compile_predictions, read_jsonl, tokenize};

#[derive(Deserialize)]
struct OracleToken {
    #[allow(dead_code)]
    start: usize,
    #[allow(dead_code)]
    end: usize,
    label: String,
    #[serde(rename = "clauseStart")]
    clause_start: bool,
}

#[derive(Deserialize)]
struct OracleCase {
    #[allow(dead_code)]
    id: String,
    text: String,
    schedule: Option<what_time::Schedule>,
    tokens: Vec<OracleToken>,
}

fn cases() -> Vec<OracleCase> {
    read_jsonl("labels")
        .iter()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn compile_oracle(example: &OracleCase) -> Vec<what_time::testing::Expression> {
    let tokens: Vec<Token> = tokenize(&example.text)
        .into_iter()
        .enumerate()
        .map(|(index, raw)| Token {
            label: Role::from_name(&example.tokens[index].label).unwrap_or(Role::O),
            clause_start: example.tokens[index].clause_start,
            score: 1.0,
            raw,
        })
        .collect();
    compile_predictions(&example.text, &tokens, DateOrder::MDY)
}

#[test]
fn assembles_the_full_oracle_contract() {
    for example in cases() {
        let expressions = compile_oracle(&example);
        assert_eq!(expressions.len(), 1, "{}", example.text);
        assert_eq!(
            serde_json::to_value(expressions[0].schedule.as_ref()).unwrap(),
            serde_json::to_value(example.schedule.as_ref()).unwrap(),
            "{}: {:?}",
            example.text,
            expressions[0].diagnostics
        );
    }
}
