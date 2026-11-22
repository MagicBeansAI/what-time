//! Compiles predicted roles into typed schedules.

use crate::labels::Role;
use crate::lexicon::{
    compound_ordinal, decimal_digit, holiday_name, month, normalize_digits, number, unit, weekday,
};
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
        "tomorrow" | "tmrw" | "tmr" | "2mrw" => Some(1),
        "yesterday" => Some(-1),
        "the day after tomorrow" | "day after tomorrow" => Some(2),
        "the day before yesterday" | "day before yesterday" => Some(-2),
        _ => None,
    }
}

fn hindi_relative(phrase: &str) -> Option<(i64, i64)> {
    match phrase {
        "कल" | "kal" | "kl" => Some((1, -1)),
        "परसों" | "parso" | "parson" => Some((2, -2)),
        _ => None,
    }
}

fn hindi_relative_offset(pair: (i64, i64), tokens: &[Token], start: usize) -> i64 {
    // Tense cues are background tokens. Consult the original sentence before
    // expression extraction discards them, stopping at the next relative day
    // or sentence boundary. Whole tokens avoid matching names such as Nathan.
    let Some(index) = tokens.iter().position(|token| token.raw.start == start) else {
        return pair.0;
    };
    let boundary = |token: &&Token| {
        token.label == Role::RelDay
            || matches!(token.raw.text.as_str(), "." | "!" | "?" | ";" | "।")
    };
    let cue = |token: &Token| match token.raw.text.to_lowercase().as_str() {
        "था" | "थी" | "थे" | "थीं" | "tha" | "thi" | "बीता" | "beeta" => {
            Some(pair.1)
        }
        "आएगा" | "आएगी" | "आएंगे" | "आएँगे" | "जाएँगे" | "है" | "aayega" | "aayegi" | "aayenge"
        | "jayenge" | "jaayenge" | "hai" => Some(pair.0),
        _ => None,
    };
    tokens[index + 1..]
        .iter()
        .take_while(|token| !boundary(token))
        .find_map(cue)
        .or_else(|| {
            tokens[..index]
                .iter()
                .rev()
                .take_while(|token| !boundary(token))
                .find_map(cue)
        })
        .unwrap_or(pair.0)
}

fn day_part(word: &str) -> Option<DayPart> {
    match word {
        "morning" | "सुबह" | "subah" => Some(DayPart::morning),
        "afternoon" | "दोपहर" | "dopahar" => Some(DayPart::afternoon),
        "evening" | "evng" | "शाम" | "shaam" => Some(DayPart::evening),
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
        "daily"
        | "nightly"
        | "everyday"
        | "roz"
        | "roj"
        | "rozz"
        | "rozana"
        | "pratidin"
        | "dainik"
        | "दैनिक"
        | "रोज़"
        | "रोज"
        | "रोज़ाना"
        | "रोजाना"
        | "प्रतिदिन" => Some((Frequency::daily, false)),
        "weekly" | "saptahik" | "साप्ताहिक" => Some((Frequency::weekly, false)),
        "biweekly" | "fortnightly" => Some((Frequency::weekly, true)),
        "monthly" | "masik" | "मासिक" => Some((Frequency::monthly, false)),
        "quarterly" => Some((Frequency::quarterly, false)),
        "yearly" | "annually" | "annual" | "salana" | "सालाना" | "varshik" | "वार्षिक" => {
            Some((Frequency::yearly, false))
        }
        _ => None,
    }
}

fn unit_frequency(unit: Unit) -> Option<Frequency> {
    match unit {
        Unit::hour => Some(Frequency::hourly),
        Unit::day => Some(Frequency::daily),
        Unit::week => Some(Frequency::weekly),
        Unit::month => Some(Frequency::monthly),
        Unit::quarter => Some(Frequency::quarterly),
        Unit::year => Some(Frequency::yearly),
        Unit::minute => Some(Frequency::minutely),
    }
}

/// Whether "of" follows a number in the original sentence ("10th of the
/// month"), skipping an ordinal suffix. Filler words are gone by the time
/// a clause compiles, so the sentence's own tokens are consulted.
fn followed_by_of(token: &Token, context: &[Token]) -> bool {
    let Some(position) = context
        .iter()
        .position(|candidate| candidate.raw.start == token.raw.start)
    else {
        return false;
    };
    context[position + 1..]
        .iter()
        .filter(|candidate| candidate.raw.kind != 3)
        .find(|candidate| {
            !matches!(
                candidate.raw.text.to_lowercase().as_str(),
                "st" | "nd" | "rd" | "th"
            )
        })
        .is_some_and(|candidate| candidate.raw.text.eq_ignore_ascii_case("of"))
}

/// तारीख: the Hindi word that marks a number as a day of the month.
fn date_selector_word(word: &str) -> bool {
    matches!(
        word,
        "तारीख" | "तारीख़" | "tareek" | "tareekh" | "taareekh" | "taareek" | "tarikh"
    )
}

/// Words that double a recurrence step: "alternate Saturdays", "every
/// alternate day".
fn alternating_word(word: &str) -> bool {
    matches!(word, "alternate" | "alternating" | "alternative")
}

/// A weekday group named by its first word: "working days", "business day".
fn working_day_word(word: &str) -> bool {
    matches!(word, "working" | "business" | "work" | "office")
}

