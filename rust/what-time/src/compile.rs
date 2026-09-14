//! Compiles predicted roles into typed schedules.

use crate::labels::Role;
use crate::lexicon::{compound_ordinal, holiday_name, month, number, unit, weekday};
use crate::quantity::{read_duration, read_number};
use crate::types::{
    CalendarDate, Clause, ClockTime, DateSpec, DayGroup, DayPart, Diagnostic, Direction, Duration,
    Edge, Expression, Frequency, Modifier, NamedClock, OpenBound, Recurrence, Shift, TimeSpec,
    Token, Unit, WEEKDAYS, Weekday, weekday_index,
};

fn filler(word: &str) -> bool {
    matches!(
        word,
        "at" | "on"
            | "the"
            | "of"
            | "a"
            | "an"
            | "and"
            | "then"
            | "from"
            | "for"
            | "end"
            | "start"
            | ","
            | ";"
            | "&"
            | ":"
            | "-"
            | "–"
            | "—"
            | "."
            | "st"
            | "nd"
            | "rd"
            | "th"
            | "को"
            | "में"
            | "पर"
            | "का"
            | "की"
            | "के"
            | "ko"
            | "me"
            | "mein"
            | "pe"
            | "par"
            | "ka"
            | "ki"
            | "ke"
    )
}

fn relative_day(phrase: &str) -> Option<i64> {
    match phrase {
        "today" | "tonight" | "tonite" | "आज" | "aaj" => Some(0),
        "tomorrow" | "tmrw" | "tmr" => Some(1),
        "yesterday" => Some(-1),
        "the day after tomorrow" | "day after tomorrow" => Some(2),
        "the day before yesterday" | "day before yesterday" => Some(-2),
        _ => None,
    }
}

fn hindi_relative(phrase: &str) -> Option<(i64, i64)> {
    match phrase {
        "कल" | "kal" => Some((1, -1)),
        "परसों" | "parso" | "parson" => Some((2, -2)),
        _ => None,
    }
}

fn hindi_relative_offset(pair: (i64, i64), tokens: &[Token]) -> i64 {
    let joined = tokens
        .iter()
        .map(|token| token.raw.text.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    let past = [
        "था",
        "आया था",
        "गया था",
        "हुआ था",
        "किया था",
        "बीता हुआ",
        "थी",
        "tha",
        "aaya tha",
        "gaya tha",
        "hua tha",
        "kiya tha",
        "beeta hua",
        "thi",
    ]
    .iter()
    .any(|cue| joined.contains(cue));
    if past { pair.1 } else { pair.0 }
}

fn day_part(word: &str) -> Option<DayPart> {
    match word {
        "morning" | "सुबह" | "subah" => Some(DayPart::morning),
        "afternoon" | "दोपहर" | "dopahar" => Some(DayPart::afternoon),
        "evening" | "शाम" | "shaam" => Some(DayPart::evening),
        "night" | "रात" | "raat" => Some(DayPart::night),
        _ => None,
    }
}

fn modifier_word(word: &str) -> Option<Modifier> {
    match word {
        "this" | "इस" | "यह" | "is" | "yeh" => Some(Modifier::this),
        "next" | "coming" | "upcoming" | "अगला" | "अगले" | "अगली" | "agla" | "agle" | "agli" => {
            Some(Modifier::next)
        }
        "last" | "previous" | "past" | "पिछला" | "पिछले" | "पिछली" | "pichhla" | "pichhle"
        | "pichhli" => Some(Modifier::last),
        _ => None,
    }
}

fn frequency_word(word: &str) -> Option<(Frequency, bool)> {
    // (frequency, doubles the interval)
    match word {
        "hourly" => Some((Frequency::hourly, false)),
        "daily" | "roz" | "रोज़" => Some((Frequency::daily, false)),
        "weekly" => Some((Frequency::weekly, false)),
        "biweekly" | "fortnightly" => Some((Frequency::weekly, true)),
        "monthly" => Some((Frequency::monthly, false)),
        "yearly" | "annually" => Some((Frequency::yearly, false)),
        _ => None,
    }
}

fn unit_frequency(unit: Unit) -> Option<Frequency> {
    match unit {
        Unit::hour => Some(Frequency::hourly),
        Unit::day => Some(Frequency::daily),
        Unit::week => Some(Frequency::weekly),
        Unit::month => Some(Frequency::monthly),
        Unit::year => Some(Frequency::yearly),
        Unit::minute => None,
    }
}

fn clock_period(word: &str) -> Option<&'static str> {
    match word {
        "inmorning" | "inthemorning" => Some("am"),
        "inafternoon" | "intheafternoon" => Some("pm"),
        "inevening" | "intheevening" => Some("pm"),
        "atnight" | "inthenight" => Some("pm"),
        _ => None,
    }
}

fn is_integer(value: f64) -> bool {
    value.is_finite() && value.fract() == 0.0
}

struct CompileError {
    diagnostic: Diagnostic,
}

type CompileResult<T> = Result<T, CompileError>;

fn diagnostic(
    token: &Token,
    code: &str,
    message: &str,
    severity: crate::types::Severity,
) -> Diagnostic {
    Diagnostic {
        code: code.to_string(),
        message: message.to_string(),
        start: token.raw.start,
        end: token.raw.end,
        severity,
    }
}

fn fail<T>(token: &Token, code: &str, message: &str) -> CompileResult<T> {
    Err(CompileError {
        diagnostic: diagnostic(token, code, message, crate::types::Severity::error),
    })
}

#[derive(Clone)]
struct ParsedClock {
    value: ClockTime,
    token: Token,
    meridiem: Option<String>,
    needs_meridiem: bool,
}

fn read_clock(tokens: &[Token], index: usize) -> CompileResult<(ParsedClock, usize)> {
    let token = tokens[index].clone();
    let word = token.raw.text.to_lowercase();
    let hour = number(&word);
    let mut minute: f64 = 0.0;
    let mut second: Option<f64> = None;
    let mut meridiem: Option<String> = None;
    let mut next = index + 1;

    if tokens.get(next).map(|t| t.raw.text.as_str()) == Some(":")
        && tokens
            .get(next + 1)
            .is_some_and(|t| t.label == Role::Minute)
    {
        minute = number(&tokens[next + 1].raw.text.to_lowercase()).unwrap_or(f64::NAN);
        next += 2;
    }

    if tokens.get(next).is_some_and(|t| t.label == Role::Minute) {
        let (spoken, spoken_next) = read_number(tokens, next, Role::Minute);
        minute = spoken.unwrap_or(f64::NAN);
        next = spoken_next;
    }

    if tokens.get(next).map(|t| t.raw.text.as_str()) == Some(":")
        && tokens
            .get(next + 1)
            .is_some_and(|t| t.label == Role::Second)
    {
        second = Some(number(&tokens[next + 1].raw.text.to_lowercase()).unwrap_or(f64::NAN));
        next += 2;
    }

    while tokens.get(next).is_some_and(|t| t.label == Role::Meridiem) {
        let part = tokens[next].raw.text.to_lowercase().replace('.', "");
        meridiem = Some(meridiem.unwrap_or_default() + &part);
        next += 1;
    }
    if let Some(meridiem_value) = meridiem.take() {
        let mut value = meridiem_value.replace('’', "'");
        if let Some(stripped) = value.strip_prefix("oclock") {
            value = format!("o'clock{stripped}");
        }
        if value.starts_with("o'clock") && value.len() > 7 {
            value = value[7..].to_string();
        }
        let hour_value = hour.unwrap_or(f64::NAN);
        if hour_value == 12.0 && (value == "atnight" || value == "inthenight") {
            value = "am".to_string();
        }
        let mut value = clock_period(&value).map(str::to_string).unwrap_or(value);
        // A bare day-part word can arrive as the meridiem ("9:30 night"):
        // morning maps to am, the rest to pm, with midnight at 12.
        if matches!(
            value.as_str(),
            "morning" | "afternoon" | "evening" | "night"
        ) {
            let pm = value != "morning";
            let crossed_midnight = hour_value == 12.0 && value == "night";
            value = if crossed_midnight || !pm {
                "am".to_string()
            } else {
                "pm".to_string()
            };
        }
        if value == "बजे" || value == "baje" {
            value = "o'clock".to_string();
        }
        meridiem = Some(value);
    }

    let invalid_hour =
        !hour.is_some_and(|value| is_integer(value) && (0.0..=23.0).contains(&value));
    let invalid_minute = !is_integer(minute) || !(0.0..=59.0).contains(&minute);
    let invalid_second =
        second.is_some_and(|value| !is_integer(value) || !(0.0..=59.0).contains(&value));
    let invalid_meridiem = meridiem.as_ref().is_some_and(|value| {
        !matches!(value.as_str(), "am" | "pm" | "o'clock")
            || !hour.is_some_and(|hour| (1.0..=12.0).contains(&hour))
    });

    if invalid_hour || invalid_minute || invalid_second || invalid_meridiem {
        return fail(&token, "invalid-time", "Clock components are out of range.");
    }

    let mut hour_value = hour.unwrap();
    if meridiem.as_deref() == Some("am") || meridiem.as_deref() == Some("pm") {
        hour_value = (hour_value % 12.0)
            + if meridiem.as_deref() == Some("pm") {
                12.0
            } else {
                0.0
            };
    }

    let value = ClockTime::Hm {
        hour: hour_value as i64,
        minute: minute as i64,
        second: second.map(|value| value as i64),
    };
    // "o'clock" and its equivalents (बजे/baje) mark a bare clock reading
    // without committing to am or pm, so a preceding day part ("in the
    // evening", "शाम को") still gets to bias the hour. Minutes do not
    // disambiguate anything: "evening 9:30" needs the bias just like
    // "evening 9".
    let needs_meridiem = !matches!(meridiem.as_deref(), Some("am") | Some("pm"))
        && hour_value > 0.0
        && hour_value <= 12.0;

    Ok((
        ParsedClock {
            value,
            token,
            meridiem,
            needs_meridiem,
        },
        next,
    ))
}

