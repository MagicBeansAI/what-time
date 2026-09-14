//! Parser facade: model predictions compiled into schedule expressions.
//!
//! This is the parse orchestrator: it owns the tagger and assembles
//! `ParseResult`s with expressions and timings.

use crate::stopwatch::Stopwatch;

use crate::compile::compile_predictions;
use crate::tagger::Tagger;
use crate::types::{
    Backend, DateOrder, Diagnostic, Expression, ParserOptions, PublicToken, ScheduleResult,
    ScheduleTimings, Severity,
};

pub struct ScheduleParser {
    tagger: Tagger,
    date_order: DateOrder,
    report_tokens: bool,
}

impl ScheduleParser {
    pub fn new(options: ParserOptions) -> ScheduleParser {
        let backend = options.backend.unwrap_or(Backend::Auto);
        ScheduleParser {
            tagger: Tagger::new(backend),
            date_order: options.date_order.unwrap_or_default(),
            report_tokens: true,
        }
    }

    fn assemble(&self, text: &str, result: crate::types::TagResult) -> ScheduleResult {
        let started = Stopwatch::start();
        let expressions: Vec<Expression> = if !result.unknown_labels {
            compile_predictions(text, &result.tokens, self.date_order)
        } else {
            vec![Expression {
                start: 0,
                end: text.len(),
                text: text.to_string(),
                confidence: 0.0,
                schedule: None,
                diagnostics: vec![Diagnostic {
                    code: "unknown-model-label".to_string(),
                    message: "The model returned an unsupported role.".to_string(),
                    start: 0,
                    end: text.len(),
                    severity: Severity::error,
                }],
            }]
        };
        let tokens = self.report_tokens.then(|| {
            result
                .tokens
                .iter()
                .map(|token| PublicToken {
                    start: token.raw.start,
                    end: token.raw.end,
                    text: token.raw.text.clone(),
                    kind: token.raw.kind,
                    label: token.label.name().to_string(),
                    clause_start: token.clause_start,
                    score: token.score,
                })
                .collect()
        });
        ScheduleResult {
            expressions,
            backend: result.backend,
            timings: ScheduleTimings {
                tokenize_ms: result.timings.tokenize_ms,
                infer_ms: result.timings.infer_ms,
                compile_ms: started.elapsed_ms(),
            },
            tokens,
            fallback_reason: result.fallback_reason,
        }
    }

    pub fn parse(&self, text: &str) -> Result<ScheduleResult, crate::Error> {
        Ok(self.assemble(text, self.tagger.tag(text)?))
    }

    pub fn parse_many(&self, texts: &[&str]) -> Result<Vec<ScheduleResult>, crate::Error> {
        let results = self.tagger.tag_many(texts)?;
        Ok(results
            .into_iter()
            .zip(texts)
            .map(|(result, text)| self.assemble(text, result))
            .collect())
    }
}
