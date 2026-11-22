"""Recurring-schedule supervision: the phrasings people use for jobs,
reminders and habits, in English, Hindi and Hinglish.

The promoted carrier grammar covers "every Monday at 9" and its relatives.
This module adds what a scheduler hears next: sub-hourly steps and the clock
windows around them, counted steps ("every third day", "alternate
Saturdays"), days counted within a month, frequency counts in both word
orders, several rules in one sentence, and the Hindi and Hinglish forms of
each.

Labels come from this generator's structure, never from the runtime parser,
and follow the conventions of `generate.py` and `natural.py`: a quantity is
NUM (including both words of "half an"), the step word of "every other" is
NUM, an ordinal that counts is ORD, `every`/`each`/`per` are RECUR wherever
they occur, digits carry the role and their fused suffix is GLUE, and a
connector where a carrier meets its expression belongs to the carrier (O).
`natural.RESERVED` carriers are not used.
"""
from __future__ import annotations

import random

import background
from generate import DAYS, MONTHS, NUMBERS, Sentence

# --------------------------------------------------------------- vocabulary

MINUTE_STEPS = [1, 2, 3, 5, 10, 15, 20, 30, 45, 90]
HOUR_STEPS = [1, 2, 3, 4, 6, 8, 12]
DAY_STEPS = [2, 3, 4, 5, 7, 10, 14]
MINUTE_WORDS = ["minutes", "minutes", "mins", "min", "minute"]
HOUR_WORDS = ["hours", "hours", "hrs", "hour"]
STEP_ORDINALS = [("second", 2), ("third", 3), ("fourth", 4), ("fifth", 5)]
POSITION_ORDINALS = ["first", "second", "third", "fourth", "last"]
FREQUENCIES = [
    "hourly", "daily", "daily", "nightly", "nightly", "weekly", "weekly",
    "biweekly", "fortnightly", "monthly", "monthly", "quarterly", "quarterly",
    "yearly", "annually",
]
DAY_GROUPS = ["weekdays", "weekday", "weekends", "weekend"]
WORKING_DAYS = ["working day", "business day", "working days", "business days"]
DAY_PARTS = ["morning", "afternoon", "evening", "night"]

# Background words that sit next to a schedule and must stay O. Short task
# nouns are absent from the borrowed prose, so the corpus names them here.
TASKS = [
    "remind me to take my medicine", "take the tablets", "standup",
    "team sync", "backup runs", "run the backup", "run the report",
    "send the invoice", "water the plants", "check the server",
    "ping the endpoint", "sync the calendar", "pay rent", "pay the bill",
    "gym", "yoga class", "call mom", "call dad", "feed the cat",
    "refresh the cache", "rotate the logs", "post the update",
    "drink water", "stretch", "take a break", "review the queue",
    "deploy the build", "email the summary", "walk the dog",
    "poll the queue", "restart the worker", "summarize my inbox",
    "check the price", "fetch the feed", "clean the kitchen",
    "status report", "billing run", "salary credit", "book club",
    "the job runs", "schedule it", "trigger the workflow", "do this",
]
TRAILERS = [
    "please", "thanks", "if possible", "without fail", "and notify me",
    "and log the result", "for the team", "from now on", "as usual",
]
HINDI_TASKS = [
    "दवाई लेनी है", "पानी पीना है", "रिपोर्ट भेजनी है", "मीटिंग है",
    "किराया देना है", "जिम जाना है", "मम्मी को फ़ोन करना है", "याद दिलाना",
    "पौधों को पानी देना", "बिल भरना है", "टहलने जाना है", "योग करना है",
]
HINGLISH_TASKS = [
    "dawai leni hai", "paani peena hai", "report bhejni hai", "meeting hai",
    "rent dena hai", "gym jana hai", "mummy ko call karna hai",
    "yaad dilana", "paudhon ko paani dena", "bill bharna hai",
    "walk pe jana hai", "yoga karna hai", "backup chalana hai",
    "status check karna", "mujhe yaad dilao", "kaam karna hai",
]
HINDI_TRAILERS = ["याद दिला देना", "ज़रूर", "ठीक है", "करना है", "बता देना"]
HINGLISH_TRAILERS = ["yaad dila dena", "zaroor", "theek hai", "karna hai", "bata dena", "pls"]

