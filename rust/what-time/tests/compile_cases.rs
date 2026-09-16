//! Schedule compilation from explicit semantic labels — an oracle-side
//! compiler test-suite. Cases hand-label tokens (the "oracle" interface) and
//! assert the assembled schedule, exercising paths the model may never emit.

use serde_json::{Value, json};
use what_time::testing::{Expression, LABELS, Role, Token, compile_predictions, tokenize};
use what_time::{DateOrder, Schedule};

/// Assigns `labels` to the non-whitespace tokens of `text`, marking clause
/// starts at the given source offsets.
fn oracle(text: &str, labels: &[&str], starts: &[usize]) -> Vec<Token> {
    let mut label_index = 0;
    tokenize(text)
        .into_iter()
        .map(|raw| {
            let label = if raw.kind == 3 {
                Role::O
            } else {
                let name = labels.get(label_index).copied().unwrap_or("O");
                label_index += 1;
                Role::from_name(name).unwrap_or_else(|| panic!("unknown label {name}"))
            };
            let clause_start = starts.contains(&raw.start);
            Token {
                raw,
                label,
                clause_start,
                score: 1.0,
            }
        })
        .collect()
}

fn compile(text: &str, tokens: Vec<Token>) -> Vec<Expression> {
    compile_predictions(text, &tokens, DateOrder::MDY)
}

fn compile_ordered(text: &str, tokens: Vec<Token>, order: DateOrder) -> Vec<Expression> {
    compile_predictions(text, &tokens, order)
}

fn expected_schedule(expected: Value) -> Option<Schedule> {
    serde_json::from_value(expected).unwrap()
}

fn assert_schedule(text: &str, tokens: Vec<Token>, expected: Value) {
    let expressions = compile(text, tokens);
    assert_eq!(
        serde_json::to_value(expressions[0].schedule.as_ref()).unwrap(),
        serde_json::to_value(expected_schedule(expected).as_ref()).unwrap(),
        "{text}"
    );
}

fn diagnostic_codes(expressions: &[Expression]) -> Vec<&str> {
    expressions[0]
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.as_str())
        .collect()
}

#[test]
fn composes_a_quantity_and_unit_as_a_duration_without_an_introducer() {
    let text = "90 days";
    assert_schedule(
        text,
        oracle(text, &["NUM", "UNIT"], &[]),
        json!({"clauses": [{"duration": {"amount": 90, "unit": "day"}}]}),
    );
    let dated = "tomorrow two hours";
    assert_schedule(
        dated,
        oracle(dated, &["REL_DAY", "NUM", "UNIT"], &[]),
        json!({"clauses": [{
            "date": {"kind": "relativeDay", "offset": 1},
            "duration": {"amount": 2, "unit": "hour"},
        }]}),
    );
}

#[test]
fn compiles_v2_date_selectors_and_quantity_labeled_durations() {
    for marker in ["तारीख", "tareek", "tareekh", "taareekh"] {
        let text = format!("15 {marker} ko");
        assert_schedule(&text, oracle(&text, &["DOM", "UNIT", "GLUE"], &[]),
            json!({"clauses": [{"date": {"kind": "calendar", "day": 15}}]}));
        let text = format!("har mahine ki 5 {marker}");
        assert_schedule(&text, oracle(&text, &["RECUR", "UNIT", "GLUE", "DOM", "UNIT"], &[]),
            json!({"clauses": [{"recurrence": {"freq": "monthly", "interval": 1, "byMonthDay": [5]}}]}));
    }
    for (text, labels, amount) in [
        ("for 10 mins", vec!["GLUE", "DUR", "UNIT"], 10),
        ("पूरे ३० मिनट", vec!["GLUE", "DUR", "UNIT"], 30),
        ("bas 5 mins ka kaam", vec!["O", "DUR", "UNIT", "GLUE", "O"], 5),
    ] {
        assert_schedule(text, oracle(text, &labels, &[]),
            json!({"clauses": [{"duration": {"amount": amount, "unit": "minute"}}]}));
    }
    let invalid = "for 0 mins";
    assert!(compile(invalid, oracle(invalid, &["GLUE", "DUR", "UNIT"], &[]))[0].schedule.is_none());
    let marker = "tareekh";
    assert!(compile(marker, oracle(marker, &["UNIT"], &[]))[0].schedule.is_none());
}

