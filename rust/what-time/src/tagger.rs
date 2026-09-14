//! Tags tokens with model roles, applying the inference-window policy.

use crate::stopwatch::Stopwatch;

use crate::labels::Role;
use crate::model::predictions::Predictions;
use crate::tokenizer::tokenize;
use crate::types::{Backend, RawToken, TagResult, TagTimings, Token};

const WINDOW_STRIDE: usize = 96;
const CONTEXT_BEFORE: usize = 16;
const CONTEXT_AFTER: usize = 112;

pub struct Tagger {
    _backend: Backend,
}

struct Job {
    text: String,
    tokens: Vec<RawToken>,
    tokenize_ms: f64,
}

struct Window {
    job: usize,
    start: usize,
    keep_start: usize,
    keep_end: usize,
    tokens: Vec<RawToken>,
}

fn windows_for(job: &mut Job, job_index: usize) -> Vec<Window> {
    // Whitespace width has no temporal meaning. Use canonical model features
    // while retaining the original tokens and offsets in the public result.
    let started = Stopwatch::start();
    let canonical_text = collapse_whitespace(job.text.trim());
    let canonical = if canonical_text == job.text {
        job.tokens.clone()
    } else {
        tokenize(&canonical_text)
    };
    job.tokenize_ms += started.elapsed_ms();

    let source_start = if job.tokens.first().is_some_and(|token| token.kind == 3) {
        1
    } else {
        0
    };
    let mut windows = Vec::new();
    let mut start = 0;
    while start < canonical.len() {
        let context_start = start.saturating_sub(CONTEXT_BEFORE);
        let context_end = canonical.len().min(start + CONTEXT_AFTER);
        let mut tokens = canonical[context_start..context_end].to_vec();
        let edge_indices: Vec<usize> = if tokens.len() == 1 {
            vec![0]
        } else {
            vec![0, tokens.len() - 1]
        };
        let last_index = tokens.len() - 1;
        for index in edge_indices {
            let mut flags = tokens[index].features.1 & !((1 << 10) | (1 << 11));
            if index == 0 {
                flags |= 1 << 10;
            }
            if index == last_index {
                flags |= 1 << 11;
            }
            tokens[index].features.1 = flags;
        }
        windows.push(Window {
            job: job_index,
            start: source_start + context_start,
            keep_start: start - context_start,
            keep_end: canonical.len().min(start + WINDOW_STRIDE) - context_start,
            tokens,
        });
        start += WINDOW_STRIDE;
    }
    windows
}

fn collapse_whitespace(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut in_whitespace = false;
    for character in text.chars() {
        if character.is_whitespace() {
            if !in_whitespace {
                result.push(' ');
                in_whitespace = true;
            }
        } else {
            result.push(character);
            in_whitespace = false;
        }
    }
    result
}

fn predict(windows: &[Window], prefer_gpu: bool) -> Result<Vec<Predictions>, crate::Error> {
    #[cfg(feature = "gpu")]
    if prefer_gpu && windows.len() >= crate::model::gpu::GPU_BATCH_THRESHOLD {
        let grouped: Vec<Vec<crate::types::RawToken>> =
            windows.iter().map(|window| window.tokens.clone()).collect();
        return crate::model::gpu::infer_windows(&grouped);
    }
    #[cfg(not(feature = "gpu"))]
    let _ = prefer_gpu;
    windows
        .iter()
        .map(|window| crate::model::transformer::infer(&window.tokens))
        .collect()
}

impl Tagger {
    pub fn new(backend: Backend) -> Tagger {
        Tagger { _backend: backend }
    }

    pub fn tag(&self, text: &str) -> Result<TagResult, crate::Error> {
        Ok(self.tag_many(std::slice::from_ref(&text))?.remove(0))
    }

    pub fn tag_many(&self, texts: &[&str]) -> Result<Vec<TagResult>, crate::Error> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        for text in texts {
            if text.len() > 1_000_000 {
                return Err(crate::Error::input_too_long());
            }
        }

        let mut jobs: Vec<Job> = texts
            .iter()
            .map(|text| {
                let started = Stopwatch::start();
                let tokens = tokenize(text);
                let tokenize_ms = started.elapsed_ms();
                Job {
                    text: text.to_string(),
                    tokens,
                    tokenize_ms,
                }
            })
            .collect();

        let mut windows = Vec::new();
        for (job_index, job) in jobs.iter_mut().enumerate() {
            windows.extend(windows_for(job, job_index));
        }

        let started = Stopwatch::start();
        let predictions = predict(&windows, self._backend != crate::types::Backend::Cpu)?;
        let infer_ms = started.elapsed_ms();

        let mut labeled: Vec<Vec<Token>> = jobs
            .iter()
            .map(|job| {
                job.tokens
                    .iter()
                    .map(|token| Token {
                        raw: token.clone(),
                        label: Role::O,
                        clause_start: false,
                        score: 0.0,
                    })
                    .collect()
            })
            .collect();
        let mut invalid = vec![false; jobs.len()];

        for (window, prediction) in windows.iter().zip(predictions.iter()) {
            for token in window.keep_start..window.keep_end {
                let label = prediction.labels[token];
                if label as usize >= crate::labels::LABELS.len() {
                    invalid[window.job] = true;
                }
                let result = &mut labeled[window.job][window.start + token];
                result.label = Role::from_u8(label).unwrap_or(Role::O);
                result.clause_start = prediction.clause_starts[token] != 0;
                result.score = prediction.scores[token];
            }
        }

        Ok(jobs
            .into_iter()
            .zip(labeled)
            .enumerate()
            .map(|(index, (job, tokens))| TagResult {
                tokens,
                unknown_labels: invalid[index],
                backend: "cpu",
                timings: TagTimings {
                    tokenize_ms: job.tokenize_ms,
                    infer_ms,
                },
                fallback_reason: None,
            })
            .collect())
    }
}
