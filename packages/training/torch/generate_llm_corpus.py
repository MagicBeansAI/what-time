"""Generate the labeled JSONL described in LLM_CORPUS_PROMPT.md.

Supervision is built from these templates, never from the runtime parser.
Output shape: {"text": ..., "tokens": [[token, label], ...]} one object per line.
"""

from __future__ import annotations

import argparse
import json
import random
from pathlib import Path

from validate_llm_corpus import split_tokens

ROOT = Path(__file__).resolve().parent.parent
OUT_DEFAULT = ROOT / "data" / "llm" / "raw.jsonl"

LABELS = {
    "O", "NUM", "ORD", "UNIT", "DIR_BEFORE", "DIR_AFTER", "NOW", "REL_DAY",
    "DEICTIC", "WEEKDAY", "DAYGROUP", "MONTH", "DOM", "YEAR", "HOUR",
    "MINUTE", "SECOND", "MERIDIEM", "TIME_NAMED", "DAYPART", "RANGE_START",
    "RANGE_END", "RECUR", "FREQ", "TIMES", "BOUND_START", "BOUND_END",
    "COUNT", "DUR", "EXCEPT", "HOLIDAY", "JOIN", "GLUE", "EDGE",
    "CLOCK_OFFSET",
}
SUFFIXES = {"st", "nd", "rd", "th"}
RESERVED = [
    "could you arrange a reminder for",
    "our rehearsal begins at",
    "the train leaves at",
    "please put this in my diary for",
    "allow extra time",
    "the workshop continues",
]

EN_QUOTAS = {
    "rel_day": 2000,
    "calendar": 4000,
    "weekday": 1500,
    "weekday_list": 1500,
    "clocks": 2000,
    "deictic": 2000,
    "qualified": 2500,
    "ranges": 2000,
    "recur": 3000,
    "ordinal": 1500,
    "duration": 2000,
    "holiday": 1000,
    "multi": 2000,
    "prose": 2000,
    "negative": 1000,
}

def is_punct(token: str) -> bool:
    return len(token) == 1 and not token.isalnum() and token != "_"


def fuse_text(tokens: list[str]) -> str:
    parts: list[str] = []
    for i, tok in enumerate(tokens):
        if i == 0:
            parts.append(tok)
            continue
        prev = tokens[i - 1]
        attach_punct = is_punct(tok)
        clock_colon = tok == ":" and prev[-1].isdigit()
        after_colon = prev == ":" and tok[0].isdigit()
        hyphen = tok in "-–" or (prev in "-–" and tok[:1].isalnum())
        suffix = prev.isdigit() and tok.lower() in SUFFIXES
        meridiem = prev.isdigit() and tok.lower() in {"am", "pm"}
        fuse = attach_punct or clock_colon or after_colon or hyphen or suffix or meridiem
        if is_punct(prev) and tok not in "-–" and prev not in ":/-–":
            fuse = False
        parts.append(tok if fuse else " " + tok)
    return "".join(parts)


def labeled(text: str, label: str) -> list[tuple[str, str]]:
    return [(tok, label) for tok in split_tokens(text)]


def o_text(text: str) -> list[tuple[str, str]]:
    return labeled(text, "O")


def emit(tokens: list[tuple[str, str]]) -> dict:
    words = [t for t, _ in tokens]
    text = fuse_text(words)
    if split_tokens(text) != words:
        text = " ".join(words)
    if split_tokens(text) != words:
        raise AssertionError(f"tokenize mismatch: {text!r} vs {words!r}")
    if any(label not in LABELS for _, label in tokens):
        unknown = {l for _, l in tokens if l not in LABELS}
        raise AssertionError(f"unknown labels {unknown} in {text!r}")
    return {"text": text, "tokens": [[t, l] for t, l in tokens]}


def maybe_lower(rng: random.Random, tokens: list[tuple[str, str]], p: float) -> list[tuple[str, str]]:
    if rng.random() >= p:
        return tokens
    out = []
    for tok, label in tokens:
        if tok.isascii() and any(c.isalpha() for c in tok):
            out.append((tok.lower(), label))
        else:
            out.append((tok, label))
    return out


def ordinal_suffix(n: int) -> str:
    if n % 100 in (11, 12, 13):
        return "th"
    return {1: "st", 2: "nd", 3: "rd"}.get(n % 10, "th")


# ---------------------------------------------------------------------------
# English lexicons
# ---------------------------------------------------------------------------

DAYS = [
    "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday",
]
DAY_FORMS = []
for name in DAYS:
    DAY_FORMS.extend([name, name.lower(), name[:3], name[:3].lower()])
DAY_FORMS.extend(["Tues", "Thur", "Thurs", "tues", "thur", "thurs", "Mon", "Fri", "Wed"])

MONTHS = [
    "January", "February", "March", "April", "May", "June",
    "July", "August", "September", "October", "November", "December",
]
DEICTICS = ["this", "next", "last", "coming", "previous", "upcoming", "past"]
PERIODS = ["day", "week", "month", "year"]
PERIODS_PL = ["days", "weeks", "months", "years"]
CLOCK_LABELS = {
    "HOUR", "MINUTE", "SECOND", "MERIDIEM", "TIME_NAMED", "DAYPART", "CLOCK_OFFSET",
}
HOUR_WORDS = "one two three four five six seven eight nine ten eleven twelve".split()
ORD_WORDS = ["first", "second", "third", "fourth", "fifth", "last"]
DAYPARTS = ["morning", "afternoon", "evening", "night"]
NAMED_TIMES = ["noon", "midnight", "midday"]
REL_PHRASES = [
    "today", "tomorrow", "yesterday", "tonight",
    "the day after tomorrow", "day after tomorrow",
    "the day before yesterday", "day before yesterday",
]
HOLIDAYS = [
    "Christmas", "Christmas Eve", "New Year's Day", "New Year's Eve",
    "Halloween", "Valentine's Day",
]
NAMES = [
    "Alex", "Jordan", "Riley", "Sam", "Taylor", "Casey", "Priya", "Rohan",
    "Leah", "Ben", "Chris", "Pat", "Quinn", "Skye", "Nina", "Omar", "Farah",
    "Dev", "Anika", "Kabir", "Meera", "Aarav", "Neha", "Amit",
]
EVENTS = [
    "standup", "demo", "kickoff", "sync", "dentist", "haircut", "flight",
    "pizza", "groceries", "interview", "offsite", "lecture", "seminar",
    "brunch", "lunch", "dinner", "coffee", "hike", "class", "exam",
    "recital", "game", "recording", "all-hands", "retro", "planning",
    "review", "checkup", "pickup", "dropoff", "rehearsal", "workshop",
    "sprint", "launch", "podcast", "shoot", "match", "picnic", "mixer",
    "call", "meeting", "appointment", "lesson", "session", "briefing",
]
LEADS = [
    "hey", "hi", "please", "quick one", "fyi", "heads up", "note",
    "can you", "could you", "let's", "pls", "btw",
]
VERBS = [
    "book", "schedule", "move", "push", "set", "lock", "hold", "block",
    "cancel", "shift", "nudge", "confirm", "skip", "keep",
]
SUFFIXES_EN = [
    "if that works", "please", "for the team", "works for me", "let's lock it",
    "nothing urgent", "need a room", "keep it short", "I'll send a note",
    "looping in Sam", "no rush", "thanks", "that would help", "just a reminder",
    "put it on the calendar", "I'll join", "can you cover", "I'll be there",
    "tell the group", "should be quick",
]
GLUE_PREP = ["at", "on", "for", "in", "by"]
STRUCT_START = {
    "from", "between", "every", "each", "starting", "ending", "for", "in",
    "on", "at", "the", "by", "to", "until", "till", "through", "except",
}


# ---------------------------------------------------------------------------
# English builders
# ---------------------------------------------------------------------------


def pick_weekday(rng: random.Random) -> list[tuple[str, str]]:
    word = rng.choice(DAY_FORMS)
    if word.endswith("."):
        return [(word[:-1], "WEEKDAY"), (".", "GLUE")]
    return [(word, "WEEKDAY")]