#[test]
fn compiles_v2_numerals_and_preserves_fractional_clock_precedence() {
    for (text, labels, hour, minute) in [
        ("शाम को ८ बजे", vec!["DAYPART", "GLUE", "HOUR", "MERIDIEM"], 20, 0),
        ("शाम को सवा ८ बजे", vec!["DAYPART", "GLUE", "CLOCK_OFFSET", "HOUR", "MERIDIEM"], 8, 15),
        ("raat ko dedh baje", vec!["DAYPART", "GLUE", "CLOCK_OFFSET", "MERIDIEM"], 1, 30),
        ("शाम को ८ : ३० am", vec!["DAYPART", "GLUE", "HOUR", "GLUE", "MINUTE", "MERIDIEM"], 8, 30),
        ("सवा ४ बजे", vec!["CLOCK_OFFSET", "HOUR", "MERIDIEM"], 4, 15),
    ] {
        assert_schedule(text, oracle(text, &labels, &[]),
            json!({"clauses": [{"time": {"start": {"hour": hour, "minute": minute}}}]}));
    }
    for word in ["roz", "roj", "rozz", "रोज़"] {
        assert_schedule(word, oracle(word, &["RECUR"], &[]),
            json!({"clauses": [{"recurrence": {"freq": "daily", "interval": 1}}]}));
    }
}

#[test]
fn compiles_corporate_units_and_casual_shorthand() {
    for (word, unit) in [("EOD", "day"), ("COB", "day"), ("EOW", "week"), ("EOM", "month")] {
        assert_schedule(word, oracle(word, &["UNIT"], &[]),
            json!({"clauses": [{"date": {"kind": "relativeUnit", "unit": unit, "modifier": "this", "edge": "end"}}]}));
    }
    assert_schedule("2MRW", oracle("2MRW", &["REL_DAY"], &[]),
        json!({"clauses": [{"date": {"kind": "relativeDay", "offset": 1}}]}));
    assert_schedule("evng", oracle("evng", &["DAYPART"], &[]),
        json!({"clauses": [{"time": {"start": {"part": "evening"}}}]}));
}

#[test]
fn hindi_relative_days_keep_background_tense_context() {
    for (text, offset) in [("कल आया था", -1), ("कल आना है", 1),
                           ("parso gaya tha", -2), ("parso aana hai", 2),
                           ("kl aaya tha", -1), ("kal Nathan aayega", 1),
                           ("kal aana tha", -1)] {
        assert_schedule(text, oracle(text, &["REL_DAY", "O", "O"], &[]),
            json!({"clauses": [{"date": {"kind": "relativeDay", "offset": offset}}]}));
    }
    let text = "कल आया था और परसों आएगा";
    let results = compile(text, oracle(text, &["REL_DAY", "O", "O", "O", "REL_DAY", "O"], &[]));
    assert_eq!(results.len(), 2);
    for (expression, offset) in results.iter().zip([-1, 2]) {
        assert_eq!(serde_json::to_value(&expression.schedule).unwrap(),
            json!({"clauses": [{"date": {"kind": "relativeDay", "offset": offset}}]}));
    }
}

#[test]
fn devanagari_numbers_share_features_without_changing_source_spans() {
    let native = tokenize("१५ तारीख ८:3० pm 2MRW");
    let ascii = tokenize("15 तारीख 8:30 pm 2MRW");
    assert_eq!(native.len(), ascii.len());
    for (left, right) in native.iter().zip(&ascii) {
        assert_eq!(left.features, right.features);
        assert_eq!(&"१५ तारीख ८:3० pm 2MRW"[left.start..left.end], left.text);
    }
    assert_eq!(native.last().unwrap().text, "2MRW");
    assert_eq!(tokenize("24th").iter().map(|t| t.text.as_str()).collect::<Vec<_>>(), ["24", "th"]);
    assert_eq!(tokenize("2mrwx").iter().map(|t| t.text.as_str()).collect::<Vec<_>>(), ["2", "mrwx"]);
    let text = "०४ / ०५ / २०२६";
    let tokens = oracle(text, &["MONTH", "GLUE", "DOM", "GLUE", "YEAR"], &[]);
    let result = compile_ordered(text, tokens, DateOrder::DMY);
    assert_eq!(serde_json::to_value(&result[0].schedule).unwrap(),
        json!({"clauses": [{"date": {"kind": "calendar", "month": 5, "day": 4, "year": 2026}}]}));
}

#[test]
fn rejects_multiple_duration_values_instead_of_silently_replacing_one() {
    let text = "for two hours for three minutes";
    let expressions = compile(
        text,
        oracle(text, &["DUR", "NUM", "UNIT", "DUR", "NUM", "UNIT"], &[]),
    );
    assert!(expressions[0].schedule.is_none());
    assert!(diagnostic_codes(&expressions).contains(&"conflicting-duration"));
}

#[test]
fn assembles_a_clock_period_identified_by_the_model() {
    for (text, hour) in [
        ("two in the afternoon", 14),
        ("five in the morning", 5),
        ("seven in the evening", 19),
        ("twelve in the morning", 0),
        ("twelve in the afternoon", 12),
        ("two in afternoon", 14),
    ] {
        let labels: Vec<&str> = tokenize(text)
            .iter()
            .filter(|token| token.kind != 3)
            .map(|token| if token.start == 0 { "HOUR" } else { "MERIDIEM" })
            .collect();
        let first_is_hour = labels.first() == Some(&"HOUR");
        assert!(first_is_hour, "{text}");
        assert_schedule(
            text,
            oracle(text, &labels, &[]),
            json!({"clauses": [{"time": {"start": {"hour": hour, "minute": 0}}}]}),
        );
    }
}

