//! English lexicon: weekday/month names, spoken numbers, units, and holidays.

use crate::types::{Unit, WEEKDAYS, Weekday};

pub const DAY_NAMES: [&str; 7] = [
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
];

pub const MONTH_NAMES: [&str; 12] = [
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];

/// Spoken numbers, ordinals, and indefinite quantities.
pub fn spoken_quantity(word: &str) -> Option<f64> {
    let value = match word {
        "zero" => 0.0,
        "a" => 1.0,
        "an" => 1.0,
        "one" => 1.0,
        "two" => 2.0,
        "three" => 3.0,
        "four" => 4.0,
        "five" => 5.0,
        "six" => 6.0,
        "seven" => 7.0,
        "eight" => 8.0,
        "nine" => 9.0,
        "ten" => 10.0,
        "eleven" => 11.0,
        "twelve" => 12.0,
        "thirteen" => 13.0,
        "fourteen" => 14.0,
        "fifteen" => 15.0,
        "sixteen" => 16.0,
        "seventeen" => 17.0,
        "eighteen" => 18.0,
        "nineteen" => 19.0,
        "twenty" => 20.0,
        "thirty" => 30.0,
        "forty" => 40.0,
        "fifty" => 50.0,
        "half" => 0.5,
        "quarter" => 0.25,
        "couple" => 2.0,
        "few" => 3.0,
        "several" => 3.0,
        "other" => 2.0,
        "once" => 1.0,
        "twice" => 2.0,
        "thrice" => 3.0,
        "first" => 1.0,
        "second" => 2.0,
        "third" => 3.0,
        "fourth" => 4.0,
        "fifth" => 5.0,
        "sixth" => 6.0,
        "seventh" => 7.0,
        "eighth" => 8.0,
        "ninth" => 9.0,
        "tenth" => 10.0,
        "eleventh" => 11.0,
        "twelfth" => 12.0,
        "thirteenth" => 13.0,
        "fourteenth" => 14.0,
        "fifteenth" => 15.0,
        "sixteenth" => 16.0,
        "seventeenth" => 17.0,
        "eighteenth" => 18.0,
        "nineteenth" => 19.0,
        "twentieth" => 20.0,
        "thirtieth" => 30.0,
        "last" => -1.0,
        "एक" | "ek" | "pehla" | "पहला" => 1.0,
        "दो" | "do" | "dusra" | "दूसरा" => 2.0,
        // Ordinals inflect for gender/number/case in Hindi ("दूसरे सोमवार",
        // "तीसरी बैठक"); the model's ORD label picks them out, so every
        // inflected form of first–fifth maps to its number.
        "पहले" | "पहली" | "pehle" | "pehli" => 1.0,
        "दूसरे" | "दूसरी" | "doosra" | "doosre" | "doosri" | "dusre" | "dusri" => {
            2.0
        }
        "तीसरा" | "तीसरे" | "तीसरी" | "teesra" | "teesre" | "teesri" => {
            3.0
        }
        "चौथा" | "चौथे" | "चौथी" | "chautha" | "chauthe" | "chauthi" => 4.0,
        "पाँचवा" | "पाँचवे" | "पाँचवी" | "पांचवा" | "पांचवे" | "पांचवी" | "paanchwa" | "paanchwe"
        | "paanchvi" => 5.0,
        "आखिरी" | "आख़िरी" | "aakhri" | "aakhiri" => -1.0,
        "तीन" | "teen" => 3.0,
        "चार" | "char" => 4.0,
        "पाँच" | "पांच" | "paanch" => 5.0,
        "छह" | "chhe" => 6.0,
        "सात" | "saat" => 7.0,
        "आठ" | "aath" => 8.0,
        "नौ" | "nau" => 9.0,
        "दस" | "das" => 10.0,
        "ग्यारह" | "gyarah" => 11.0,
        "बारह" | "barah" => 12.0,
        _ => return None,
    };
    Some(value)
}

/// "twenty-first" reaches the compiler as three tokens, so the tens word and the
/// ones ordinal are combined rather than listed as thirty more entries.
fn tens_word(word: &str) -> Option<f64> {
    match word {
        "twenty" => Some(20.0),
        "thirty" => Some(30.0),
        _ => None,
    }
}

pub fn compound_ordinal(tens: &str, ones: &str) -> Option<f64> {
    let base = tens_word(tens)?;
    let unit = spoken_quantity(ones)?;
    if !(1.0..=9.0).contains(&unit) {
        return None;
    }
    Some(base + unit)
}

pub fn unit_from_abbreviation(word: &str) -> Option<Unit> {
    match word {
        "min" | "mins" | "m" => Some(Unit::minute),
        "hr" | "hrs" | "h" => Some(Unit::hour),
        "wk" | "wks" | "week" | "weeks" => Some(Unit::week),
        "d" => Some(Unit::day),
        "mo" => Some(Unit::month),
        "yr" | "yrs" => Some(Unit::year),
        _ => None,
    }
}

/// Parses a spoken or numeric quantity, failing like `parseFloat`:
/// `None`.
pub fn number(text: &str) -> Option<f64> {
    if let Some(value) = spoken_quantity(text) {
        return Some(value);
    }
    parse_integer(text)
}

fn parse_integer(text: &str) -> Option<f64> {
    let digits = text.strip_prefix('-').unwrap_or(text);
    if digits.is_empty() || !digits.chars().all(|c| decimal_digit(c).is_some()) {
        return None;
    }
    normalize_digits(text).parse::<f64>().ok()
}