# Sentences that use schedule vocabulary without naming a schedule.
NEGATIVES = [
    "every single one of them agreed",
    "each of us has a second opinion",
    "the alternate route is closed",
    "this is my third attempt",
    "the working title changed again",
    "summer was hot and winter was cold",
    "the quarterly report looks fine",
    "a weekly magazine sat on the table",
    "nobody read the daily newspaper",
    "the last page is missing",
    "he came first and she came second",
    "a single ticket costs 40 rupees",
    "the minute hand is broken",
    "we lost by 10 seconds",
    "half the team is new",
    "an hour is a unit of time",
    "the night sky was clear",
    "business is slow",
    "the other option is cheaper",
    "the nightly build failed with 3 errors",
    "version 15 has 30 open issues",
    "ghar mein teen kamre hain",
    "do log aaye the",
    "ek baar phir try karo",
    "yeh doosra mauka hai",
    "घर में तीन कमरे हैं",
    "दो लोग आए थे",
    "एक बार फिर कोशिश करो",
    "यह दूसरा मौका है",
]

# --- Hindi / Hinglish
HAR = ["har", "har", "हर", "hr"]
ROMAN_DAYS = [
    ["somvar", "somvaar", "somwar", "somwaar"],
    ["mangalwar", "mangalvaar", "mangalvar", "mangal"],
    ["budhwar", "budhvaar", "budhvar", "budh"],
    ["guruwar", "guruvaar", "guruvar"],
    ["shukrawar", "shukravaar", "shukravar"],
    ["shanivar", "shanivaar", "shaniwar"],
    ["ravivar", "ravivaar", "raviwar", "itwar"],
]
DEVANAGARI_DAYS = ["सोमवार", "मंगलवार", "बुधवार", "गुरुवार", "शुक्रवार", "शनिवार", "रविवार"]
ROMAN_NUMBERS = {1: "ek", 2: "do", 3: "teen", 4: "char", 5: "paanch", 6: "chhe",
                 7: "saat", 8: "aath", 9: "nau", 10: "das", 11: "gyarah", 12: "barah"}
DEVANAGARI_NUMBERS = {1: "एक", 2: "दो", 3: "तीन", 4: "चार", 5: "पाँच", 6: "छह",
                      7: "सात", 8: "आठ", 9: "नौ", 10: "दस", 11: "ग्यारह", 12: "बारह"}
ROMAN_ORDINALS = [("doosre", 2), ("dusre", 2), ("teesre", 3), ("chauthe", 4)]
DEVANAGARI_ORDINALS = [("दूसरे", 2), ("तीसरे", 3), ("चौथे", 4)]
ROMAN_UNITS = {"minute": ["minute", "min", "minat"], "hour": ["ghante", "ghanta"],
               "day": ["din"], "week": ["hafte", "hafta"],
               "month": ["mahine", "mahina"], "year": ["saal"]}
DEVANAGARI_UNITS = {"minute": ["मिनट"], "hour": ["घंटे", "घंटा"], "day": ["दिन"],
                    "week": ["हफ़्ते", "हफ्ते"], "month": ["महीने"], "year": ["साल"]}
ROMAN_PARTS = ["subah", "dopahar", "shaam", "raat"]
DEVANAGARI_PARTS = ["सुबह", "दोपहर", "शाम", "रात"]
ROMAN_DAILY = ["roz", "roz", "rozana", "har roz", "har din", "pratidin"]
DEVANAGARI_DAILY = ["रोज़", "रोज", "रोज़ाना", "हर रोज़", "हर दिन", "प्रतिदिन"]
ROMAN_MONTHS = ["january", "february", "march", "april", "may", "june", "july",
                "august", "september", "october", "november", "december"]
DEVANAGARI_MONTHS = ["जनवरी", "फरवरी", "मार्च", "अप्रैल", "मई", "जून", "जुलाई",
                     "अगस्त", "सितंबर", "अक्टूबर", "नवंबर", "दिसंबर"]
TARIKH = ["tarikh", "tareekh", "tareek", "taareekh"]


# ------------------------------------------------------------------ helpers

def suffix_of(value: int) -> str:
    if value % 100 in (11, 12, 13):
        return "th"
    return {1: "st", 2: "nd", 3: "rd"}.get(value % 10, "th")


def digits_ordinal(s: Sentence, value: int, label: str) -> None:
    s.add(str(value), label)
    s.add(suffix_of(value), "GLUE", separator="")


def number(s: Sentence, value: int, label: str = "NUM") -> None:
    spoken = value <= 12 and s.rng.random() < 0.3
    s.add(NUMBERS[value] if spoken else str(value), label)


def weekday(s: Sentence, plural: float = 0.25) -> None:
    name = s.rng.choice(DAYS)
    form = s.rng.choice([name, name, name.lower(), name[:3], name[:3].lower()])
    if s.rng.random() < plural:
        form = name + "s"
    s.add(form, "WEEKDAY")