fn leading_zero_clock(clock: &ParsedClock) -> bool {
    let bytes = clock.token.raw.text.as_bytes();
    bytes.len() >= 2 && bytes[0] == b'0' && bytes[1].is_ascii_digit()
}

fn inherit_meridiem(
    clock: Option<&mut ParsedClock>,
    partner: Option<&ParsedClock>,
    is_start: bool,
) {
    let Some(clock) = clock else { return };
    let Some(partner) = partner else { return };
    if clock.meridiem.is_some() || leading_zero_clock(clock) {
        return;
    }
    let Some((hour, minute, second)) = clock.value.as_hm() else {
        return;
    };
    if !(1..=12).contains(&hour) {
        return;
    }
    let partner_meridiem_ok = matches!(partner.meridiem.as_deref(), Some("am") | Some("pm"));
    let partner_is_named = matches!(partner.value, ClockTime::Named { .. });
    if !partner_meridiem_ok && !partner_is_named {
        return;
    }
    let Some(fixed) = literal_seconds(&partner.value) else {
        return;
    };
    let base = hour % 12;
    let minute_seconds = (minute * 60 + second.unwrap_or(0)) as f64;
    let mut candidates: [(i64, f64); 2] = [(0, 0.0); 2];
    for (index, candidate_hour) in [base, base + 12].iter().enumerate() {
        let seconds = (*candidate_hour as f64) * 3600.0 + minute_seconds;
        let elapsed = if is_start {
            fixed - seconds
        } else {
            seconds - fixed
        };
        candidates[index] = (*candidate_hour, (elapsed + 86_400.0) % 86_400.0);
    }
    let chosen = if candidates[0].1 <= candidates[1].1 {
        candidates[0].0
    } else {
        candidates[1].0
    };
    if let ClockTime::Hm { hour, .. } = &mut clock.value {
        *hour = chosen;
    }
    clock.needs_meridiem = false;
}

fn literal_seconds(clock: &ClockTime) -> Option<f64> {
    match clock {
        ClockTime::Named { named } => Some(if *named == NamedClock::noon {
            43_200.0
        } else {
            0.0
        }),
        ClockTime::Hm {
            hour,
            minute,
            second,
        } => Some((*hour * 3600 + *minute * 60 + second.unwrap_or(0)) as f64),
        ClockTime::Part { .. } => None,
    }
}

fn json_equal(left: &ClockTime, right: &ClockTime) -> bool {
    serde_json::to_string(left).ok() == serde_json::to_string(right).ok()
}

fn compile_time(
    clocks: &mut [ParsedClock],
    diagnostics: &mut Vec<Diagnostic>,
) -> CompileResult<Option<TimeSpec>> {
    if clocks.is_empty() {
        return Ok(None);
    }
    if clocks.len() > 2 {
        return fail(
            &clocks[2].token,
            "unsupported",
            "More than two clocks need a new clause.",
        );
    }

    if clocks.len() == 2 {
        let partner = clocks[1].clone();
        inherit_meridiem(clocks.first_mut(), Some(&partner), true);
    } else {
        inherit_meridiem(clocks.first_mut(), None, true);
    }
    if clocks.len() == 2 {
        let start_snapshot = clocks[0].clone();
        inherit_meridiem(clocks.get_mut(1), Some(&start_snapshot), false);
    }

    let can_infer_working_hours =
        clocks[0].needs_meridiem && clocks.get(1).is_some_and(|value| value.needs_meridiem);
    if can_infer_working_hours {
        let start_hour = clocks[0].value.as_hm().map(|(hour, _, _)| hour);
        let end_hour = clocks[1].value.as_hm().map(|(hour, _, _)| hour);
        if let (Some(start_hour), Some(end_hour)) = (start_hour, end_hour)
            && end_hour < start_hour
        {
            if let ClockTime::Hm { hour, .. } = &mut clocks[1].value {
                *hour += 12;
            }
            clocks[0].needs_meridiem = false;
            clocks[1].needs_meridiem = false;
            diagnostics.push(diagnostic(
                &clocks[0].token,
                "working-hours",
                "Assumed a daytime working-hours range.",
                crate::types::Severity::warning,
            ));
        }
    }

    for clock in clocks.iter() {
        if clock.needs_meridiem {
            diagnostics.push(diagnostic(
                &clock.token,
                "ambiguous-meridiem",
                "No AM/PM marker; interpreted as a 24-hour clock.",
                crate::types::Severity::warning,
            ));
        }
    }

    let start = &clocks[0];
    let end = clocks.get(1);
    if let Some(end) = end {
        let literal_equal = literal_seconds(&start.value).is_some_and(|start_seconds| {
            Some(start_seconds)
                == end
                    .value
                    .as_hm()
                    .map(|(hour, minute, second)| {
                        (hour * 3600 + minute * 60 + second.unwrap_or(0)) as f64
                    })
                    .or(match &end.value {
                        ClockTime::Named { .. } => literal_seconds(&end.value),
                        _ => None,
                    })
        });
        if literal_equal || json_equal(&start.value, &end.value) {
            return fail(
                &end.token,
                "end-equals-start",
                "Start and end times are equal.",
            );
        }
    }

    let time = TimeSpec {
        start: start.value.clone(),
        end: end.map(|value| value.value.clone()),
        open: None,
    };
    Ok(Some(time))
}