#[test]
fn ignores_model_labeled_filler_inside_semantic_values_while_retaining_source_spans() {
    let text = "twelve in the afternoon";
    let mut tokens = oracle(text, &["HOUR", "MERIDIEM", "GLUE", "MERIDIEM"], &[]);
    let glue = tokens
        .iter_mut()
        .find(|token| token.raw.text == "the")
        .unwrap();
    glue.score = 0.01;
    let expressions = compile(text, tokens);
    assert_eq!(
        serde_json::to_value(expressions[0].schedule.as_ref()).unwrap(),
        json!({"clauses": [{"time": {"start": {"hour": 12, "minute": 0}}}]}),
        "{text}"
    );
    assert_eq!(expressions[0].text, text);
    assert_eq!(expressions[0].start, 0);
    assert_eq!(expressions[0].end, text.len());
    assert_eq!(expressions[0].confidence, 1.0);
}

#[test]
fn assembles_a_learned_relative_quantity_range_and_rejects_reversed_bounds() {
    let labels = ["DIR_AFTER", "NUM", "RANGE_END", "NUM", "UNIT"];
    let text = "in 5 to 10 minutes";
    assert_schedule(
        text,
        oracle(text, &labels, &[]),
        json!({"clauses": [{
            "shift": {"amount": 5, "endAmount": 10, "unit": "minute", "direction": "after"},
        }]}),
    );
    let reversed = "in 10 to 5 minutes";
    assert!(
        compile(reversed, oracle(reversed, &labels, &[]))[0]
            .schedule
            .is_none()
    );
}

#[test]
fn distributes_each_time_window_to_its_adjacent_weekday_list_without_connectors() {
    let text = "Sat Sun 1pm-8pm Mon 10pm-12am";
    let tokens = oracle(
        text,
        &[
            "WEEKDAY",
            "WEEKDAY",
            "HOUR",
            "MERIDIEM",
            "RANGE_END",
            "HOUR",
            "MERIDIEM",
            "WEEKDAY",
            "HOUR",
            "MERIDIEM",
            "RANGE_END",
            "HOUR",
            "MERIDIEM",
        ],
        &[text.find("Mon").unwrap()],
    );
    let expressions = compile(text, tokens);
    let expression = &expressions[0];
    assert_eq!(expression.start, 0);
    assert_eq!(expression.end, text.len());
    assert_eq!(expression.text, text);
    assert_eq!(
        serde_json::to_value(expression.schedule.as_ref()).unwrap(),
        serde_json::to_value(
            expected_schedule(json!({"clauses": [
                {
                    "date": {"kind": "weekday", "days": ["SA", "SU"]},
                    "time": {
                        "start": {"hour": 13, "minute": 0},
                        "end": {"hour": 20, "minute": 0},
                    },
                },
                {
                    "date": {"kind": "weekday", "days": ["MO"]},
                    "time": {
                        "start": {"hour": 22, "minute": 0},
                        "end": {"hour": 0, "minute": 0},
                    },
                },
            ]}))
            .as_ref()
        )
        .unwrap()
    );
    assert!(expression.diagnostics.is_empty());
}

#[test]
fn preserves_explicit_recurrence_intervals_bounds_and_excluded_weekdays() {
    let text = "every other Tuesday until Dec except Friday";
    let tokens = oracle(
        text,
        &[
            "RECUR",
            "NUM",
            "WEEKDAY",
            "BOUND_END",
            "MONTH",
            "EXCEPT",
            "WEEKDAY",
        ],
        &[],
    );
    assert_schedule(
        text,
        tokens,
        json!({"clauses": [{
            "recurrence": {
                "freq": "weekly",
                "interval": 2,
                "byDay": ["TU"],
                "until": {"kind": "calendar", "month": 12},
                "except": [{"kind": "weekday", "days": ["FR"]}],
            },
        }]}),
    );
}

#[test]
fn retains_a_relative_amount_and_its_named_anchor_instead_of_resolving_now() {
    let text = "two hours before tomorrow at noon";
    let tokens = oracle(
        text,
        &["NUM", "UNIT", "DIR_BEFORE", "REL_DAY", "O", "TIME_NAMED"],
        &[],
    );
    assert_schedule(
        text,
        tokens,
        json!({"clauses": [{
            "date": {"kind": "relativeDay", "offset": 1},
            "time": {"start": {"named": "noon"}},
            "shift": {"amount": 2, "unit": "hour", "direction": "before"},
        }]}),
    );
}