def pick_month(rng: random.Random) -> list[tuple[str, str]]:
    name = rng.choice(MONTHS)
    if rng.random() < 0.4:
        word = "Sept" if name == "September" and rng.random() < 0.35 else name[:3]
        if rng.random() < 0.25:
            word = word.lower()
        if rng.random() < 0.18:
            return [(word, "MONTH"), (".", "GLUE")]
        return [(word, "MONTH")]
    if rng.random() < 0.3:
        name = name.lower()
    return [(name, "MONTH")]


def pick_dom(rng: random.Random, n: int | None = None) -> list[tuple[str, str]]:
    n = n if n is not None else rng.randint(1, 28)
    tokens = [(str(n), "DOM")]
    if rng.random() < 0.6:
        tokens.append((ordinal_suffix(n), "GLUE"))
    return tokens


def pick_year(rng: random.Random) -> list[tuple[str, str]]:
    if rng.random() < 0.25:
        return [(str(rng.choice([24, 25, 26, 27, 28])), "YEAR")]
    return [(str(rng.choice([2024, 2025, 2026, 2027, 2028, 2029])), "YEAR")]


def pick_hour_token(rng: random.Random) -> tuple[str, str]:
    if rng.random() < 0.35:
        return (rng.choice(HOUR_WORDS), "HOUR")
    hour = rng.randint(1, 12)
    text = f"{hour:02d}" if rng.random() < 0.15 else str(hour)
    return (text, "HOUR")


def pick_meridiem(rng: random.Random) -> tuple[str, str]:
    return (rng.choice(["am", "pm", "AM", "PM"]), "MERIDIEM")


def en_clock(rng: random.Random) -> list[tuple[str, str]]:
    kind = rng.randrange(9)
    if kind == 0:
        return labeled(rng.choice(NAMED_TIMES), "TIME_NAMED")
    if kind == 1:
        return labeled(rng.choice(DAYPARTS), "DAYPART")
    if kind == 2:
        offset = rng.choice(["half", "quarter"])
        direction = rng.choice(["past", "to"])
        tokens = [(offset, "CLOCK_OFFSET"), (direction, "GLUE")]
        tokens.append(pick_hour_token(rng))
        if rng.random() < 0.55:
            tokens.append(pick_meridiem(rng))
        return tokens
    if kind == 3:
        hour = rng.choice(HOUR_WORDS)
        if rng.random() < 0.35:
            return [(hour, "HOUR"), ("o'clock", "MERIDIEM")]
        return [(hour, "HOUR"), pick_meridiem(rng)]
    hour = pick_hour_token(rng)
    tokens = [hour]
    spoken = hour[0].isalpha()
    if not spoken and (kind >= 5 or rng.random() < 0.7):
        tokens += [(":", "GLUE"), (f"{rng.randint(0, 59):02d}", "MINUTE")]
        if kind == 8:
            tokens += [(":", "GLUE"), (f"{rng.randint(0, 59):02d}", "SECOND")]
    if rng.random() < 0.85 and hour[0].lower() not in NAMED_TIMES:
        tokens.append(pick_meridiem(rng))
    return tokens


def has_clock(tokens: list[tuple[str, str]]) -> bool:
    return any(label in CLOCK_LABELS for _, label in tokens)


def with_clock(rng: random.Random, tokens: list[tuple[str, str]], p: float = 0.45) -> list[tuple[str, str]]:
    if has_clock(tokens) or rng.random() >= p:
        return tokens
    glue = rng.choice(["at", "around", "by"])
    return tokens + [(glue, "GLUE")] + en_clock(rng)


def en_calendar(rng: random.Random) -> list[tuple[str, str]]:
    kind = rng.randrange(6)
    month = pick_month(rng)
    dom = pick_dom(rng)
    if kind == 0:
        tokens = month + ([(",", "GLUE")] if rng.random() < 0.12 else []) + dom
        if rng.random() < 0.45:
            tokens += pick_year(rng)
        return tokens
    if kind == 1:
        tokens = dom + month
        if rng.random() < 0.4:
            tokens += pick_year(rng)
        return tokens
    if kind == 2:
        return pick_year(rng)
    if kind == 3:
        tokens = ([("the", "GLUE")] if rng.random() < 0.7 else []) + pick_dom(rng)
        return tokens
    if kind == 4:
        y = rng.randint(2024, 2028)
        m = rng.randint(1, 12)
        d = rng.randint(1, 28)
        return [
            (str(y), "YEAR"), ("-", "GLUE"),
            (f"{m:02d}", "MONTH"), ("-", "GLUE"),
            (f"{d:02d}", "DOM"),
        ]
    tokens = month + ([("the", "GLUE")] if rng.random() < 0.4 else []) + dom
    if rng.random() < 0.35:
        tokens += pick_year(rng)
    return tokens


def en_rel_day(rng: random.Random) -> list[tuple[str, str]]:
    return labeled(rng.choice(REL_PHRASES), "REL_DAY")


def en_deictic_period(rng: random.Random) -> list[tuple[str, str]]:
    tokens: list[tuple[str, str]] = []
    if rng.random() < 0.2:
        tokens.append((rng.choice(DEICTICS), "DEICTIC"))
        tokens.append((rng.choice(["weekend", "weekends"]), "DAYGROUP"))
        return tokens
    if rng.random() < 0.28:
        tokens += labeled(rng.choice(["start", "beginning", "end"]), "EDGE")
        tokens.append(("of", "GLUE"))
    tokens.append((rng.choice(DEICTICS), "DEICTIC"))
    unit = rng.choice(PERIODS if rng.random() < 0.75 else PERIODS_PL)
    tokens.append((unit, "UNIT"))
    return tokens


def en_qualified(rng: random.Random) -> list[tuple[str, str]]:
    kind = rng.randrange(5)
    deictic = (rng.choice(DEICTICS), "DEICTIC")
    if kind == 0:
        return pick_weekday(rng) + [deictic, ("week", "UNIT")]
    if kind == 1:
        return [deictic, ("week", "UNIT")] + pick_weekday(rng)
    if kind == 2:
        return pick_dom(rng) + pick_month(rng) + [deictic, ("year", "UNIT")]
    if kind == 3:
        return pick_dom(rng) + [deictic, (rng.choice(["month", "year"]), "UNIT")]
    return pick_month(rng) + [deictic, ("year", "UNIT")]


def en_weekday_list(rng: random.Random) -> list[tuple[str, str]]:
    kind = rng.randrange(6)
    if kind == 0:
        a, b = pick_weekday(rng), pick_weekday(rng)
        join = rng.choice(["and", "or"])
        return a + [(join, "JOIN")] + b
    if kind == 1:
        a, b, c = pick_weekday(rng), pick_weekday(rng), pick_weekday(rng)
        return a + b + [("and", "JOIN")] + c
    if kind == 2:
        return pick_weekday(rng) + [("-", "RANGE_END")] + pick_weekday(rng)
    if kind == 3:
        return labeled(rng.choice(["weekday", "weekdays", "weekend", "weekends"]), "DAYGROUP")
    if kind == 4:
        return [("every", "RECUR"), ("other", "NUM")] + pick_weekday(rng)
    return [("every", "RECUR")] + labeled(rng.choice(["weekday", "weekdays", "weekend", "weekends"]), "DAYGROUP")


def en_md(rng: random.Random) -> list[tuple[str, str]]:
    return pick_month(rng) + pick_dom(rng)


def en_range(rng: random.Random) -> list[tuple[str, str]]:
    kind = rng.randrange(4)
    end = rng.choice(["to", "until", "till", "through"])
    if kind == 0:
        return [("from", "RANGE_START")] + en_md(rng) + [(end, "RANGE_END")] + en_md(rng)
    if kind == 1:
        h1, h2 = rng.sample(range(1, 12), 2)
        mer = pick_meridiem(rng)
        left = [(str(h1), "HOUR"), mer]
        right = [(str(h2), "HOUR"), mer]
        if rng.random() < 0.5:
            return [("from", "RANGE_START")] + left + [(end, "RANGE_END")] + right
        return [("between", "RANGE_START")] + left + [("and", "RANGE_END")] + right
    if kind == 2:
        return [("between", "RANGE_START")] + en_md(rng) + [("and", "RANGE_END")] + en_md(rng)
    return pick_month(rng) + pick_dom(rng) + [("-", "RANGE_END")] + pick_dom(rng)