fn compile_date_and_time(
    tokens: &[Token],
    diagnostics: &mut Vec<Diagnostic>,
) -> CompileResult<Clause> {
    let mut clause = Clause::default();
    let mut days: Vec<Weekday> = Vec::new();
    let mut clocks: Vec<ParsedClock> = Vec::new();
    let mut modifier: Option<Modifier> = None;
    let mut edge: Option<Edge> = None;
    let mut calendar: Option<CalendarDate> = None;
    let mut calendar_end: Option<CalendarDate> = None;
    let mut ordinal: Option<i64> = None;
    let mut ranged_days = false;
    let mut pending_day_range = false;
    let mut first_day_index: i64 = -1;
    let mut open_bound: Option<OpenBound> = None;
    let mut open_token: Option<Token> = None;
    let mut pending_day_part: Option<DayPart> = None;
    // A deictic period ("last year", "next month", "last week") captured as
    // (unit, modifier) so it can qualify a following calendar or weekday
    // ("24th august last year", "15th last month", "friday last week")
    // instead of conflicting with it.
    let mut period: Option<(Unit, Modifier)> = None;

    let mut index = 0usize;
    while index < tokens.len() {
        let token = &tokens[index];
        let word = token.raw.text.to_lowercase();

        match token.label {
            Role::O | Role::RangeStart | Role::Recur => {}

            Role::DirBefore | Role::DirAfter => {
                // extract_shift already took the directions that carry an amount
                // and a unit ("3 days after Friday"). Whatever survives to here
                // is an open bound.
                if open_bound.is_some() {
                    return fail(token, "unsupported", "An expression takes one open bound.");
                }
                open_bound = Some(if token.label == Role::DirAfter {
                    OpenBound::end
                } else {
                    OpenBound::start
                });
                open_token = Some(token.clone());
            }

            Role::RangeEnd => {
                if calendar.is_some() && clocks.is_empty() {
                    calendar_end.get_or_insert_with(CalendarDate::default);
                }
                if !days.is_empty()
                    && tokens[index + 1..]
                        .iter()
                        .find(|token| token.label != Role::O)
                        .is_some_and(|token| token.label == Role::Weekday)
                {
                    pending_day_range = true;
                    ranged_days = true;
                }
            }

            Role::RelDay => {
                let mut phrase = word.clone();
                while tokens
                    .get(index + 1)
                    .is_some_and(|token| token.label == Role::RelDay)
                {
                    index += 1;
                    phrase.push(' ');
                    phrase.push_str(&tokens[index].raw.text.to_lowercase());
                }
                let offset = relative_day(&phrase).or_else(|| {
                    hindi_relative(&phrase).map(|pair| hindi_relative_offset(pair, tokens))
                });
                let Some(offset) = offset else {
                    return fail(token, "unsupported", "Unknown relative day.");
                };
                clause.date = Some(DateSpec::RelativeDay { offset });
            }

            Role::Edge => {
                if !matches!(
                    word.as_str(),
                    "start" | "beginning" | "end" | "शुरुआत" | "आखिर" | "shuruaat" | "aakhir"
                ) {
                    return fail(token, "unsupported", "Unknown calendar edge.");
                }
                edge = Some(if matches!(word.as_str(), "end" | "आखिर" | "aakhir") {
                    Edge::end
                } else {
                    Edge::start
                });
            }

            Role::Now => {
                if !matches!(word.as_str(), "now" | "immediately" | "asap") {
                    return fail(token, "unsupported", "Unknown immediate-time expression.");
                }
                clause.date = Some(DateSpec::Now);
            }

            Role::Deictic => {
                modifier = modifier_word(&word);
                if modifier.is_none() {
                    return fail(token, "unsupported", "Unknown date modifier.");
                }
            }

            Role::Unit => {
                let Some(value) = unit(&word) else {
                    return fail(token, "unsupported", "Unknown calendar unit.");
                };
                let boundary = edge.or(
                    if tokens
                        .iter()
                        .any(|part| part.raw.text.to_lowercase() == "end")
                    {
                        Some(Edge::end)
                    } else {
                        None
                    },
                );
                if ordinal.is_some() && value == Unit::week {
                    clause.date = Some(DateSpec::CalendarPeriod {
                        month: 0,
                        year: None,
                        modifier: None,
                        week: ordinal,
                    });
                    index += 1;
                    continue;
                }
                if ordinal.is_some() && value == Unit::month && modifier.is_none() {
                    index += 1;
                    continue;
                }
                // A bare day/week directly before a weekday list is a
                // selector ("gym push day Mon Wed and Fri at 6pm"):
                // the unit says what the weekdays schedule.
                if modifier.is_none()
                    && boundary.is_none()
                    && ordinal.is_none()
                    && matches!(value, Unit::day | Unit::week)
                    && tokens
                        .get(index + 1)
                        .is_some_and(|next| next.label == Role::Weekday)
                {
                    let mut selected: Vec<Weekday> = Vec::new();
                    let mut scan = index + 1;
                    while let Some(next) = tokens.get(scan) {
                        match next.label {
                            Role::Weekday => {
                                if let Some(day) = weekday(&next.raw.text.to_lowercase())
                                    && !selected.contains(&day)
                                {
                                    selected.push(day);
                                }
                                scan += 1;
                            }
                            Role::Join => scan += 1,
                            _ => break,
                        }
                    }
                    if !selected.is_empty() {
                        let mut recurrence = empty_recurrence(Frequency::weekly);
                        recurrence.by_day = Some(selected);
                        clause.recurrence = Some(recurrence);
                        index = scan;
                        continue;
                    }
                }
                if modifier.is_none() && boundary.is_none() {
                    return fail(
                        token,
                        "unsupported",
                        "A standalone unit needs a modifier or a quantity.",
                    );
                }
                clause.date = Some(DateSpec::RelativeUnit {
                    unit: value,
                    modifier: modifier.unwrap_or(Modifier::this),
                    edge: boundary,
                });
                if boundary.is_none()
                    && matches!(value, Unit::year | Unit::month | Unit::week)
                    && !tokens
                        .iter()
                        .any(|token| matches!(token.label, Role::Recur | Role::Freq))
                    && let Some(captured) = modifier
                {
                    period = Some((value, captured));
                }
            }

            Role::Ord => {
                let Some(value) = number(&word) else {
                    return fail(
                        token,
                        "invalid-ordinal",
                        "Use first through fifth, or last.",
                    );
                };
                if !is_integer(value) || value == 0.0 || value.abs() > 5.0 {
                    return fail(
                        token,
                        "invalid-ordinal",
                        "Use first through fifth, or last.",
                    );
                }
                ordinal = Some(value as i64);
            }

            Role::DayGroup => {
                let group = if matches!(word.as_str(), "weekend" | "weekends" | "वीकेंड")
                {
                    Some(DayGroup::weekend)
                } else if matches!(
                    word.as_str(),
                    "weekday" | "weekdays" | "workday" | "workdays" | "वीकडे"
                ) {
                    Some(DayGroup::weekday)
                } else {
                    None
                };
                let Some(group) = group else {
                    return fail(token, "unsupported", "Unknown day group.");
                };
                clause.date = Some(DateSpec::DayGroup { group, modifier });
            }

            Role::ClockOffset => {
                if word == "डेढ़" || word == "dedh" || word == "ढाई" || word == "dhai"
                {
                    clocks.push(ParsedClock {
                        value: ClockTime::Hm {
                            hour: if word == "ढाई" || word == "dhai" {
                                2
                            } else {
                                1
                            },
                            minute: 30,
                            second: None,
                        },
                        token: token.clone(),
                        meridiem: None,
                        needs_meridiem: false,
                    });
                    while tokens
                        .get(index + 1)
                        .is_some_and(|token| matches!(token.label, Role::Meridiem | Role::Glue))
                    {
                        index += 1;
                    }
                } else {
                    let hindi_offset = if word == "सवा" || word == "sava" || word == "sawa" {
                        Some(15.0)
                    } else if word == "पौने" || word == "paune" {
                        Some(-15.0)
                    } else if matches!(word.as_str(), "आधा" | "आधे" | "adha" | "aadha" | "aadhe")
                    {
                        Some(30.0)
                    } else {
                        None
                    };
                    let offset = hindi_offset.unwrap_or(if word == "half" {
                        30.0
                    } else if word == "quarter" {
                        15.0
                    } else {
                        f64::NAN
                    });
                    let mut target = index + 1;
                    while tokens
                        .get(target)
                        .is_some_and(|token| token.label == Role::Glue)
                    {
                        target += 1;
                    }
                    if let Some(hindi_offset) = hindi_offset {
                        if !tokens
                            .get(target)
                            .is_some_and(|token| token.label == Role::Hour)
                        {
                            return fail(
                                token,
                                "invalid-time",
                                "A fractional clock needs an hour.",
                            );
                        }
                        let (mut clock, next) = read_clock(tokens, target)?;
                        let Some((hour, minute, _)) = clock.value.as_hm() else {
                            return fail(
                                token,
                                "invalid-time",
                                "A fractional clock needs a whole hour.",
                            );
                        };
                        if minute != 0 {
                            return fail(
                                token,
                                "invalid-time",
                                "A fractional clock needs a whole hour.",
                            );
                        }
                        let total = ((hour as f64 * 60.0 + hindi_offset + 1440.0) % 1440.0) as i64;
                        clock.value = ClockTime::Hm {
                            hour: total / 60,
                            minute: total % 60,
                            second: None,
                        };
                        clocks.push(clock);
                        index = next - 1;
                    } else {
                        let Some(direction) = tokens
                            .get(target)
                            .map(|token| token.raw.text.to_lowercase())
                        else {
                            return fail(
                                token,
                                "invalid-time",
                                "A fractional clock needs past or to and an hour.",
                            );
                        };
                        if (direction != "past" && direction != "to") || !offset.is_finite() {
                            return fail(
                                token,
                                "invalid-time",
                                "A fractional clock needs past or to and an hour.",
                            );
                        }
                        target += 1;
                        if !tokens
                            .get(target)
                            .is_some_and(|token| token.label == Role::Hour)
                        {
                            return fail(
                                token,
                                "invalid-time",
                                "A fractional clock needs an hour.",
                            );
                        }
                        let (mut clock, next) = read_clock(tokens, target)?;
                        let Some((hour, minute, _)) = clock.value.as_hm() else {
                            return fail(
                                token,
                                "invalid-time",
                                "A fractional clock needs a whole hour.",
                            );
                        };
                        if minute != 0 {
                            return fail(
                                token,
                                "invalid-time",
                                "A fractional clock needs a whole hour.",
                            );
                        }
                        let total = ((hour as f64 * 60.0
                            + if direction == "to" { -offset } else { offset }
                            + 1440.0)
                            % 1440.0) as i64;
                        clock.value = ClockTime::Hm {
                            hour: total / 60,
                            minute: total % 60,
                            second: None,
                        };
                        clocks.push(clock);
                        index = next - 1;
                    }
                }
            }

            Role::TimeNamed => {
                if !matches!(word.as_str(), "noon" | "midday" | "midnight") {
                    return fail(token, "unsupported", "Unknown named clock time.");
                }
                clocks.push(ParsedClock {
                    value: ClockTime::Named {
                        named: if word == "midnight" {
                            NamedClock::midnight
                        } else {
                            NamedClock::noon
                        },
                    },
                    token: token.clone(),
                    meridiem: None,
                    needs_meridiem: false,
                });
            }

            Role::DayPart => {
                let Some(part) = day_part(&word) else {
                    return fail(token, "unsupported", "Unknown day part.");
                };
                let hour_later = tokens[index + 1..]
                    .iter()
                    .any(|item| item.label == Role::Hour);
                if hour_later {
                    pending_day_part = Some(part);
                } else if let Some(clock) = clocks.last_mut().filter(|clock| clock.needs_meridiem) {
                    // A trailing day part ("9:30 night") qualifies the clock
                    // it follows instead of opening a new one.
                    if let Some((hour, minute, second)) = clock.value.as_hm() {
                        let pm = part != DayPart::morning;
                        let hour = (hour % 12) + if pm { 12 } else { 0 };
                        clock.value = ClockTime::Hm {
                            hour,
                            minute,
                            second,
                        };
                        clock.needs_meridiem = false;
                    }
                } else {
                    clocks.push(ParsedClock {
                        value: ClockTime::Part { part },
                        token: token.clone(),
                        meridiem: None,
                        needs_meridiem: false,
                    });
                }
            }

            Role::Month => {
                let value = month(&word)
                    .map(|value| value as f64)
                    .or_else(|| number(&word));
                let Some(value) =
                    value.filter(|value| is_integer(*value) && (1.0..=12.0).contains(value))
                else {
                    return fail(token, "invalid-date", "Unknown month.");
                };
                let calendar_ref = calendar.get_or_insert_with(CalendarDate::default);
                let target = calendar_end.as_mut().unwrap_or(calendar_ref);
                if target.month.is_some() {
                    return fail(
                        token,
                        "invalid-date",
                        "A calendar date has more than one month.",
                    );
                }
                target.month = Some(value as i64);
            }

            Role::Dom => {
                let mut value = number(&word).unwrap_or(f64::NAN);
                // "twenty-first" arrives as separate tokens, optionally hyphenated.
                let ones_index = if tokens.get(index + 1).map(|t| t.raw.text.as_str()) == Some("-")
                {
                    index + 2
                } else {
                    index + 1
                };
                if let Some(ones) = tokens.get(ones_index)
                    && let Some(combined) = compound_ordinal(&word, &ones.raw.text.to_lowercase())
                {
                    value = combined;
                    index = ones_index;
                }
                if !is_integer(value) || !(1.0..=31.0).contains(&value) {
                    return fail(
                        token,
                        "invalid-date",
                        "Day of month must be between 1 and 31.",
                    );
                }
                let calendar_ref = calendar.get_or_insert_with(CalendarDate::default);
                let target = calendar_end.as_mut().unwrap_or(calendar_ref);
                if target.day.is_some() {
                    return fail(
                        token,
                        "invalid-date",
                        "Multiple dates need a range or recurrence.",
                    );
                }
                target.day = Some(value as i64);
            }

            Role::Year => {
                let Some(value) = number(&word) else {
                    return fail(token, "invalid-date", "Year is out of range.");
                };
                if !is_integer(value) || !(1.0..=9999.0).contains(&value) {
                    return fail(token, "invalid-date", "Year is out of range.");
                }
                let year = if value < 100.0 {
                    2000 + value as i64
                } else {
                    value as i64
                };
                let calendar_ref = calendar.get_or_insert_with(CalendarDate::default);
                if let Some(calendar_end) = calendar_end.as_mut() {
                    calendar_end.year = Some(year);
                }
                if calendar_end.is_none() || calendar_ref.year.is_none() {
                    calendar_ref.year = Some(year);
                }
            }

            Role::Weekday => {
                let Some(day) = weekday(&word) else {
                    return fail(token, "unsupported", "Unknown weekday.");
                };
                if first_day_index < 0 {
                    first_day_index = index as i64;
                }
                if pending_day_range {
                    let mut position = weekday_index(*days.last().unwrap());
                    while WEEKDAYS[position] != day {
                        position = (position + 1) % 7;
                        days.push(WEEKDAYS[position]);
                    }
                    pending_day_range = false;
                } else {
                    days.push(day);
                }
            }

            Role::Holiday => {
                let mut text = word.clone();
                while tokens
                    .get(index + 1)
                    .is_some_and(|token| token.label == Role::Holiday)
                {
                    index += 1;
                    text.push_str(&tokens[index].raw.text.to_lowercase());
                }
                let key: String = text
                    .chars()
                    .filter(|c| !matches!(c, '\'' | '’' | ' ' | '-'))
                    .collect();
                let Some(name) = holiday_name(&key) else {
                    return fail(token, "unsupported", "Unknown fixed-date holiday.");
                };
                clause.date = Some(DateSpec::Holiday { name });
            }

            Role::Meridiem => {
                // "at" introduces a following clock; it is also part of "at night".
                if word == "at"
                    && tokens
                        .get(index + 1)
                        .is_some_and(|token| token.label == Role::Hour)
                {
                    // fall through: the clock arm consumes it
                } else if let Some(part) = day_part(&word) {
                    // A bare day-part word arriving as MERIDIEM ("every
                    // night 10pm") qualifies the clock that follows.
                    pending_day_part = Some(part);
                } else {
                    return fail(
                        token,
                        "invalid-time",
                        "A time-of-day qualifier needs a clock.",
                    );
                }
            }

            Role::Hour => {
                let (mut clock, next) = read_clock(tokens, index)?;
                if let (Some(part), true) = (pending_day_part, clock.needs_meridiem)
                    && let Some((hour, minute, second)) = clock.value.as_hm()
                {
                    let pm = part != DayPart::morning;
                    let hour = (hour % 12) + if pm { 12 } else { 0 };
                    clock.value = ClockTime::Hm {
                        hour,
                        minute,
                        second,
                    };
                    clock.needs_meridiem = false;
                    pending_day_part = None;
                }
                clocks.push(clock);
                index = next - 1;
            }

            _ => {
                let label = token.label.name();
                return fail(
                    token,
                    "unsupported",
                    &format!("Unsupported token role {label}."),
                );
            }
        }
        index += 1;
    }

    if matches!(clause.date, Some(DateSpec::CalendarPeriod { month: 0, .. })) {
        let Some(month_value) = calendar.as_ref().and_then(|value| value.month) else {
            return fail(
                &tokens[0],
                "invalid-date",
                "A week of a month needs a named month.",
            );
        };
        clause.date = Some(DateSpec::CalendarPeriod {
            month: month_value,
            year: calendar.and_then(|value| value.year),
            modifier: None,
            week: ordinal,
        });
        calendar = None;
        ordinal = None;
    }
    if let Some(ordinal_value) = ordinal {
        if days.len() != 1 {
            return fail(
                &tokens[0],
                "invalid-ordinal",
                "An ordinal needs one weekday.",
            );
        }
        let of = match clause.date {
            Some(DateSpec::RelativeUnit {
                unit: unit @ (Unit::month | Unit::year),
                modifier,
                ..
            }) => crate::types::MonthRef::RelativeUnit {
                unit: match unit {
                    Unit::month => crate::types::RelativeUnitKind::month,
                    _ => crate::types::RelativeUnitKind::year,
                },
                modifier,
            },
            _ => crate::types::MonthRef::Calendar {
                year: calendar.as_ref().and_then(|value| value.year),
                month: calendar.as_ref().and_then(|value| value.month),
            },
        };
        let recurring = tokens.iter().any(|value| {
            value.label == Role::Recur
                || (value.label == Role::Unit
                    && unit(&value.raw.text.to_lowercase()) == Some(Unit::month))
        }) && modifier.is_none();
        clause.date = Some(DateSpec::OrdinalWeekday {
            ordinal: ordinal_value,
            day: days[0],
            of: Box::new(of),
            recurring: recurring.then_some(true),
        });
        calendar = None;
    } else if !days.is_empty() {
        let mut selected: Vec<Weekday> = Vec::new();
        for day in days {
            if !selected.contains(&day) {
                selected.push(day);
            }
        }
        let preceding = &tokens[..first_day_index.max(0) as usize];
        let explicit_date_range = ranged_days
            && preceding
                .iter()
                .any(|token| token.label == Role::RangeStart)
            && !preceding
                .iter()
                .any(|token| token.label == Role::Hour || token.label == Role::TimeNamed);
        if explicit_date_range {
            clause.date = Some(DateSpec::WeekdayRange {
                from: selected[0],
                to: *selected.last().unwrap(),
            });
        } else if ranged_days {
            clause.recurrence = Some(Recurrence {
                freq: Frequency::weekly,
                interval: 1,
                by_day: Some(selected),
                by_month_day: None,
                by_set_pos: None,
                by_month: None,
                times_per: None,
                count: None,
                until: None,
                start: None,
                except: None,
                span: None,
            });
        } else if period.is_some_and(|(captured, _)| captured == Unit::week) {
            // "friday last week" / "monday next week": anchor the weekday to
            // the deictic week rather than reading it as "last friday".
            clause.date = Some(DateSpec::PeriodWeekday {
                modifier: period
                    .map(|(_, captured)| captured)
                    .unwrap_or(Modifier::this),
                days: selected,
            });
        } else {
            clause.date = Some(DateSpec::Weekday {
                days: selected,
                modifier,
            });
        }
    }
    if let Some(calendar_value) = calendar {
        clause.recurrence = None;
        if matches!(
            clause.date,
            Some(DateSpec::Weekday { .. }) | Some(DateSpec::WeekdayRange { .. })
        ) && calendar_value.day.is_some()
        {
            clause.date = None;
        }
        // A deictic year/month plus calendar fields is one anchored date
        // ("24th august last year"), not two competing specifications.
        let mut merged_period = false;
        if calendar_end.is_none()
            && calendar_value.year.is_none()
            && matches!(clause.date, Some(DateSpec::RelativeUnit { .. }))
            && let Some((captured, captured_modifier)) = period
            && matches!(captured, Unit::year | Unit::month)
        {
            clause.date = Some(DateSpec::PeriodCalendar {
                period: if captured == Unit::year {
                    crate::types::RelativeUnitKind::year
                } else {
                    crate::types::RelativeUnitKind::month
                },
                modifier: captured_modifier,
                month: calendar_value.month,
                day: calendar_value.day,
            });
            merged_period = true;
        }
        if !merged_period {
            if clause.date.is_some() {
                return fail(
                    &tokens[0],
                    "invalid-date",
                    "Conflicting date specifications need separate clauses.",
                );
            }
            if let Some(calendar_end_value) = calendar_end {
                if calendar_value.day.is_none() || calendar_end_value.day.is_none() {
                    return fail(
                        &tokens[0],
                        "invalid-date",
                        "Both ends of a calendar range need a day.",
                    );
                }
                let mut from = calendar_value.clone();
                let mut to = calendar_end_value;
                // to inherits unspecified fields from a spread over `calendar`.
                to.year = to.year.or(calendar_value.year);
                to.month = to.month.or(calendar_value.month);
                if from.month.is_none() && to.month.is_some() {
                    from.month = to.month;
                }
                clause.date = Some(DateSpec::CalendarRange { from, to });
            } else if calendar_value.month.is_some()
                && calendar_value.day.is_none()
                && modifier.is_some()
            {
                clause.date = Some(DateSpec::CalendarPeriod {
                    month: calendar_value.month.unwrap(),
                    year: calendar_value.year,
                    modifier,
                    week: None,
                });
            } else {
                clause.date = Some(DateSpec::Calendar {
                    year: calendar_value.year,
                    month: calendar_value.month,
                    day: calendar_value.day,
                });
            }
        }
    }

    if clocks.len() == 2
        && !tokens.iter().any(|token| {
            token.label == Role::RangeEnd
                && token.raw.start > clocks[0].token.raw.start
                && token.raw.start < clocks[1].token.raw.start
        })
    {
        return fail(
            &clocks[1].token,
            "unlinked-times",
            "Two clocks need a range separator or separate clauses.",
        );
    }
    let mut time = compile_time(&mut clocks, diagnostics)?;
    if let (Some(open_token), Some(open_bound)) = (&open_token, open_bound) {
        let Some(time_value) = time.as_mut() else {
            return fail(
                open_token,
                "open-bound-needs-time",
                "An open bound needs a clock time, as in \"after 6pm\".",
            );
        };
        if time_value.end.is_some() {
            return fail(
                open_token,
                "open-bound-needs-time",
                "An open bound takes one time, not a range.",
            );
        }
        if open_bound == OpenBound::end {
            time_value.open = Some(OpenBound::end);
        } else {
            // "before 6pm" reads as midnight up to 6pm; the start is the floor.
            time_value.end = Some(time_value.start.clone());
            time_value.start = ClockTime::hm(0, 0);
            time_value.open = Some(OpenBound::start);
        }
    } else if let Some(value) = time.as_mut() {
        // "from 6pm" means the same as "after 6pm". A "from" that opens a
        // real range ("from 8 to 10pm") keeps both edges.
        if value.end.is_none()
            && clocks.len() == 1
            && tokens.iter().any(|token| token.label == Role::RangeStart)
            && !tokens.iter().any(|token| token.label == Role::RangeEnd)
        {
            value.open = Some(OpenBound::end);
        }
    }
    if time.is_some() {
        clause.time = time;
    }
    if clause.date.is_none() && clause.time.is_none() && clause.recurrence.is_none() {
        return fail(
            &tokens[0],
            "unsupported",
            "The expression has no date or time.",
        );
    }

    Ok(clause)
}