#[test]
fn rejects_unknown_values_even_when_the_model_assigns_a_confident_temporal_label() {
    for label in ["REL_DAY", "TIME_NAMED", "DAYPART"] {
        let text = "constructor";
        assert!(
            compile(text, oracle(text, &[label], &[]))[0]
                .schedule
                .is_none(),
            "{label}"
        );
    }

    let invalid_minute = "2:banana";
    assert!(
        compile(
            invalid_minute,
            oracle(invalid_minute, &["HOUR", "O", "MINUTE"], &[]),
        )[0]
        .schedule
        .is_none()
    );
}

#[test]
fn returns_diagnostics_when_the_model_predicts_a_bound_without_an_attached_date() {
    for (text, labels) in [
        (
            "every Monday until",
            vec!["RECUR", "WEEKDAY", "BOUND_START"],
        ),
        ("every Monday until", vec!["RECUR", "WEEKDAY", "BOUND_END"]),
        ("starting", vec!["BOUND_START"]),
    ] {
        let expressions = compile(text, oracle(text, &labels, &[]));
        assert!(expressions[0].schedule.is_none());
        assert_eq!(expressions[0].diagnostics[0].code, "invalid-bound");
    }
}

#[test]
fn does_not_silently_complete_an_unfinished_range_or_recurrence() {
    let range = "Monday 5pm to";
    let expressions = compile(
        range,
        oracle(range, &["WEEKDAY", "HOUR", "MERIDIEM", "RANGE_END"], &[]),
    );
    assert_eq!(expressions[0].diagnostics[0].code, "incomplete-range");
    let recurrence = "every";
    let expressions = compile(recurrence, oracle(recurrence, &["RECUR"], &[]));
    assert_eq!(expressions[0].diagnostics[0].code, "incomplete-recurrence");
}

#[test]
fn does_not_invent_a_time_window_when_the_model_omitted_its_relationship() {
    let text = "Monday 9 Tuesday 10";
    let expressions = compile(
        text,
        oracle(text, &["WEEKDAY", "HOUR", "WEEKDAY", "HOUR"], &[]),
    );
    assert!(expressions[0].schedule.is_none());
    assert_eq!(expressions[0].diagnostics[0].code, "unlinked-times");
}

#[test]
fn infers_the_missing_period_across_noon_and_midnight_without_changing_explicit_periods() {
    for (text, labels, start, end) in [
        (
            "9am to 5",
            vec!["HOUR", "MERIDIEM", "RANGE_END", "HOUR"],
            9,
            17,
        ),
        (
            "10 to 2am",
            vec!["HOUR", "RANGE_END", "HOUR", "MERIDIEM"],
            22,
            2,
        ),
        (
            "8 to midnight",
            vec!["HOUR", "RANGE_END", "TIME_NAMED"],
            20,
            -1,
        ),
        (
            "10pm to 12pm",
            vec!["HOUR", "MERIDIEM", "RANGE_END", "HOUR", "MERIDIEM"],
            22,
            12,
        ),
    ] {
        let expressions = compile(text, oracle(text, &labels, &[]));
        let clause = &expressions[0].schedule.as_ref().unwrap().clauses[0];
        let time = clause.time.as_ref().unwrap();
        let expected_start: Value = json!({"hour": start, "minute": 0});
        assert_eq!(
            serde_json::to_value(&time.start).unwrap(),
            expected_start,
            "{text}"
        );
        let expected_end = if end < 0 {
            json!({"named": "midnight"})
        } else {
            json!({"hour": end, "minute": 0})
        };
        assert_eq!(
            serde_json::to_value(time.end.as_ref()).unwrap(),
            expected_end,
            "{text}"
        );
    }
    let equal = "9:00:00 to 9am";
    assert!(
        compile(
            equal,
            oracle(
                equal,
                &[
                    "HOUR",
                    "GLUE",
                    "MINUTE",
                    "GLUE",
                    "SECOND",
                    "RANGE_END",
                    "HOUR",
                    "MERIDIEM",
                ],
                &[],
            ),
        )[0]
        .schedule
        .is_none()
    );
}

#[test]
fn handles_malformed_model_roles_and_boundaries_without_invalid_diagnostic_offsets() {
    let text = "Monday 5 at noon every 3 days until tomorrow";
    let mut state: u32 = 123456;
    let mut random = move || {
        state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        state
    };
    for _ in 0..1000 {
        let tokens: Vec<Token> = tokenize(text)
            .into_iter()
            .map(|raw| {
                let label = if raw.kind == 3 {
                    Role::O
                } else {
                    let name = LABELS[(random() % LABELS.len() as u32) as usize];
                    Role::from_name(name).unwrap()
                };
                Token {
                    label,
                    clause_start: random() % 5 == 0,
                    score: 0.9,
                    raw,
                }
            })
            .collect();
        for expression in compile(text, tokens) {
            for diagnostic in &expression.diagnostics {
                assert!(diagnostic.start <= diagnostic.end);
                assert!(diagnostic.end <= text.len());
            }
        }
    }
}