def en_recur(rng: random.Random) -> list[tuple[str, str]]:
    kind = rng.randrange(8)
    if kind == 0:
        tokens = [(rng.choice(["every", "each"]), "RECUR"), (rng.choice(PERIODS + PERIODS_PL + ["day"]), "UNIT")]
        return with_clock(rng, tokens, 0.4)
    if kind == 1:
        word = rng.choice(["daily", "weekly", "monthly", "yearly", "annually", "biweekly"])
        tokens = [(word, "FREQ")]
        if rng.random() < 0.5:
            tokens += [("on", "GLUE")] + pick_weekday(rng)
        elif rng.random() < 0.4:
            tokens += [("on", "GLUE"), ("the", "GLUE")] + pick_dom(rng)
        return with_clock(rng, tokens, 0.35)
    if kind == 2:
        count = rng.choice(["twice", "once", "thrice"])
        unit = rng.choice(["week", "day", "month"])
        return [(count, "TIMES"), ("a", "GLUE"), (unit, "UNIT")]
    if kind == 3:
        n = rng.choice(["2", "3", "4", "three", "five"])
        unit = rng.choice(["day", "week", "month"])
        return [(n, "NUM"), ("times", "TIMES"), ("a", "GLUE"), (unit, "UNIT")]
    if kind == 4:
        return [("every", "RECUR")] + labeled(rng.choice(["weekday", "weekdays", "weekend", "weekends"]), "DAYGROUP") + with_clock(rng, [], 0.8)
    if kind == 5:
        return [("every", "RECUR"), ("other", "NUM")] + pick_weekday(rng)
    if kind == 6:
        tokens = [("every", "RECUR")] + pick_weekday(rng)
        if rng.random() < 0.4:
            tokens += [("except", "EXCEPT")] + pick_weekday(rng)
        return with_clock(rng, tokens, 0.4)
    return (
        [("every", "RECUR")]
        + pick_weekday(rng)
        + [("starting", "BOUND_START")]
        + en_md(rng)
    )


def en_ordinal(rng: random.Random) -> list[tuple[str, str]]:
    ord_tok: list[tuple[str, str]]
    if rng.random() < 0.45:
        n = rng.randint(1, 5)
        ord_tok = [(str(n), "ORD"), (ordinal_suffix(n), "GLUE")]
    else:
        ord_tok = [(rng.choice(ORD_WORDS), "ORD")]
    tokens = ([("the", "GLUE")] if rng.random() < 0.7 else []) + ord_tok + pick_weekday(rng)
    if rng.random() < 0.55:
        tokens += [("of", "GLUE"), (rng.choice(["next", "this", "last"]), "DEICTIC"), ("month", "UNIT")]
    else:
        tokens += [("of", "GLUE")] if rng.random() < 0.6 else []
        tokens += [(rng.choice(["each", "every"]), "RECUR"), ("month", "UNIT")]
    return tokens


def en_duration(rng: random.Random) -> list[tuple[str, str]]:
    kind = rng.randrange(6)
    unit = rng.choice(["minutes", "hours", "days", "weeks"])
    amount = rng.choice(["2", "3", "5", "10", "15", "20", "30", "45", "90", "two", "three"])
    if kind == 0:
        return [("for", "DUR"), (amount, "NUM"), (unit, "UNIT")]
    if kind == 1:
        return [("in", "DIR_AFTER"), (amount, "NUM"), (unit, "UNIT")]
    if kind == 2:
        return [(amount, "NUM"), (unit, "UNIT"), ("from", "DIR_AFTER"), ("now", "NOW")]
    if kind == 3:
        return [(amount, "NUM"), (unit, "UNIT"), ("ago", "DIR_BEFORE")]
    if kind == 4:
        holiday = labeled(rng.choice(HOLIDAYS), "HOLIDAY")
        direction = rng.choice(["before", "after"])
        article = [("a", "NUM")] if rng.random() < 0.5 else [(rng.choice(["2", "3", "one"]), "NUM")]
        unit_one = rng.choice(["week", "day", "month"])
        return article + [(unit_one, "UNIT"), (direction, "DIR_BEFORE" if direction == "before" else "DIR_AFTER")] + holiday
    return labeled(rng.choice(["now", "immediately", "asap"]), "NOW")


def en_holiday(rng: random.Random) -> list[tuple[str, str]]:
    tokens = labeled(rng.choice(HOLIDAYS), "HOLIDAY")
    return with_clock(rng, tokens, 0.4)


def random_prefix(rng: random.Random, heavy: bool = False) -> str:
    name = rng.choice(NAMES)
    event = rng.choice(EVENTS)
    lead = rng.choice(LEADS)
    verb = rng.choice(VERBS)
    patterns = [
        f"{verb} the {event}",
        f"{verb} {event} with {name}",
        f"{name} wants the {event}",
        f"{lead} {verb} the {event}",
        f"the {event} is",
        f"ping {name} about the {event}",
        f"remind {name}",
        f"hold the {event}",
        f"I need the {event}",
        f"we should {verb} the {event}",
        f"put the {event} down",
        f"{name}'s {event} is",
        f"can someone cover the {event}",
        f"don't forget the {event}",
        f"office {event}",
        f"gym",
        f"call {name}",
        f"payday is",
        f"out of office",
        f"the retreat is",
        f"the party is",
        f"standup is",
        f"let's catch up",
        f"book dinner",
        f"school pickup",
        f"team lunch",
        f"ship the build",
        f"review the PR",
        f"dentist with {name}",
    ]
    if heavy:
        patterns.extend([
            f"hey {name} can you move the {event}",
            f"just circling back on the {event} with {name}",
            f"emailing so we have the {event} on record",
            f"voice note to self about the {event}",
            f"calendar invite for the {event} with {name}",
            f"running late but the {event} still stands",
            f"if people are traveling we can slide the {event}",
            f"need a decision on the {event} so {name} can book rooms",
        ])
    return rng.choice(patterns)


def random_suffix(rng: random.Random) -> str:
    return rng.choice(SUFFIXES_EN + [
        f"works for {rng.choice(NAMES)}",
        f"loop in {rng.choice(NAMES)}",
        f"I'll text {rng.choice(NAMES)}",
        "and I'll send the deck",
        "if the room is free",
    ])


def _leading_glue(rng: random.Random, expr: list[tuple[str, str]]) -> str | None:
    first = next((label for _, label in expr if label not in {"GLUE", "O"}), None)
    if first in {
        "RECUR", "RANGE_START", "BOUND_START", "BOUND_END", "EXCEPT", "NOW",
        "NUM", "DUR", "DIR_AFTER", "DIR_BEFORE", "FREQ", "TIMES",
    }:
        return None
    if first in {"HOUR", "TIME_NAMED", "DAYPART", "CLOCK_OFFSET", "MERIDIEM", "MINUTE"}:
        return rng.choice(["at", "around", "by"])
    if first in {"REL_DAY", "WEEKDAY", "MONTH", "DOM", "HOLIDAY", "DAYGROUP"}:
        return rng.choice(["on", "for", "by"])
    if first in {"DUR", "DEICTIC", "UNIT", "EDGE"}:
        return rng.choice(["for", "in"])
    return rng.choice(["at", "on", "for"])


def wrap_en(rng: random.Random, expr: list[tuple[str, str]], heavy: bool = False) -> list[tuple[str, str]]:
    tokens: list[tuple[str, str]] = []
    p_pre = 0.92 if heavy else 0.72
    if rng.random() < p_pre:
        tokens += o_text(random_prefix(rng, heavy=heavy))
        if expr and expr[0][0].lower() not in STRUCT_START and rng.random() < 0.78:
            glue = _leading_glue(rng, expr)
            if glue:
                tokens.append((glue, "GLUE"))
    tokens += expr
    if rng.random() < (0.55 if heavy else 0.38):
        if rng.random() < 0.55:
            tokens.append((rng.choice([",", "."]), "GLUE"))
        tokens += o_text(random_suffix(rng))
    return maybe_lower(rng, tokens, 0.38 if not heavy else 0.22)