fn extract_shift(tokens: &[Token]) -> CompileResult<(Vec<Token>, Option<Shift>)> {
    let Some(direction_index) = tokens
        .iter()
        .position(|token| token.label == Role::DirBefore || token.label == Role::DirAfter)
    else {
        return Ok((tokens.to_vec(), None));
    };

    let Some(amount_index) = tokens.iter().position(|token| token.label == Role::Num) else {
        return Ok((tokens.to_vec(), None));
    };
    let Some(unit_index) = tokens.iter().position(|token| token.label == Role::Unit) else {
        return Ok((tokens.to_vec(), None));
    };

    let amount_token = tokens[amount_index].clone();
    let quantity = read_duration(tokens, amount_index);
    let amount = quantity
        .as_ref()
        .map(|(duration, _)| duration.amount)
        .or_else(|| number(&amount_token.raw.text.to_lowercase()));
    let duration_unit = quantity
        .as_ref()
        .map(|(duration, _)| duration.unit)
        .or_else(|| unit(&tokens[unit_index].raw.text.to_lowercase()));

    let Some(duration_unit) = duration_unit else {
        return fail(&amount_token, "invalid-shift", "Invalid relative quantity.");
    };
    let Some(amount) = amount else {
        return fail(&amount_token, "invalid-shift", "Invalid relative quantity.");
    };
    if amount < 0.0 || (!is_integer(amount) && !matches!(duration_unit, Unit::minute | Unit::hour))
    {
        return fail(&amount_token, "invalid-shift", "Invalid relative quantity.");
    }

    let mut consumed = vec![amount_index, unit_index, direction_index];
    if let Some((_, quantity_next)) = &quantity {
        for i in amount_index..*quantity_next {
            consumed.push(i);
        }
    }
    let mut end_amount: Option<f64> = None;
    if let Some(range_index) = tokens
        .iter()
        .enumerate()
        .position(|(i, token)| i > amount_index && i < unit_index && token.label == Role::RangeEnd)
    {
        let end_token = tokens.get(range_index + 1);
        let value = end_token
            .filter(|token| token.label == Role::Num)
            .and_then(|token| number(&token.raw.text.to_lowercase()))
            .unwrap_or(f64::NAN);
        if !is_integer(value) || value < amount {
            return fail(
                &tokens[range_index],
                "invalid-shift",
                "A relative range needs an end amount at least as large as its start.",
            );
        }
        end_amount = Some(value);
        consumed.push(range_index);
        consumed.push(range_index + 1);
    }
    let remaining: Vec<Token> = tokens
        .iter()
        .enumerate()
        .filter(|(index, _)| !consumed.contains(index))
        .map(|(_, token)| token.clone())
        .collect();
    let (duration_value, _) = quantity.unwrap_or((
        Duration {
            components: None,
            amount,
            unit: duration_unit,
        },
        0,
    ));
    Ok((
        remaining,
        Some(Shift {
            components: duration_value.components,
            amount,
            unit: duration_unit,
            direction: if tokens[direction_index].label == Role::DirBefore {
                Direction::before
            } else {
                Direction::after
            },
            end_amount,
            approximate: None,
        }),
    ))
}

