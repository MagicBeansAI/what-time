"""Balanced festival-vocabulary + drift-protection batch.

Lesson (MINING.md): narrow supplements knock neighbouring vocabulary
loose across retrains. This batch trains the festival words AND
positively reinforces every token that drifted in earlier attempts —
bare weekdays in all recurrence contexts, month abbreviations, "har",
common carriers, and the Good-Friday contrast — so one retrain can move
the whole set together. Labels authored here, never taken from runtime
predictions.
"""

import json
import random

FESTIVALS_SINGLE = [
    "Holi", "holi", "होली", "Rakhi", "राखी", "राखड़ी", "Dussehra", "दशहरा", "दशहरे",
    "Eid", "ईद", "Bakrid", "बकरीद", "Mahashivratri", "महाशिवरात्रि", "Diwali",
    "दिवाली", "दीपावली", "Thanksgiving", "Halloween", "Juneteenth", "Easter",
]
FESTIVALS_MULTI = [
    ["Raksha", "Bandhan"], ["रक्षा", "बंधन"], ["Ganesh", "Chaturthi"],
    ["गणेश", "चतुर्थी"], ["Karwa", "Chauth"], ["karwa", "chauth"],
    ["करवा", "चौथ"], ["Good", "Friday"], ["good", "friday"],
    ["Boxing", "Day"], ["boxing", "day"], ["Gandhi", "Jayanti"],
    ["गांधी", "जयंती"], ["gandhi", "jayanti"], ["Memorial", "Day"],
    ["Labor", "Day"], ["Columbus", "Day"], ["Veterans", "Day"],
    ["Canada", "Day"], ["Anzac", "Day"], ["Mothering", "Sunday"],
]
CARRIERS = [
    ["meeting"], ["family", "dinner"], ["school", "closed"], ["office", "shut"],
    ["party"], ["brunch"], ["puja"], ["lunch"], ["travel", "plans"], ["deadline"],
    ["gym"], ["clinic"], ["review"], ["fast"], ["celebration"],
]
TAILS_EN = [["please"], ["works", "for", "me"], ["let's", "plan"], ["ok"], ["as", "discussed"]]
TAILS_HI = [["को", "मिलते", "हैं"], ["को", "छुट्टी", "है"], ["पर", "मिलते", "हैं"],
            ["की", "छुट्टी", "है"], ["पर", "छुट्टी", "है"], ["पर", "बंद", "है"],
            ["पे", "मिलते", "हैं"], ["को", "बंद", "रखना", "है"]]
CLOCKS = [["at", "9", "am"], ["at", "noon"], ["at", "6", "pm"], ["शाम", "को", "6", "बजे"]]

WEEKDAYS = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday",
            "mon", "tue", "wed", "thu", "fri", "sat", "sun"]
MONTHS = ["dec", "jan", "feb", "mar", "apr", "jun", "jul", "sep", "nov"]


def holiday_tokens(entry):
    if isinstance(entry, list):
        return [(w, "HOLIDAY") for w in entry]
    return [(entry, "HOLIDAY")]


def clock_tokens(clock):
    out = []
    for w in clock:
        if w in ("at", "को"):
            out.append((w, "GLUE"))
        elif w in ("am", "pm", "बजे"):
            out.append((w, "MERIDIEM"))
        elif w == "noon":
            out.append((w, "TIME_NAMED"))
        elif w == "शाम":
            out.append((w, "DAYPART"))
        else:
            out.append((w, "HOUR"))
    return out


def festival_rows(rng, rows, seen):
    for _ in range(30000):
        holiday = rng.choice(FESTIVALS_SINGLE + FESTIVALS_MULTI)
        is_hindi = isinstance(holiday, str) and any(ord(c) > 0x900 for c in holiday)
        tokens = []
        shape = rng.random()
        if shape < 0.3:
            tokens += [(w, "O") for w in rng.choice(CARRIERS)]
            tokens += holiday_tokens(holiday)
        elif shape < 0.5:
            tokens.append((rng.choice(["on", "by", "before", "after"]), "GLUE"))
            tokens += holiday_tokens(holiday)
            tokens += [(w, "O") for w in rng.choice(TAILS_HI if is_hindi else TAILS_EN)]
        elif shape < 0.65:
            tokens += holiday_tokens(holiday)
            tokens += [(w, "O") for w in rng.choice(TAILS_HI if is_hindi else TAILS_EN)]
        else:
            tokens += holiday_tokens(holiday)
            tokens += clock_tokens(rng.choice(CLOCKS))
        text = " ".join(t for t, _ in tokens)
        if text in seen:
            continue
        seen.add(text)
        rows.append({"text": text, "tokens": [list(p) for p in tokens]})