def weekdays(s: Sentence, plural: float = 0.25, limit: int = 3) -> None:
    count = s.rng.choices([1, 2, 3], weights=[6, 3, 1])[0]
    count = min(count, limit)
    for index in range(count):
        if index:
            last = index == count - 1
            s.add("and" if last and s.rng.random() < 0.7 else ",", "JOIN",
                  separator=" " if last else "")
        weekday(s, plural)


def english_clock(s: Sentence, short: bool = False) -> None:
    """A clock a scheduler would be given: no seconds."""
    s.clock(s.rng.choice([2, 2, 2, 3, 3, 5, 7] if not short else [2, 2, 3, 7]))


def at_clock(s: Sentence) -> None:
    s.add("at")
    english_clock(s)


def day_selector(s: Sentence) -> None:
    """ "on weekdays", "on Mondays and Thursdays", "on weekends". """
    kind = s.rng.random()
    if kind < 0.45:
        s.add("on")
        s.add(s.rng.choice(DAY_GROUPS), "DAYGROUP")
    elif kind < 0.6:
        s.add("on")
        s.add(s.rng.choice(WORKING_DAYS), "DAYGROUP")
    else:
        s.add("on")
        weekdays(s, plural=0.5)


def english_window(s: Sentence) -> None:
    kind = s.rng.random()
    if kind < 0.75:
        s.window(s.rng.randrange(9))
    elif kind < 0.9:
        s.add(s.rng.choice(["in the", "during the", "every"]), "GLUE")
        s.add(s.rng.choice(DAY_PARTS), "DAYPART")
    else:
        s.add(s.rng.choice(["after", "from"]), "DIR_AFTER")
        english_clock(s, short=True)


def hindi_clock(s: Sentence, devanagari: bool) -> None:
    rng = s.rng
    parts = DEVANAGARI_PARTS if devanagari else ROMAN_PARTS
    numbers = DEVANAGARI_NUMBERS if devanagari else ROMAN_NUMBERS
    if rng.random() < 0.65:
        s.add(rng.choice(parts), "DAYPART")
        if rng.random() < 0.4:
            s.add("को" if devanagari else "ko", "GLUE")
    hour = rng.randint(1, 12)
    s.add(numbers[hour] if rng.random() < 0.35 else str(hour), "HOUR")
    if rng.random() < 0.25:
        s.add(":", "GLUE", separator="")
        s.add(rng.choice(["15", "30", "45"]), "MINUTE", separator="")
    s.add("बजे" if devanagari else rng.choice(["baje", "baje", "bje"]), "MERIDIEM")


def hindi_weekday(s: Sentence, devanagari: bool) -> None:
    rng = s.rng
    index = rng.randrange(7)
    if devanagari:
        s.add(DEVANAGARI_DAYS[index], "WEEKDAY")
    elif rng.random() < 0.3:
        s.add(rng.choice([DAYS[index], DAYS[index].lower()]), "WEEKDAY")
    else:
        s.add(rng.choice(ROMAN_DAYS[index]), "WEEKDAY")


def hindi_weekdays(s: Sentence, devanagari: bool) -> None:
    count = s.rng.choices([1, 2, 3], weights=[6, 3, 1])[0]
    for index in range(count):
        if index:
            s.add("और" if devanagari else s.rng.choice(["aur", "aur", ","]), "JOIN")
        hindi_weekday(s, devanagari)
    if s.rng.random() < 0.6:
        s.add("को" if devanagari else "ko", "GLUE")


def har(s: Sentence, devanagari: bool) -> None:
    s.add("हर" if devanagari else s.rng.choice(HAR), "RECUR")


def hindi_unit(s: Sentence, name: str, devanagari: bool) -> None:
    table = DEVANAGARI_UNITS if devanagari else ROMAN_UNITS
    s.add(s.rng.choice(table[name]), "UNIT")


def hindi_number(s: Sentence, value: int, devanagari: bool, label: str = "NUM") -> None:
    table = DEVANAGARI_NUMBERS if devanagari else ROMAN_NUMBERS
    spoken = value in table and s.rng.random() < 0.55
    s.add(table[value] if spoken else str(value), label)


# ----------------------------------------------------------------- families