/// Applies a day part to a 12-hour clock reading. Night runs past
/// midnight: "2:30 at night" is 02:30 and "12 at night" is midnight, while
/// "10 at night" is 22:00.
fn hour_in_day_part(hour: i64, part: DayPart) -> i64 {
    match part {
        DayPart::morning => hour % 12,
        DayPart::night if hour == 12 || hour < 5 => hour % 12,
        _ => (hour % 12) + 12,
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
    let normalized = normalize_digits(&clock.token.raw.text);
    let bytes = normalized.as_bytes();
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
        let minutes = |value: &ClockTime| value.as_hm().map(|(hour, minute, _)| hour * 60 + minute);
        let start_minutes = minutes(&clocks[0].value);
        let end_minutes = minutes(&clocks[1].value);
        // "from 9 to 5", and "from 8 to 8": two unmarked clocks whose end
        // is not after the start run into the afternoon ("4:15 to 4:45"
        // already is after it).
        if let (Some(start_minutes), Some(end_minutes)) = (start_minutes, end_minutes)
            && end_minutes <= start_minutes
            && end_minutes + 720 > start_minutes
            && end_minutes + 720 < 1440
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
    context: &[Token],
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
            Role::O | Role::Recur => {}

            Role::RangeStart => {
                // Postposed से follows the date it opens ("20 तारीख से 24
                // तारीख तक"): mirror the RangeEnd arm below so the date
                // that follows lands in the end calendar. A preposed
                // English "from" arrives before any calendar content and
                // is unaffected.
                if calendar.is_some() && clocks.is_empty() {
                    calendar_end.get_or_insert_with(CalendarDate::default);
                }
            }

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
                    hindi_relative(&phrase)
                        .map(|pair| hindi_relative_offset(pair, context, token.raw.start))
                });
                let Some(offset) = offset else {
                    // "आज रात 8 बजे मासिक किराया": a word that drew this label
                    // after a complete date or clock is not a day; skip it.
                    if clause.date.is_some() || !clocks.is_empty() {
                        index += 1;
                        continue;
                    }
                    return fail(token, "unsupported", "Unknown relative day.");
                };
                // "tonight at 10" is 22:00.
                if matches!(phrase.as_str(), "tonight" | "tonite")
                    && pending_day_part.is_none()
                    && tokens[index + 1..]
                        .iter()
                        .any(|item| item.label == Role::Hour)
                {
                    pending_day_part = Some(DayPart::night);
                }
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
                let shorthand_unit = match word.as_str() {
                    "eod" | "cob" => Some(Unit::day),
                    "eow" => Some(Unit::week),
                    "eom" => Some(Unit::month),
                    _ => None,
                };
                let Some(value) = shorthand_unit.or_else(|| unit(&word)) else {
                    return fail(token, "unsupported", "Unknown calendar unit.");
                };
                let boundary = shorthand_unit.map(|_| Edge::end).or(edge).or(
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
                ) || working_day_word(&word)
                {
                    Some(DayGroup::weekday)
                } else if matches!(word.as_str(), "day" | "days")
                    && (index
                        .checked_sub(1)
                        .and_then(|previous| tokens.get(previous))
                        .is_some_and(|previous| {
                            previous.label == Role::DayGroup
                                && working_day_word(&previous.raw.text.to_lowercase())
                        })
                        // "on day 25": the number that follows is the day
                        || tokens.get(index + 1).is_some_and(|next| next.label == Role::Dom))
                {
                    // the second word of "working day"
                    index += 1;
                    continue;
                } else {
                    None
                };
                let Some(group) = group else {
                    return fail(token, "unsupported", "Unknown day group.");
                };
                clause.date = Some(DateSpec::DayGroup { group, modifier });
            }

            Role::ClockOffset => {
                // The explicit fractional clock wins over a preceding day-part.
                pending_day_part = None;
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
                // "every night at midnight": a named clock later still
                // takes the day part's place.
                let hour_later = tokens[index + 1..].iter().any(|item| {
                    matches!(item.label, Role::Hour | Role::ClockOffset | Role::TimeNamed)
                });
                if hour_later {
                    pending_day_part = Some(part);
                } else if let Some(clock) = clocks.last_mut().filter(|clock| clock.needs_meridiem) {
                    // A trailing day part ("9:30 night") qualifies the clock
                    // it follows instead of opening a new one.
                    if let Some((hour, minute, second)) = clock.value.as_hm() {
                        let hour = hour_in_day_part(hour, part);
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
                    let hour = hour_in_day_part(hour, part);
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
                until_time: None,
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
            } else if let Some(month) = calendar_value.month
                && calendar_value.day.is_none()
                && modifier.is_some()
            {
                clause.date = Some(DateSpec::CalendarPeriod {
                    month,
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
        // A postposed तक/tak closes after the second clock ("sava char se
        // paune paanch tak"), so it links the pair without sitting
        // between them.
        && !tokens.iter().any(|token| {
            token.label == Role::RangeEnd
                && matches!(token.raw.text.to_lowercase().as_str(), "तक" | "tak")
                && token.raw.start > clocks[1].token.raw.start
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
        until_time: None,
    }
}

/// "on the last day of the month", "the first day of next month": without
/// a recurrence, the first or last day of a period is that period's edge
/// ("end of the month"), which resolves to the single day.
fn period_edge_days(mut tokens: Vec<Token>) -> Vec<Token> {
    if tokens
        .iter()
        .any(|token| matches!(token.label, Role::Recur | Role::Freq | Role::DayGroup))
    {
        return tokens;
    }
    let Some(position) = tokens.iter().position(|token| {
        matches!(token.label, Role::Ord | Role::Deictic | Role::Dom)
            && matches!(
                token.raw.text.to_lowercase().as_str(),
                "last" | "first" | "final"
            )
    }) else {
        return tokens;
    };
    let is_day = |token: &Token| {
        matches!(token.label, Role::Unit | Role::DayGroup)
            && matches!(
                token.raw.text.to_lowercase().as_str(),
                "day" | "din" | "दिन"
            )
    };
    let period_follows = tokens
        .get(position + 2..)
        .unwrap_or_default()
        .iter()
        .find(|token| token.label != Role::O && token.label != Role::Deictic)
        .is_some_and(|token| {
            token.label == Role::Unit
                && matches!(
                    unit(&token.raw.text.to_lowercase()),
                    Some(Unit::week | Unit::month | Unit::quarter | Unit::year)
                )
        });
    if !tokens.get(position + 1).is_some_and(is_day) || !period_follows {
        return tokens;
    }
    let edge = if tokens[position].raw.text.eq_ignore_ascii_case("first") {
        "start"
    } else {
        "end"
    };
    tokens[position].label = Role::Edge;
    tokens[position].raw.text = edge.into();
    tokens[position + 1].label = Role::O;
    tokens
}

/// Hindi and Hinglish put a frequency count after its period ("hafte mein
/// teen baar", "din mein do baar"). Move the count in front of the period
/// so one reading order covers "three times a week" in every language.
fn prepose_frequency_count(mut tokens: Vec<Token>) -> Vec<Token> {
    let Some(times) = tokens.iter().position(|token| token.label == Role::Times) else {
        return tokens;
    };
    let names_period = |token: &Token| matches!(token.label, Role::Unit | Role::Freq);
    if tokens[times + 1..].iter().any(names_period) {
        return tokens;
    }
    let count_start = if times > 0 && tokens[times - 1].label == Role::Num {
        times - 1
    } else {
        times
    };
    let Some(period) = tokens[..count_start].iter().rposition(names_period) else {
        return tokens;
    };
    if tokens[period + 1..count_start]
        .iter()
        .any(|token| token.label != Role::O)
    {
        return tokens;
    }
    let count: Vec<Token> = tokens.drain(count_start..=times).collect();
    for (offset, token) in count.into_iter().enumerate() {
        tokens.insert(period + offset, token);
    }
    tokens
}

/// Hindi and Hinglish put the period first, in the genitive: "har mahine
/// ke aakhri din" is "the last day of every month", "agle mahine ka pehla
/// Somvaar" "the first Monday of next month". Rewrite the period-first
/// order into the English one the grammar reads, so every edge and
/// position rule covers both languages.
fn genitive_period_order(mut tokens: Vec<Token>) -> Vec<Token> {
    let words: Vec<usize> = (0..tokens.len())
        .filter(|index| tokens[*index].raw.kind != 3)
        .collect();
    for window in words.windows(5) {
        let [marker, period, genitive, position, day] = [
            &tokens[window[0]],
            &tokens[window[1]],
            &tokens[window[2]],
            &tokens[window[3]],
            &tokens[window[4]],
        ];
        let names_period = period.label == Role::Unit
            && matches!(
                unit(&period.raw.text.to_lowercase()),
                Some(Unit::week | Unit::month | Unit::quarter | Unit::year)
            );
        let names_day = day.label == Role::Weekday
            || (matches!(day.label, Role::Unit | Role::DayGroup)
                && unit(&day.raw.text.to_lowercase()) == Some(Unit::day));
        if matches!(marker.label, Role::Recur | Role::Deictic)
            && names_period
            && matches!(
                genitive.raw.text.to_lowercase().as_str(),
                "ke" | "ki" | "ka" | "के" | "की" | "का"
            )
            && position.label == Role::Ord
            && names_day
        {
            let mut of = genitive.clone();
            of.label = Role::Glue;
            of.raw.text = "of".into();
            let reordered = [
                position.clone(),
                day.clone(),
                of,
                marker.clone(),
                period.clone(),
            ];
            // Move the words and their labels, not the tokens: each slot
            // keeps its source offsets, so spans stay in text order.
            for (slot, token) in window.iter().zip(reordered) {
                tokens[*slot].label = token.label;
                tokens[*slot].raw.text = token.raw.text;
            }
            return tokens;
        }
    }
    tokens
}

/// "ek din chhod ke ek din", "एक दिन छोड़कर", "do din chhodkar": skipping
/// N units between runs is a step of N + 1 ("every 2 days"). The skip
/// verb is a closed set, like "except"; the rest is the quantity and unit
/// the model already labels. Rewritten into "every N+1 units".
fn skip_step(mut tokens: Vec<Token>) -> Vec<Token> {
    let words: Vec<usize> = (0..tokens.len())
        .filter(|index| tokens[*index].raw.kind != 3)
        .collect();
    let skips = |text: &str| {
        matches!(
            text,
            "chhod" | "chhodkar" | "chhodke" | "chod" | "chodkar"
                // छोड़, with ड़ decomposed (ड + nukta) or precomposed.
                | "\u{91b}\u{94b}\u{921}\u{93c}"
                | "\u{91b}\u{94b}\u{921}\u{93c}\u{915}\u{930}"
                | "\u{91b}\u{94b}\u{921}\u{93c}\u{915}\u{947}"
                | "\u{91b}\u{94b}\u{95c}"
                | "\u{91b}\u{94b}\u{95c}\u{915}\u{930}"
                | "\u{91b}\u{94b}\u{95c}\u{915}\u{947}"
        )
    };
    for position in 0..words.len().saturating_sub(2) {
        let (count, period, verb) = (words[position], words[position + 1], words[position + 2]);
        // "do din chhodkar": the model may read Hinglish "do" as the
        // English verb; inside the skip construction it is the count.
        if !matches!(tokens[count].label, Role::Num | Role::O)
            || tokens[period].label != Role::Unit
            || !skips(&tokens[verb].raw.text.to_lowercase())
        {
            continue;
        }
        let Some(skipped) = number(&tokens[count].raw.text.to_lowercase()) else {
            continue;
        };
        if skipped.fract() != 0.0
            || skipped < 1.0
            || unit(&tokens[period].raw.text.to_lowercase()).is_none()
        {
            continue;
        }
        let period_text = tokens[period].raw.text.clone();
        tokens[count].label = Role::Recur;
        tokens[count].raw.text = "every".into();
        tokens[period].label = Role::Num;
        tokens[period].raw.text = format!("{}", skipped as i64 + 1);
        tokens[verb].label = Role::Unit;
        tokens[verb].raw.text = period_text.clone();
        // "ke"/"kar" after the verb, and the repeated "ek din", restate it.
        let mut rest = position + 3;
        if words.get(rest).is_some_and(|&w| {
            matches!(
                tokens[w].raw.text.to_lowercase().as_str(),
                "ke" | "kar" | "के" | "कर"
            )
        }) {
            tokens[words[rest]].label = Role::Glue;
            rest += 1;
        }
        if let (Some(&n), Some(&u)) = (words.get(rest), words.get(rest + 1))
            && tokens[n].label == Role::Num
            && tokens[u].label == Role::Unit
            && unit(&tokens[u].raw.text.to_lowercase()) == unit(&period_text.to_lowercase())
        {
            tokens[n].label = Role::Glue;
            tokens[u].label = Role::Glue;
        }
        return tokens;
    }
    tokens
}

fn compile_clause(
    input: &[Token],
    diagnostics: &mut Vec<Diagnostic>,
    context: &[Token],
) -> CompileResult<Vec<Clause>> {
    // "quarter" right after a deictic or recurrence marker names the
    // calendar period ("next quarter", "every quarter"), never a clock
    // offset — the clock sense only exists in "quarter past/to N".
    // Relabel first so every downstream check sees the unit.
    let relabeled: Vec<Token> = input
        .iter()
        .enumerate()
        .map(|(index, token)| {
            let mut token = token.clone();
            if matches!(token.label, Role::ClockOffset | Role::DayGroup)
                && token.raw.text.to_lowercase() == "quarter"
                && input[..index]
                    .iter()
                    .rev()
                    .find(|token| token.raw.kind != 3 && token.label != Role::O)
                    .is_some_and(|token| matches!(token.label, Role::Deictic | Role::Recur))
            {
                token.label = Role::Unit;
            }
            // A unit-labeled word that names a holiday is that holiday
            // ("दिवाली" drifts between UNIT and HOLIDAY); the word, not
            // the fine label, carries the meaning.
            if token.label == Role::Unit
                && crate::lexicon::holiday_name(&token.raw.text.to_lowercase()).is_some()
            {
                token.label = Role::Holiday;
            }
            // "monthly on day 25": "day" introduces the day number.
            if matches!(token.label, Role::DayGroup | Role::Unit)
                && matches!(token.raw.text.to_lowercase().as_str(), "day" | "days")
                && input
                    .get(index + 1)
                    .is_some_and(|next| next.label == Role::Dom)
            {
                token.label = Role::O;
            }
            // "working day" / "business days" is one weekday group; its
            // second word must not read as a daily frequency.
            if token.label == Role::Unit
                && matches!(token.raw.text.to_lowercase().as_str(), "day" | "days")
                && index.checked_sub(1).is_some_and(|previous| {
                    input[previous].label == Role::DayGroup
                        && working_day_word(&input[previous].raw.text.to_lowercase())
                })
            {
                token.label = Role::DayGroup;
            }
            token
        })
        .collect();
    let relabeled = prepose_frequency_count(relabeled);
    let relabeled = period_edge_days(relabeled);
    let input: &[Token] = &relabeled;
    let last_meaningful = input.iter().rev().find(|token| token.label != Role::O);
    if let Some(last) = last_meaningful.filter(|token| token.label == Role::RangeEnd) {
        // Hindi/Hinglish postpose the range end ("20 tareekh se 24 tareekh
        // tak", "sava char se paune paanch tak"): the closing marker
        // follows the date or clock it closes, and the opening से is
        // still present earlier, so the range is complete, not unfinished.
        let word = last.raw.text.to_lowercase();
        let postposed = matches!(word.as_str(), "तक" | "tak")
            && input.iter().any(|token| token.label == Role::RangeStart)
            && input.iter().any(|token| {
                matches!(
                    token.label,
                    Role::Dom
                        | Role::Month
                        | Role::Year
                        | Role::Hour
                        | Role::TimeNamed
                        | Role::ClockOffset
                )
            });
        // "remind me every Sunday at 8 pm to plan the week": a closing "to"
        // with nothing after it and no "from"/"between" before it is the
        // infinitive, not a range.
        // A sentence that stops at "to" ("Monday 5pm to") is unfinished.
        let word_follows = context.iter().any(|token| {
            token.raw.kind != 3
                && token.raw.start >= last.raw.end
                && token.raw.text.chars().any(char::is_alphabetic)
        });
        let infinitive = matches!(word.as_str(), "to" | "till")
            && word_follows
            && !input.iter().any(|token| token.label == Role::RangeStart)
            && input.iter().any(|token| {
                matches!(
                    token.label,
                    Role::Hour
                        | Role::TimeNamed
                        | Role::Weekday
                        | Role::DayGroup
                        | Role::Dom
                        | Role::Unit
                        | Role::RelDay
                )
            });
        if infinitive {
            let trimmed: Vec<Token> = input
                .iter()
                .filter(|token| token.raw.start != last.raw.start)
                .cloned()
                .collect();
            return compile_clause(&trimmed, diagnostics, context);
        }
        if !postposed {
            if word == "until"
                && input
                    .iter()
                    .any(|token| matches!(token.label, Role::Recur | Role::Freq | Role::DayGroup))
            {
                return fail(last, "invalid-bound", "A bound needs a date.");
            }
            return fail(last, "incomplete-range", "A range needs an end value.");
        }
    }
    let day_part_selects = |token: &Token| {
        token.label == Role::DayPart
            || (token.label == Role::Meridiem && day_part(&token.raw.text.to_lowercase()).is_some())
    };
    if input.iter().any(|token| token.label == Role::Recur)
        && !input.iter().any(|token| {
            matches!(
                token.label,
                Role::Unit | Role::Freq | Role::Weekday | Role::DayGroup | Role::Month | Role::Dom
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
    // Whether the phrase named its frequency. A recurrence that only lists
    // selectors ("every 15th", "every January") takes the frequency its
    // selectors imply once the clause is assembled.
    let mut explicit_frequency = implicit_ordinal;
    // A clock that arrived inside a bound ("except Wednesday at 9am")
    // belongs to the clause.
    let mut bound_time: Option<TimeSpec> = None;
    let mut excluded_times: Vec<TimeSpec> = Vec::new();
    let mut until_time: Option<ClockTime> = None;

    let mut index = 0usize;
    while index < tokens.len() {
        let token = &tokens[index];

        if token.label == Role::Unit && date_selector_word(&token.raw.text.to_lowercase()) {
            // तारीख names a DOM selector, not a recurrence frequency or a
            // standalone calendar period. The model must also supply its day.
            let adjacent_day = index
                .checked_sub(1)
                .and_then(|i| tokens.get(i))
                .is_some_and(|t| matches!(t.label, Role::Dom | Role::Ord))
                || tokens.get(index + 1).is_some_and(|t| t.label == Role::Dom);
            if !adjacent_day {
                return fail(
                    token,
                    "invalid-date",
                    "A date selector needs a day of month.",
                );
            }
            index += 1;
            continue;
        }

        if token.label == Role::Recur {
            recurrence.get_or_insert_with(|| empty_recurrence(Frequency::weekly));
            // A lone "roz" / "रोज़" names its own frequency.
            if let Some((freq, doubled)) = frequency_word(&token.raw.text.to_lowercase())
                && let Some(rule) = recurrence.as_mut()
            {
                rule.freq = freq;
                explicit_frequency = true;
                if doubled {
                    rule.interval = 2;
                }
            }
            // "alternate Saturdays": the marker itself doubles the step.
            if alternating_word(&token.raw.text.to_lowercase())
                && let Some(rule) = recurrence.as_mut()
            {
                rule.interval = 2;
            }
            // "every other Friday": the corpus briefs label "other" GLUE and
            // the structural generators NUM; either way it is a step of two.
            if let Some(next) = tokens.get(index + 1)
                && next.label == Role::O
                && matches!(
                    next.raw.text.to_lowercase().as_str(),
                    "other" | "alternate" | "alternating"
                )
                && tokens.get(index + 2).is_some_and(|after| {
                    matches!(after.label, Role::Weekday | Role::DayGroup | Role::Unit)
                })
            {
                if let Some(rule) = recurrence.as_mut() {
                    rule.interval = 2;
                }
                index += 2;
                continue;
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
                    explicit_frequency = true;
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
                    explicit_frequency = true;
                    index += 2;
                    continue;
                }
            }
            index += 1;
            continue;
        }

        if token.label == Role::Freq {
            let word = token.raw.text.to_lowercase();
            let named = frequency_word(&word)
                .or_else(|| Some((unit(&word).and_then(unit_frequency)?, false)));
            let Some((freq, doubled)) = named else {
                return fail(token, "unsupported", "Unknown recurrence frequency.");
            };
            explicit_frequency = true;
            if word == "nightly" {
                // "nightly at 2" is every night: the night biases its clock.
                let mut night = token.clone();
                night.label = Role::DayPart;
                night.raw.text = "night".into();
                body.push(night);
            }
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
                .is_some_and(|token| matches!(token.label, Role::Unit | Role::Freq))
            {
                return fail(
                    token,
                    "invalid-frequency",
                    "A frequency count needs a positive number and a period.",
                );
            }
            let period_word = tokens[period].raw.text.to_lowercase();
            // "twice daily", "thrice weekly"; a period noun may also arrive
            // under either label ("week" in "twice a week").
            let named = frequency_word(&period_word)
                .filter(|(_, doubled)| !doubled)
                .map(|(frequency, _)| frequency)
                .or_else(|| unit(&period_word).and_then(unit_frequency));
            explicit_frequency = true;
            let Some(frequency) = named.filter(|frequency| *frequency != Frequency::minutely)
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
            index = period + 1;
            continue;
        }

        // An ordinal before a period is either a day of that period ("the
        // last day of every month") or the step ("every third day").
        if recurrence.is_some() && matches!(token.label, Role::Ord | Role::Dom | Role::Deictic) {
            let unit_of = |token: &Token| {
                (token.label == Role::Unit)
                    .then(|| unit(&token.raw.text.to_lowercase()))
                    .flatten()
            };
            let value = number(&token.raw.text.to_lowercase()).filter(|value| is_integer(*value));
            // "every 10th of the month", "the 1st of every month": an
            // ordinal followed by "of" and a month is that day of the month.
            if matches!(token.label, Role::Ord | Role::Dom)
                && let Some(value) =
                    value.filter(|value| *value == -1.0 || (1.0..=31.0).contains(value))
                && followed_by_of(token, context)
                // "of the month", "of every month", "of each month"
                && tokens[index + 1..]
                    .iter()
                    .find(|next| next.label != Role::Recur)
                    .is_some_and(|next| {
                        next.label == Role::Unit
                            && unit(&next.raw.text.to_lowercase()) == Some(Unit::month)
                    })
            {
                if let Some(rule) = recurrence.as_mut() {
                    rule.by_month_day
                        .get_or_insert_with(Vec::new)
                        .push(value as i64);
                    rule.freq = Frequency::monthly;
                }
                explicit_frequency = true;
                index += 1;
                continue;
            }
            // "har mahine ki pehli tarikh", "आख़िरी तारीख": an ordinal before
            // तारीख is that day of the month.
            if token.label == Role::Ord
                && let Some(value) = value.filter(|value| *value == -1.0 || *value >= 1.0)
                && tokens.get(index + 1).is_some_and(|next| {
                    next.label == Role::Unit && date_selector_word(&next.raw.text.to_lowercase())
                })
            {
                if let Some(rule) = recurrence.as_mut() {
                    rule.by_month_day
                        .get_or_insert_with(Vec::new)
                        .push(value as i64);
                }
                index += 1;
                continue;
            }
            if let (Some(next_unit), Some(value)) = (tokens.get(index + 1).and_then(unit_of), value)
            {
                // "the last day of every month"; "the last day of December"
                // names its month directly.
                let enclosing = tokens[index + 2..]
                    .iter()
                    .find_map(|token| {
                        if token.label == Role::Month {
                            Some(Unit::month)
                        } else {
                            unit_of(token)
                        }
                    })
                    .filter(|unit| matches!(unit, Unit::month | Unit::quarter | Unit::year));
                let day_of_period = next_unit == Unit::day
                    && enclosing.is_some()
                    && (value == -1.0 || (1.0..=31.0).contains(&value));
                if day_of_period && let Some(rule) = recurrence.as_mut() {
                    rule.by_month_day
                        .get_or_insert_with(Vec::new)
                        .push(value as i64);
                    if enclosing == Some(Unit::year) {
                        rule.by_month
                            .get_or_insert_with(Vec::new)
                            .push(if value < 0.0 { 12 } else { 1 });
                    }
                    index += 2;
                    continue;
                }
                if token.label != Role::Deictic && value >= 2.0 && enclosing.is_none() {
                    if let Some(rule) = recurrence.as_mut() {
                        rule.interval = value as i64;
                    }
                    index += 1;
                    continue;
                }
            }
            // "every 2nd Saturday": a small day number directly before a
            // weekday counts that weekday within the month.
            if token.label == Role::Dom
                && let Some(value) = value.filter(|value| (1.0..=5.0).contains(value))
                && tokens
                    .get(index + 1)
                    .is_some_and(|next| next.label == Role::Weekday)
                && !index
                    .checked_sub(1)
                    .is_some_and(|previous| tokens[previous].label == Role::Month)
            {
                if let Some(rule) = recurrence.as_mut() {
                    rule.by_set_pos
                        .get_or_insert_with(Vec::new)
                        .push(value as i64);
                }
                index += 1;
                continue;
            }
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
            explicit_frequency = true;
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
            // Legacy structural data labels the introducer DUR (for/lasting),
            // while the corpus briefs put DUR on the quantity itself.
            let quantity_here = bare_duration || number(&token.raw.text.to_lowercase()).is_some();
            let mut amount_index = index + if quantity_here { 0 } else { 1 };
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

        // "every half an hour": an article may sit between the quantity
        // and its unit.
        // आधे/half reaches here as a clock offset when the model reads it
        // the way it does in "साढ़े आठ"; before a unit it is a quantity.
        let quantity_token = token.label == Role::Num
            || (token.label == Role::ClockOffset
                && number(&token.raw.text.to_lowercase()) == Some(0.5));
        let mut interval_target = index + 1;
        while quantity_token
            && tokens.get(interval_target).is_some_and(|token| {
                matches!(token.label, Role::O | Role::Num)
                    && matches!(token.raw.text.to_lowercase().as_str(), "a" | "an")
            })
        {
            interval_target += 1;
        }
        let is_weekday_interval = quantity_token
            && tokens.get(interval_target).is_some_and(|token| {
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
            // A fraction of an hour is a whole number of minutes.
            let hour_fraction = !is_integer(interval)
                && interval > 0.0
                && is_integer(interval * 60.0)
                && tokens[interval_target].label == Role::Unit
                && unit(&tokens[interval_target].raw.text.to_lowercase()) == Some(Unit::hour);
            if hour_fraction {
                if let Some(rule) = recurrence.as_mut() {
                    rule.freq = Frequency::minutely;
                    rule.interval = (interval * 60.0) as i64;
                }
                explicit_frequency = true;
                index = interval_target + 1;
                continue;
            }
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
            index = interval_target;
            continue;
        }

        if recurrence_bounds(token.label) {
            if recurrence.is_none() && token.label != Role::BoundStart {
                return fail(token, "unsupported", "This bound requires recurrence.");
            }

            // "except on", "starting from": one marker in two words.
            let mut marker_end = index;
            while tokens
                .get(marker_end + 1)
                .is_some_and(|next| next.label == token.label)
            {
                marker_end += 1;
            }
            let mut end = marker_end + 1;
            // "starting tomorrow, every day at 9am": the bound's date ends
            // where the recurrence begins.
            while end < tokens.len()
                && !recurrence_bounds(tokens[end].label)
                && !matches!(tokens[end].label, Role::Recur | Role::Freq)
                && tokens[end].label != Role::Dur
                && !(tokens[end].label == Role::Num
                    && tokens
                        .get(end + 1)
                        .is_some_and(|token| token.label == Role::Count))
            {
                end += 1;
            }

            let mut bound = &tokens[marker_end + 1..end];
            while bound
                .last()
                .is_some_and(|last| matches!(last.label, Role::Join | Role::Glue | Role::O))
            {
                bound = &bound[..bound.len() - 1];
            }
            // "kal se roz": a postposition marker follows its date.
            let mut preposed: Vec<Token> = Vec::new();
            if bound.is_empty()
                && token.label == Role::BoundStart
                && matches!(token.raw.text.to_lowercase().as_str(), "se" | "से")
            {
                while body.last().is_some_and(|last| {
                    matches!(
                        last.label,
                        Role::RelDay
                            | Role::Weekday
                            | Role::Month
                            | Role::Dom
                            | Role::Year
                            | Role::Deictic
                            | Role::Ord
                            | Role::Unit
                    ) || (last.raw.kind == 3 && !preposed.is_empty())
                }) {
                    preposed.insert(0, body.pop().unwrap());
                }
                bound = &preposed;
            }
            if bound.is_empty() {
                return fail(token, "invalid-bound", "A bound needs a date.");
            }
            let bound_clause = compile_date_and_time(bound, diagnostics, context)?;
            // "weekdays except Wednesday at 9am": the clock after an
            // exception is the rule's. A start or end bound's clock is the
            // bound's own ("until 8pm today").
            let sub_daily = recurrence
                .as_ref()
                .is_some_and(|rule| matches!(rule.freq, Frequency::minutely | Frequency::hourly));
            // "every hour except 1pm": a clock exception removes those
            // times from a repeating clock.
            if token.label == Role::Except
                && sub_daily
                && let Some(time) = bound_clause.time.clone()
            {
                excluded_times.push(time);
                // "except between 12 and 2pm on weekdays": the days after a
                // clock exception select the rule's days.
                let days = match &bound_clause.date {
                    None => None,
                    Some(DateSpec::Weekday {
                        days,
                        modifier: None,
                    }) => Some(days.clone()),
                    Some(DateSpec::DayGroup {
                        group,
                        modifier: None,
                    }) => Some(if *group == DayGroup::weekday {
                        WEEKDAYS[..5].to_vec()
                    } else {
                        WEEKDAYS[5..].to_vec()
                    }),
                    Some(_) => {
                        return fail(
                            token,
                            "unsupported",
                            "A clock exception can only carry the rule's weekdays.",
                        );
                    }
                };
                if let (Some(days), Some(rule)) = (days, recurrence.as_mut()) {
                    rule.by_day = Some(days);
                }
                index = end;
                continue;
            }
            // "every day starting tomorrow at 9am": a start bound's clock
            // is the rule's too, unless the rule names its own.
            if bound_time.is_none() && matches!(token.label, Role::Except | Role::BoundStart) {
                bound_time = bound_clause.time.clone();
            }
            // "until 8pm today", "until 8pm": the end bound keeps its clock.
            if token.label == Role::BoundEnd
                && let Some(time) = &bound_clause.time
                && time.end.is_none()
            {
                until_time = Some(time.start.clone());
            }
            let date = match (bound_clause.date, &bound_clause.time, token.label) {
                (Some(date), _, _) => date,
                (None, Some(_), Role::BoundEnd) => DateSpec::RelativeDay { offset: 0 },
                _ => return fail(token, "invalid-bound", "A bound needs a date."),
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
        compile_date_and_time(&body, diagnostics, context)?
    } else {
        Clause::default()
    };
    if shift.is_some() {
        clause.shift = shift;
    }
    if clause.time.is_none() && recurrence.is_some() {
        clause.time = bound_time;
    }
    if let Some(duration) = duration {
        clause.duration = Some(duration);
    }
    if let Some(rule) = recurrence.as_mut()
        && let Some(date) = starting_date.take()
    {
        // The recurrence followed its leading start bound.
        rule.start = Some(Box::new(date));
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
    // A bare plural names the habit, not one date: "Mondays and Thursdays
    // at 7" recurs weekly.
    if recurrence.is_none()
        && matches!(clause.date, Some(DateSpec::Weekday { modifier: None, .. }))
        && clause.end_date.is_none()
        && body.iter().any(|token| {
            let word = token.raw.text.to_lowercase();
            token.label == Role::Weekday && word.ends_with("days") && weekday(&word).is_some()
        })
    {
        recurrence = Some(empty_recurrence(Frequency::weekly));
        explicit_frequency = true;
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
        if !explicit_frequency {
            if rule.by_set_pos.is_some() {
                // "every second Saturday" counts within the month.
                rule.freq = Frequency::monthly;
            } else if rule.by_month.is_some() && rule.by_day.is_none() {
                // "every January", "each March 3rd"
                rule.freq = Frequency::yearly;
            } else if rule.by_month_day.is_some() && rule.by_day.is_none() {
                // "every 15th"
                rule.freq = Frequency::monthly;
            }
        }
        if rule.times_per == Some(1) {
            // "once a day at 9am" is the plain daily rule.
            rule.times_per = None;
        }
        // "every 24 hours" is daily, "every 120 minutes" every 2 hours.
        if rule.freq == Frequency::minutely && rule.interval % 60 == 0 {
            rule.freq = Frequency::hourly;
            rule.interval /= 60;
        }
        if rule.freq == Frequency::hourly
            && rule.interval % 24 == 0
            && clause.time.as_ref().is_some_and(|time| time.end.is_none())
        {
            // "every 24 hours at 9am" is daily at 9.
            rule.freq = Frequency::daily;
            rule.interval /= 24;
        }
        if rule.freq == Frequency::hourly && rule.interval % 24 == 0 && clause.time.is_none() {
            // An hour step stays timed: on the wall-clock grid every other
            // hour step uses, 24 hours fire at midnight ("0 0 * * *"), not
            // as an all-day event that already includes today.
            rule.freq = Frequency::daily;
            rule.interval /= 24;
            clause.time = Some(TimeSpec {
                start: ClockTime::Hm {
                    hour: 0,
                    minute: 0,
                    second: None,
                },
                end: None,
                open: None,
            });
        }
        if matches!(rule.freq, Frequency::minutely | Frequency::hourly)
            && rule.span.is_none()
            && rule.count.is_none()
            && rule.until.is_none()
            && clause.duration.is_some()
        {
            // "every 15 minutes for 2 hours": the duration bounds the series.
            rule.span = clause.duration.take();
        }
        if rule.until.is_some() {
            rule.until_time = until_time;
        }
        clause.recurrence = Some(rule);
    }

    if !excluded_times.is_empty() {
        return exclude_times(clause, &excluded_times, &input[0]);
    }
    Ok(vec![clause])
}

fn minutes_of(clock: &ClockTime, end: bool) -> Option<i64> {
    let options = crate::types::ResolveOptions::default();
    crate::clock::clock_seconds(clock, &options, end)
        .ok()
        .map(|seconds| (seconds / 60.0) as i64)
}

/// Removes excepted clocks from a repeating clock by splitting its daily
/// window: "every hour except 1pm" is 00:00–12:00 and 14:00–23:00, which
/// every export states exactly.
fn exclude_times(clause: Clause, excluded: &[TimeSpec], at: &Token) -> CompileResult<Vec<Clause>> {
    let unsupported = |message: &str| fail(at, "unsupported", message);
    let Some(rule) = clause.recurrence.as_ref() else {
        return unsupported("A clock exception needs a repeating clock.");
    };
    let step = match rule.freq {
        Frequency::minutely => rule.interval,
        Frequency::hourly => rule.interval * 60,
        _ => return unsupported("A clock exception needs a repeating clock."),
    };
    let (first, last) = match &clause.time {
        Some(TimeSpec {
            start,
            end: Some(end),
            ..
        }) => match (minutes_of(start, false), minutes_of(end, true)) {
            (Some(first), Some(last)) if last > first => (first, last),
            _ => {
                return unsupported(
                    "A clock exception inside an overnight window is not supported.",
                );
            }
        },
        None if 1440 % step == 0 => (0, 1440 - step),
        _ => {
            return unsupported("A clock exception needs a window or a step that divides the day.");
        }
    };
    let removed = |minute: i64| {
        excluded.iter().any(|time| {
            let start = minutes_of(&time.start, false);
            let end = time.end.as_ref().and_then(|end| minutes_of(end, true));
            match (start, end) {
                // "except between 12 and 2pm": resume at 2pm.
                (Some(start), Some(end)) => minute >= start && minute < end,
                (Some(start), None) => minute == start,
                _ => false,
            }
        })
    };
    let kept: Vec<i64> = (0..)
        .map(|index| first + index * step)
        .take_while(|minute| *minute <= last)
        .filter(|minute| !removed(*minute))
        .collect();
    if kept.is_empty() {
        return unsupported("The exception removes every time.");
    }
    let mut runs: Vec<Vec<i64>> = Vec::new();
    for minute in kept {
        match runs.last_mut() {
            Some(run) if minute - run[run.len() - 1] == step => run.push(minute),
            _ => runs.push(vec![minute]),
        }
    }
    let hm = |minute: i64| ClockTime::hm(minute / 60, minute % 60);
    Ok(runs
        .into_iter()
        .map(|run| {
            let mut part = clause.clone();
            if run.len() == 1 {
                part.time = Some(TimeSpec {
                    start: hm(run[0]),
                    end: None,
                    open: None,
                });
                if let Some(rule) = part.recurrence.as_mut() {
                    rule.freq = if rule.by_day.is_some() && rule.by_month_day.is_none() {
                        Frequency::weekly
                    } else {
                        Frequency::daily
                    };
                    rule.interval = 1;
                }
            } else {
                part.time = Some(TimeSpec {
                    start: hm(run[0]),
                    end: Some(hm(run[run.len() - 1])),
                    open: None,
                });
            }
            part
        })
        .collect())
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

/// "every Tuesday at 3pm and every Friday at 5pm": a conjunction followed
/// by a new recurrence marker starts an independent rule.
/// "every Tuesday at 3pm and every Friday at 5pm", "weekdays at 9am and
/// weekends at 11am", "every Monday at 10am and the 1st of every month at
/// noon": a conjunction that opens a new day selector after a complete
/// timed rule starts an independent rule. A later part without its own
/// recurrence marker keeps the first part's ("every Monday at 9 and
/// Tuesday at 10" recurs on both days).
fn split_recurrences(tokens: &[Token]) -> Vec<Vec<Token>> {
    let marks = |token: &Token| matches!(token.label, Role::Recur | Role::Freq);
    let selector = |token: &Token| {
        matches!(
            token.label,
            Role::Recur | Role::Freq | Role::Weekday | Role::DayGroup | Role::Dom | Role::Ord
        )
    };
    let clock = |token: &Token| {
        matches!(
            token.label,
            Role::Hour | Role::TimeNamed | Role::ClockOffset | Role::DayPart
        )
    };
    let conjunction = |token: &Token| {
        token.label == Role::O
            && matches!(
                token.raw.text.to_lowercase().as_str(),
                "and" | "," | ";" | "&" | "plus" | "also" | "aur" | "और"
            )
    };
    let mut cuts: Vec<usize> = Vec::new();
    let mut start = 0usize;
    for index in 1..tokens.len().saturating_sub(1) {
        let left = &tokens[start..index];
        let next = &tokens[index + 1];
        let new_recurrence = next.label == Role::Recur
            && left.iter().any(marks)
            // "har roz" style doubled markers stay together
            && left
                .iter()
                .any(|token| !matches!(token.label, Role::O | Role::Recur));
        let new_timed_rule = selector(next)
            && left.iter().any(clock)
            && left.iter().any(|token| {
                selector(token) && token.label != Role::Dom || token.label == Role::Dom
            })
            && tokens[index + 1..].iter().any(clock)
            && left.iter().any(|token| {
                matches!(
                    token.label,
                    Role::Recur | Role::Freq | Role::DayGroup | Role::Weekday | Role::Dom
                )
            });
        if conjunction(&tokens[index]) && (new_recurrence || new_timed_rule) {
            cuts.push(index);
            start = index + 1;
        }
    }
    if cuts.is_empty() {
        return vec![tokens.to_vec()];
    }
    let mut groups = Vec::new();
    let mut start = 0usize;
    for cut in cuts.into_iter().chain(std::iter::once(tokens.len())) {
        groups.push(tokens[start..cut].to_vec());
        start = cut + 1;
    }
    let leading_marker = groups[0].iter().find(|token| marks(token)).cloned();
    for group in groups.iter_mut().skip(1) {
        if !group
            .iter()
            .any(|token| marks(token) || token.label == Role::DayGroup)
            && let Some(marker) = &leading_marker
            && marker.label == Role::Recur
        {
            group.insert(0, marker.clone());
        }
    }
    groups
}

fn compile_group(
    tokens: &[Token],
    diagnostics: &mut Vec<Diagnostic>,
    context: &[Token],
) -> CompileResult<Vec<Clause>> {
    let independent = split_recurrences(tokens);
    if independent.len() > 1 {
        let mut clauses = Vec::new();
        for group in &independent {
            clauses.extend(compile_group(group, diagnostics, context)?);
        }
        return Ok(clauses);
    }
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
        return compile_clause(tokens, diagnostics, context);
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
            let start = compile_date_and_time(left, diagnostics, context)?;
            let end = compile_date_and_time(right, diagnostics, context)?;
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
            // "subah 9 baje aur raat 9 baje": a day part may open the
            // next clock.
            let opens_clock = |at: usize| {
                tokens.get(at).is_some_and(|next| {
                    is_clock(next)
                        || (next.label == Role::DayPart
                            && tokens.get(at + 1).is_some_and(&is_clock))
                })
            };
            if ["and", ",", "&", "aur", "और"].contains(&token.raw.text.to_lowercase().as_str())
                && groups.last().unwrap().iter().any(&is_clock)
                && (opens_clock(index + 1) || (index + 1 == tokens.len() && is_clock(token)))
            {
                groups.push(Vec::new());
            } else {
                groups.last_mut().unwrap().push(token.clone());
            }
        }
        if groups.len() > 1 {
            let mut clauses = compile_clause(&groups[0], diagnostics, context)?;
            let first = clauses[0].clone();
            for group in &groups[1..] {
                for next in compile_clause(group, diagnostics, context)? {
                    clauses.push(merge_clause(&first, next));
                }
            }
            // "twice a day at 9am and 9pm": the listed clocks are the count.
            let listed = clauses.len() as i64;
            if clauses.iter().all(|clause| {
                clause.time.is_some()
                    && clause
                        .recurrence
                        .as_ref()
                        .is_some_and(|rule| rule.times_per == Some(listed))
            }) {
                for clause in clauses.iter_mut() {
                    if let Some(rule) = clause.recurrence.as_mut() {
                        rule.times_per = None;
                    }
                }
            }
            return Ok(clauses);
        }
    }
    compile_clause(tokens, diagnostics, context)
}

fn compile_expression(text: &str, tokens: Vec<Token>, context: &[Token]) -> Expression {
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
                        || ["past", "to", "and", "a", "an", "other"]
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
        .map(|clause| compile_group(&clause, &mut diagnostics, context))
        .collect();

    match compiled {
        Ok(groups) => {
            schedule = Some(crate::types::Schedule {
                clauses: merge_selector_clauses(groups.into_iter().flatten().collect()),
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

fn selects_days(rule: &Recurrence) -> bool {
    rule.by_day.is_some()
        || rule.by_month_day.is_some()
        || rule.by_set_pos.is_some()
        || rule.by_month.is_some()
}

/// A recurring clause with no clock next to a timed one narrows it:
/// "every 15 minutes between 9 and 11 every Saturday" is one rule on
/// Saturdays, and "weekly" in "send the weekly report every Friday at
/// 4:30pm" adds nothing to the Friday rule.
fn merge_selector_clauses(mut clauses: Vec<Clause>) -> Vec<Clause> {
    if clauses.len() < 2 {
        return clauses;
    }
    let bare = |clause: &Clause| {
        clause.time.is_none()
            && clause.date.is_none()
            && clause.shift.is_none()
            && clause.duration.is_none()
            && clause.recurrence.as_ref().is_some_and(|rule| {
                rule.count.is_none()
                    && rule.until.is_none()
                    && rule.start.is_none()
                    && rule.except.is_none()
                    && rule.span.is_none()
                    && rule.times_per.is_none()
                    && rule.interval == 1
            })
    };
    let mut index = 0;
    while index < clauses.len() {
        if !bare(&clauses[index]) {
            index += 1;
            continue;
        }
        let donor = clauses[index].recurrence.clone().unwrap();
        let target = (0..clauses.len()).find(|other| {
            *other != index
                && clauses[*other].time.is_some()
                && clauses[*other]
                    .recurrence
                    .as_ref()
                    .is_some_and(|rule| !selects_days(rule) || !selects_days(&donor))
        });
        let Some(target) = target else {
            index += 1;
            continue;
        };
        if selects_days(&donor)
            && let Some(rule) = clauses[target].recurrence.as_mut()
        {
            rule.by_day = donor.by_day;
            rule.by_month_day = donor.by_month_day;
            rule.by_set_pos = donor.by_set_pos;
            rule.by_month = donor.by_month;
            if rule.by_set_pos.is_some() && rule.freq == Frequency::weekly {
                rule.freq = Frequency::monthly;
            }
        }
        clauses.remove(index);
    }
    clauses
}

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
        let digits =
            |text: &str| !text.is_empty() && text.chars().all(|c| decimal_digit(c).is_some());
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
        let a = number(&first.raw.text).unwrap();
        let b = number(&second.raw.text).unwrap();
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

/// Structural label repairs the grammar makes certain, before compiling:
/// - a number right after a month name, with no colon or meridiem after it,
///   is the day ("Nov 22", "Jan 1 at midnight"), not an hour;
/// - an HOUR right after `HOUR :` is the minutes ("7:30pm");
/// - a MINUTE right after a list separator is another hour ("8, 12, 16");
/// - "everyday" beside an expression is the daily frequency;
/// - "quarter" right after "every"/"next"/"har" is the calendar period
///   ("every quarter"), whatever fine label it drew.
fn repair_labels(tokens: &[Token]) -> Vec<Token> {
    let mut repaired = tokens.to_vec();
    let words: Vec<usize> = (0..tokens.len())
        .filter(|index| tokens[*index].raw.kind != 3)
        .collect();
    for (position, &index) in words.iter().enumerate() {
        let previous = position.checked_sub(1).map(|p| &tokens[words[p]]);
        let before_previous = position.checked_sub(2).map(|p| &tokens[words[p]]);
        let next = words.get(position + 1).map(|&n| &tokens[n]);
        let token = &tokens[index];
        match token.label {
            Role::Hour
                if previous.is_some_and(|p| p.label == Role::Month)
                    && !next.is_some_and(|n| {
                        n.raw.text == ":" || matches!(n.label, Role::Meridiem | Role::Minute)
                    }) =>
            {
                repaired[index].label = Role::Dom;
            }
            // "every quarter hour" / "half hour": a fraction of the hour.
            _ if matches!(token.raw.text.to_lowercase().as_str(), "quarter" | "half")
                && token.label != Role::Num
                && previous.is_some_and(|p| matches!(p.label, Role::Recur | Role::Freq))
                && next.is_some_and(|n| {
                    matches!(n.raw.text.to_lowercase().as_str(), "hour" | "hours")
                }) =>
            {
                repaired[index].label = Role::Num;
            }
            // "remind me to take out the trash every Tuesday night": a
            // recurrence marker right before a day, a unit or a day part is
            // never background ("every customer" has no such word after it).
            Role::O
                if matches!(
                    token.raw.text.to_lowercase().as_str(),
                    "every" | "each" | "har" | "हर"
                ) && next.is_some_and(|n| {
                    matches!(n.label, Role::Weekday | Role::DayGroup | Role::DayPart)
                        || (n.label == Role::Unit
                            && crate::lexicon::unit(&n.raw.text.to_lowercase()).is_some())
                }) =>
            {
                repaired[index].label = Role::Recur;
            }
            // "every 3 hours from 10am", "every 2 hours starting (at) 8am":
            // after a sub-daily step, "from" / "starting" before a clock
            // opens the step's daily window, not a shift or a date bound.
            Role::DirAfter | Role::BoundStart
                if matches!(
                    token.raw.text.to_lowercase().as_str(),
                    "from" | "starting" | "beginning"
                ) && previous.is_some_and(|p| {
                    p.label == Role::Unit
                        && matches!(
                            crate::lexicon::unit(&p.raw.text.to_lowercase()),
                            Some(Unit::hour | Unit::minute)
                        )
                }) && before_previous
                    .is_some_and(|b| matches!(b.label, Role::Recur | Role::Num | Role::Freq))
                    && words[position + 1..]
                        .iter()
                        .map(|&n| &tokens[n])
                        .find(|n| n.label != Role::Glue)
                        .is_some_and(|n| matches!(n.label, Role::Hour | Role::TimeNamed))
                    // "every hour starting 3pm tomorrow" stays a dated start.
                    && !words[position + 1..].iter().any(|&n| {
                        matches!(
                            tokens[n].label,
                            Role::RelDay | Role::Weekday | Role::Month | Role::Dom
                        )
                    }) =>
            {
                repaired[index].label = Role::RangeStart;
            }
            // "kal se roz 7 baje": a postposition "se" after a date and
            // before a recurrence starts the series (from tomorrow).
            Role::RangeStart | Role::DirAfter | Role::Glue | Role::O
                if matches!(token.raw.text.to_lowercase().as_str(), "se" | "से")
                    && previous.is_some_and(|p| {
                        matches!(
                            p.label,
                            Role::RelDay | Role::Weekday | Role::Month | Role::Dom | Role::Unit
                        ) || (p.label == Role::O
                            && crate::lexicon::weekday(&p.raw.text.to_lowercase()).is_some())
                    })
                    && next.is_some_and(|n| matches!(n.label, Role::Recur | Role::Freq)) =>
            {
                repaired[index].label = Role::BoundStart;
                // "सोमवार से रोज़": the weekday the series starts on.
                let day = words[position - 1];
                if tokens[day].label == Role::O {
                    repaired[day].label = Role::Weekday;
                }
            }
            // "hourly from 8 to 8": a start-bound "from" before a clock that
            // a range end follows is the range's start.
            Role::BoundStart
                if next.is_some_and(|n| matches!(n.label, Role::Hour | Role::TimeNamed))
                    && words[position + 1..]
                        .iter()
                        .take(4)
                        .any(|&n| tokens[n].label == Role::RangeEnd) =>
            {
                repaired[index].label = Role::RangeStart;
            }
            // "every 2 hours from tomorrow": after a recurrence, "from"
            // before a date starts the series; it is not a shift.
            Role::DirAfter
                if token.raw.text.eq_ignore_ascii_case("from")
                    && words[..position]
                        .iter()
                        .any(|&p| matches!(tokens[p].label, Role::Recur | Role::Freq))
                    && next.is_some_and(|n| {
                        matches!(n.label, Role::RelDay | Role::Weekday | Role::Month)
                    }) =>
            {
                repaired[index].label = Role::BoundStart;
            }
            // "every day starting at 9am": "starting" before a clock names
            // the time of a daily-or-longer series, not a date bound.
            Role::BoundStart
                if matches!(
                    token.raw.text.to_lowercase().as_str(),
                    "starting" | "beginning"
                ) && words[..position]
                    .iter()
                    .any(|&p| matches!(tokens[p].label, Role::Recur | Role::Freq))
                    && words[position + 1..]
                        .iter()
                        .map(|&n| &tokens[n])
                        .find(|n| n.label != Role::Glue)
                        .is_some_and(|n| matches!(n.label, Role::Hour | Role::TimeNamed))
                    && !words[position + 1..].iter().any(|&n| {
                        matches!(
                            tokens[n].label,
                            Role::RelDay | Role::Weekday | Role::Month | Role::Dom
                        )
                    }) =>
            {
                repaired[index].label = Role::Glue;
            }
            // "catch up in 45 minutes": a bound marker right before "in N
            // minutes" is not a bound.
            Role::BoundEnd | Role::BoundStart
                if next.is_some_and(|n| matches!(n.label, Role::DirAfter | Role::DirBefore)) =>
            {
                repaired[index].label = Role::O;
            }
            // "हर महीने की 1 तारीख़ को": after a day of month, a day-part
            // label on a word that names no day part is the date's own marker.
            // Elsewhere an unknown day part still fails with a diagnostic.
            Role::DayPart
                if day_part(&token.raw.text.to_lowercase()).is_none()
                    && previous.is_some_and(|p| p.label == Role::Dom) =>
            {
                repaired[index].label = Role::Glue;
            }
            // "remind me in 45 min": a unit word right after "in N" is the
            // quantity's unit.
            Role::O
                if previous.is_some_and(|p| p.label == Role::Num)
                    && before_previous
                        .is_some_and(|b| matches!(b.label, Role::DirAfter | Role::DirBefore))
                    && crate::lexicon::unit(&token.raw.text.to_lowercase()).is_some() =>
            {
                repaired[index].label = Role::Unit;
            }
            // "कल सुबह 9 बजे डॉक्टर": बजे / baje closes the clock, so a word
            // after it is not part of the clock's marker.
            Role::Meridiem
                if previous.is_some_and(|p| {
                    matches!(
                        p.raw.text.to_lowercase().as_str(),
                        "बजे" | "baje" | "bje" | "बज"
                    )
                }) =>
            {
                repaired[index].label = Role::O;
            }
            // "monthly on day 25"
            Role::Hour
                if previous.is_some_and(|p| {
                    matches!(p.raw.text.to_lowercase().as_str(), "day" | "din" | "दिन")
                }) && !next.is_some_and(|n| {
                    n.raw.text == ":" || matches!(n.label, Role::Meridiem | Role::Minute)
                }) =>
            {
                repaired[index].label = Role::Dom;
            }
            // "Monday through Friday": a range of days, not an end bound.
            Role::BoundEnd
                if previous.is_some_and(|p| p.label == Role::Weekday)
                    && next.is_some_and(|n| n.label == Role::Weekday) =>
            {
                repaired[index].label = Role::RangeEnd;
            }
            Role::Hour
                if previous.is_some_and(|p| p.raw.text == ":")
                    && before_previous.is_some_and(|p| p.label == Role::Hour) =>
            {
                repaired[index].label = Role::Minute;
            }
            Role::Minute
                if previous.is_some_and(|p| {
                    matches!(p.raw.text.as_str(), "," | "&") || p.label == Role::Join
                }) =>
            {
                repaired[index].label = Role::Hour;
            }
            _ if token.raw.text.eq_ignore_ascii_case("quarter")
                && token.label != Role::Unit
                && previous.is_some_and(|p| matches!(p.label, Role::Recur | Role::Deictic))
                && !next.is_some_and(|n| {
                    matches!(
                        n.raw.text.to_lowercase().as_str(),
                        "past" | "to" | "of" | "hour" | "hours"
                    )
                }) =>
            {
                repaired[index].label = Role::Unit;
            }
            // "every hour on weekdays except 1pm": an exception marker that
            // stands apart from a later one is not the exception.
            Role::Except
                if !previous.is_some_and(|p| p.label == Role::Except)
                    && !next.is_some_and(|n| n.label == Role::Except)
                    && words[position + 1..]
                        .iter()
                        .any(|&later| tokens[later].label == Role::Except) =>
            {
                repaired[index].label = Role::Glue;
            }
            Role::O
                if token.raw.text.eq_ignore_ascii_case("everyday")
                    && (next.is_some_and(|n| n.label != Role::O)
                        || previous.is_some_and(|p| p.label != Role::O)) =>
            {
                repaired[index].label = Role::Freq;
            }
            _ => {}
        }
    }
    repaired
}

pub fn compile_predictions(
    text: &str,
    tokens: &[Token],
    date_order: crate::types::DateOrder,
) -> Vec<Expression> {
    // The skip construction spans words the model leaves unlabeled, so it
    // is rewritten before the sentence is split into expressions.
    let repaired = genitive_period_order(skip_step(repair_labels(tokens)));
    let tokens = repaired.as_slice();
    let expressions: Vec<Expression> = rejoin_clocks(split_expressions(tokens), tokens)
        .into_iter()
        .map(|expression| strip_adjective_frequency(expression, tokens))
        .filter(|expression| !expression.is_empty())
        .map(|expression| {
            let ordered = numeric_date_order(&expression, date_order);
            compile_expression(text, ordered, tokens)
        })
        .collect();
    let expressions = drop_redundant_frequencies(expressions);
    drop_subject_day_mentions(expressions, tokens)
}

/// "weekends go by too fast", "honestly Tuesdays drag on": a bare day
/// group or plural weekday that opens an ordinary sentence is a topic, not
/// a schedule. It still recurs when it is the whole input ("weekends"),
/// follows a preposition or recurrence word ("on weekends", "har
/// shanivar"), or carries a clock ("weekdays at 9").
fn drop_subject_day_mentions(expressions: Vec<Expression>, tokens: &[Token]) -> Vec<Expression> {
    let introduces = |word: &str| {
        matches!(
            word,
            "on" | "at"
                | "for"
                | "every"
                | "each"
                | "all"
                | "by"
                | "until"
                | "till"
                | "from"
                | "har"
                | "हर"
                | "ko"
                | "को"
                | "pe"
                | "par"
                | "पर"
                | "se"
                | "से"
        )
    };
    expressions
        .into_iter()
        .filter(|expression| {
            let Some(schedule) = &expression.schedule else {
                return true;
            };
            let words: Vec<&Token> = tokens.iter().filter(|token| token.raw.kind != 3).collect();
            let inside: Vec<&&Token> = words
                .iter()
                .filter(|token| {
                    token.raw.start >= expression.start && token.raw.end <= expression.end
                })
                .collect();
            let bare = !inside.is_empty()
                && inside.iter().all(|token| {
                    matches!(token.label, Role::DayGroup | Role::Join)
                        || (token.label == Role::Weekday
                            && token.raw.text.to_lowercase().ends_with('s'))
                        || matches!(token.raw.text.as_str(), "," | "&")
                })
                && schedule.clauses.iter().all(|clause| clause.time.is_none());
            if !bare {
                return true;
            }
            let before = words
                .iter()
                .rev()
                .find(|token| token.raw.end <= expression.start);
            let after = words
                .iter()
                .filter(|token| token.raw.start >= expression.end && token.label == Role::O)
                .count();
            let introduced = before.is_some_and(|token| introduces(&token.raw.text.to_lowercase()));
            // Keep it unless it opens a sentence that goes on without it.
            introduced || after < 2
        })
        .collect()
}

/// "Agle Monday subah 11 baje weekly report bhejni hai": a frequency word
/// that ends a dated one-off expression and runs into a noun ("weekly
/// report") describes the noun; the date is the schedule.
fn strip_adjective_frequency(mut expression: Vec<Token>, tokens: &[Token]) -> Vec<Token> {
    let Some(last) = expression.last() else {
        return expression;
    };
    if last.label != Role::Freq {
        return expression;
    }
    let dated = expression.iter().any(|token| {
        matches!(
            token.label,
            Role::Deictic | Role::RelDay | Role::Month | Role::Dom | Role::Year
        )
    }) && !expression.iter().any(|token| token.label == Role::Recur);
    let noun_follows = tokens
        .iter()
        .find(|token| token.raw.kind != 3 && token.raw.start >= last.raw.end)
        .is_some_and(|token| {
            token.label == Role::O && token.raw.text.chars().any(char::is_alphabetic)
        });
    if dated && noun_follows {
        expression.pop();
    }
    expression
}

/// "every night before bed at 10:30", "daily after lunch at 2": a stray
/// word splits a recurrence from its clock. A clock-only expression a few
/// words after a recurrence or day that has no clock of its own is that
/// rule's clock; a direction word left dangling before the gap ("before
/// bed") points at an event the parser cannot place, so it is dropped.
fn rejoin_clocks(expressions: Vec<Vec<Token>>, tokens: &[Token]) -> Vec<Vec<Token>> {
    let clock_only = |expression: &[Token]| {
        expression.iter().any(|token| {
            matches!(
                token.label,
                Role::Hour | Role::TimeNamed | Role::ClockOffset
            )
        }) && expression.iter().all(|token| {
            matches!(
                token.label,
                Role::Hour
                    | Role::Minute
                    | Role::Second
                    | Role::Meridiem
                    | Role::TimeNamed
                    | Role::ClockOffset
                    | Role::Glue
                    | Role::O
            )
        })
    };
    let wants_clock = |expression: &[Token]| {
        expression.iter().any(|token| {
            matches!(
                token.label,
                Role::Recur | Role::Freq | Role::Weekday | Role::DayGroup | Role::DayPart
            )
        }) && !expression.iter().any(|token| {
            matches!(
                token.label,
                Role::Hour | Role::TimeNamed | Role::ClockOffset
            )
        })
    };
    let gap_words = |left: &[Token], right: &[Token]| {
        let (Some(end), Some(start)) = (left.last(), right.first()) else {
            return usize::MAX;
        };
        tokens
            .iter()
            .filter(|token| {
                token.raw.kind != 3
                    && token.raw.start >= end.raw.end
                    && token.raw.end <= start.raw.start
            })
            .count()
    };
    let mut merged: Vec<Vec<Token>> = Vec::new();
    for expression in expressions {
        if let Some(previous) = merged.last_mut()
            && clock_only(&expression)
            && wants_clock(previous)
            && gap_words(previous, &expression) <= 3
        {
            while previous
                .last()
                .is_some_and(|token| matches!(token.label, Role::DirBefore | Role::DirAfter))
            {
                previous.pop();
            }
            previous.extend(expression);
            continue;
        }
        merged.push(expression);
    }
    merged
}

/// "send the weekly report every Friday at 4:30pm": a bare frequency word
/// ("weekly") standing alone, beside another expression that recurs at the
/// same frequency with its own days or clock, names the same rule. It is
/// dropped rather than read as a second, all-day rule. A different
/// frequency ("daily standup and weekly sync at 5pm") is kept.
fn drop_redundant_frequencies(expressions: Vec<Expression>) -> Vec<Expression> {
    let bare_frequency = |expression: &Expression| -> Option<Frequency> {
        let clauses = &expression.schedule.as_ref()?.clauses;
        let [clause] = clauses.as_slice() else {
            return None;
        };
        let rule = clause.recurrence.as_ref()?;
        let bare = clause.time.is_none()
            && clause.date.is_none()
            && clause.duration.is_none()
            && !selects_days(rule)
            && rule.interval == 1
            && rule.count.is_none()
            && rule.until.is_none()
            && rule.start.is_none()
            && rule.times_per.is_none();
        bare.then_some(rule.freq)
    };
    let specific = |expression: &Expression, freq: Frequency| {
        expression.schedule.as_ref().is_some_and(|schedule| {
            schedule.clauses.iter().any(|clause| {
                clause.recurrence.as_ref().is_some_and(|rule| {
                    rule.freq == freq && (clause.time.is_some() || selects_days(rule))
                })
            })
        })
    };
    // "pay the monthly fee on December 5 at 10am": beside a dated one-off,
    // a bare frequency word describes a noun.
    let dated_one_off = |expression: &Expression| {
        expression.schedule.as_ref().is_some_and(|schedule| {
            !schedule.clauses.is_empty()
                && schedule
                    .clauses
                    .iter()
                    .all(|clause| clause.recurrence.is_none() && clause.date.is_some())
        })
    };
    let redundant: Vec<bool> = expressions
        .iter()
        .enumerate()
        .map(|(index, expression)| {
            bare_frequency(expression).is_some_and(|freq| {
                expressions.iter().enumerate().any(|(other, candidate)| {
                    other != index && (specific(candidate, freq) || dated_one_off(candidate))
                })
            })
        })
        .collect();
    expressions
        .into_iter()
        .zip(redundant)
        .filter_map(|(expression, redundant)| (!redundant).then_some(expression))
        .collect()
}