def gen_en_rel_day(rng: random.Random) -> list[tuple[str, str]]:
    return wrap_en(rng, with_clock(rng, en_rel_day(rng), 0.5))


def gen_en_calendar(rng: random.Random) -> list[tuple[str, str]]:
    return wrap_en(rng, with_clock(rng, en_calendar(rng), 0.4))


def gen_en_weekday(rng: random.Random) -> list[tuple[str, str]]:
    kind = rng.randrange(5)
    if kind == 0:
        expr = pick_weekday(rng)
    elif kind == 1:
        expr = [(rng.choice(DEICTICS), "DEICTIC")] + pick_weekday(rng)
    elif kind == 2:
        expr = pick_weekday(rng) + [(rng.choice(["next", "this", "last"]), "DEICTIC"), ("week", "UNIT")]
    elif kind == 3:
        expr = [(rng.choice(["next", "this", "last"]), "DEICTIC"), ("week", "UNIT")] + pick_weekday(rng)
    else:
        expr = pick_weekday(rng)
    return wrap_en(rng, with_clock(rng, expr, 0.4))


def gen_en_weekday_list(rng: random.Random) -> list[tuple[str, str]]:
    return wrap_en(rng, with_clock(rng, en_weekday_list(rng), 0.45))


def gen_en_clocks(rng: random.Random) -> list[tuple[str, str]]:
    if rng.random() < 0.12:
        expr = labeled(rng.choice(["now", "immediately", "asap"]), "NOW")
    else:
        expr = en_clock(rng)
        if rng.random() < 0.2:
            expr = [(rng.choice(["at", "around", "by"]), "GLUE")] + expr
    return wrap_en(rng, expr)


def gen_en_deictic(rng: random.Random) -> list[tuple[str, str]]:
    return wrap_en(rng, with_clock(rng, en_deictic_period(rng), 0.35))


def gen_en_qualified(rng: random.Random) -> list[tuple[str, str]]:
    return wrap_en(rng, with_clock(rng, en_qualified(rng), 0.3))


def gen_en_ranges(rng: random.Random) -> list[tuple[str, str]]:
    return wrap_en(rng, en_range(rng))


def gen_en_recur(rng: random.Random) -> list[tuple[str, str]]:
    return wrap_en(rng, en_recur(rng))


def gen_en_ordinal(rng: random.Random) -> list[tuple[str, str]]:
    return wrap_en(rng, with_clock(rng, en_ordinal(rng), 0.25))


def gen_en_duration(rng: random.Random) -> list[tuple[str, str]]:
    return wrap_en(rng, en_duration(rng))


def gen_en_holiday(rng: random.Random) -> list[tuple[str, str]]:
    return wrap_en(rng, en_holiday(rng))


def gen_en_multi(rng: random.Random) -> list[tuple[str, str]]:
    date = rng.choice([en_rel_day, en_calendar, pick_weekday, en_deictic_period])(rng)
    return wrap_en(rng, with_clock(rng, date, 1.0))


def gen_en_prose(rng: random.Random) -> list[tuple[str, str]]:
    inner = rng.choice([
        en_rel_day, en_calendar, en_clock, en_deictic_period, en_qualified,
        en_range, en_recur, en_holiday, en_weekday_list, en_ordinal,
    ])(rng)
    return wrap_en(rng, with_clock(rng, inner, 0.2), heavy=True)


def gen_en_negative(rng: random.Random) -> list[tuple[str, str]]:
    name = rng.choice(NAMES)
    event = rng.choice(EVENTS)
    doc = rng.choice(["deck", "agenda", "notes", "report", "slides", "doc", "brief", "summary"])
    adj = rng.choice(["quick", "careful", "final", "light", "honest"])
    clean = [
        f"please send the {doc} to {name}",
        f"the {doc} still needs a {adj} pass",
        f"can you review the {event} notes with {name}",
        f"I'll ping {name} about the {doc}",
        f"the room booking tool is down again",
        f"we still need a decision on the {event} scope",
        f"please print extra copies of the {doc}",
        f"the {event} invite has the wrong people",
        f"{name} will share the {doc} in the channel",
        f"keep the {event} discussion in the thread",
        f"I cannot find the {doc} in the drive",
        f"ask {name} to rewrite the {event} blurb",
        f"the {event} run of show is in the {doc}",
        f"don't ship the {doc} until {name} signs off",
        f"we need a volunteer to take notes",
        f"the projector in that room is broken",
        f"please add {name} to the {event} channel",
        f"the {doc} is too long for the {event}",
        f"I'll bring snacks if someone brings plates",
        f"no need to wait on me for the {doc}",
    ]
    traps = [
        f"the last slide has three diagrams for {name}",
        f"May I have your second opinion on the {doc}",
        f"please call twelve people in room four",
        f"our second attempt was the last one",
        f"a month is a unit in this glossary",
        f"from Alice to Bob the message says hello",
        f"this number is {rng.randint(10, 90)} and that one is {rng.randint(1990, 2030)}",
        f"the next chapter is to review the {doc}",
        f"March of the robots is the working title",
        f"we may succeed if the {event} stays focused",
    ]
    text = rng.choice(clean if rng.random() < 0.75 else traps)
    return maybe_lower(rng, o_text(text), 0.4)


EN_GENERATORS = {
    "rel_day": gen_en_rel_day,
    "calendar": gen_en_calendar,
    "weekday": gen_en_weekday,
    "weekday_list": gen_en_weekday_list,
    "clocks": gen_en_clocks,
    "deictic": gen_en_deictic,
    "qualified": gen_en_qualified,
    "ranges": gen_en_ranges,
    "recur": gen_en_recur,
    "ordinal": gen_en_ordinal,
    "duration": gen_en_duration,
    "holiday": gen_en_holiday,
    "multi": gen_en_multi,
    "prose": gen_en_prose,
    "negative": gen_en_negative,
}


# ---------------------------------------------------------------------------
# Hindi
# ---------------------------------------------------------------------------

HI_WEEKDAYS = ["सोमवार", "मंगलवार", "बुधवार", "गुरुवार", "शुक्रवार", "शनिवार", "रविवार"]
HI_MONTHS = [
    "जनवरी", "फरवरी", "मार्च", "अप्रैल", "मई", "जून",
    "जुलाई", "अगस्त", "सितंबर", "अक्टूबर", "नवंबर", "दिसंबर",
]
HI_NUMS = "एक दो तीन चार पाँच पांच छह सात आठ नौ दस ग्यारह बारह".split()
HI_DAYPARTS = ["सुबह", "दोपहर", "शाम", "रात"]
HI_HOLIDAYS = ["क्रिसमस", "नया साल"]
HI_NAMES = [
    "राहुल", "प्रिया", "अनिका", "रोहन", "नेहा", "अमित", "कबीर", "मीरा", "आरव",
    "देव", "फराह", "ओमर", "ईशा", "विवान", "सारा", "नूह", "ईशान", "तन्वी",
    "आर्यन", "दिया", "कृष", "जोया", "रेहान", "माला", "कीर्ति", "यश", "अनाया",
    "वीर", "ज़ोया", "आदित्य",
]
HI_EVENTS = [
    "मीटिंग", "कॉल", "जमा", "डॉक्टर", "क्लास", "इंटीव्यू", "डिनर", "लंच",
    "जिम", "फ्लाइट", "स्टैंडअप", "डेमो", "ऑफिस", "पार्टी", "रिपोर्ट",
    "सिंक", "रिव्यू", "चेकअप", "पिकअप", "वर्कशॉप", "लॉन्च", "गेम", "एग्जाम",
    "ब्रंच", "कॉफी", "हेयरकट", "ऑफसाइट", "प्लानिंग", "रेट्रो", "किकऑफ",
]
HI_FUTURE = ["आएगा", "आना है", "होगा", "करना है", "मिलना है", "जाना है", "भेजना है"]
HI_PAST = ["आया था", "गया था", "हुआ था", "किया था", "था"]