def sub_hourly(s: Sentence) -> None:
    """every 15 minutes [between 9am and 6pm] [on weekdays]"""
    rng = s.rng
    s.add(rng.choice(["every", "every", "every", "each"]), "RECUR")
    shape = rng.random()
    if shape < 0.5:
        value = rng.choice(MINUTE_STEPS)
        if value == 1 and rng.random() < 0.6:
            s.add("minute", "UNIT")
        else:
            number(s, value)
            s.add("minute" if value == 1 else rng.choice(MINUTE_WORDS), "UNIT")
    elif shape < 0.62:
        s.add("half", "NUM")
        if rng.random() < 0.5:
            s.add(rng.choice(["an", "a"]), "NUM")
        s.add("hour", "UNIT")
    elif shape < 0.68:
        number(s, rng.choice([5, 10, 15, 30, 45]))
        s.add(rng.choice(["seconds", "secs", "second"]), "UNIT")
    else:
        value = rng.choice(HOUR_STEPS)
        if value == 1 and rng.random() < 0.7:
            s.add("hour", "UNIT")
        else:
            number(s, value)
            s.add("hour" if value == 1 else rng.choice(HOUR_WORDS), "UNIT")
    extras = rng.random()
    if extras < 0.35:
        english_window(s)
        if rng.random() < 0.5:
            day_selector(s)
    elif extras < 0.5:
        day_selector(s)
        if rng.random() < 0.6:
            english_window(s)
    elif extras < 0.6:
        s.add("for", "DUR")
        number(s, rng.choice([1, 2, 3, 6, 12, 24]))
        s.add(rng.choice(["hours", "days", "hrs"]), "UNIT")
    elif extras < 0.68:
        s.add(rng.choice(["until", "till"]), "BOUND_END")
        weekday(s, plural=0)


def counted_step(s: Sentence) -> None:
    """every other day, every third week, alternate Saturdays, every 3 days"""
    rng = s.rng
    shape = rng.random()
    unit = rng.choice(["day", "day", "week", "week", "month", "year"])
    if shape < 0.22:
        s.add("every", "RECUR")
        s.add(rng.choice(["other", "other", "alternate"]), "NUM")
        if rng.random() < 0.55:
            s.add(unit, "UNIT")
        else:
            weekday(s, plural=0)
    elif shape < 0.34:
        s.add(rng.choice(["alternate", "alternating", "alternate"]), "RECUR")
        if rng.random() < 0.6:
            weekday(s, plural=0.8)
        else:
            s.add(unit + "s", "UNIT")
    elif shape < 0.56:
        s.add("every", "RECUR")
        if rng.random() < 0.6:
            word, _ = rng.choice(STEP_ORDINALS)
            s.add(word, "ORD")
        else:
            digits_ordinal(s, rng.choice([2, 3, 4, 5]), "ORD")
        s.add(unit, "UNIT")
    else:
        s.add(rng.choice(["every", "every", "each"]), "RECUR")
        if rng.random() < 0.15:
            # "every quarter": the calendar period, not "quarter past"
            s.add("quarter", "UNIT")
            if rng.random() < 0.5:
                at_clock(s)
            return
        number(s, rng.choice(DAY_STEPS))
        s.add(unit + "s", "UNIT")
    tail = rng.random()
    if unit in ("week",) and tail < 0.4:
        s.add("on")
        weekdays(s, plural=0.3, limit=2)
    if rng.random() < 0.5:
        at_clock(s)


def weekday_of_month(s: Sentence) -> None:
    """first Monday of every month, every 2nd Saturday, last working day"""
    rng = s.rng

    def position() -> None:
        if rng.random() < 0.6:
            s.add(rng.choice(POSITION_ORDINALS), "ORD")
        else:
            digits_ordinal(s, rng.randint(1, 4), "ORD")

    def target() -> None:
        if rng.random() < 0.82:
            weekday(s, plural=0)
        else:
            s.add(rng.choice(["working day", "business day", "weekday"]), "DAYGROUP")

    shape = rng.random()
    if shape < 0.4:
        if rng.random() < 0.5:
            s.add(rng.choice(["the", "on the"]))
        position()
        if rng.random() < 0.3:
            s.add("and", "GLUE")
            position()
        target()
        s.add("of")
        s.add(rng.choice(["every", "each", "every"]), "RECUR")
        s.add("month", "UNIT")
    elif shape < 0.65:
        s.add(rng.choice(["every", "each"]), "RECUR")
        position()
        target()
        if rng.random() < 0.6:
            s.add("of")
            s.add(rng.choice(["the", "each", "every"]), "GLUE")
            s.add("month", "UNIT")
    elif shape < 0.82:
        # "every month on the first Monday" / "monthly on the last Friday"
        if rng.random() < 0.35:
            s.add("monthly", "FREQ")
        else:
            s.add(rng.choice(["every", "each"]), "RECUR")
            s.add("month", "UNIT")
        s.add(rng.choice(["on the", "on"]))
        position()
        target()
    else:
        # "last Friday of the month": the ordinal alone makes it recur.
        position()
        target()
        s.add("of")
        if rng.random() < 0.5:
            s.add("the", "GLUE")
        else:
            s.add("each", "RECUR")
        s.add("month", "UNIT")
    if rng.random() < 0.5:
        at_clock(s)