def protection_rows(rng, rows, seen):
    """Reinforce the labels that past narrow batches knocked loose."""
    def add(tokens):
        text = " ".join(t for t, _ in tokens)
        if text in seen:
            return
        seen.add(text)
        rows.append({"text": text, "tokens": [list(p) for p in tokens]})

    # bare weekdays across recurrence contexts
    for day in WEEKDAYS:
        for opener, labels in [
            (["every"], ["RECUR"]), (["every", "other"], ["RECUR", "GLUE"]),
            (["each"], ["RECUR"]), (["alternate"], ["RECUR"]),
        ]:
            add([(w, l) for w, l in zip(opener, labels)] + [(day, "WEEKDAY")])
        add([(day, "WEEKDAY"), ("next", "DEICTIC"), ("week", "UNIT")])
        add([("this", "DEICTIC"), (day.lower(), "WEEKDAY")])
        add([("gym", "O"), ("on", "GLUE"), (day, "WEEKDAY")])
        add([("meeting", "O"), ("on", "GLUE"), (day, "WEEKDAY"), ("at", "GLUE"), ("9", "HOUR"), ("am", "MERIDIEM")])
    # every weekday/weekend except <weekday>
    for day in WEEKDAYS:
        for group in ("weekday", "weekend"):
            add([("every", "RECUR"), (group, "DAYGROUP"), ("except", "EXCEPT"), (day, "WEEKDAY")])
    # month abbreviations and "until <month>"
    for month in MONTHS:
        add([("every", "RECUR"), ("tuesday", "WEEKDAY"), ("until", "BOUND_END"), (month, "MONTH")])
        add([("trip", "O"), ("in", "GLUE"), (month, "MONTH")])
        add([(month, "MONTH"), ("15", "DOM")])
    # "har" is always a recurrence marker
    for tail in (["hafte", "Tuesday", "ko", "gym"], ["mahine", "ki", "5", "tareek", "ko"],
                 ["din", "8", "baje"], ["roz", "subah"]):
        tokens = [("har", "RECUR")]
        for w in tail:
            if w in ("hafte", "mahine", "din", "roz"):
                tokens.append((w, "UNIT"))
            elif w in ("Tuesday",):
                tokens.append((w, "WEEKDAY"))
            elif w.isdigit():
                tokens.append((w, "DOM"))
            elif w in ("ko",):
                tokens.append((w, "GLUE"))
            elif w in ("tareek",):
                tokens.append((w, "UNIT"))
            elif w in ("baje",):
                tokens.append((w, "MERIDIEM"))
            elif w in ("subah",):
                tokens.append((w, "DAYPART"))
            else:
                tokens.append((w, "O"))
        add(tokens)
    # common English carriers stay background, including Hinglish
    # sentence-final position after a weekday — the slot festival words
    # also occupy, so it needs explicit counter-weight
    for carrier in ("gym", "meeting", "office", "dinner", "review", "party"):
        add([(carrier, "O"), ("next", "DEICTIC"), ("week", "UNIT")])
        add([(carrier, "O"), ("tomorrow", "REL_DAY"), ("at", "GLUE"), ("5", "HOUR"), ("pm", "MERIDIEM")])
        add([("har", "RECUR"), ("hafte", "UNIT"), ("Tuesday", "WEEKDAY"), ("ko", "GLUE"), (carrier, "O")])
        add([("har", "RECUR"), ("hafte", "UNIT"), ("somvaar", "WEEKDAY"), ("ko", "GLUE"), (carrier, "O")])
        add([("kal", "REL_DAY"), ("shaam", "DAYPART"), ("ko", "GLUE"), (carrier, "O")])
    # Good Friday is the holiday; bare Friday stays a weekday
    add([("Good", "HOLIDAY"), ("Friday", "HOLIDAY"), ("off", "O")])
    add([("good", "O"), ("friday", "WEEKDAY"), ("dinner", "O")])
    # Hindi background verbs and tense cues stay O — the kal/parso
    # past-vs-future disambiguation reads them after extraction
    for verb in ("आया", "गया", "था", "थी", "थे", "बीता", "aaya", "gaya", "tha", "thi", "beeta"):
        add([("कल", "REL_DAY"), (verb, "O")])
        add([("kal", "REL_DAY"), (verb, "O")])
        add([("परसों", "REL_DAY"), (verb, "O")])
    for verb_pair in (("आया", "था"), ("गया", "था"), ("aaya", "tha"), ("gaya", "tha")):
        add([("कल", "REL_DAY"), (verb_pair[0], "O"), (verb_pair[1], "O")])
        add([("kal", "REL_DAY"), (verb_pair[0], "O"), (verb_pair[1], "O")])
    # day-parts stay DAYPART after every relative day
    for rel in ("कल", "परसों", "आज", "kal", "parso", "aaj"):
        for part_hi, part_en in (("सुबह", "subah"), ("दोपहर", "dopahar"), ("शाम", "shaam"), ("रात", "raat")):
            for word in (part_hi, part_en):
                add([(rel, "REL_DAY"), (word, "DAYPART")])
                add([(rel, "REL_DAY"), (word, "DAYPART"), ("को", "GLUE")]) if word == part_hi else None
                if word == part_en:
                    add([(rel, "REL_DAY"), (word, "DAYPART"), ("10", "HOUR"), ("baje", "MERIDIEM")])


def main() -> None:
    import sys
    rng = random.Random(20260921)
    rows, seen = [], set()
    festival_rows(rng, rows, seen)
    protection_rows(rng, rows, seen)
    for row in rows:
        print(json.dumps(row, ensure_ascii=False, separators=(",", ":")))
    print(f"wrote {len(rows)} unique rows", file=sys.stderr)


if __name__ == "__main__":
    main()
