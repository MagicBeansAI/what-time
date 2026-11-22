//! # what-time
//!
//! A compact neural parser for English, Hindi and Hinglish schedules. It
//! converts natural-language text into dates, time ranges, RFC 5545
//! recurrence rules and cron expressions, running entirely locally.
//!
//! The caller provides the reference instant and timezone; context never
//! enters the model. Timezones, daylight-saving transitions, the reference
//! date, and expansion limits are resolved after inference.
//!
//! ```no_run
//! use what_time::{parse, ParseContext};
//!
//! let context = ParseContext {
//!     reference: "2026-09-09T12:00:00+06:00".into(),
//!     time_zone: "Asia/Dhaka".into(),
//!     limit: Some(30),
//!     ..Default::default()
//! };
//! let result = parse("tomorrow at 3pm", &context).unwrap();
//! for occurrence in &result.occurrences {
//!     println!("{} (all day: {})", occurrence.start, occurrence.all_day);
//! }
//! ```

mod calendar;
mod clock;
mod compile;
mod cron;
mod cron_text;
mod exclusions;
mod labels;
mod lexicon;
mod model;
mod occurrence;
mod quantity;
mod recurrence;
mod resolve;
mod rrule;
mod schedule;
mod tagger;
mod tokenizer;
mod types;
mod zoned;

use std::sync::OnceLock;
mod stopwatch;

pub use schedule::ScheduleParser;
pub use types::{
    Backend, BareWeekday, BareWeekdaysPolicy, Clause, ClockTime, DateOrder, DateSpec, DayPart,
    Diagnostic, Duration, Frequency, NamedClock, NextWeekday, Occurrence, OpenBound, ParseContext,
    ParseResult, ParserOptions, Quantity, RawToken, Recurrence, ResolveOptions, Resolved, Schedule,
    Severity, Shift, TimeRange, TimeSpec, WeekStart, Weekday,
};

/// The library version, mirroring the workspace `Cargo.toml`.
///
/// Every surface reports the same string: the Rust crate
/// (`what_time::VERSION`), the CLI (`what-time --version`), the wasm
/// module (`version()`), and the npm package (`VERSION`).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Internal entry points for the in-repo test and evaluation suites. Not a
/// stable API.
#[doc(hidden)]
pub mod testing {
    pub use crate::compile::compile_predictions;
    pub use crate::labels::{LABELS, Role};
    pub use crate::model::predictions::Predictions;
    pub use crate::model::transformer;

    #[cfg(feature = "gpu")]
    pub mod gpu {
        pub use crate::model::gpu::{debug_windows, infer_windows_debug};
    }
    pub use crate::tokenizer::{FEATURE_ROWS, ROWS_PER_TOKEN, feature_rows, tokenize};
    pub use crate::types::{Expression, RawToken, Token};

    /// Per-token `(text, label-name)` pairs from the real tagging pipeline,
    /// including whitespace tokens (label `GLUE`). Inspection helper.
    pub fn tag_labels(text: &str) -> Vec<(String, &'static str)> {
        tagged_tokens(text)
            .into_iter()
            .map(|token| {
                (
                    token.raw.text.clone(),
                    LABELS.get(token.label as usize).copied().unwrap_or("?"),
                )
            })
            .collect()
    }

    /// Live-pipeline tokens with labels and per-token confidence, ready for
    /// `compile_predictions`. `Token::score` is the top1−top2 probability
    /// gap, so low scores mark uncertain tokens. Failure-mining helper.
    pub fn tagged_tokens(text: &str) -> Vec<Token> {
        let result = crate::tagger::Tagger::new(crate::types::Backend::Cpu).tag(text);
        match result {
            Ok(result) => result.tokens,
            Err(_) => Vec::new(),
        }
    }

    fn workspace_path(relative: &str) -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
    }

    /// The tracked evaluation corpora (JSONL, one case per line).
    pub fn eval_data_dir() -> std::path::PathBuf {
        workspace_path("../evals/data")
    }

    /// The reference fixtures for numeric model parity (binary).
    pub fn eval_active_dir() -> std::path::PathBuf {
        workspace_path("../evals/active")
    }

    pub fn read_jsonl(name: &str) -> Vec<String> {
        let path = eval_data_dir().join(format!("{name}.jsonl"));
        let file = std::fs::File::open(path)
            .unwrap_or_else(|error| panic!("missing eval corpus {name}: {error}"));
        std::io::BufRead::lines(std::io::BufReader::new(file))
            .map(|line| line.unwrap())
            .filter(|line| !line.trim().is_empty())
            .collect()
    }
}