pub(crate) fn decimal_digit(character: char) -> Option<u32> {
    match character {
        '0'..='9' => Some(character as u32 - '0' as u32),
        '०'..='९' => Some(character as u32 - '०' as u32),
        _ => None,
    }
}

pub(crate) fn normalize_digits(text: &str) -> String {
    text.chars()
        .map(|c| {
            decimal_digit(c)
                .and_then(|d| char::from_digit(d, 10))
                .unwrap_or(c)
        })
        .collect()
}

pub fn weekday(text: &str) -> Option<Weekday> {
    let lowered = text.to_lowercase();
    if let Some(day) = extra_weekday(text).or_else(|| extra_weekday(&lowered)) {
        return Some(day);
    }
    let trimmed = lowered.strip_suffix('.').unwrap_or(&lowered);
    let word = trimmed.strip_suffix('s').unwrap_or(trimmed);
    DAY_NAMES
        .iter()
        .position(|name| {
            if *name == word || name.len() >= 3 && &name[..3] == word {
                return true;
            }
            *name == "thursday" && (word == "thur" || word == "thurs")
        })
        .map(|index| WEEKDAYS[index])
}

fn extra_weekday(text: &str) -> Option<Weekday> {
    match text {
        "सोमवार" | "somvaar" | "somwar" => Some(WEEKDAYS[0]),
        "मंगलवार" | "mangal" | "mangalvaar" => Some(WEEKDAYS[1]),
        "बुधवार" | "budh" | "budhvaar" => Some(WEEKDAYS[2]),
        "गुरुवार" | "guruvaar" | "guruwar" => Some(WEEKDAYS[3]),
        "शुक्रवार" | "shukravaar" | "shukrawar" => Some(WEEKDAYS[4]),
        "शनिवार" | "shanivaar" | "shanivar" => Some(WEEKDAYS[5]),
        "रविवार" | "ravivaar" | "raviwar" => Some(WEEKDAYS[6]),
        _ => None,
    }
}

pub fn month(text: &str) -> Option<i64> {
    if let Some(value) = extra_month(text) {
        return Some(value);
    }
    let lowered = text.to_lowercase();
    let word = lowered.strip_suffix('.').unwrap_or(&lowered);
    MONTH_NAMES
        .iter()
        .position(|name| {
            if *name == word || name.len() >= 3 && &name[..3] == word {
                return true;
            }
            *name == "september" && word == "sept"
        })
        .map(|index| index as i64 + 1)
}

fn extra_month(text: &str) -> Option<i64> {
    match text {
        "जनवरी" => Some(1),
        "फरवरी" => Some(2),
        "मार्च" => Some(3),
        "अप्रैल" => Some(4),
        "मई" => Some(5),
        "जून" => Some(6),
        "जुलाई" => Some(7),
        "अगस्त" => Some(8),
        "सितंबर" | "सितम्बर" => Some(9),
        "अक्टूबर" => Some(10),
        "नवंबर" | "नवम्बर" => Some(11),
        "दिसंबर" | "दिसम्बर" => Some(12),
        _ => None,
    }
}

pub fn unit(text: &str) -> Option<Unit> {
    if let Some(value) = unit_from_abbreviation(text) {
        return Some(value);
    }
    match text {
        "दिन" | "दिनों" | "din" => return Some(Unit::day),
        "हफ़्ते" | "हफ्ते" | "हफ़्ता" | "हफ्ता" | "सप्ताह" | "hafte" | "hafta" =>
        {
            return Some(Unit::week);
        }
        "महीने" | "महीना" | "mahine" | "mahina" => return Some(Unit::month),
        "साल" | "वर्ष" | "saal" => return Some(Unit::year),
        "तिमाही" => return Some(Unit::quarter),
        "मिनट" => return Some(Unit::minute),
        "घंटे" | "घंटा" | "ghante" | "ghanta" => return Some(Unit::hour),
        _ => {}
    }
    let singular = text.strip_suffix('s').unwrap_or(text);
    match singular {
        "minute" => Some(Unit::minute),
        "hour" => Some(Unit::hour),
        "day" => Some(Unit::day),
        "week" => Some(Unit::week),
        "month" => Some(Unit::month),
        "quarter" => Some(Unit::quarter),
        "year" => Some(Unit::year),
        _ => None,
    }
}

/// Holiday vocabulary. Returns the key into the bundled holiday asset
/// (`rust/what-time/assets/holidays.json`); dates live in that data file,
/// never in code, so new holidays are data additions.
pub fn holiday_name(key: &str) -> Option<String> {
    let key = match key {
        "christmas" | "क्रिसमस" => "christmas",
        "christmaseve" => "christmas-eve",
        "newyear" | "newyearsday" | "नयासाल" => "new-year",
        "newyearseve" => "new-years-eve",
        "halloween" => "halloween",
        "valentinesday" | "valentines" => "valentines",
        "thanksgiving" => "thanksgiving",
        "independenceday" | "julyth" => "independence-day",
        "juneteenth" => "juneteenth",
        "memorialday" => "memorial-day",
        "laborday" => "labor-day",
        "mlkday" | "martinlutherkingjrday" => "mlk-day",
        "presidentsday" => "presidents-day",
        "mothersday" => "mothers-day",
        "fathersday" => "fathers-day",
        "boxingday" => "boxing-day",
        "goodfriday" => "good-friday",
        "easter" => "easter",
        "eastermonday" => "easter-monday",
        "दिवाली" | "दीपावली" | "diwali" | "deepavali" => "diwali",
        _ => return None,
    };
    Some(key.to_string())
}