fn recurrence_bounds(label: Role) -> bool {
    matches!(label, Role::BoundStart | Role::BoundEnd | Role::Except)
}

fn empty_recurrence(freq: Frequency) -> Recurrence {
    Recurrence {
        freq,
        interval: 1,
        by_day: None,
        by_month_day: None,
        by_set_pos: None,
        by_month: None,
        times_per: None,
        count: None,
        until: None,
        start: None,
        except: None,
        span: None,
    }
}

fn compile_clause(input: &[Token], diagnostics: &mut Vec<Diagnostic>) -> CompileResult<Clause> {
    let last_meaningful = input.iter().rev().find(|token| token.label != Role::O);
    if let Some(last) = last_meaningful.filter(|token| token.label == Role::RangeEnd) {
        if last.raw.text.to_lowercase() == "until"
            && input
                .iter()
                .any(|token| matches!(token.label, Role::Recur | Role::Freq | Role::DayGroup))
        {
            return fail(last, "invalid-bound", "A bound needs a date.");
        }
        return fail(last, "incomplete-range", "A range needs an end value.");
    }
    let day_part_selects = |token: &Token| {
        token.label == Role::DayPart
            || (token.label == Role::Meridiem && day_part(&token.raw.text.to_lowercase()).is_some())
    };
    if input.iter().any(|token| token.label == Role::Recur)
        && !input.iter().any(|token| {
            matches!(
                token.label,
                Role::Unit | Role::Freq | Role::Weekday | Role::DayGroup | Role::Month
            ) || day_part_selects(token)
                // "roz" arrives as RECUR but names a frequency itself
                || (token.label == Role::Recur
                    && frequency_word(&token.raw.text.to_lowercase()).is_some())
        })
    {
        return fail(
            &input[0],
            "incomplete-recurrence",
            "A recurrence needs a frequency or calendar selector.",
        );
    }
    let (tokens, shift) = extract_shift(input)?;
    let mut body: Vec<Token> = Vec::new();
    let bound_index = tokens
        .iter()
        .position(|token| recurrence_bounds(token.label));
    let selectors: &[Token] = match bound_index {
        Some(bound_index) => &tokens[..bound_index],
        None => &tokens,
    };
    let implicit_ordinal = selectors.iter().any(|token| token.label == Role::Ord)
        && selectors.iter().any(|token| token.label == Role::Weekday)
        && selectors.iter().any(|token| {
            token.label == Role::Unit && unit(&token.raw.text.to_lowercase()) == Some(Unit::month)
        })
        && !selectors.iter().any(|token| token.label == Role::Deictic);
    let mut recurrence: Option<Recurrence> =
        if selectors.iter().any(|token| token.label == Role::Recur)
            || implicit_ordinal
            || (selectors.iter().any(|token| token.label == Role::DayGroup)
                && !selectors.iter().any(|token| token.label == Role::Deictic))
        {
            Some(empty_recurrence(if implicit_ordinal {
                Frequency::monthly
            } else {
                Frequency::weekly
            }))
        } else {
            None
        };
    let mut duration: Option<Duration> = None;
    let mut starting_date: Option<DateSpec> = None;

    let mut index = 0usize;
    while index < tokens.len() {
        let token = &tokens[index];

        if token.label == Role::Recur {
            recurrence.get_or_insert_with(|| empty_recurrence(Frequency::weekly));
            // A lone "roz" / "रोज़" names its own frequency.
            if let Some((freq, doubled)) = frequency_word(&token.raw.text.to_lowercase())
                && let Some(rule) = recurrence.as_mut()
            {
                rule.freq = freq;
                if doubled {
                    rule.interval = 2;
                }
            }
            // "every morning 9am" / "every night 10pm": the day part right
            // after the recurrence marker selects a daily frequency and
            // will bias the clock that follows.
            if let Some(next) = tokens.get(index + 1) {
                let as_part = if matches!(next.label, Role::DayPart | Role::Meridiem) {
                    day_part(&next.raw.text.to_lowercase())
                } else {
                    None
                };
                if let Some(part) = as_part {
                    if let Some(rule) = recurrence.as_mut() {
                        rule.freq = Frequency::daily;
                    }
                    // the day-part token itself is consumed by the body's
                    // own DayPart handling, which queues it for the clock
                    let _ = part;
                    index += 1;
                    continue;
                }
                // "har roz" / "रोज़": a second RECUR token that names a
                // frequency ("roz" = daily) merges into the recurrence.
                if next.label == Role::Recur
                    && let Some((freq, doubled)) = frequency_word(&next.raw.text.to_lowercase())
                {
                    if let Some(rule) = recurrence.as_mut() {
                        rule.freq = freq;
                        if doubled {
                            rule.interval = 2;
                        }
                    }
                    index += 2;
                    continue;
                }
            }
            index += 1;
            continue;
        }

        if token.label == Role::Freq {
            let word = token.raw.text.to_lowercase();
            let Some((freq, doubled)) = frequency_word(&word) else {
                return fail(token, "unsupported", "Unknown recurrence frequency.");
            };
            if let Some(recurrence) = recurrence.as_mut() {
                recurrence.freq = freq;
                if doubled {
                    recurrence.interval = 2;
                } else {
                    recurrence.interval = recurrence.interval.max(1);
                }
            } else {
                let mut rule = empty_recurrence(freq);
                if doubled {
                    rule.interval = 2;
                }
                recurrence = Some(rule);
            }
            index += 1;
            continue;
        }

        if token.label == Role::Times
            || (token.label == Role::Num
                && tokens
                    .get(index + 1)
                    .is_some_and(|token| token.label == Role::Times))
        {
            let count = number(&token.raw.text.to_lowercase());
            let mut period = index + if token.label == Role::Num { 2 } else { 1 };
            while tokens
                .get(period)
                .is_some_and(|token| token.label == Role::O || token.label == Role::Recur)
            {
                period += 1;
            }
            let Some(count) = count.filter(|value| is_integer(*value) && *value >= 1.0) else {
                return fail(
                    token,
                    "invalid-frequency",
                    "A frequency count needs a positive number and a period.",
                );
            };
            if !tokens
                .get(period)
                .is_some_and(|token| token.label == Role::Unit)
            {
                return fail(
                    token,
                    "invalid-frequency",
                    "A frequency count needs a positive number and a period.",
                );
            }
            let Some(frequency) =
                unit(&tokens[period].raw.text.to_lowercase()).and_then(unit_frequency)
            else {
                return fail(
                    token,
                    "unsupported",
                    "Expected an hourly, daily, weekly, monthly, or yearly period.",
                );
            };
            let mut rule = recurrence
                .take()
                .unwrap_or_else(|| empty_recurrence(Frequency::weekly));
            rule.freq = frequency;
            rule.times_per = Some(count as i64);
            recurrence = Some(rule);
            index = period;
            continue;
        }

        if recurrence.is_some() && token.label == Role::Unit {
            let Some(frequency) = unit(&token.raw.text.to_lowercase()).and_then(unit_frequency)
            else {
                return fail(
                    token,
                    "unsupported",
                    "Expected an hourly, daily, weekly, monthly, or yearly period.",
                );
            };
            if let Some(rule) = recurrence.as_mut() {
                rule.freq = frequency;
            }
            index += 1;
            continue;
        }
        if recurrence.is_some() && token.label == Role::Ord {
            let Some(value) = number(&token.raw.text.to_lowercase()) else {
                return fail(
                    token,
                    "invalid-ordinal",
                    "Use first through fifth, or last.",
                );
            };
            if !is_integer(value) || value == 0.0 || value.abs() > 5.0 {
                return fail(
                    token,
                    "invalid-ordinal",
                    "Use first through fifth, or last.",
                );
            }
            if let Some(rule) = recurrence.as_mut() {
                rule.by_set_pos
                    .get_or_insert_with(Vec::new)
                    .push(value as i64);
            }
            index += 1;
            continue;
        }
        if recurrence.is_some() && token.label == Role::Dom {
            let Some(value) = number(&token.raw.text.to_lowercase()) else {
                return fail(token, "invalid-date", "Day of month is out of range.");
            };
            if !is_integer(value) || !(1.0..=31.0).contains(&value) {
                return fail(token, "invalid-date", "Day of month is out of range.");
            }
            if let Some(rule) = recurrence.as_mut() {
                rule.by_month_day
                    .get_or_insert_with(Vec::new)
                    .push(value as i64);
            }
            index += 1;
            continue;
        }
        if recurrence.is_some() && token.label == Role::Month {
            let value = month(&token.raw.text)
                .map(|value| value as f64)
                .or_else(|| number(&token.raw.text.to_lowercase()));
            let Some(value) =
                value.filter(|value| is_integer(*value) && (1.0..=12.0).contains(value))
            else {
                return fail(token, "invalid-date", "Month is out of range.");
            };
            if let Some(rule) = recurrence.as_mut() {
                rule.by_month
                    .get_or_insert_with(Vec::new)
                    .push(value as i64);
            }
            index += 1;
            continue;
        }
        if recurrence.is_some()
            && token.label == Role::Num
            && tokens
                .get(index + 1)
                .is_some_and(|token| token.label == Role::Count)
        {
            let Some(count) = number(&token.raw.text.to_lowercase()) else {
                return fail(token, "invalid-count", "Occurrence count must be positive.");
            };
            if !is_integer(count) || count < 1.0 {
                return fail(token, "invalid-count", "Occurrence count must be positive.");
            }
            if let Some(rule) = recurrence.as_mut() {
                rule.count = Some(count as i64);
            }
            index += 2;
            continue;
        }

        let bare_duration = recurrence.is_none()
            && token.label == Role::Num
            && tokens
                .get(index + 1)
                .is_some_and(|token| token.label == Role::Unit);
        if token.label == Role::Dur || bare_duration {
            let mut amount_index = index + if bare_duration { 0 } else { 1 };
            while tokens
                .get(amount_index)
                .is_some_and(|token| token.label == Role::O || token.label == Role::Deictic)
            {
                amount_index += 1;
            }
            let Some(quantity) = read_duration(&tokens, amount_index) else {
                return fail(
                    token,
                    "invalid-duration",
                    "A duration needs a positive quantity and a time unit.",
                );
            };
            let (value, quantity_next) = quantity;
            let duration_unit = value.unit;
            if recurrence.is_some()
                && !matches!(duration_unit, Unit::minute | Unit::hour)
                && token.raw.text.to_lowercase() != "lasting"
            {
                let Some(rule) = recurrence.as_mut() else {
                    index += 1;
                    continue;
                };
                if rule.span.is_some() {
                    return fail(
                        token,
                        "conflicting-duration",
                        "A recurrence has more than one series duration.",
                    );
                }
                rule.span = Some(value);
            } else {
                if duration.is_some() {
                    return fail(
                        token,
                        "conflicting-duration",
                        "A clause has more than one occurrence duration.",
                    );
                }
                duration = Some(value);
            }
            index = quantity_next - 1;
            index += 1;
            continue;
        }

        let is_weekday_interval = token.label == Role::Num
            && tokens.get(index + 1).is_some_and(|token| {
                matches!(token.label, Role::Weekday | Role::DayGroup | Role::Unit)
            });
        if recurrence.is_some() && is_weekday_interval {
            let Some(interval) = number(&token.raw.text.to_lowercase()) else {
                return fail(
                    token,
                    "invalid-interval",
                    "Recurrence interval must be positive.",
                );
            };
            if !is_integer(interval) || interval < 1.0 {
                return fail(
                    token,
                    "invalid-interval",
                    "Recurrence interval must be positive.",
                );
            }
            if let Some(rule) = recurrence.as_mut() {
                rule.interval = interval as i64;
            }
            index += 1;
            continue;
        }

        if recurrence_bounds(token.label) {
            if recurrence.is_none() && token.label != Role::BoundStart {
                return fail(token, "unsupported", "This bound requires recurrence.");
            }

            let mut end = index + 1;
            while end < tokens.len()
                && !recurrence_bounds(tokens[end].label)
                && tokens[end].label != Role::Dur
                && !(tokens[end].label == Role::Num
                    && tokens
                        .get(end + 1)
                        .is_some_and(|token| token.label == Role::Count))
            {
                end += 1;
            }

            let bound = &tokens[index + 1..end];
            if bound.is_empty() {
                return fail(token, "invalid-bound", "A bound needs a date.");
            }
            let date = compile_date_and_time(bound, diagnostics)?.date;
            let Some(date) = date else {
                return fail(token, "invalid-bound", "A bound needs a date.");
            };

            match token.label {
                Role::BoundStart => {
                    if let Some(recurrence) = recurrence.as_mut() {
                        recurrence.start = Some(Box::new(date));
                    } else {
                        starting_date = Some(date);
                    }
                }
                Role::BoundEnd => {
                    recurrence.as_mut().unwrap().until = Some(Box::new(date));
                }
                _ => {
                    let Some(rule) = recurrence.as_mut() else {
                        index += 1;
                        continue;
                    };
                    rule.except.get_or_insert_with(Vec::new).push(date);
                }
            }

            index = end;
            continue;
        }

        body.push(token.clone());
        index += 1;
    }

    let has_date_or_time = body.iter().any(|token| token.label != Role::O);
    let mut clause = if has_date_or_time {
        compile_date_and_time(&body, diagnostics)?
    } else {
        Clause::default()
    };
    if shift.is_some() {
        clause.shift = shift;
    }
    if let Some(duration) = duration {
        clause.duration = Some(duration);
    }
    if let Some(starting_date) = starting_date {
        if clause.date.is_some() {
            return fail(
                &input[0],
                "invalid-date",
                "A one-off clause has conflicting starting dates.",
            );
        }
        clause.date = Some(starting_date);
    }

    if let Some(clause_recurrence) = clause.recurrence.take() {
        recurrence = Some(match recurrence {
            Some(outer) => Recurrence {
                by_day: outer.by_day.or(clause_recurrence.by_day),
                ..outer
            },
            None => clause_recurrence,
        });
    }
    if matches!(clause.date, Some(DateSpec::DayGroup { modifier: None, .. })) {
        let mut rule = recurrence
            .take()
            .unwrap_or_else(|| empty_recurrence(Frequency::weekly));
        let group = match clause.date {
            Some(DateSpec::DayGroup { group, .. }) => group,
            _ => unreachable!(),
        };
        rule.by_day = Some(if group == DayGroup::weekday {
            WEEKDAYS[..5].to_vec()
        } else {
            WEEKDAYS[5..].to_vec()
        });
        clause.date = None;
        recurrence = Some(rule);
    }
    if let Some(mut rule) = recurrence {
        if rule.count.is_some() && (rule.until.is_some() || rule.span.is_some()) {
            return fail(
                &input[0],
                "conflicting-bounds",
                "Use a count or an end bound, not both.",
            );
        }
        if let Some(DateSpec::Weekday { days, .. }) = clause.date.take() {
            rule.by_day = Some(days);
        }
        clause.recurrence = Some(rule);
    }

    Ok(clause)
}

