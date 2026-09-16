"""Holiday-vocabulary supplement for the LLM corpus pool.

Holiday words had no corpus rows teaching the HOLIDAY label — they worked
via compiler word-over-label rescue of drifting UNIT tags, which eroded
across retrains. These rows teach the label directly, including
multi-word holidays (each token HOLIDAY, matching the compiler's
concatenation) and दिवाली in both scripts. Labels authored here, never
taken from runtime predictions.
"""

import json
import random

SINGLE = [
    "Thanksgiving", "thanksgiving", "Easter",
    "Halloween", "Diwali", "diwali", "दिवाली", "दीपावली", "Juneteenth",
    "Holi", "holi", "होली", "Rakhi", "राखी", "राखड़ी", "Dussehra", "दशहरा",
    "Eid", "ईद", "Bakrid", "बकरीद", "Mahashivratri", "महाशिवरात्रि",
]
MULTI = [
    ["Good", "Friday"], ["good", "friday"], ["Boxing", "Day"], ["boxing", "day"],
    ["Memorial", "Day"], ["memorial", "day"], ["Labor", "Day"], ["labor", "day"],
    ["Independence", "Day"], ["independence", "day"], ["Easter", "Monday"],
    ["Christmas", "Eve"], ["christmas", "eve"], ["New", "Year's", "Day"],
    ["new", "year's", "day"],
    ["Karwa", "Chauth"], ["karwa", "chauth"], ["करवा", "चौथ"],
    ["Raksha", "Bandhan"], ["रक्षा", "बंधन"], ["Ganesh", "Chaturthi"],
    ["गणेश", "चतुर्थी"], ["Columbus", "Day"], ["columbus", "day"],
    ["Veterans", "Day"], ["veterans", "day"], ["St", "Patrick's", "Day"],
    ["st", "patrick's", "day"], ["Canada", "Day"], ["canada", "day"],
    ["Canadian", "Thanksgiving"], ["australia", "day"], ["Australia", "Day"],
    ["Anzac", "Day"], ["anzac", "day"], ["Mothering", "Sunday"],
    ["Guy", "Fawkes", "Night"], ["guy", "fawkes"], ["bank", "holiday"],
]
CARRIERS = [
    ["family", "dinner"], ["team", "sync"], ["shipping", "cutoff"], ["office", "party"],
    ["parade"], ["brunch"], ["deadline"], ["celebration"], ["lunch"], ["review"],
    ["clinic", "closed"], ["store", "hours"],
]
HINDI_TAILS = [["को", "मिलते", "हैं"], ["को", "छुट्टी", "है"], ["की", "शाम", "को"], ["पर", "मिलते", "हैं"]]
CLOCKS = [["at", "9", "am"], ["at", "noon"], ["शाम", "को", "6", "बजे"], ["at", "11", "am"]]


def clock_tokens(clock):
    labeled = []
    for word in clock:
        if word in ("at", "को"):
            labeled.append((word, "GLUE"))
        elif word in ("am", "pm", "बजे"):
            labeled.append((word, "MERIDIEM"))
        elif word == "noon":
            labeled.append((word, "TIME_NAMED"))
        elif word == "शाम":
            labeled.append((word, "DAYPART"))
        else:
            labeled.append((word, "HOUR"))
    return labeled


def tokens_for(word_or_multi):
    if isinstance(word_or_multi, list):
        return [(w, "HOLIDAY") for w in word_or_multi]
    return [(word_or_multi, "HOLIDAY")]


def main() -> None:
    import sys
    rng = random.Random(20260919)
    rows, seen = [], set()
    choices = SINGLE + MULTI
    for _ in range(40000):
        holiday = rng.choice(choices)
        tokens = []
        shape = rng.random()
        if shape < 0.25:
            carrier = rng.choice(CARRIERS)
            tokens += [(w, "O") for w in carrier]
            tokens += tokens_for(holiday)
        elif shape < 0.45:
            tokens += tokens_for(holiday)
            if rng.random() < 0.5:
                tokens.append(("on", "GLUE") if rng.random() < 0.5 else ("को", "GLUE"))
            tail = rng.choice(HINDI_TAILS if isinstance(holiday, str) and any(
                ord(c) > 0x900 for c in holiday) else [["is", "a", "holiday"], ["works", "for", "me"], ["please"]])
            tokens += [(w, "O") for w in tail]
        elif shape < 0.6:
            tokens.append((rng.choice(["on", "by", "before", "after"]), "GLUE"))
            tokens += tokens_for(holiday)
            tokens += [(w, "O") for w in rng.choice([["please"], ["works", "for", "me"], ["let's", "avoid", "it"]])]
        else:
            tokens += tokens_for(holiday)
            tokens += clock_tokens(rng.choice(CLOCKS))
        text = " ".join(t for t, _ in tokens)
        if text in seen:
            continue
        seen.add(text)
        rows.append({"text": text, "tokens": [list(p) for p in tokens]})
    for row in rows:
        print(json.dumps(row, ensure_ascii=False, separators=(",", ":")))
    print(f"wrote {len(rows)} unique rows", file=sys.stderr)


if __name__ == "__main__":
    main()