/// Errors follow the public TypeError/RangeError contract.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    TypeError(String),
    RangeError(String),
}

impl Error {
    pub(crate) fn input_too_long() -> Error {
        Error::RangeError("An input supports at most one million characters.".into())
    }

    pub(crate) fn model_unavailable(message: String) -> Error {
        Error::TypeError(message)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::TypeError(message) | Error::RangeError(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for Error {}

/// A reusable parser instance (one per options set).
/// Disposal is handled by Rust's ownership; there is no `dispose()` method.
pub struct Parser {
    inner: schedule::ScheduleParser,
}

impl Parser {
    pub fn new(options: ParserOptions) -> Parser {
        Parser {
            inner: schedule::ScheduleParser::new(options),
        }
    }

    pub(crate) fn validate(context: &ParseContext) -> Result<usize, Error> {
        // Context belongs to calendar resolution and never enters the model.
        if context.time_zone.trim().is_empty() {
            return Err(Error::TypeError("timeZone is required.".into()));
        }
        let reference = zoned::instant(&context.reference).map_err(Error::RangeError)?;
        zoned::civil(reference, &context.time_zone).map_err(Error::RangeError)?;
        let limit = context.limit.unwrap_or(30);
        if !(1..=1000).contains(&limit) {
            return Err(Error::RangeError(
                "limit must be an integer from 1 through 1000.".into(),
            ));
        }
        Ok(limit as usize)
    }

    fn finish(
        parsed: types::ScheduleResult,
        context: &ParseContext,
        prepared: &Result<resolve::Prepared, String>,
        limit: usize,
    ) -> Result<ParseResult, Error> {
        let started = stopwatch::Stopwatch::start();
        let mut entries: Vec<(f64, TimeRange)> = Vec::new();
        let mut rrules: Vec<String> = Vec::new();
        let mut crons: Vec<String> = Vec::new();
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        for expression in &parsed.expressions {
            diagnostics.extend(expression.diagnostics.iter().cloned());
        }
        let mut truncated = false;
        for expression in &parsed.expressions {
            let Some(schedule) = &expression.schedule else {
                continue;
            };
            // The shared context is prepared lazily: a context that
            // cannot prepare (for example a horizon before the reference)
            // reports a resolution error per expression instead of failing the
            // whole call.
            let prepared = match prepared {
                Ok(prepared) => prepared,
                Err(message) => {
                    diagnostics.push(Diagnostic {
                        code: "resolution-error".into(),
                        severity: Severity::error,
                        message: message.clone(),
                        start: expression.start,
                        end: expression.end,
                    });
                    continue;
                }
            };
            match resolve::resolve_prepared(schedule, context, prepared) {
                Ok(result) => {
                    for occurrence in result.occurrences {
                        let epoch =
                            zoned::iso_epoch(&occurrence.start).map_err(Error::RangeError)?;
                        entries.push((
                            epoch,
                            TimeRange {
                                start: occurrence.start,
                                end: occurrence.end,
                                open: occurrence.open,
                                all_day: occurrence.all_day,
                            },
                        ));
                    }
                    rrules.extend(result.rrules);
                    for cron in result.crons {
                        if !crons.contains(&cron) {
                            crons.push(cron);
                        }
                    }
                    diagnostics.extend(result.diagnostics.into_iter().map(|mut value| {
                        value.start = expression.start;
                        value.end = expression.end;
                        value
                    }));
                    truncated = truncated || result.truncated;
                }
                Err(message) => {
                    diagnostics.push(Diagnostic {
                        code: "resolution-error".into(),
                        severity: Severity::error,
                        message,
                        start: expression.start,
                        end: expression.end,
                    });
                }
            }
        }
        if parsed.expressions.len() > 1 {
            entries.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        }
        let total = entries.len();
        let occurrences: Vec<TimeRange> = entries
            .into_iter()
            .take(limit)
            .map(|(_, range)| range)
            .collect();
        Ok(ParseResult {
            truncated: truncated || total > limit,
            occurrences,
            rrules,
            crons,
            diagnostics,
            backend: parsed.backend,
            timings: types::PublicTimings {
                tokenize_ms: parsed.timings.tokenize_ms,
                infer_ms: parsed.timings.infer_ms,
                resolve_ms: parsed.timings.compile_ms + started.elapsed_ms(),
            },
            fallback_reason: parsed.fallback_reason,
        })
    }

    pub fn parse(&self, text: &str, context: &ParseContext) -> Result<ParseResult, Error> {
        let limit = Self::validate(context)?;
        let parsed = self.inner.parse(text)?;
        let prepared = resolve::prepare(context);
        Self::finish(parsed, context, &prepared, limit)
    }

    pub fn parse_many(
        &self,
        texts: &[&str],
        context: &ParseContext,
    ) -> Result<Vec<ParseResult>, Error> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let limit = Self::validate(context)?;
        let parsed = self.inner.parse_many(texts)?;
        let prepared = resolve::prepare(context);
        parsed
            .into_iter()
            .map(|result| Self::finish(result, context, &prepared, limit))
            .collect()
    }
}

fn default_parser() -> &'static Parser {
    static DEFAULT: OnceLock<Parser> = OnceLock::new();
    DEFAULT.get_or_init(|| Parser::new(ParserOptions::default()))
}

/// Parses one string with the default parser instance.
pub fn parse(text: &str, context: &ParseContext) -> Result<ParseResult, Error> {
    default_parser().parse(text, context)
}

/// Batches several inputs with the default parser instance.
pub fn parse_many(texts: &[&str], context: &ParseContext) -> Result<Vec<ParseResult>, Error> {
    default_parser().parse_many(texts, context)
}

/// A cron line read back into a schedule: an English description, the typed
/// schedule, previewed occurrences, RRULEs, and the line re-exported by the
/// cron exporter (the normalized form: `1-5` becomes `MON-FRI`).
#[derive(Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CronResult {
    pub expression: String,
    pub description: String,
    pub schedule: Schedule,
    pub occurrences: Vec<TimeRange>,
    pub rrules: Vec<String>,
    pub crons: Vec<String>,
    pub truncated: bool,
    pub diagnostics: Vec<Diagnostic>,
}