fn split_expressions(tokens: &[Token]) -> Vec<Vec<Token>> {
    let mut expressions: Vec<Vec<Token>> = Vec::new();
    let mut current: Vec<Token> = Vec::new();

    for token in tokens {
        if token.raw.kind == 3 {
            continue;
        }

        if token.label != Role::O || filler(&token.raw.text.to_lowercase()) {
            current.push(token.clone());
        } else if !current.is_empty() {
            expressions.push(current);
            current = Vec::new();
        }
    }

    if !current.is_empty() {
        expressions.push(current);
    }

    expressions
        .into_iter()
        .map(|mut expression| {
            while expression
                .first()
                .is_some_and(|token| matches!(token.label, Role::O | Role::Glue | Role::Join))
            {
                expression.remove(0);
            }
            while expression
                .last()
                .is_some_and(|token| matches!(token.label, Role::O | Role::Glue | Role::Join))
            {
                expression.pop();
            }
            expression
        })
        .filter(|expression| !expression.is_empty())
        .collect()
}

fn split_clauses(tokens: &[Token]) -> Vec<Vec<Token>> {
    let mut clauses: Vec<Vec<Token>> = vec![Vec::new()];

    for token in tokens {
        let has_meaning = clauses.last().is_some_and(|current| {
            current
                .iter()
                .any(|part| !matches!(part.label, Role::O | Role::Glue | Role::Join))
        });
        if token.clause_start && has_meaning {
            clauses.push(Vec::new());
        }
        clauses.last_mut().unwrap().push(token.clone());
    }

    clauses
}