def hi_hour(rng: random.Random) -> tuple[str, str]:
    if rng.random() < 0.5:
        return (rng.choice(HI_NUMS), "HOUR")
    return (str(rng.randint(1, 12)), "HOUR")


def hi_clock(rng: random.Random) -> list[tuple[str, str]]:
    tokens: list[tuple[str, str]] = []
    kind = rng.randrange(5)
    standalone_offset = kind == 1
    if not standalone_offset and rng.random() < 0.65:
        tokens.append((rng.choice(HI_DAYPARTS), "DAYPART"))
        if rng.random() < 0.85:
            tokens.append(("को", "GLUE"))
    if kind == 0:
        tokens += [rng.choice([("सवा", "CLOCK_OFFSET"), ("पौने", "CLOCK_OFFSET")]), hi_hour(rng), ("बजे", "MERIDIEM")]
    elif kind == 1:
        tokens += [rng.choice([("डेढ़", "CLOCK_OFFSET"), ("ढाई", "CLOCK_OFFSET")]), ("बजे", "MERIDIEM")]
    elif kind == 2:
        tokens += [("आधे", "CLOCK_OFFSET"), hi_hour(rng), ("बजे", "MERIDIEM")]
    else:
        tokens += [hi_hour(rng), ("बजे", "MERIDIEM")]
    return tokens


def hi_rel(rng: random.Random) -> list[tuple[str, str]]:
    word = rng.choice(["आज", "कल", "परसों"])
    tokens = [(word, "REL_DAY")]
    if word == "कल":
        extra = rng.choice(HI_FUTURE if rng.random() < 0.55 else HI_PAST)
        # disambiguating verb sits outside the time expression
        return tokens, extra
    return tokens, rng.choice(HI_FUTURE if word in {"आज", "परसों"} and rng.random() < 0.5 else [""])


def hi_date(rng: random.Random) -> list[tuple[str, str]]:
    kind = rng.randrange(3)
    month = [(rng.choice(HI_MONTHS), "MONTH")]
    dom = pick_dom(rng)
    if kind == 0:
        return month + [("की", "GLUE")] + dom
    if kind == 1:
        return dom + month
    return month + dom + pick_year(rng)


def hi_period(rng: random.Random, units: list[str] | None = None) -> list[tuple[str, str]]:
    deictic = rng.choice(["इस", "यह", "अगला", "अगले", "अगली", "पिछला", "पिछले", "पिछली"])
    unit = rng.choice(units or ["दिन", "हफ़्ते", "हफ्ते", "सप्ताह", "महीने", "महीना", "साल", "वर्ष"])
    return [(deictic, "DEICTIC"), (unit, "UNIT")]


def hi_prefix(rng: random.Random, heavy: bool = False) -> str:
    name = rng.choice(HI_NAMES)
    other = rng.choice(HI_NAMES)
    event = rng.choice(HI_EVENTS)
    patterns = [
        f"{event} है",
        f"{name} को याद दिलाना",
        f"{event} रखना",
        f"{name} से बात",
        f"ऑफिस जाना है",
        f"डॉक्टर के पास",
        f"रिपोर्ट भेजनी है",
        f"{event} शेड्यूल करो",
        f"{name} को बता देना",
        f"{name} का {event}",
        f"{event} फिक्स करो",
        f"{name} को बुलाना",
        f"{event} का प्लान",
        f"{name} और {other} का {event}",
        f"जल्दी से {event} सेट करो",
        f"{name} पूछ रहे हैं {event} के लिए",
        f"{event} वाला रिमाइंडर",
        f"{name} के साथ {event}",
    ]
    if heavy:
        patterns.extend([
            f"व्हाट्सएप पर {name} को लिखना",
            f"ऑफिस चैट में {event} पिन करो",
            f"{name} बोल रहे थे {event} के बारे में",
            f"{other} ने कहा {event} आगे बढ़ाओ",
            f"ग्रुप में {name} को {event} बताना बाकी है",
        ])
    return rng.choice(patterns)


def hi_suffix(rng: random.Random) -> str:
    return rng.choice([
        "ठीक है", "प्लीज", "जरूर", "हो जाएगा", "बता देना", "मिस मत करना",
        f"{rng.choice(HI_NAMES)} को भी बताना", "कमरे का इंतजाम कर लेना",
        "जल्दी जवाब देना", "मैं आ जाऊँगा", "कम समय रखना", "थ्रेड में लिख देना",
        f"{rng.choice(HI_NAMES)} कवर कर लेंगे", "नोट्स मैं ले लूँगा",
    ])


def wrap_hi(rng: random.Random, expr: list[tuple[str, str]], extra_o: str = "", heavy: bool = False) -> list[tuple[str, str]]:
    tokens: list[tuple[str, str]] = []
    if rng.random() < (0.92 if heavy else 0.82):
        tokens += o_text(hi_prefix(rng, heavy=heavy))
    tokens += expr
    if extra_o:
        tokens += o_text(extra_o)
    if rng.random() < 0.4:
        tokens += o_text(hi_suffix(rng))
    return tokens


def gen_hi_rel_day(rng: random.Random) -> list[tuple[str, str]]:
    expr, extra = hi_rel(rng)
    if rng.random() < 0.55:
        expr = expr + hi_clock(rng)
    return wrap_hi(rng, expr, extra)


def gen_hi_calendar(rng: random.Random) -> list[tuple[str, str]]:
    return wrap_hi(rng, hi_date(rng) + (hi_clock(rng) if rng.random() < 0.35 else []))


def gen_hi_weekday(rng: random.Random) -> list[tuple[str, str]]:
    day = [(rng.choice(HI_WEEKDAYS), "WEEKDAY")]
    kind = rng.randrange(4)
    if kind == 0:
        expr = day + ([("को", "GLUE")] if rng.random() < 0.7 else [])
    elif kind == 1:
        expr = hi_period(rng) + day + [("को", "GLUE")]
    elif kind == 2:
        expr = day + hi_period(rng)
    else:
        deictic = rng.choice(["अगले", "पिछले", "इस"])
        expr = [(deictic, "DEICTIC")] + day
    if rng.random() < 0.4:
        expr += hi_clock(rng)
    return wrap_hi(rng, expr)


def gen_hi_weekday_list(rng: random.Random) -> list[tuple[str, str]]:
    a, b = rng.sample(HI_WEEKDAYS, 2)
    kind = rng.randrange(4)
    if kind == 0:
        expr = [(a, "WEEKDAY"), ("और", "JOIN"), (b, "WEEKDAY")]
    elif kind == 1:
        expr = [("हर", "RECUR"), (a, "WEEKDAY"), ("और", "JOIN"), (b, "WEEKDAY")]
    elif kind == 2:
        expr = [("हर", "RECUR"), (rng.choice(["वीकेंड", "वीकडे"]), "DAYGROUP")]
    else:
        expr = [("हर", "RECUR"), ("दूसरे", "GLUE"), (a, "WEEKDAY")]
    return wrap_hi(rng, expr + (hi_clock(rng) if rng.random() < 0.4 else []))


def gen_hi_clocks(rng: random.Random) -> list[tuple[str, str]]:
    return wrap_hi(rng, hi_clock(rng))


def gen_hi_deictic(rng: random.Random) -> list[tuple[str, str]]:
    expr = hi_period(rng)
    if rng.random() < 0.3:
        expr = [("शुरुआत", "EDGE"), ("में", "GLUE")] + expr
    elif rng.random() < 0.25:
        expr = [("आखिर", "EDGE"), ("में", "GLUE")] + expr
    return wrap_hi(rng, expr)


