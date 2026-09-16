//! Splits input into tokens and emits a sparse feature row per token.
//!
//! The canonical tokenizer: the FNV-1a hash runs over UTF-16
//! code units, character classes follow Unicode lowercase mapping
//! encoding, and length buckets use UTF-16 lengths. Token offsets, however,
//! are byte offsets into the Rust string (the Rust-natural equivalent of the
//! JS UTF-16 indices).

use crate::lexicon::{decimal_digit, normalize_digits};
use crate::types::RawToken;

const PUNCTUATION_CHARACTERS: &str = ":-/.,(');&+@!?_=<>[]{}\\\"%#*~`";
const PUNCTUATION_CLASSES: [&str; 11] = [":", "-", "/", ".", ",", "(", "'", ";", "&", "+", "@"];
const LENGTH_BUCKETS: [f64; 8] = [1.0, 2.0, 3.0, 4.0, 6.0, 8.0, 12.0, f64::INFINITY];
const NUMBER_BUCKETS: [f64; 14] = [
    0.0,
    1.0,
    2.0,
    9.0,
    12.0,
    23.0,
    24.0,
    31.0,
    59.0,
    99.0,
    999.0,
    1899.0,
    2099.0,
    f64::INFINITY,
];

fn hash(text: &str) -> u32 {
    let mut value: u32 = 2166136261;
    for unit in text.encode_utf16() {
        value = (value ^ u32::from(unit)).wrapping_mul(16777619);
    }
    value
}

fn character_class(character: char) -> u32 {
    let code = lowercase_char(character);

    if (97..=122).contains(&code) {
        return code - 97;
    }
    if (48..=57).contains(&code) {
        return code - 48 + 26;
    }

    let punctuation = PUNCTUATION_CHARACTERS
        .find(character)
        .map(|index| index as u32)
        .unwrap_or(0);
    36 + (punctuation % 28)
}

/// The UTF-16 code unit of a single-char `toLowerCase`, as JS produces.
fn lowercase_char(character: char) -> u32 {
    let mut lowered = character.to_lowercase();
    let first = lowered.next().unwrap_or(character);
    first as u32
}

fn punctuation_class(text: &str) -> u32 {
    let single_punctuation = text.chars().count() == 1 && is_punctuation(text);
    if !single_punctuation {
        return 0;
    }
    let normalized = text.replace(['–', '—'], "-");
    match PUNCTUATION_CLASSES
        .iter()
        .position(|class| *class == normalized)
    {
        Some(index) => index as u32 + 1,
        None => 12,
    }
}

fn is_punctuation(text: &str) -> bool {
    let Some(character) = text.chars().next() else {
        return false;
    };
    !character.is_alphabetic() && decimal_digit(character).is_none() && !character.is_whitespace()
}

fn token_kind(text: &str) -> u8 {
    let Some(first) = text.chars().next() else {
        return 2;
    };
    if first.is_whitespace() {
        3
    } else if text.eq_ignore_ascii_case("2mrw") {
        0
    } else if decimal_digit(first).is_some() {
        1
    } else if first.is_alphabetic() || first == '_' {
        0
    } else {
        2
    }
}

fn number_bucket(text: &str, kind: u8) -> u32 {
    if kind != 1 {
        return 15;
    }
    if text.len() > 1 && text.starts_with('0') {
        return 14;
    }

    let value: f64 = text.parse().unwrap_or(f64::NAN);
    NUMBER_BUCKETS
        .iter()
        .position(|upper_bound| value <= *upper_bound)
        .unwrap_or(15) as u32
}

#[derive(Clone, Copy, PartialEq, Debug)]
struct TokenShape {
    kind: u8,
    identity: u32,
    hash: u32,
    flags: u32,
    punctuation: u32,
    ordinal: bool,
}

fn shape(word: &str) -> TokenShape {
    // Numeral scripts share numeric features, while RawToken retains the
    // original spelling and byte offsets for source-aligned predictions.
    let normalized = normalize_digits(word);
    let word = normalized.as_str();
    let folded = word.to_lowercase();
    let kind = token_kind(word);
    let utf16_length = word.encode_utf16().count() as f64;
    let length_bucket = LENGTH_BUCKETS
        .iter()
        .position(|upper_bound| utf16_length <= *upper_bound)
        .unwrap_or(7) as u32;
    let has_uppercase = word.chars().any(|c| c.is_ascii_uppercase());
    let first = word.chars().next().unwrap_or('\0');
    let last = word.chars().next_back().unwrap_or('\0');
    let is_upper_word = has_uppercase && word.chars().all(|c| !c.is_lowercase());
    let flags = (has_uppercase as u32)
        | ((has_uppercase && is_upper_word) as u32) << 1
        | (word.chars().any(|c| c.is_ascii_digit()) as u32) << 2;
    let consonant_hash = hash(&folded.replace(['a', 'e', 'i', 'o', 'u'], "")) & 127;
    let ordinal = matches!(folded.as_str(), "st" | "nd" | "rd" | "th");
    TokenShape {
        kind,
        identity: (kind as u32
            | (length_bucket << 2)
            | (character_class(first) << 5)
            | (character_class(last) << 11)
            | ((hash(&folded) & 255) << 17)
            | (number_bucket(word, kind) << 25)) as u32,
        hash: consonant_hash,
        flags,
        punctuation: punctuation_class(word),
        ordinal,
    }
}