/// `{ ...structuredClone(first), ...next }`: only fields present on `next`
/// overwrite the clone.
fn merge_clause(first: &Clause, next: Clause) -> Clause {
    let mut merged = first.clone();
    if next.end_date.is_some() {
        merged.end_date = next.end_date;
    }
    if next.date.is_some() {
        merged.date = next.date;
    }
    if next.time.is_some() {
        merged.time = next.time;
    }
    if next.shift.is_some() {
        merged.shift = next.shift;
    }
    if next.duration.is_some() {
        merged.duration = next.duration;
    }
    if next.recurrence.is_some() {
        merged.recurrence = next.recurrence;
    }
    merged
}

fn compile_group(
    tokens: &[Token],
    diagnostics: &mut Vec<Diagnostic>,
) -> CompileResult<Vec<Clause>> {
    let mut clocks = 0;
    for token in tokens.iter() {
        if matches!(token.label, Role::Hour | Role::TimeNamed | Role::DayPart) {
            clocks += 1;
            if clocks == 2 {
                break;
            }
        }
    }
    if clocks < 2 {
        return Ok(vec![compile_clause(tokens, diagnostics)?]);
    }
    let has_recurrence = tokens
        .iter()
        .any(|token| matches!(token.label, Role::Recur | Role::Freq | Role::DayGroup));
    let separator = tokens
        .iter()
        .position(|token| token.label == Role::RangeEnd || token.label == Role::BoundEnd);
    let is_date = |token: &Token| {
        matches!(
            token.label,
            Role::RelDay | Role::Weekday | Role::Month | Role::Dom | Role::Year
        )
    };
    let is_clock = |token: &Token| {
        matches!(
            token.label,
            Role::Hour | Role::TimeNamed | Role::ClockOffset
        )
    };
    if !has_recurrence && separator.is_some_and(|separator| separator > 0) {
        let separator = separator.unwrap();
        let (left, right) = tokens.split_at(separator);
        let right = &right[1..];
        if left.iter().any(&is_date)
            && right.iter().any(is_date)
            && left.iter().any(&is_clock)
            && right.iter().any(&is_clock)
        {
            let start = compile_date_and_time(left, diagnostics)?;
            let end = compile_date_and_time(right, diagnostics)?;
            if start.date.is_none()
                || end.date.is_none()
                || start.time.is_none()
                || end.time.is_none()
            {
                return fail(
                    &tokens[separator],
                    "incomplete-range",
                    "Both endpoints need a date and clock.",
                );
            }
            return Ok(vec![Clause {
                date: start.date,
                end_date: end.date,
                time: Some(TimeSpec {
                    start: start.time.unwrap().start,
                    end: Some(end.time.unwrap().start),
                    open: None,
                }),
                shift: None,
                duration: None,
                recurrence: None,
            }]);
        }
    }
    // Conjoined clock points inherit the date and recurrence preceding the
    // first clock.
    if separator.is_none() {
        let mut groups: Vec<Vec<Token>> = vec![Vec::new()];
        for index in 0..tokens.len() {
            let token = &tokens[index];
            if ["and", ",", "&"].contains(&token.raw.text.to_lowercase().as_str())
                && groups.last().unwrap().iter().any(&is_clock)
                && tokens
                    .get(index + 1)
                    .map(&is_clock)
                    .unwrap_or_else(|| is_clock(token))
            {
                groups.push(Vec::new());
            } else {
                groups.last_mut().unwrap().push(token.clone());
            }
        }
        if groups.len() > 1 {
            let first = compile_clause(&groups[0], diagnostics)?;
            let mut clauses = vec![first.clone()];
            for group in &groups[1..] {
                let next = compile_clause(group, diagnostics)?;
                clauses.push(merge_clause(&first, next));
            }
            return Ok(clauses);
        }
    }
    Ok(vec![compile_clause(tokens, diagnostics)?])
}