#[test]
fn applies_numeric_date_order_only_to_ambiguous_date_fields_identified_by_the_model() {
    let text = "3/4/2026";
    let labels = ["MONTH", "GLUE", "DOM", "GLUE", "YEAR"];
    let dmy = compile_ordered(text, oracle(text, &labels, &[]), DateOrder::DMY);
    assert_eq!(
        serde_json::to_value(dmy[0].schedule.as_ref()).unwrap(),
        json!({"clauses": [{
            "date": {"kind": "calendar", "day": 3, "month": 4, "year": 2026},
        }]}),
        "{text} DMY"
    );
    let mdy = compile_ordered(text, oracle(text, &labels, &[]), DateOrder::MDY);
    assert_eq!(
        serde_json::to_value(mdy[0].schedule.as_ref()).unwrap(),
        json!({"clauses": [{
            "date": {"kind": "calendar", "month": 3, "day": 4, "year": 2026},
        }]}),
        "{text} MDY"
    );

    for (source, labels, expected) in [
        (
            "2026-3-4",
            vec!["YEAR", "GLUE", "MONTH", "GLUE", "DOM"],
            json!({"year": 2026, "month": 3, "day": 4}),
        ),
        (
            "March 4",
            vec!["MONTH", "DOM"],
            json!({"month": 3, "day": 4}),
        ),
        (
            "23/4",
            vec!["DOM", "GLUE", "MONTH"],
            json!({"month": 4, "day": 23}),
        ),
    ] {
        let expressions = compile_ordered(source, oracle(source, &labels, &[]), DateOrder::DMY);
        let mut date = json!({
            "kind": "calendar",
            "month": expected["month"],
            "day": expected["day"],
        });
        if let Some(year) = expected.get("year") {
            date["year"] = year.clone();
        }
        assert_eq!(
            serde_json::to_value(expressions[0].schedule.as_ref()).unwrap(),
            json!({"clauses": [{"date": date}]}),
            "{source}"
        );
    }
}

#[test]
fn assembles_the_uncovered_core_forms_from_explicit_semantic_labels() {
    let examples: [(&str, Vec<&str>, Value); 6] = [
        (
            "the day after tomorrow",
            vec!["REL_DAY", "REL_DAY", "REL_DAY", "REL_DAY"],
            json!({"date": {"kind": "relativeDay", "offset": 2}}),
        ),
        (
            "end of next month",
            vec!["EDGE", "GLUE", "DEICTIC", "UNIT"],
            json!({"date": {
                "kind": "relativeUnit",
                "unit": "month",
                "modifier": "next",
                "edge": "end",
            }}),
        ),
        (
            "3 weeks from now",
            vec!["NUM", "UNIT", "DIR_AFTER", "NOW"],
            json!({
                "date": {"kind": "now"},
                "shift": {"amount": 3, "unit": "week", "direction": "after"},
            }),
        ),
        (
            "a week before Christmas",
            vec!["NUM", "UNIT", "DIR_BEFORE", "HOLIDAY"],
            json!({
                "date": {"kind": "holiday", "name": "christmas"},
                "shift": {"amount": 1, "unit": "week", "direction": "before"},
            }),
        ),
        (
            "this weekend",
            vec!["DEICTIC", "DAYGROUP"],
            json!({"date": {"kind": "dayGroup", "group": "weekend", "modifier": "this"}}),
        ),
        (
            "every day through Friday",
            vec!["RECUR", "UNIT", "BOUND_END", "WEEKDAY"],
            json!({"recurrence": {
                "freq": "daily",
                "interval": 1,
                "until": {"kind": "weekday", "days": ["FR"]},
            }}),
        ),
    ];
    for (text, labels, clause) in examples {
        assert_schedule(
            text,
            oracle(text, &labels, &[]),
            json!({"clauses": [clause]}),
        );
    }
}

#[test]
fn composes_model_labeled_spoken_minutes_and_fractional_clocks() {
    let examples: [(&str, Vec<&str>, i64, i64); 3] = [
        ("eight forty", vec!["HOUR", "MINUTE"], 8, 40),
        (
            "ten thirty-five pm",
            vec!["HOUR", "MINUTE", "MINUTE", "MINUTE", "MERIDIEM"],
            22,
            35,
        ),
        (
            "quarter to twelve am",
            vec!["CLOCK_OFFSET", "GLUE", "HOUR", "MERIDIEM"],
            23,
            45,
        ),
    ];
    for (text, labels, hour, minute) in examples {
        assert_schedule(
            text,
            oracle(text, &labels, &[]),
            json!({"clauses": [{"time": {"start": {"hour": hour, "minute": minute}}}]}),
        );
    }
}