def day_of_month(s: Sentence) -> None:
    """last day of every month, the 1st and 15th of each month, every 15th"""
    rng = s.rng

    def day(label: str = "DOM") -> None:
        digits_ordinal(s, rng.choice([1, 1, 5, 10, 15, 15, 20, 25, 28, rng.randint(1, 28)]), label)

    shape = rng.random()
    if shape < 0.25:
        if rng.random() < 0.5:
            s.add(rng.choice(["the", "on the"]))
        s.add(rng.choice(["last", "last", "first"]), "ORD")
        s.add("day", "UNIT")
        s.add("of")
        s.add(rng.choice(["every", "each", "every"]), "RECUR")
        s.add(rng.choice(["month", "month", "month", "year"]), "UNIT")
    elif shape < 0.5:
        s.add(rng.choice(["on the", "the"]))
        day()
        if rng.random() < 0.35:
            s.add("and", "GLUE")
            day()
        s.add("of")
        s.add(rng.choice(["every", "each"]), "RECUR")
        s.add("month", "UNIT")
    elif shape < 0.7:
        s.add(rng.choice(["every", "each"]), "RECUR")
        s.add("month", "UNIT")
        s.add(rng.choice(["on the", "on"]))
        day()
        if rng.random() < 0.3:
            s.add("and", "GLUE")
            day()
    elif shape < 0.85:
        s.add(rng.choice(["monthly", "quarterly"]), "FREQ")
        s.add(rng.choice(["on the", "on"]))
        day()
    else:
        s.add("every", "RECUR")
        day()
        if rng.random() < 0.4:
            s.add("of")
            s.add("the", "GLUE")
            s.add("month", "UNIT")
    if rng.random() < 0.45:
        at_clock(s)


def yearly(s: Sentence) -> None:
    """every year on March 3, every January, annually on 25 December"""
    rng = s.rng
    month = rng.choice(MONTHS)
    month = rng.choice([month, month, month.lower(), month[:3]])
    shape = rng.random()
    if shape < 0.3:
        s.add(rng.choice(["every", "each"]), "RECUR")
        s.add("year", "UNIT")
        s.add(rng.choice(["on", "in"]))
        s.add(month, "MONTH")
        if rng.random() < 0.7:
            if rng.random() < 0.5:
                digits_ordinal(s, rng.randint(1, 28), "DOM")
            else:
                s.add(str(rng.randint(1, 28)), "DOM")
    elif shape < 0.55:
        s.add(rng.choice(["yearly", "annually", "annually"]), "FREQ")
        s.add("on")
        if rng.random() < 0.5:
            s.add(str(rng.randint(1, 28)), "DOM")
            s.add(month, "MONTH")
        else:
            s.add(month, "MONTH")
            s.add(str(rng.randint(1, 28)), "DOM")
    elif shape < 0.8:
        s.add(rng.choice(["every", "each"]), "RECUR")
        s.add(month, "MONTH")
        if rng.random() < 0.5:
            digits_ordinal(s, rng.randint(1, 28), "DOM")
    else:
        s.add(rng.choice(["every", "each"]), "RECUR")
        number(s, rng.choice([2, 3, 6]))
        s.add("months", "UNIT")
        if rng.random() < 0.5:
            s.add("on the")
            digits_ordinal(s, rng.randint(1, 28), "DOM")
    if rng.random() < 0.3:
        at_clock(s)


