//! Reads numeric quantities and durations selected by the model.

use crate::labels::Role;
use crate::lexicon::{number, unit};
use crate::types::{Duration, Quantity, Token, Unit};

/// `a`/`an` or a bare number word that the model selected as a quantity.
fn is_a_or_an(text: &str) -> bool {
    text == "a" || text == "an"
}

/// Read the numeric pieces selected by the model, preserving their source tokens.
/// Returns `(value, next)`; `value` is `None` where the reference yields NaN.
pub fn read_number(tokens: &[Token], index: usize, label: Role) -> (Option<f64>, usize) {
    let mut index = index;
    // "a few" and "a couple" carry the article as its own number token.
    if is_a_or_an(&tokens[index].raw.text)
        && tokens
            .get(index + 1)
            .is_some_and(|token| token.label == label)
        && tokens
            .get(index + 1)
            .and_then(|token| number(&token.raw.text.to_lowercase()))
            .is_some_and(|value| value.is_finite())
    {
        index += 1;
    }
    let mut value = tokens
        .get(index)
        .and_then(|token| number(&token.raw.text.to_lowercase()));
    let mut next = index + 1;
    if tokens.get(next).map(|t| t.raw.text.as_str()) == Some(".")
        && tokens
            .get(next + 1)
            .is_some_and(|token| token.label == label)
    {
        let fraction = &tokens[next + 1].raw.text;
        value = match value {
            Some(value) => format!("{}.{}", format_number(value), fraction)
                .parse::<f64>()
                .ok(),
            None => None,
        };
        next += 2;
    } else {
        if tokens.get(next).map(|t| t.raw.text.as_str()) == Some("-")
            && tokens
                .get(next + 1)
                .is_some_and(|token| token.label == label)
        {
            next += 1;
        }
        if tokens.get(next).map(|t| t.raw.text.as_str()) == Some("of")
            && tokens.get(next).is_some_and(|token| token.label == label)
        {
            next += 1;
        }
        let suffix = tokens
            .get(next)
            .filter(|token| token.label == label)
            .and_then(|token| number(&token.raw.text.to_lowercase()));
        if let Some(value_now) = value
            && let Some(suffix) = suffix
            && value_now >= 20.0
            && value_now % 10.0 == 0.0
            && suffix > 0.0
            && suffix < 10.0
        {
            value = Some(value_now + suffix);
            next += 1;
        }
    }
    (value, next)
}

fn format_number(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

pub fn read_duration(tokens: &[Token], index: usize) -> Option<(Duration, usize)> {
    let mut components: Vec<Quantity> = Vec::new();
    let mut next = index;
    while tokens
        .get(next)
        .is_some_and(|token| matches!(token.label, Role::Num | Role::Dur))
    {
        let (quantity_value, quantity_next) = read_number(tokens, next, tokens[next].label);
        next = quantity_next;
        // "half an hour": the article belongs to the same quantity.
        if quantity_value.is_some_and(|value| value < 1.0)
            && tokens
                .get(next)
                .is_some_and(|token| is_a_or_an(&token.raw.text))
        {
            next += 1;
        }
        let duration_unit = tokens
            .get(next)
            .filter(|token| token.label == Role::Unit)
            .and_then(|token| unit(&token.raw.text.to_lowercase()));
        let duration_unit = duration_unit?;
        let value = quantity_value?;
        if !value.is_finite() || value <= 0.0 {
            return None;
        }
        let mut amount = value;
        next += 1;
        if tokens
            .get(next)
            .is_some_and(|token| token.raw.text.to_lowercase() == "and")
        {
            let mut tail = next + 1;
            if tokens
                .get(tail)
                .is_some_and(|token| is_a_or_an(&token.raw.text))
            {
                tail += 1;
            }
            if tokens.get(tail).is_some_and(|token| {
                matches!(token.label, Role::Num | Role::Dur)
                    && token.raw.text.to_lowercase() == "half"
            }) {
                amount += 0.5;
                next = tail + 1;
            }
        }
        // Fractions of calendar months/days need a separate policy. Clock units are exact.
        if amount.fract() != 0.0 && !matches!(duration_unit, Unit::hour | Unit::minute) {
            return None;
        }
        components.push(Quantity {
            amount,
            unit: duration_unit,
        });
        let candidate = if tokens
            .get(next)
            .is_some_and(|token| token.raw.text.to_lowercase() == "and")
        {
            next + 1
        } else {
            next
        };
        if !tokens
            .get(candidate)
            .is_some_and(|token| matches!(token.label, Role::Num | Role::Dur))
        {
            break;
        }
        let following = read_number(tokens, candidate, tokens[candidate].label).1;
        if !tokens
            .get(following)
            .is_some_and(|token| token.label == Role::Unit)
        {
            break;
        }
        next = candidate;
    }
    let first = *components.first()?;
    let rest: Vec<Quantity> = components[1..].to_vec();
    Some((
        Duration {
            components: if rest.is_empty() { None } else { Some(rest) },
            amount: first.amount,
            unit: first.unit,
        },
        next,
    ))
}
