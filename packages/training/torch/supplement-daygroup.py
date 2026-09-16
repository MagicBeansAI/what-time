"""Day-group supplement for the LLM corpus pool.

Bare "weekdays"/"weekends" selectors drifted off DAYGROUP once the v1
LLM corpus (rich in weekday families) left the pool: the tagger's nearest
bare-token neighbors are single-token HOLIDAYs and bare RECUR words. The
structural terse family emits bare forms, but not with enough weight to
hold the boundary at fine-tune learning rates.

Writes raw {text, tokens} rows to stdout in the LLM-corpus shape; append
to data/llm/raw.jsonl and re-run the gate. Labels are authored here, never
taken from runtime predictions.
"""

import json
import random

GROUPS = ["weekdays", "weekends", "weekday", "weekend", "workdays", "वीकेंड"]
CARRIERS = [
    ["lets", "sync"],
    ["team", "sync"],
    ["standup"],
    ["gym"],
    ["yoga"],
    ["cricket"],
    ["site", "visit"],
    ["design", "review"],
    ["retro"],
    ["oncall"],
    ["deep", "work"],
    ["family", "call"],
]
CLOCKS = [
    ["at", "9", "am"],
    ["at", "10:30", "am"],
    ["at", "6", "pm"],
    ["8", "baje"],
    ["shaam", "ko", "6", "baje"],
    ["at", "noon"],
]
MODIFIERS = [["this"], ["next"], ["har"], ["every"], ["alternate"]]


def bare(rng: random.Random):
    word = rng.choice(GROUPS)
    casing = rng.random()
    text = word.upper() if casing < 0.2 else word.capitalize() if casing < 0.4 else word
    return [(text, "DAYGROUP")]


def with_carrier(rng: random.Random):
    carrier = rng.choice(CARRIERS)
    return [(word, "O") for word in carrier] + [(rng.choice(GROUPS), "DAYGROUP")]


def with_clock(rng: random.Random):
    tokens = [(rng.choice(GROUPS), "DAYGROUP")]
    for word in rng.choice(CLOCKS):
        tokens.append((word, "GLUE" if word in ("at", "ko") else
                       "HOUR" if word.replace(":", "").isdigit() else
                       "MERIDIEM" if word in ("am", "pm", "baje") else "O"))
    return tokens


def with_modifier(rng: random.Random):
    mod = rng.choice(MODIFIERS)
    tokens = [(mod[0], "RECUR" if mod[0] in ("har", "every", "alternate") else "DEICTIC")]
    tokens.append((rng.choice(GROUPS), "DAYGROUP"))
    if rng.random() < 0.5:
        tokens.extend(with_clock_tail(rng))
    return tokens


def with_carrier_clock(rng: random.Random):
    carrier = rng.choice(CARRIERS)
    tokens = [(word, "O") for word in carrier]
    tokens.append((rng.choice(GROUPS), "DAYGROUP"))
    if rng.random() < 0.6:
        tokens.extend(with_clock_tail(rng))
    return tokens


def with_clock_tail(rng: random.Random):
    tail = []
    for word in rng.choice(CLOCKS):
        tail.append((word, "GLUE" if word in ("at", "ko") else
                     "HOUR" if word.replace(":", "").isdigit() else
                     "MERIDIEM" if word in ("am", "pm", "baje") else "O"))
    return tail


TAILS = [["please"], ["works", "for", "me"], ["lets", "do", "it"], ["ok"], ["if", "that", "works"]]


def with_tail(rng: random.Random):
    tokens = rng.choice([bare, with_carrier, with_modifier])(rng)
    if tokens[-1][1] == "DAYGROUP" or rng.random() < 0.5:
        for word in rng.choice(TAILS):
            tokens.append((word, "O"))
    return tokens


SHAPES = [bare, with_carrier, with_clock, with_modifier, with_carrier_clock, with_tail]
WEIGHTS = [0.35, 0.15, 0.1, 0.15, 0.15, 0.1]


def main() -> None:
    rng = random.Random(20260916)
    seen = set()
    rows = []
    for _ in range(30000):
        shape = rng.choices(SHAPES, weights=WEIGHTS, k=1)[0]
        tokens = shape(rng)
        text = " ".join(token for token, _ in tokens)
        if text in seen:
            continue
        seen.add(text)
        rows.append({"text": text, "tokens": [list(pair) for pair in tokens]})
    for row in rows:
        print(json.dumps(row, ensure_ascii=False, separators=(",", ":")))
    print(f"wrote {len(rows)} unique rows", file=__import__("sys").stderr)


if __name__ == "__main__":
    main()