#[test]
fn keeps_a_combined_shift_distinct_from_an_occurrence_duration() {
    let text = "in two days and six hours for half an hour";
    let tokens = oracle(
        text,
        &[
            "DIR_AFTER",
            "NUM",
            "UNIT",
            "GLUE",
            "NUM",
            "UNIT",
            "DUR",
            "NUM",
            "NUM",
            "UNIT",
        ],
        &[],
    );
    assert_schedule(
        text,
        tokens,
        json!({"clauses": [{
            "shift": {
                "amount": 2,
                "unit": "day",
                "direction": "after",
                "components": [{"amount": 6, "unit": "hour"}],
            },
            "duration": {"amount": 0.5, "unit": "hour"},
        }]}),
    );
}

#[test]
fn does_not_invent_fractional_calendar_durations() {
    let text = "for 1.5 months";
    assert!(
        compile(
            text,
            oracle(text, &["DUR", "NUM", "NUM", "NUM", "UNIT"], &[]),
        )[0]
        .schedule
        .is_none()
    );
}

#[test]
fn validates_iso_date_order_independently_of_the_models_month_day_roles() {
    let text = "2026-13-01";
    let expressions = compile(
        text,
        oracle(text, &["YEAR", "GLUE", "DOM", "GLUE", "MONTH"], &[]),
    );
    assert!(expressions[0].schedule.is_none());
    assert!(
        expressions[0]
            .diagnostics
            .iter()
            .any(|value| value.code == "invalid-date")
    );
}

#[test]
fn reports_a_missing_recurrence_bound_when_until_is_recognized_as_a_range_separator() {
    let text = "every Monday until";
    let expressions = compile(text, oracle(text, &["RECUR", "WEEKDAY", "RANGE_END"], &[]));
    assert!(expressions[0].schedule.is_none());
    assert!(
        expressions[0]
            .diagnostics
            .iter()
            .any(|value| value.code == "invalid-bound")
    );
}

#[test]
fn reads_an_open_upper_bound_from_a_bare_direction_token() {
    let text = "after 6pm";
    assert_schedule(
        text,
        oracle(text, &["DIR_AFTER", "HOUR", "MERIDIEM"], &[]),
        json!({"clauses": [{"time": {"start": {"hour": 18, "minute": 0}, "open": "end"}}]}),
    );
}

#[test]
fn floors_an_open_lower_bound_at_midnight() {
    let text = "before 6pm";
    assert_schedule(
        text,
        oracle(text, &["DIR_BEFORE", "HOUR", "MERIDIEM"], &[]),
        json!({"clauses": [{
            "time": {
                "start": {"hour": 0, "minute": 0},
                "end": {"hour": 18, "minute": 0},
                "open": "start",
            },
        }]}),
    );
}

#[test]
fn rejects_an_open_bound_with_no_clock_instead_of_dropping_the_direction() {
    let text = "after Friday";
    let expressions = compile(text, oracle(text, &["DIR_AFTER", "WEEKDAY"], &[]));
    assert!(expressions[0].schedule.is_none());
    assert!(diagnostic_codes(&expressions).contains(&"open-bound-needs-time"));
}

#[test]
fn rejects_an_open_bound_applied_to_a_range() {
    let text = "after 8 to 10pm";
    let expressions = compile(
        text,
        oracle(
            text,
            &["DIR_AFTER", "HOUR", "RANGE_END", "HOUR", "MERIDIEM"],
            &[],
        ),
    );
    assert!(expressions[0].schedule.is_none());
    assert!(diagnostic_codes(&expressions).contains(&"open-bound-needs-time"));
}

#[test]
fn marks_a_bare_range_start_as_open_rather_than_returning_a_bare_instant() {
    let open = "from 6pm";
    assert_schedule(
        open,
        oracle(open, &["RANGE_START", "HOUR", "MERIDIEM"], &[]),
        json!({"clauses": [{"time": {"start": {"hour": 18, "minute": 0}, "open": "end"}}]}),
    );

    let closed = "from 8 to 10pm";
    assert_schedule(
        closed,
        oracle(
            closed,
            &["RANGE_START", "HOUR", "RANGE_END", "HOUR", "MERIDIEM"],
            &[],
        ),
        json!({"clauses": [{
            "time": {"start": {"hour": 20, "minute": 0}, "end": {"hour": 22, "minute": 0}},
        }]}),
    );
}

