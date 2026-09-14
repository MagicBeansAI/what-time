"""Extra supervision families for the transformer proof of concept.

These cover phrasings the promoted carrier grammar never emits, so the
transformer learns them natively instead of relying on compiler rescues:

- article-less "day after tomorrow" / "day before yesterday" (labeled the
  same all-REL_DAY way as the trained "the day after tomorrow"),
- deictic periods as qualifiers: "24th august last year", "15th last month",
  "friday next week" — in both word orders, with optional clocks.

Supervision comes only from this generator's structure, never from the
runtime parser. RESERVED carrier phrases from natural.py are not used.
"""
from __future__ import annotations

import random

import background
from generate import DAYS, MONTHS, Sentence

DEICTICS = ["last", "next", "this"]
PERIODS = {
    "year": "year",
    "month": "month",
    "week": "week",
}


def ordinal_day(rng: random.Random) -> int:
    return rng.randint(1, 28)


def add_ordinal_day(sentence: Sentence, day: int) -> None:
    # Match the standard corpus convention: digits carry DOM, the fused
    # suffix is a separate GLUE span ("24" + "th").
    suffix = (
        "th"
        if day % 100 in (11, 12, 13)
        else {1: "st", 2: "nd", 3: "rd"}.get(day % 10, "th")
    )
    sentence.add(str(day), "DOM")
    sentence.add(suffix, "GLUE", separator="")


def clock(sentence: Sentence, rng: random.Random) -> None:
    hour = rng.randint(1, 12)
    sentence.add("at", "GLUE")
    sentence.add(str(hour), "HOUR")
    if rng.random() < 0.4:
        sentence.add(":", "GLUE", separator="")
        sentence.add(f"{rng.randint(0, 59):02d}", "HOUR")
    sentence.add(rng.choice(["am", "pm"]), "MERIDIEM")


def bare_relative_day(sentence: Sentence, rng: random.Random) -> None:
    after = rng.random() < 0.6
    anchor = "tomorrow" if after else "yesterday"
    direction = "after" if after else "before"
    for word, label in (
        ("day", "REL_DAY"),
        (direction, "REL_DAY"),
        (anchor, "REL_DAY"),
    ):
        sentence.add(word, label)


def period_qualified(sentence: Sentence, rng: random.Random) -> None:
    period = rng.choice(list(PERIODS))
    deictic = rng.choice(DEICTICS)
    leading = rng.random() < 0.5
    if leading:
        sentence.add(deictic, "DEICTIC")
        sentence.add(period, "UNIT")
    if period == "week":
        sentence.add(rng.choice(DAYS), "WEEKDAY")
    else:
        day = ordinal_day(rng)
        if period == "year" or rng.random() < 0.5:
            sentence.add(rng.choice(MONTHS), "MONTH")
        add_ordinal_day(sentence, day)
    if not leading:
        sentence.add(deictic, "DEICTIC")
        sentence.add(period, "UNIT")


def standalone(sentence: Sentence, rng: random.Random) -> None:
    # Single-token expressions the undertrained PoC mislabeled on the gold
    # corpora ("now" as HOLIDAY, "tonight" as O).
    word = rng.choice(["now", "right now", "tonight", "today", "immediately"])
    for part, label in [(piece, "NOW" if word in ("now", "right now", "immediately") else "REL_DAY") for piece in word.split()]:
        sentence.add(part, label)


# Carrier verbs + callee names. Short m-words ("mom", "mum") are absent
# from background prose and, out of vocabulary, their character-hash
# neighborhood lands on day-part/unit vocabulary — so the corpus must teach
# them explicitly as O tokens adjacent to expressions.
# "roz" is Hinglish "daily": teach it as RECUR whether alone or after "har".
ROZ_CLOCKS = ["10 baje", "9 baje", "8 pm", "9 am", "shaam ko 8 baje", "raat 10 baje"]