def gen_hi_qualified(rng: random.Random) -> list[tuple[str, str]]:
    kind = rng.randrange(3)
    if kind == 0:
        expr = [(rng.choice(HI_WEEKDAYS), "WEEKDAY")] + hi_period(rng, ["हफ़्ते", "हफ्ते", "सप्ताह"])
    elif kind == 1:
        expr = pick_dom(rng) + [(rng.choice(HI_MONTHS), "MONTH")] + hi_period(
            rng, ["महीने", "महीना", "साल", "वर्ष"]
        )
    else:
        expr = hi_period(rng, ["हफ़्ते", "हफ्ते", "सप्ताह"]) + [
            (rng.choice(HI_WEEKDAYS), "WEEKDAY"),
            ("को", "GLUE"),
        ]
    return wrap_hi(rng, expr)


def gen_hi_ranges(rng: random.Random) -> list[tuple[str, str]]:
    kind = rng.randrange(3)
    if kind == 0:
        expr = (
            [("से", "RANGE_START")]
            + [(rng.choice(HI_MONTHS), "MONTH")]
            + pick_dom(rng)
            + [("तक", "RANGE_END")]
            + [(rng.choice(HI_MONTHS), "MONTH")]
            + pick_dom(rng)
        )
    elif kind == 1:
        a, b = rng.sample(range(1, 12), 2)
        expr = [
            ("से", "RANGE_START"),
            (str(a), "HOUR"),
            ("बजे", "MERIDIEM"),
            ("तक", "RANGE_END"),
            (str(b), "HOUR"),
            ("बजे", "MERIDIEM"),
        ]
    else:
        expr = [("से", "RANGE_START"), ("आज", "REL_DAY"), ("तक", "RANGE_END"), ("परसों", "REL_DAY")]
    return wrap_hi(rng, expr)


def gen_hi_recur(rng: random.Random) -> list[tuple[str, str]]:
    kind = rng.randrange(4)
    if kind == 0:
        expr = [("हर", "RECUR"), (rng.choice(["दिन", "हफ़्ते", "महीने", "साल"]), "UNIT")]
    elif kind == 1:
        expr = [("हरेक", "RECUR"), (rng.choice(HI_WEEKDAYS), "WEEKDAY"), ("को", "GLUE")]
    elif kind == 2:
        n = rng.choice(["2", "3", "तीन"])
        expr = [(n, "NUM"), ("बार", "TIMES"), ("हफ़्ते", "UNIT")]
    else:
        expr = [("हर", "RECUR"), ("वीकडे", "DAYGROUP"), ("को", "GLUE")] + hi_clock(rng)
    return wrap_hi(rng, expr)


def gen_hi_ordinal(rng: random.Random) -> list[tuple[str, str]]:
    ord_w = rng.choice([("पहला", "ORD"), ("दूसरा", "ORD"), ("आखिरी", "ORD"), ("last", "ORD")])
    expr = [ord_w, (rng.choice(HI_WEEKDAYS), "WEEKDAY"), ("को", "GLUE")] + hi_period(
        rng, ["महीने", "महीना"]
    )
    return wrap_hi(rng, expr)


def gen_hi_duration(rng: random.Random) -> list[tuple[str, str]]:
    kind = rng.randrange(4)
    n = rng.choice(["10", "20", "30", "90", "दो", "तीन"])
    unit = rng.choice(["मिनट", "घंटे", "दिन", "हफ़्ते"])
    if kind == 0:
        expr = [(n, "NUM"), (unit, "UNIT"), ("में", "DIR_AFTER")]
    elif kind == 1:
        expr = [("के", "GLUE"), ("लिए", "DUR"), (n, "NUM"), (unit, "UNIT")]
    elif kind == 2:
        expr = [(n, "NUM"), (unit, "UNIT"), ("पहले", "DIR_BEFORE")] + labeled(rng.choice(HI_HOLIDAYS), "HOLIDAY")
    else:
        expr = [(n, "NUM"), (unit, "UNIT"), ("बाद", "DIR_AFTER")]
    return wrap_hi(rng, expr)


def gen_hi_holiday(rng: random.Random) -> list[tuple[str, str]]:
    expr = labeled(rng.choice(HI_HOLIDAYS), "HOLIDAY")
    if rng.random() < 0.3:
        expr += hi_clock(rng)
    return wrap_hi(rng, expr)


def gen_hi_multi(rng: random.Random) -> list[tuple[str, str]]:
    date = rng.choice([lambda r: [(r.choice(["आज", "कल"]), "REL_DAY")], hi_date])(rng)
    return wrap_hi(rng, date + hi_clock(rng) if rng.random() < 0.7 else date)


def gen_hi_prose(rng: random.Random) -> list[tuple[str, str]]:
    inner = rng.choice([hi_clock, hi_date, hi_period, lambda r: [(r.choice(["आज", "कल", "परसों"]), "REL_DAY")]])(rng)
    return wrap_hi(rng, inner, heavy=True)


def gen_hi_negative(rng: random.Random) -> list[tuple[str, str]]:
    name = rng.choice(HI_NAMES)
    other = rng.choice(HI_NAMES)
    event = rng.choice(HI_EVENTS)
    doc = rng.choice(["रिपोर्ट", "डेक", "एजेंडा", "नोट्स", "स्लाइड्स", "डॉक", "सारांश", "लिस्ट", "फाइल", "ड्राफ्ट"])
    verb = rng.choice(["भेज दो", "लिखा दो", "चेक करो", "फॉरवर्ड करो", "पिन करो", "शेयर करो"])
    place = rng.choice(["चैनल", "ड्राइव", "ईमेल", "ग्रुप", "थ्रेड", "फोल्डर"])
    adj = rng.choice(["लंबा", "छोटा", "अधूरा", "गलत", "पुराना", "खाली"])
    thing = rng.choice(["प्रिंटर", "प्रोजेक्टर", "वाईफाई", "बोर्ड", "माइक", "कैमरा"])
    text = rng.choice([
        f"{name} को {doc} {verb}",
        f"{doc} अभी तैयार नहीं हैं",
        f"{event} का {doc} {adj} है",
        f"{thing} फिर से खराब है",
        f"{name} {place} में {doc} डाल देंगे",
        f"कमरे में {thing} काम नहीं कर रहा",
        f"कृपया {event} वाले लोगों को जोड़ो",
        f"मुझे {doc} {place} में नहीं मिला",
        f"{name} साइन ऑफ करें उसके बाद भेजना",
        f"{doc} कोई ले ले प्लीज",
        f"स्नैक्स मैं लाऊंगा प्लेट्स कोई और",
        f"{event} का स्कोप अभी फाइनल नहीं",
        f"{name} और {other} {doc} पर साथ काम कर रहे हैं",
        f"{place} में {event} की बात मत करना",
        f"{name} से {doc} माँग लो",
        f"{event} के लोगों की लिस्ट {adj} है",
        f"{other} को {place} की एक्सेस दे दो",
        f"{doc} को {adj} मत छोड़ो {name}",
        f"{thing} वाला कमरा बंद है",
        f"{name} {event} का ब्लर्ब लिखेंगे",
        f"{doc} प्रिंट करो {rng.randint(2, 12)} कॉपी",
        f"{event} के नोट्स {place} में रखो",
        f"{name} बिना {doc} के आ गए",
        f"{other} कह रहे हैं {event} का स्कोप बदलो",
        f"{doc} पर {name} का कमेंट आ गया",
    ])
    return o_text(text)


HI_GENERATORS = {
    "rel_day": gen_hi_rel_day,
    "calendar": gen_hi_calendar,
    "weekday": gen_hi_weekday,
    "weekday_list": gen_hi_weekday_list,
    "clocks": gen_hi_clocks,
    "deictic": gen_hi_deictic,
    "qualified": gen_hi_qualified,
    "ranges": gen_hi_ranges,
    "recur": gen_hi_recur,
    "ordinal": gen_hi_ordinal,
    "duration": gen_hi_duration,
    "holiday": gen_hi_holiday,
    "multi": gen_hi_multi,
    "prose": gen_hi_prose,
    "negative": gen_hi_negative,
}


# ---------------------------------------------------------------------------
# Hinglish
# ---------------------------------------------------------------------------