def frequency_count(s: Sentence) -> None:
    """twice a day, 3 times a week, twice daily, twice a day at 9am and 9pm"""
    rng = s.rng
    unit = rng.choice(["day", "day", "week", "week", "month", "year"])
    count = None
    if rng.random() < 0.55:
        word = rng.choice(["once", "twice", "twice", "thrice"])
        count = {"once": 1, "twice": 2, "thrice": 3}[word]
        s.add(word, "TIMES")
    else:
        count = rng.randint(2, 5)
        number(s, count)
        s.add("times", "TIMES")
    if rng.random() < 0.25:
        s.add({"day": "daily", "week": "weekly", "month": "monthly", "year": "yearly"}[unit],
              "FREQ")
    else:
        word = rng.choice(["a", "a", "per", "every", "each"])
        s.add(word, "GLUE" if word == "a" else "RECUR")
        s.add(unit, "UNIT")
    if unit == "day" and count in (2, 3) and rng.random() < 0.45:
        s.add("at")
        for index in range(count):
            if index:
                s.add("and" if index == count - 1 else ",", "JOIN",
                      separator=" " if index == count - 1 else "")
            s.clock(2)
    elif unit == "week" and rng.random() < 0.3:
        s.add("on")
        weekdays(s, plural=0.4)


def clock_list(s: Sentence) -> None:
    """every day at 8am, 1pm and 8pm"""
    rng = s.rng
    lead = rng.random()
    if lead < 0.4:
        s.add(rng.choice(["every", "each"]), "RECUR")
        s.add("day", "UNIT")
    elif lead < 0.6:
        s.add("daily", "FREQ")
    elif lead < 0.8:
        s.add(rng.choice(["every", "each"]), "RECUR")
        s.add(rng.choice(DAY_GROUPS), "DAYGROUP")
    else:
        s.add(rng.choice(["every", "each"]), "RECUR")
        weekday(s, plural=0)
    s.add("at")
    count = rng.choice([2, 2, 3])
    for index in range(count):
        if index:
            s.add("and" if index == count - 1 else ",", "JOIN",
                  separator=" " if index == count - 1 else "")
        s.clock(rng.choice([2, 2, 3]))


def several_rules(s: Sentence) -> None:
    """every Tuesday at 3pm and every Friday at 5pm"""
    rng = s.rng
    count = rng.choice([2, 2, 3])
    for index in range(count):
        if index:
            s.add(rng.choice(["and", "and", ",", "plus", "and also"]), "JOIN")
            s.clause()
        kind = rng.random()
        if kind < 0.5:
            s.add(rng.choice(["every", "each"]), "RECUR")
            weekdays(s, plural=0, limit=2)
        elif kind < 0.7:
            s.add(rng.choice(["every", "each"]), "RECUR")
            s.add(rng.choice(DAY_GROUPS), "DAYGROUP")
        elif kind < 0.85:
            s.add(rng.choice(["every", "each"]), "RECUR")
            s.add("day", "UNIT")
        else:
            s.add(rng.choice(["every", "each"]), "RECUR")
            number(s, rng.choice([15, 30, 2, 3]))
            s.add(rng.choice(["minutes", "hours"]), "UNIT")
            continue
        if rng.random() < 0.85:
            at_clock(s)


def period(s: Sentence, choices: list[str]) -> None:
    """A unit or a day part after "every", each under its own role."""
    word = s.rng.choice(choices)
    s.add(word, "DAYPART" if word in DAY_PARTS else "UNIT")


def frequency_word(s: Sentence) -> None:
    """nightly at 2am, quarterly on the 1st, 9am daily, every single day"""
    rng = s.rng
    shape = rng.random()
    if shape < 0.4:
        word = rng.choice(FREQUENCIES)
        s.add(word, "FREQ")
        if word in ("weekly", "biweekly", "fortnightly") and rng.random() < 0.5:
            s.add("on")
            weekdays(s, plural=0.5, limit=2)
        elif word in ("monthly", "quarterly") and rng.random() < 0.5:
            s.add(rng.choice(["on the", "on"]))
            digits_ordinal(s, rng.choice([1, 5, 15, 28]), "DOM")
        elif word == "hourly" and rng.random() < 0.5:
            day_selector(s)
        if rng.random() < 0.6:
            at_clock(s)
    elif shape < 0.6:
        english_clock(s, short=True)
        s.add(rng.choice(FREQUENCIES[1:7]), "FREQ")
    elif shape < 0.8:
        s.add("every", "RECUR")
        s.add(rng.choice(["single", "single", "damn"]), "GLUE")
        period(s, ["day", "week", "morning", "night"])
        if rng.random() < 0.5:
            at_clock(s)
    else:
        s.add("at")
        english_clock(s, short=True)
        s.add(rng.choice(["every", "each"]), "RECUR")
        period(s, ["day", "night", "morning", "evening"])