def roz_family(sentence: Sentence, rng: random.Random) -> None:
    form = rng.choice(["roz", "har roz", "roz", "हर रोज़", "रोज़"])
    for piece in form.split():
        sentence.add(piece, "RECUR")
    clock = rng.choice(ROZ_CLOCKS)
    for piece, label in [
        (word, "GLUE" if word in ("ko",) else "HOUR" if word.isdigit() else "MERIDIEM" if word in ("pm", "am", "baje") else "DAYPART")
        for word in clock.split()
    ]:
        sentence.add(piece, label)


# Bare-year false-positive guards: the negatives corpus tracks these. A
# lone year in a technical sentence must stay O so it never extracts.
YEAR_NEGATIVES = [
    'build 2026 failed with 3 warnings',
    'version 2026 is ready',
    'release 2026 shipped on time',
    'error 2026 in module 7',
]


def year_negative(sentence: Sentence, rng: random.Random) -> None:
    sentence.add(rng.choice(YEAR_NEGATIVES), 'O')


CARRIERS = [
    ("call", "mom"), ("call", "dad"), ("call", "mum"), ("call", "mother"),
    ("call", "papa"), ("call", "zoya"), ("call", "rohan"), ("call", "priya"),
    ("call", "sam"), ("call", "alice"), ("call", "boss"), ("call", "team"),
    ("ping", "mom"), ("ping", "dad"), ("text", "priya"), ("text", "mom"),
    ("remind", "mom"), ("remind", "sam"), ("book", "alice"), ("book", "rohan"),
]


# Post-clock durations with an explicit introducer: the model must keep the
# quantity NUM and the unit UNIT even when "for" precedes them.
DURATION_UNITS = [
    ("minutes", "mins"), ("hours", "hrs"), ("days", None),
    ("weeks", None), ("months", None),
]


def explicit_duration(sentence: Sentence, rng: random.Random) -> None:
    introducer = rng.choice(["for ", "", "for "])
    amount = rng.choice([15, 20, 30, 45, 60, 90, 2, 3])
    name, short = rng.choice(DURATION_UNITS)
    unit_word = short if short and rng.random() < 0.5 else name
    if introducer:
        sentence.add(introducer.strip(), "GLUE")
    sentence.add(str(amount), "NUM")
    sentence.add(unit_word, "UNIT")


# A bare day/week before a weekday list is a selector.
SELECTOR_DAYS = [
    ("Monday", "somvaar"), ("Wednesday", "budhvaar"), ("Friday", "shanivaar"),
    ("Tuesday", "mangalvaar"), ("Thursday", "guruvaar"), ("Saturday", "shanivar"),
    ("Sunday", "ravivaar"),
]


def bare_day_selector(sentence: Sentence, rng: random.Random) -> None:
    unit_word = rng.choice(["day", "week"])
    sentence.add(unit_word, "UNIT")
    count = rng.choice([1, 2, 3])
    picks = rng.sample(SELECTOR_DAYS, count)
    for index, (english, _) in enumerate(picks):
        if index:
            if rng.random() < 0.5:
                sentence.add("and", "JOIN")
            else:
                sentence.add(",", "GLUE", separator=" ")
        sentence.add(english, "WEEKDAY")


def render(rng: random.Random) -> Sentence:
    sentence = Sentence(rng)
    if rng.random() < 0.3:
        verb, name = rng.choice(CARRIERS)
        sentence.add(verb, "O")
        sentence.add(name, "O")
    elif rng.random() < 0.45:
        sentence.add(background.prefix(rng))
    sentence.clause()
    kind = rng.random()
    if kind < 0.25:
        standalone(sentence, rng)
    elif kind < 0.5:
        bare_relative_day(sentence, rng)
    else:
        period_qualified(sentence, rng)
    if rng.random() < 0.08:
        year_negative(sentence, rng)
        return sentence
    if rng.random() < 0.12:
        roz_family(sentence, rng)
    elif rng.random() < 0.15:
        explicit_duration(sentence, rng)
    elif rng.random() < 0.12:
        bare_day_selector(sentence, rng)
    if rng.random() < 0.35:
        clock(sentence, rng)
    if rng.random() < 0.3:
        sentence.add(background.suffix(rng))
    return sentence