/// Describes a five-field cron line (or `@daily`-style macro) in English:
/// `"*/15 9-17 * * 1-5"` → "Every 15 minutes from 9:00 AM to 5:45 PM on
/// weekdays". An invalid line returns a `TypeError` that says what is wrong.
pub fn describe_cron(expression: &str) -> Result<String, Error> {
    Ok(cron_text::parse(expression)
        .map_err(Error::TypeError)?
        .describe())
}

/// Reads a cron line in the context's time zone: its description, schedule,
/// and the occurrences it fires at from the reference instant, plus RRULE
/// and normalized cron exports. When day-of-month and day-of-week are both
/// restricted, a day matches when either does (POSIX).
pub fn parse_cron(expression: &str, context: &ParseContext) -> Result<CronResult, Error> {
    let limit = Parser::validate(context)?;
    let parsed = cron_text::parse(expression).map_err(Error::TypeError)?;
    let schedule = parsed.schedule().map_err(Error::TypeError)?;
    // The either-day rule can produce the same instant twice; ask for more
    // so the preview still fills after removing them.
    let mut wide = context.clone();
    wide.limit = Some(((limit * 2).min(1000)) as i64);
    let resolved = match resolve::resolve_with(&wide, &schedule) {
        Ok(resolved) => resolved,
        // A valid line whose days never occur ("0 0 30 2 *") describes
        // itself and says so instead of failing.
        Err(message) if message.starts_with("No eligible recurrence start") => {
            return Ok(CronResult {
                expression: expression.trim().to_string(),
                description: parsed.describe(),
                schedule,
                occurrences: Vec::new(),
                rrules: Vec::new(),
                crons: Vec::new(),
                truncated: false,
                diagnostics: vec![Diagnostic {
                    code: "never-fires".into(),
                    message: "No date matches this line within eight years (for example the 30th of February).".into(),
                    start: 0,
                    end: expression.len(),
                    severity: Severity::warning,
                }],
            });
        }
        Err(message) => return Err(Error::RangeError(message)),
    };
    let mut occurrences: Vec<TimeRange> = Vec::new();
    for occurrence in resolved.occurrences {
        let range = TimeRange {
            start: occurrence.start,
            end: occurrence.end,
            open: occurrence.open,
            all_day: occurrence.all_day,
        };
        if !occurrences.contains(&range) {
            occurrences.push(range);
        }
    }
    let truncated = resolved.truncated || occurrences.len() > limit;
    occurrences.truncate(limit);
    Ok(CronResult {
        expression: expression.trim().to_string(),
        description: parsed.describe(),
        schedule,
        occurrences,
        rrules: resolved.rrules,
        crons: resolved.crons,
        truncated,
        diagnostics: resolved.diagnostics,
    })
}

/// Resolves an already-compiled schedule against a caller context.
pub fn resolve(schedule: &Schedule, options: &ResolveOptions) -> Result<Resolved, String> {
    resolve::resolve_with(options, schedule)
}