HX_DAYS = [
    "somvaar", "mangal", "budh", "guruvaar", "shukravaar", "shanivaar", "ravivaar",
    "monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday",
    "Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun",
]
HX_REL = ["kal", "aaj", "parso", "parson"]
HX_UNITS = ["din", "hafte", "hafte", "week", "mahine", "mahina", "saal", "year"]
HX_DEICTIC = ["agla", "agle", "agli", "pichhla", "pichhle", "pichhli", "is", "yeh", "next", "last"]
HX_PARTS = ["subah", "dopahar", "shaam", "raat", "morning", "evening"]
HX_FUTURE = ["aana hai", "hoga", "milte hain", "call kar dena", "yaad dilana", "jana hai"]
HX_PAST = ["aaya tha", "gaya tha", "hua tha", "kiya tha"]
HX_NAMES = NAMES
HX_EVENTS = [
    "meeting", "call", "gym", "dinner", "lunch", "interview", "class", "flight",
    "standup", "demo", "office", "party", "report", "sync", "doctor",
    "review", "checkup", "pickup", "workshop", "launch", "game", "exam",
    "brunch", "coffee", "haircut", "offsite", "planning", "retro", "kickoff",
]


def hx_clock(rng: random.Random) -> list[tuple[str, str]]:
    if rng.random() < 0.3:
        return en_clock(rng)
    tokens: list[tuple[str, str]] = []
    if rng.random() < 0.6:
        tokens.append((rng.choice(HX_PARTS), "DAYPART"))
        if rng.random() < 0.8:
            tokens.append(("ko", "GLUE"))
    hour = rng.choice([str(rng.randint(1, 12)), rng.choice(HOUR_WORDS)])
    tokens += [(hour, "HOUR"), ("baje", "MERIDIEM")]
    return tokens


def hx_prefix(rng: random.Random, heavy: bool = False) -> str:
    name = rng.choice(HX_NAMES)
    other = rng.choice(HX_NAMES)
    event = rng.choice(HX_EVENTS)
    patterns = [
        f"{event} hai",
        f"{name} ko yaad dila dena",
        f"{event} schedule karo",
        f"{name} se baat karni hai",
        f"office jana hai",
        f"report bhejni hai",
        f"{name} ko bata dena",
        f"plan banate hain",
        f"hold the {event}",
        f"book the {event}",
        f"{name} ka {event}",
        f"{event} fix karo",
        f"{name} ko bula lena",
        f"{event} ka plan",
        f"{name} aur {other} ka {event}",
        f"jaldi se {event} set karo",
        f"{name} pooch rahe hain {event} ke liye",
        f"{event} wala reminder",
        f"{name} ke saath {event}",
        f"ping {name} about the {event}",
    ]
    if heavy:
        patterns.extend([
            f"whatsapp pe {name} ko likh dena",
            f"office chat me {event} pin karo",
            f"{name} bol rahe the {event} ke baare me",
            f"{other} ne kaha {event} aage badhao",
            f"group me {name} ko {event} batana baaki hai",
        ])
    return rng.choice(patterns)


def wrap_hx(rng: random.Random, expr: list[tuple[str, str]], extra_o: str = "", heavy: bool = False) -> list[tuple[str, str]]:
    tokens: list[tuple[str, str]] = []
    if rng.random() < (0.92 if heavy else 0.82):
        tokens += o_text(hx_prefix(rng, heavy=heavy))
    tokens += expr
    if extra_o:
        tokens += o_text(extra_o)
    if rng.random() < 0.4:
        tokens += o_text(rng.choice([
            "please", "theek hai", "zaroor", "miss mat karna", "I'll join",
            "jaldi reply dena", "notes main le lunga", "room book kar dena",
            f"{rng.choice(HX_NAMES)} ko bhi bata dena", "keep it short",
        ]))
    return maybe_lower(rng, tokens, 0.55)


def gen_hx_rel_day(rng: random.Random) -> list[tuple[str, str]]:
    word = rng.choice(HX_REL + ["today", "tomorrow"])
    extra = ""
    if word in {"kal", "parso", "parson"}:
        extra = rng.choice(HX_FUTURE if rng.random() < 0.55 else HX_PAST)
    expr = [(word, "REL_DAY")]
    if rng.random() < 0.6:
        expr += hx_clock(rng)
    return wrap_hx(rng, expr, extra)


def gen_hx_calendar(rng: random.Random) -> list[tuple[str, str]]:
    if rng.random() < 0.5:
        expr = pick_month(rng) + pick_dom(rng)
    else:
        expr = pick_dom(rng) + [(rng.choice(HI_MONTHS + MONTHS), "MONTH")]
    if rng.random() < 0.3:
        expr += hx_clock(rng)
    return wrap_hx(rng, expr)


def gen_hx_weekday(rng: random.Random) -> list[tuple[str, str]]:
    day = [(rng.choice(HX_DAYS), "WEEKDAY")]
    kind = rng.randrange(4)
    if kind == 0:
        expr = [("agle", "DEICTIC"), ("hafte", "UNIT")] + day + [("ko", "GLUE")]
    elif kind == 1:
        expr = [("next", "DEICTIC")] + day
    elif kind == 2:
        expr = day + [("next", "DEICTIC"), ("week", "UNIT")]
    else:
        expr = day + [("ko", "GLUE")]
    if rng.random() < 0.4:
        expr += hx_clock(rng)
    return wrap_hx(rng, expr)


def gen_hx_weekday_list(rng: random.Random) -> list[tuple[str, str]]:
    a, b = rng.sample(HX_DAYS, 2)
    kind = rng.randrange(3)
    if kind == 0:
        expr = [("har", "RECUR"), (a, "WEEKDAY"), ("aur", "JOIN"), (b, "WEEKDAY")]
    elif kind == 1:
        expr = [("har", "RECUR"), ("hafte", "UNIT"), (a, "WEEKDAY"), ("ko", "GLUE")]
    else:
        expr = [("every", "RECUR"), ("other", "GLUE"), (a, "WEEKDAY")]
    return wrap_hx(rng, expr)


def gen_hx_clocks(rng: random.Random) -> list[tuple[str, str]]:
    return wrap_hx(rng, hx_clock(rng))


def gen_hx_deictic(rng: random.Random) -> list[tuple[str, str]]:
    return wrap_hx(rng, [(rng.choice(HX_DEICTIC), "DEICTIC"), (rng.choice(HX_UNITS), "UNIT")])


def gen_hx_qualified(rng: random.Random) -> list[tuple[str, str]]:
    if rng.random() < 0.5:
        expr = [(rng.choice(HX_DAYS), "WEEKDAY"), (rng.choice(["agle", "next"]), "DEICTIC"), (rng.choice(["hafte", "week"]), "UNIT")]
    else:
        expr = pick_dom(rng) + pick_month(rng) + [("last", "DEICTIC"), ("year", "UNIT")]
    return wrap_hx(rng, expr)


def gen_hx_ranges(rng: random.Random) -> list[tuple[str, str]]:
    if rng.random() < 0.5:
        h1, h2 = rng.sample(range(1, 12), 2)
        expr = [
            ("se", "RANGE_START"),
            (str(h1), "HOUR"),
            ("baje", "MERIDIEM"),
            ("tak", "RANGE_END"),
            (str(h2), "HOUR"),
            ("baje", "MERIDIEM"),
        ]
    else:
        expr = [("from", "RANGE_START")] + pick_month(rng) + pick_dom(rng) + [("tak", "RANGE_END")] + pick_month(rng) + pick_dom(rng)
    return wrap_hx(rng, expr)


def gen_hx_recur(rng: random.Random) -> list[tuple[str, str]]:
    kind = rng.randrange(4)
    if kind == 0:
        expr = [("har", "RECUR"), (rng.choice(HX_UNITS), "UNIT")]
    elif kind == 1:
        expr = [("har", "RECUR"), (rng.choice(HX_DAYS), "WEEKDAY"), ("ko", "GLUE")] + hx_clock(rng)
    elif kind == 2:
        expr = [("3", "NUM"), ("times", "TIMES"), ("a", "GLUE"), ("week", "UNIT")]
    else:
        expr = [("weekly", "FREQ"), ("on", "GLUE"), (rng.choice(HX_DAYS), "WEEKDAY")]
    return wrap_hx(rng, expr)