def exceptions(s: Sentence) -> None:
    """weekdays except Wednesday at 9am, every day except Sundays"""
    rng = s.rng
    lead = rng.random()
    if lead < 0.4:
        s.add(rng.choice(["every", "each"]), "RECUR")
        s.add("day", "UNIT")
    elif lead < 0.75:
        if rng.random() < 0.5:
            s.add(rng.choice(["every", "each"]), "RECUR")
        s.add(rng.choice(["weekdays", "weekday"]), "DAYGROUP")
    else:
        s.add("daily", "FREQ")
    s.add(rng.choice(["except", "except", "excluding", "but not", "except on"]), "EXCEPT")
    weekdays(s, plural=0.4, limit=2)
    if rng.random() < 0.7:
        at_clock(s)


def plural_days(s: Sentence) -> None:
    """Mondays and Thursdays at 7pm, on Fridays, all Saturdays"""
    rng = s.rng
    lead = rng.random()
    if lead < 0.3:
        s.add("on")
    elif lead < 0.4:
        s.add("all", "GLUE")
    weekdays(s, plural=1.0)
    if rng.random() < 0.75:
        at_clock(s)


def hindi_step(s: Sentence, devanagari: bool) -> None:
    """har 15 minute mein, har do ghante, har teesre din, har aadhe ghante"""
    rng = s.rng
    har(s, devanagari)
    shape = rng.random()
    if shape < 0.3:
        hindi_number(s, rng.choice([5, 10, 15, 20, 30, 45]), devanagari)
        hindi_unit(s, "minute", devanagari)
    elif shape < 0.55:
        value = rng.choice([1, 2, 3, 4, 6, 8, 12])
        if value == 1 and rng.random() < 0.6:
            hindi_unit(s, "hour", devanagari)
        else:
            hindi_number(s, value, devanagari)
            hindi_unit(s, "hour", devanagari)
    elif shape < 0.65:
        s.add("आधे" if devanagari else rng.choice(["aadhe", "aadha"]), "NUM")
        hindi_unit(s, "hour", devanagari)
    elif shape < 0.85:
        word, _ = rng.choice(DEVANAGARI_ORDINALS if devanagari else ROMAN_ORDINALS)
        s.add(word, "ORD")
        hindi_unit(s, rng.choice(["day", "day", "week", "month"]), devanagari)
    else:
        hindi_number(s, rng.choice([2, 3, 4, 5, 7, 10, 15]), devanagari)
        hindi_unit(s, rng.choice(["day", "day", "week", "month"]), devanagari)
    if rng.random() < 0.55:
        s.add("में" if devanagari else rng.choice(["mein", "me", "mein", "baad"]), "GLUE")
    if shape < 0.65 and rng.random() < 0.35:
        # subah 9 baje se shaam 6 baje tak
        hindi_clock(s, devanagari)
        s.add("से" if devanagari else "se", "RANGE_START")
        hindi_clock(s, devanagari)
        s.add("तक" if devanagari else "tak", "RANGE_END")
    elif shape >= 0.65 and rng.random() < 0.4:
        hindi_clock(s, devanagari)


def hindi_weekly(s: Sentence, devanagari: bool) -> None:
    """har somvar ko, har mangalwar aur shukrawar ko shaam 6 baje"""
    rng = s.rng
    har(s, devanagari)
    if rng.random() < 0.25:
        hindi_unit(s, "week", devanagari)
    hindi_weekdays(s, devanagari)
    if rng.random() < 0.6:
        hindi_clock(s, devanagari)


def hindi_daily(s: Sentence, devanagari: bool) -> None:
    """roz subah 7 baje, har raat 11 baje, rozana, pratidin subah 6 baje"""
    rng = s.rng
    if rng.random() < 0.6:
        for piece in rng.choice(DEVANAGARI_DAILY if devanagari else ROMAN_DAILY).split():
            unit = piece in ("din", "दिन")
            s.add(piece, "UNIT" if unit else "RECUR")
        if rng.random() < 0.8:
            hindi_clock(s, devanagari)
    else:
        har(s, devanagari)
        s.add(rng.choice(DEVANAGARI_PARTS if devanagari else ROMAN_PARTS), "DAYPART")
        if rng.random() < 0.7:
            hour = rng.randint(1, 12)
            s.add(str(hour), "HOUR")
            s.add("बजे" if devanagari else "baje", "MERIDIEM")