#[test]
fn accepts_the_article_less_day_after_tomorrow_through_the_phrase_table() {
    // The transformer labels the bare phrasing exactly like the trained
    // "the day after tomorrow" — all REL_DAY — so the relative-day table
    // entry (added alongside the article-less forms) resolves it.
    let after = "day after tomorrow";
    assert_schedule(
        after,
        oracle(after, &["REL_DAY", "REL_DAY", "REL_DAY"], &[]),
        json!({"clauses": [{"date": {"kind": "relativeDay", "offset": 2}}]}),
    );

    let before = "day before yesterday";
    assert_schedule(
        before,
        oracle(before, &["REL_DAY", "REL_DAY", "REL_DAY"], &[]),
        json!({"clauses": [{"date": {"kind": "relativeDay", "offset": -2}}]}),
    );

    // A "day" labeled as a bare unit with nothing anchoring it is rejected.
    let bare = "day";
    let expressions = compile(bare, oracle(bare, &["UNIT"], &[]));
    assert!(expressions[0].schedule.is_none());
    assert!(diagnostic_codes(&expressions).contains(&"unsupported"));
}

#[test]
fn merges_a_deictic_period_with_a_calendar_or_weekday_into_one_anchored_date() {
    // "24th august last year": DEICTIC + UNIT + DOM + MONTH used to conflict.
    let dated = "24th august last year";
    assert_schedule(
        dated,
        oracle(dated, &["DOM", "GLUE", "MONTH", "DEICTIC", "UNIT"], &[]),
        json!({"clauses": [{
            "date": {"kind": "periodCalendar", "period": "year", "modifier": "last", "month": 8, "day": 24},
        }]}),
    );

    // "15th last month": the period picks month and year, the ordinal the day.
    let month_anchored = "15th last month";
    assert_schedule(
        month_anchored,
        oracle(month_anchored, &["DOM", "GLUE", "DEICTIC", "UNIT"], &[]),
        json!({"clauses": [{
            "date": {"kind": "periodCalendar", "period": "month", "modifier": "last", "day": 15},
        }]}),
    );

    // "friday last week": the weekday anchors to the deictic week.
    let week_anchored = "friday last week";
    assert_schedule(
        week_anchored,
        oracle(week_anchored, &["WEEKDAY", "DEICTIC", "UNIT"], &[]),
        json!({"clauses": [{
            "date": {"kind": "periodWeekday", "modifier": "last", "days": ["FR"]},
        }]}),
    );

    // Plain deictic weekdays keep their own reading.
    let plain = "last friday";
    assert_schedule(
        plain,
        oracle(plain, &["DEICTIC", "WEEKDAY"], &[]),
        json!({"clauses": [{"date": {"kind": "weekday", "days": ["FR"], "modifier": "last"}}]}),
    );
}

#[test]
fn a_day_part_qualifies_a_clock_in_either_position() {
    // Before the clock, with minutes ("evening 9:30" -> 21:30).
    let before = "evening 9:30";
    assert_schedule(
        before,
        oracle(before, &["DAYPART", "HOUR", "GLUE", "MINUTE"], &[]),
        json!({"clauses": [{"time": {"start": {"hour": 21, "minute": 30}}}]}),
    );

    // After the clock, labeled as a day part ("9:30 evening").
    let after = "9:30 evening";
    assert_schedule(
        after,
        oracle(after, &["HOUR", "GLUE", "MINUTE", "DAYPART"], &[]),
        json!({"clauses": [{"time": {"start": {"hour": 21, "minute": 30}}}]}),
    );

    // After the clock, labeled as a meridiem ("9:30 night").
    let as_meridiem = "9:30 night";
    assert_schedule(
        as_meridiem,
        oracle(as_meridiem, &["HOUR", "GLUE", "MINUTE", "MERIDIEM"], &[]),
        json!({"clauses": [{"time": {"start": {"hour": 21, "minute": 30}}}]}),
    );

    // Midnight edge: 12 at night is am, 12 in the morning is noon-adjacent am.
    let midnight = "12 night";
    assert_schedule(
        midnight,
        oracle(midnight, &["HOUR", "MERIDIEM"], &[]),
        json!({"clauses": [{"time": {"start": {"hour": 0, "minute": 0}}}]}),
    );
}

#[test]
fn compiles_a_postposed_hindi_date_range() {
    // Hindi/Hinglish postpose both range markers: से follows the start
    // date, तक closes after the end date ("20 तारीख से 24 तारीख तक").
    let text = "इंटरव्यू 24 तारीख से 28 तारीख तक रात को 11 बजे";
    assert_schedule(
        text,
        oracle(
            text,
            &[
                "O", "DOM", "UNIT", "RANGE_START", "DOM", "UNIT", "RANGE_END", "DAYPART", "GLUE",
                "HOUR", "MERIDIEM",
            ],
            &[],
        ),
        json!({"clauses": [{
            "date": {"kind": "calendarRange", "from": {"day": 24}, "to": {"day": 28}},
            "time": {"start": {"hour": 23, "minute": 0}},
        }]}),
    );

    // Hinglish, same construction in Latin script.
    let hinglish = "leave 15 tareekh se 20 tareekh tak";
    assert_schedule(
        hinglish,
        oracle(
            hinglish,
            &["O", "DOM", "UNIT", "RANGE_START", "DOM", "UNIT", "RANGE_END"],
            &[],
        ),
        json!({"clauses": [{
            "date": {"kind": "calendarRange", "from": {"day": 15}, "to": {"day": 20}},
        }]}),
    );

    // A trailing तक without calendar material is still unfinished.
    let unfinished = "Monday 5pm tak";
    let expressions = compile(
        unfinished,
        oracle(unfinished, &["WEEKDAY", "HOUR", "MERIDIEM", "RANGE_END"], &[]),
    );
    assert_eq!(expressions[0].diagnostics[0].code, "incomplete-range");
}