fn compile_expression(text: &str, tokens: Vec<Token>) -> Expression {
    let start = tokens.first().map(|token| token.raw.start).unwrap_or(0);
    let end = tokens.last().map(|token| token.raw.end).unwrap_or(0);
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut schedule: Option<ScheduleAlias> = None;

    let filtered: Vec<Vec<Token>> = split_clauses(&tokens)
        .into_iter()
        .map(|clause| {
            clause
                .into_iter()
                .filter(|token| {
                    token.label != Role::Glue
                        || token.raw.kind == 2
                        || ["past", "to", "and", "a", "an"]
                            .contains(&token.raw.text.to_lowercase().as_str())
                })
                .map(|mut token| {
                    if token.label == Role::Glue || token.label == Role::Join {
                        token.label = Role::O;
                    }
                    token
                })
                .collect()
        })
        .collect();

    let compiled: Result<Vec<Vec<Clause>>, CompileError> = filtered
        .into_iter()
        .map(|clause| compile_group(&clause, &mut diagnostics))
        .collect();

    match compiled {
        Ok(groups) => {
            schedule = Some(crate::types::Schedule {
                clauses: groups.into_iter().flatten().collect(),
            });
        }
        Err(error) => {
            diagnostics.push(error.diagnostic);
        }
    }

    let scores: Vec<f64> = tokens
        .iter()
        .filter(|token| !matches!(token.label, Role::O | Role::Glue | Role::Join))
        .map(|token| f64::from(token.score))
        .collect();
    let confidence = scores.iter().copied().fold(f64::INFINITY, f64::min);
    if confidence < 0.5 {
        diagnostics.push(diagnostic(
            &tokens[0],
            "low-confidence",
            "The model is uncertain about this expression.",
            crate::types::Severity::warning,
        ));
    }

    Expression {
        start,
        end,
        text: text[start..end].to_string(),
        confidence,
        schedule,
        diagnostics,
    }
}

type ScheduleAlias = crate::types::Schedule;

fn numeric_date_order(tokens: &[Token], order: crate::types::DateOrder) -> Vec<Token> {
    let mut result = tokens.to_vec();
    let separator = |token: Option<&Token>| {
        token.is_some_and(|token| {
            matches!(token.label, Role::O | Role::Glue)
                && matches!(token.raw.text.as_str(), "/" | "." | "-")
        })
    };
    for index in 0..tokens.len().saturating_sub(2) {
        let first = &tokens[index];
        let second = &tokens[index + 2];
        let date_pair = matches!(first.label, Role::Month | Role::Dom)
            && matches!(second.label, Role::Month | Role::Dom);
        if !date_pair || !separator(tokens.get(index + 1)) || second.clause_start {
            continue;
        }
        let year_first = tokens.get(index.wrapping_sub(1)).map(|token| token.label)
            == Some(Role::Year)
            || (index >= 2
                && separator(tokens.get(index - 1))
                && tokens.get(index - 2).map(|token| token.label) == Some(Role::Year));
        let digits = |text: &str| !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
        if !digits(&first.raw.text) || !digits(&second.raw.text) {
            continue;
        }
        if year_first {
            // A year-first numeric date always uses year/month/day, even when
            // invalid.
            result[index].label = Role::Month;
            result[index + 2].label = Role::Dom;
            continue;
        }
        let a: f64 = first.raw.text.parse().unwrap();
        let b: f64 = second.raw.text.parse().unwrap();
        if !(1.0..=31.0).contains(&a) || !(1.0..=31.0).contains(&b) || (a > 12.0 && b > 12.0) {
            continue;
        }
        let selected = if a > 12.0 {
            crate::types::DateOrder::DMY
        } else if b > 12.0 {
            crate::types::DateOrder::MDY
        } else {
            order
        };
        result[index].label = if selected == crate::types::DateOrder::MDY {
            Role::Month
        } else {
            Role::Dom
        };
        result[index + 2].label = if selected == crate::types::DateOrder::MDY {
            Role::Dom
        } else {
            Role::Month
        };
    }
    result
}

pub fn compile_predictions(
    text: &str,
    tokens: &[Token],
    date_order: crate::types::DateOrder,
) -> Vec<Expression> {
    split_expressions(tokens)
        .into_iter()
        .map(|expression| {
            let ordered = numeric_date_order(&expression, date_order);
            compile_expression(text, ordered)
        })
        .collect()
}