def hindi_monthly(s: Sentence, devanagari: bool) -> None:
    """har mahine ki 5 tarikh ko, har mahine ki pehli tarikh, har saal 15 august ko"""
    rng = s.rng
    har(s, devanagari)
    shape = rng.random()
    if shape < 0.6:
        hindi_unit(s, "month", devanagari)
        s.add("की" if devanagari else "ki", "GLUE")
        if rng.random() < 0.3:
            ordinal = rng.choice(
                [("पहली", 1), ("आख़िरी", -1), ("आखिरी", -1)] if devanagari
                else [("pehli", 1), ("aakhri", -1), ("aakhiri", -1)]
            )
            s.add(ordinal[0], "ORD")
        else:
            s.add(str(rng.randint(1, 28)), "DOM")
            if rng.random() < 0.25:
                s.add("और" if devanagari else "aur", "GLUE")
                s.add(str(rng.randint(1, 28)), "DOM")
        s.add("तारीख" if devanagari else rng.choice(TARIKH), "UNIT")
    else:
        hindi_unit(s, "year", devanagari)
        s.add(str(rng.randint(1, 28)), "DOM")
        index = rng.randrange(12)
        s.add(DEVANAGARI_MONTHS[index] if devanagari else ROMAN_MONTHS[index], "MONTH")
    if rng.random() < 0.6:
        s.add("को" if devanagari else "ko", "GLUE")
    if rng.random() < 0.35:
        hindi_clock(s, devanagari)


def hindi_count(s: Sentence, devanagari: bool) -> None:
    """din mein do baar, hafte mein teen baar, mahine mein ek baar"""
    rng = s.rng
    if rng.random() < 0.25:
        har(s, devanagari)
    hindi_unit(s, rng.choice(["day", "day", "week", "week", "month", "year"]), devanagari)
    s.add("में" if devanagari else rng.choice(["mein", "me", "mein"]), "GLUE")
    hindi_number(s, rng.choice([1, 2, 2, 3, 3, 4]), devanagari)
    s.add("बार" if devanagari else rng.choice(["baar", "baar", "bar"]), "TIMES")


def hindi_group(s: Sentence, devanagari: bool) -> None:
    """har weekday subah 9 baje, har weekend"""
    rng = s.rng
    har(s, devanagari)
    s.add(rng.choice(["weekday", "weekdays", "weekend", "weekends"]), "DAYGROUP")
    if rng.random() < 0.3:
        s.add("पर" if devanagari else rng.choice(["pe", "par", "ko"]), "GLUE")
    if rng.random() < 0.7:
        hindi_clock(s, devanagari)


ENGLISH = [
    (sub_hourly, 20),
    (counted_step, 12),
    (weekday_of_month, 10),
    (day_of_month, 9),
    (yearly, 6),
    (frequency_count, 9),
    (clock_list, 6),
    (several_rules, 9),
    (frequency_word, 10),
    (exceptions, 4),
    (plural_days, 5),
]
HINDI = [
    (hindi_step, 30),
    (hindi_weekly, 22),
    (hindi_daily, 14),
    (hindi_monthly, 14),
    (hindi_count, 14),
    (hindi_group, 6),
]


def pick(rng: random.Random, table):
    return rng.choices([entry[0] for entry in table], weights=[entry[1] for entry in table])[0]


def render(rng: random.Random) -> Sentence:
    sentence = Sentence(rng)
    roll = rng.random()
    if roll < 0.06:
        sentence.add(rng.choice(NEGATIVES))
        return sentence
    language = "english" if roll < 0.58 else "hinglish" if roll < 0.82 else "hindi"
    devanagari = language == "hindi"
    tasks = {"english": TASKS, "hinglish": HINGLISH_TASKS, "hindi": HINDI_TASKS}[language]
    trailers = {
        "english": TRAILERS, "hinglish": HINGLISH_TRAILERS, "hindi": HINDI_TRAILERS,
    }[language]

    lead = rng.random()
    carried = lead < 0.4 or (lead < 0.5 and language == "english")
    if lead < 0.4:
        sentence.add(rng.choice(tasks))
    elif carried:
        sentence.add(background.prefix(rng))
    sentence.clause()
    first = len(sentence.spans)
    if language == "english":
        pick(rng, ENGLISH)(sentence)
    else:
        pick(rng, HINDI)(sentence, devanagari)
    if carried:
        # "remind me at 9", "standup on weekdays": where a carrier meets its
        # expression, the connector is the carrier's, as in generate.py.
        for span in sentence.spans[first:]:
            if span["label"] != "GLUE":
                break
            span["label"] = "O"
    tail = rng.random()
    if tail < 0.25 and lead >= 0.4:
        # The task can follow its schedule: "every 15 minutes check the server".
        sentence.in_expression = False
        sentence.add(rng.choice(tasks))
    elif tail < 0.4:
        sentence.in_expression = False
        sentence.add(rng.choice(trailers))
    return sentence