def gen_hx_ordinal(rng: random.Random) -> list[tuple[str, str]]:
    expr = [("pehla" if rng.random() < 0.5 else rng.choice(ORD_WORDS), "ORD"), (rng.choice(HX_DAYS), "WEEKDAY"), ("of", "GLUE"), ("har", "RECUR"), ("mahine", "UNIT")]
    return wrap_hx(rng, expr)


def gen_hx_duration(rng: random.Random) -> list[tuple[str, str]]:
    n = rng.choice(["10", "20", "30", "90", "do", "teen"])
    unit = rng.choice(["minutes", "min", "ghante", "din", "hafte"])
    kind = rng.randrange(3)
    if kind == 0:
        expr = [(n, "NUM"), (unit, "UNIT"), ("me", "DIR_AFTER")]
    elif kind == 1:
        expr = [("in", "DIR_AFTER"), (n, "NUM"), (unit, "UNIT")]
    else:
        expr = [(n, "NUM"), (unit, "UNIT"), ("baad", "DIR_AFTER")]
    return wrap_hx(rng, expr)


def gen_hx_holiday(rng: random.Random) -> list[tuple[str, str]]:
    holiday = rng.choice(["christmas", "Christmas Eve", "Halloween", "New Year"])
    return wrap_hx(rng, labeled(holiday, "HOLIDAY"))


def gen_hx_multi(rng: random.Random) -> list[tuple[str, str]]:
    date = [(rng.choice(HX_REL), "REL_DAY")]
    return wrap_hx(rng, date + hx_clock(rng))


def gen_hx_prose(rng: random.Random) -> list[tuple[str, str]]:
    inner = rng.choice([hx_clock, lambda r: [(r.choice(HX_REL), "REL_DAY")], lambda r: [(r.choice(HX_DEICTIC), "DEICTIC"), (r.choice(HX_UNITS), "UNIT")]])(rng)
    return wrap_hx(rng, inner, heavy=True)


def gen_hx_negative(rng: random.Random) -> list[tuple[str, str]]:
    name = rng.choice(HX_NAMES)
    other = rng.choice(HX_NAMES)
    event = rng.choice(HX_EVENTS)
    doc = rng.choice(["report", "deck", "agenda", "notes", "slides", "doc", "summary", "list", "file", "draft"])
    verb = rng.choice(["bhej do", "likh do", "check karo", "forward karo", "pin karo", "share karo"])
    place = rng.choice(["channel", "drive", "email", "group", "thread", "folder"])
    adj = rng.choice(["lamba", "chhota", "adhura", "galat", "purana", "khaali"])
    thing = rng.choice(["printer", "projector", "wifi", "board", "mic", "camera"])
    text = rng.choice([
        f"{name} ko {doc} {verb}",
        f"{doc} abhi ready nahi hain",
        f"{event} ka {doc} {adj} hai",
        f"{thing} phir se kharab hai",
        f"{name} {place} me {doc} daal denge",
        f"room me {thing} kaam nahi kar raha",
        f"please {name} ko {event} channel me add karo",
        f"mujhe {doc} {place} me nahi mila",
        f"{name} sign off karein uske baad bhejna",
        f"{doc} koi le le please",
        f"snacks main launga plates koi aur",
        f"{event} ka scope abhi final nahi",
        f"I'll ping {name} about the {doc}",
        f"{name} aur {other} {doc} par saath kaam kar rahe hain",
        f"{place} me {event} ki baat mat karna",
        f"{name} se {doc} maang lo",
        f"{event} ke logon ki list {adj} hai",
        f"{other} ko {place} ki access de do",
        f"{doc} ko {adj} mat chhodo {name}",
        f"{thing} wala room band hai",
        f"{name} {event} ka blurb likhenge",
        f"{doc} print karo {rng.randint(2, 12)} copy",
        f"{event} ke notes {place} me rakho",
        f"{name} bina {doc} ke aa gaye",
        f"{other} keh rahe hain {event} ka scope badlo",
        f"{doc} par {name} ka comment aa gaya",
    ])
    return maybe_lower(rng, o_text(text), 0.6)


HX_GENERATORS = {
    "rel_day": gen_hx_rel_day,
    "calendar": gen_hx_calendar,
    "weekday": gen_hx_weekday,
    "weekday_list": gen_hx_weekday_list,
    "clocks": gen_hx_clocks,
    "deictic": gen_hx_deictic,
    "qualified": gen_hx_qualified,
    "ranges": gen_hx_ranges,
    "recur": gen_hx_recur,
    "ordinal": gen_hx_ordinal,
    "duration": gen_hx_duration,
    "holiday": gen_hx_holiday,
    "multi": gen_hx_multi,
    "prose": gen_hx_prose,
    "negative": gen_hx_negative,
}


# ---------------------------------------------------------------------------
# Driver
# ---------------------------------------------------------------------------


def distribute(total: int, weights: dict[str, int]) -> dict[str, int]:
    raw_total = sum(weights.values())
    raw = {k: total * v / raw_total for k, v in weights.items()}
    rounded = {k: int(v) for k, v in raw.items()}
    remainders = sorted(((raw[k] - rounded[k], k) for k in weights), reverse=True)
    missing = total - sum(rounded.values())
    for i in range(missing):
        rounded[remainders[i][1]] += 1
    return rounded


def fill(
    name: str,
    count: int,
    factory,
    rng: random.Random,
    seen: set[str],
    rows: list[dict],
) -> int:
    got = 0
    attempts = 0
    limit = max(count * 80, 4000)
    while got < count and attempts < limit:
        attempts += 1
        try:
            tokens = factory(rng)
        except Exception:
            continue
        if not tokens or len(tokens) > 40 or len(tokens) < 2:
            continue
        if name.endswith("negative") or name.split(":")[-1] == "negative":
            if any(label != "O" for _, label in tokens):
                continue
            if len(tokens) < 3:
                continue
        try:
            row = emit(tokens)
        except AssertionError:
            continue
        text = row["text"]
        lowered = text.lower()
        if any(phrase in lowered for phrase in RESERVED):
            continue
        if text in seen:
            continue
        seen.add(text)
        rows.append(row)
        got += 1
    return got


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", type=Path, default=OUT_DEFAULT)
    parser.add_argument("--seed", type=int, default=20260913)
    parser.add_argument("--lang", choices=["en", "hi", "hx", "all"], default="all")
    args = parser.parse_args()
    rng = random.Random(args.seed)
    seen: set[str] = set()
    rows: list[dict] = []
    report: dict[str, int] = {}

    oversample = 2
    jobs: list[tuple[str, int, object]] = []
    if args.lang in ("en", "all"):
        for family, n in EN_QUOTAS.items():
            jobs.append((f"en:{family}", n * oversample, EN_GENERATORS[family]))
    if args.lang in ("hi", "all"):
        for family, n in EN_QUOTAS.items():
            jobs.append((f"hi:{family}", n * oversample, HI_GENERATORS[family]))
    if args.lang in ("hx", "all"):
        for family, n in EN_QUOTAS.items():
            jobs.append((f"hx:{family}", n * oversample, HX_GENERATORS[family]))

    for name, count, factory in jobs:
        got = fill(name, count, factory, rng, seen, rows)
        report[name] = got
        if got < count:
            print(f"short {name}: {got}/{count}")

    args.out.parent.mkdir(parents=True, exist_ok=True)
    with args.out.open("w", encoding="utf-8") as sink:
        for row in rows:
            sink.write(json.dumps(row, ensure_ascii=False) + "\n")
    print(json.dumps({"wrote": len(rows), "path": str(args.out), "byFamily": report}, indent=2, ensure_ascii=False))
    expected = sum(n for _, n, _ in jobs)
    return 0 if len(rows) == expected else 1


if __name__ == "__main__":
    raise SystemExit(main())