fn is_mark(character: char) -> bool {
    // JS `\p{M}` (Mn/Mc/Me). Covers combining diacritics plus Devanagari signs
    // so हफ़्ते stays one token.
    let code = character as u32;
    (0x0300..=0x036F).contains(&code)
        || (0x1AB0..=0x1AFF).contains(&code)
        || (0x1DC0..=0x1DFF).contains(&code)
        || (0x20D0..=0x20FF).contains(&code)
        || (0xFE20..=0xFE2F).contains(&code)
        || (0x0900..=0x0903).contains(&code)
        || (0x093A..=0x094F).contains(&code)
        || (0x0951..=0x0957).contains(&code)
        || (0x0962..=0x0963).contains(&code)
}

fn is_letter_run(character: char) -> bool {
    character.is_alphabetic() || character == '_' || is_mark(character)
}

/// Equivalent of `/(?:[\p{L}_]\p{M}*)+(?:['’](?:[\p{L}_]\p{M}*)+)*|\d+|\s+|[^\s]/gu`.
fn scan_tokens(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut index = 0;
    while index < text.len() {
        let start = index;
        let first = text[index..].chars().next().unwrap();
        if text[index..]
            .get(..4)
            .is_some_and(|word| word.eq_ignore_ascii_case("2mrw"))
            && text[index + 4..]
                .chars()
                .next()
                .is_none_or(|c| !c.is_alphanumeric() && c != '_')
        {
            index += 4;
        } else if first.is_alphabetic() || first == '_' {
            let mut end = index + first.len_utf8();
            loop {
                let rest = &text[end..];
                let mut apostrophe = None;
                if rest.starts_with('\'') {
                    apostrophe = Some('\'');
                } else if rest.starts_with('’') {
                    apostrophe = Some('’');
                }
                if let Some(apostrophe) = apostrophe {
                    let after = &text[end + apostrophe.len_utf8()..];
                    if after
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_alphabetic() || c == '_')
                    {
                        let letters = after
                            .char_indices()
                            .take_while(|(_, c)| is_letter_run(*c))
                            .last()
                            .map(|(offset, c)| offset + c.len_utf8())
                            .unwrap_or(0);
                        end += apostrophe.len_utf8() + letters;
                        continue;
                    }
                }
                let rest = &text[end..];
                match rest.chars().next() {
                    Some(c) if is_letter_run(c) => {
                        end += c.len_utf8();
                    }
                    _ => break,
                }
            }
            index = end;
        } else if decimal_digit(first).is_some() {
            index += first.len_utf8();
            while let Some(c) = text[index..]
                .chars()
                .next()
                .filter(|c| decimal_digit(*c).is_some())
            {
                index += c.len_utf8();
            }
        } else if first.is_whitespace() {
            index += first.len_utf8();
            while let Some(c) = text[index..].chars().next() {
                if c.is_whitespace() {
                    index += c.len_utf8();
                } else {
                    break;
                }
            }
        } else {
            index += first.len_utf8();
        }
        parts.push(&text[start..index]);
    }
    parts
}

pub fn tokenize(text: &str) -> Vec<RawToken> {
    let parts = scan_tokens(text);
    let facts: Vec<TokenShape> = parts.iter().map(|part| shape(part)).collect();
    parts
        .iter()
        .enumerate()
        .map(|(index, part)| {
            let current = facts[index];
            let previous = if index > 0 {
                Some(facts[index - 1])
            } else {
                None
            };
            let next = facts.get(index + 1).copied();
            let mut flags = current.flags;
            if index == 0 {
                flags |= 1 << 3;
            }
            if index == parts.len() - 1 {
                flags |= 1 << 4;
            }
            if previous.is_some_and(|shape| shape.kind == 3) {
                flags |= 1 << 5;
            }
            if next.is_some_and(|shape| shape.kind == 3) {
                flags |= 1 << 6;
            }
            if current.ordinal && previous.is_some_and(|shape| shape.kind == 1) {
                flags |= 1 << 7;
            }
            let context = (current.hash | (flags << 7))
                | ((previous.map(|shape| shape.punctuation).unwrap_or(0)) << 15)
                | ((next.map(|shape| shape.punctuation).unwrap_or(0)) << 19);
            RawToken {
                start: part.as_ptr() as usize - text.as_ptr() as usize,
                end: part.as_ptr() as usize + part.len() - text.as_ptr() as usize,
                text: part.to_string(),
                kind: current.kind,
                features: (current.identity, context),
            }
        })
        .collect()
}

pub fn feature_rows(features: (u32, u32)) -> Vec<u16> {
    let (identity, context) = features;
    // Each feature owns a disjoint region of the 580-row embedding table.
    let mut rows = vec![
        (identity & 3) as u16,                   // Kind
        (4 + ((identity >> 2) & 7)) as u16,      // Length
        (12 + ((identity >> 5) & 63)) as u16,    // First character
        (76 + ((identity >> 11) & 63)) as u16,   // Last character
        (140 + ((identity >> 17) & 255)) as u16, // Word hash
        (396 + (context & 127)) as u16,          // Consonant hash
        (532 + ((context >> 15) & 15)) as u16,   // Previous punctuation
        (548 + ((context >> 19) & 15)) as u16,   // Next punctuation
        (564 + ((identity >> 25) & 15)) as u16,  // Number bucket
    ];

    let flags = (context >> 7) & 255;
    for bit in 0..8 {
        if flags & (1 << bit) != 0 {
            rows.push((524 + bit) as u16);
        }
    }

    rows
}