#[test]
fn compiles_tarikh_date_selectors_and_postposed_clock_ranges() {
    // "tarikh" is the single-a romanization of तारीख; it names a DOM
    // selector like its double-e spellings.
    let text = "December ki 14 tarikh ko";
    assert_schedule(
        text,
        oracle(text, &["MONTH", "GLUE", "DOM", "UNIT", "GLUE"], &[]),
        json!({"clauses": [{
            "date": {"kind": "calendar", "month": 12, "day": 14},
        }]}),
    );
    let bare = "22 tarikh sava char baje";
    assert_schedule(
        bare,
        oracle(bare, &["DOM", "UNIT", "CLOCK_OFFSET", "HOUR", "MERIDIEM"], &[]),
        json!({"clauses": [{
            "date": {"kind": "calendar", "day": 22},
            "time": {"start": {"hour": 4, "minute": 15}},
        }]}),
    );

    // Postposed clock range: से/तक bracket the pair from the outside.
    let hinglish = "sava char se paune paanch tak";
    assert_schedule(
        hinglish,
        oracle(
            hinglish,
            &["CLOCK_OFFSET", "HOUR", "RANGE_START", "CLOCK_OFFSET", "HOUR", "RANGE_END"],
            &[],
        ),
        json!({"clauses": [{
            "time": {"start": {"hour": 4, "minute": 15}, "end": {"hour": 4, "minute": 45}},
        }]}),
    );

    // Without its opening से, a trailing तक cannot complete a range.
    let unlinked = "3 baje 5 baje tak";
    let expressions = compile(
        unlinked,
        oracle(unlinked, &["HOUR", "MERIDIEM", "HOUR", "MERIDIEM", "RANGE_END"], &[]),
    );
    assert_eq!(expressions[0].diagnostics[0].code, "incomplete-range");
}

#[test]
fn compiles_inflected_hindi_ordinals_as_recurrence_positions() {
    let text = "हर महीने के दूसरे सोमवार को";
    assert_schedule(
        text,
        oracle(text, &["RECUR", "UNIT", "GLUE", "ORD", "WEEKDAY", "GLUE"], &[]),
        json!({"clauses": [{
            "recurrence": {"freq": "monthly", "interval": 1, "byDay": ["MO"], "bySetPos": [2]},
        }]}),
    );
}

#[test]
fn compiles_quarter_periods_and_recurrences() {
    // "quarter" after a deictic names the calendar period.
    let text = "next quarter";
    assert_schedule(
        text,
        oracle(text, &["DEICTIC", "UNIT"], &[]),
        json!({"clauses": [{
            "date": {"kind": "relativeUnit", "unit": "quarter", "modifier": "next"},
        }]}),
    );
    // The same word in its clock sense is untouched.
    let clock = "quarter past four";
    assert_schedule(
        clock,
        oracle(clock, &["CLOCK_OFFSET", "CLOCK_OFFSET", "HOUR"], &[]),
        json!({"clauses": [{"time": {"start": {"hour": 4, "minute": 15}}}]}),
    );
    // "every quarter" is a quarterly recurrence.
    let recur = "every quarter";
    assert_schedule(
        recur,
        oracle(recur, &["RECUR", "UNIT"], &[]),
        json!({"clauses": [{
            "recurrence": {"freq": "quarterly", "interval": 1},
        }]}),
    );
    // A unit-labeled word that names a holiday is that holiday.
    let diwali = "दिवाली को मिलते हैं";
    assert_schedule(
        diwali,
        oracle(diwali, &["UNIT", "GLUE", "O", "O"], &[]),
        json!({"clauses": [{
            "date": {"kind": "holiday", "name": "diwali"},
        }]}),
    );
}

#[test]
fn compiles_data_asset_holidays() {
    let text = "family dinner on Thanksgiving";
    assert_schedule(
        text,
        oracle(text, &["O", "O", "GLUE", "HOLIDAY"], &[]),
        json!({"clauses": [{"date": {"kind": "holiday", "name": "thanksgiving"}}]}),
    );
    let uk = "closed Good Friday";
    assert_schedule(
        uk,
        oracle(uk, &["O", "HOLIDAY", "HOLIDAY"], &[]),
        json!({"clauses": [{"date": {"kind": "holiday", "name": "good-friday"}}]}),
    );
}
