"""Ordinal-recurrence and clock-verb supplement for the LLM corpus pool.

Two thin families from the mined residuals: inflected Hindi/Hinglish
ordinals before a weekday ("हर महीने के दूसरे सोमवार को" — ORD was never
learned because the external batch's rows were gate-rejected before the
compiler fix), and verbs following a fractional clock ("sava char baje
utth ja" — the tagger reads the verb as HOUR). Labels authored here, never
taken from runtime predictions.
"""

import json
import random

ORDINALS = [
    ("पहले", "pehle"), ("दूसरे", "doosre"), ("तीसरे", "teesre"),
    ("चौथे", "chauthe"), ("पाँचवे", "paanchve"), ("आखिरी", "aakhri"),
]
WEEKDAYS = [
    ("सोमवार", "somvaar"), ("मंगलवार", "mangalvaar"), ("बुधवार", "budhvaar"),
    ("गुरुवार", "guruvaar"), ("शुक्रवार", "shukravaar"), ("शनिवार", "shanivaar"),
    ("रविवार", "ravivaar"),
]
MONTHS = [("हर महीने के", "har mahine ke"), ("हर हफ़्ते के", "har hafte ke")]
CARRIERS = [["meeting"], ["baithak"], ["review"], ["sync"], ["yoga"], ["clinic"]]

CLOCKS = [
    ("सवा चार बजे", "sava char baje"), ("सवा पाँच बजे", "sava paanch baje"),
    ("पौने छह बजे", "paune chhe baje"), ("डेढ़ बजे", "dedh baje"),
]
VERBS = [
    ("उठ जाना", "utth ja"), ("उठ जाओ", "uth ja"), ("निकलना", "nikalna"),
    ("चल देना", "chal dena"), ("आ जाना", "aa jaana"), ("पहुँच जाना", "pahunch jaana"),
    ("मिलना", "milna"), ("सो जाना", "so jaana"),
]


def ordinal_rows(rng: random.Random):
    (month_hi, month_en), (ord_hi, ord_en) = (
        rng.choice(MONTHS), rng.choice(ORDINALS))
    (day_hi, day_en) = rng.choice(WEEKDAYS)
    if rng.random() < 0.5:
        words = month_hi.split()
        text = f"{month_hi} {ord_hi} {day_hi} को"
        prefix = [(words[0], "RECUR"), (words[1], "UNIT")]
        if len(words) > 2:
            prefix.append((words[2], "GLUE"))
        tokens = prefix + [
            (ord_hi, "ORD"), (day_hi, "WEEKDAY"), ("को", "GLUE"),
        ]
    else:
        words = month_en.split()
        text = f"{month_en} {ord_en} {day_en} ko"
        tokens = [
            (words[0], "RECUR"), (words[1], "UNIT"), ("ke", "GLUE"),
            (ord_en, "ORD"), (day_en, "WEEKDAY"), ("ko", "GLUE"),
        ]
    if rng.random() < 0.35:
        carrier = rng.choice(CARRIERS)
        text = f"{carrier[0]} {text}"
        tokens = [(carrier[0], "O")] + tokens
    return text, tokens


def clock_verb_rows(rng: random.Random):
    (clock_hi, clock_en) = rng.choice(CLOCKS)
    (verb_hi, verb_en) = rng.choice(VERBS)
    if rng.random() < 0.5:
        text = f"{clock_hi} {verb_hi}"
        tokens = [
            (clock_hi.split()[0], "CLOCK_OFFSET"),
            (clock_hi.split()[1], "HOUR"), ("बजे", "MERIDIEM"),
            *[(w, "O") for w in verb_hi.split()],
        ]
    else:
        words = clock_en.split()
        text = f"{clock_en} {verb_en}"
        tokens = [
            (words[0], "CLOCK_OFFSET"), (words[1], "HOUR"), ("baje", "MERIDIEM"),
            *[(w, "O") for w in verb_en.split()],
        ]
    return text, tokens


def main() -> None:
    rng = random.Random(20260917)
    seen = set()
    rows = []
    for _ in range(30000):
        text, tokens = (
            ordinal_rows(rng) if rng.random() < 0.55 else clock_verb_rows(rng)
        )
        if text in seen:
            continue
        seen.add(text)
        rows.append({"text": text, "tokens": [list(pair) for pair in tokens]})
    for row in rows:
        print(json.dumps(row, ensure_ascii=False, separators=(",", ":")))
    print(f"wrote {len(rows)} unique rows", file=__import__("sys").stderr)


if __name__ == "__main__":
    main()
