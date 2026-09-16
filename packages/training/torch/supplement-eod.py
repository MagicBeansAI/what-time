"""Sentence-initial EOD/COB supplement for the LLM corpus pool.

The tagger eroded sentence-initial "EOD" -> UNIT after the gap-closing
retrain (mid-sentence "by EOD" kept working); these rows restore the
position-independent reading. Labels authored here, never taken from
runtime predictions.
"""

import json
import random

MARKERS = ["EOD", "eod", "COB", "EOW", "EOM"]
SUBJECTS = [
    "report", "invoice", "slides", "budget", "patch", "summary", "draft",
    "notes", "design", "memo", "timesheet", "update", None,
]
HINGLISH_TAILS = [
    ["pe", "bhej", "dena"], ["pe", "review", "kar", "dena"], ["tak", "bhej", "do"],
    ["pe", "update", "kar", "dena"], ["tak", "complete", "kar", "lena"],
    ["pe", "share", "kar", "dena"], ["pe", "ping", "kar", "dena"],
    ["pe", "aa", "jaana", "chahiye"], ["tak", "bhej", "dena"],
    ["pe", "finalize", "kar", "dena"],
]
ENGLISH_TAILS = [
    ["please"], ["works", "for", "me"], ["lets", "do", "it"], ["okay"],
    ["send", "it", "over"], ["no", "rush"], ["would", "be", "great"],
    ["as", "discussed"],
]


def main() -> None:
    rng = random.Random(20260918)
    rows, seen = [], set()
    for _ in range(60000):
        marker = rng.choice(MARKERS)
        subject = rng.choice(SUBJECTS)
        tail = rng.choice(HINGLISH_TAILS if rng.random() < 0.6 else ENGLISH_TAILS)
        tokens = []
        if subject:
            tokens.append((subject, "O"))
        tokens.append((marker, "UNIT"))
        tokens.extend((word, "O") for word in tail)
        text = " ".join(token for token, _ in tokens)
        if text in seen:
            continue
        seen.add(text)
        rows.append({"text": text, "tokens": [list(pair) for pair in tokens]})
    import sys
    for row in rows:
        print(json.dumps(row, ensure_ascii=False, separators=(",", ":")))
    print(f"wrote {len(rows)} unique rows", file=sys.stderr)


if __name__ == "__main__":
    main()
