"""Sample natural sentences that contain time words, for intent labelling.

The intent head decides whether a message asks for something to be
scheduled ("request") or only talks about time ("mention": past events,
habits, facts, titles, idioms). Generated rows cannot teach that alone: a
head trained on generated requests against natural non-requests learns
the writing style. So both labels come from the same natural source here,
Tatoeba (CC-BY 2.0 FR), English and Hindi.

This script only selects sentences; labels are written separately
(`data/intent/labels.jsonl`, one {"id", "intent"} per line, labelled by
readers who did not see the parser). Selection is seeded, so the sample
files are reproducible from the pinned downloads. English is stratified
by a rough tense bucket purely so the sample is varied; the bucket is not
a label.

Usage (from packages/training): uv run python torch/fetch_intent_prose.py
"""
from __future__ import annotations

import bz2
import json
import random
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DOWNLOADS = ROOT / "data" / "downloads"
OUT = ROOT / "data" / "intent"
SEED = 20261122
PER_BUCKET = 1500

ENGLISH_TIME = re.compile(
    r"\b(monday|tuesday|wednesday|thursday|friday|saturday|sunday|mondays|fridays|sundays|"
    r"weekends?|weekdays?|january|february|april|june|july|august|september|october|"
    r"november|december|tomorrow|tonight|today|yesterday|morning|evening|night|afternoon|"
    r"noon|midnight|o'clock|every|daily|weekly|monthly|yearly|hourly|minutes?|hours?|"
    r"week|month|year|\d{1,2}:\d{2}|\d{1,2} ?(am|pm|a\.m\.|p\.m\.))\b",
    re.I,
)
# Sampling strata only (variety), never labels.
STRATA = [
    ("ask", re.compile(r"^(please |remind me|don't forget|let's|can you|could you|will you|"
                       r"would you|wake me|call me|meet me)|\b(remind me|wake me up)\b", re.I)),
    ("past", re.compile(r"\b(was|were|did|had|used to|ago|yesterday|last)\b|\b\w+ed\b", re.I)),
    ("future", re.compile(r"\b(will|going to|tomorrow|tonight|next|'ll)\b", re.I)),
    ("other", re.compile(r"")),
]
HINDI_TIME = [
    "कल", "आज", "परसों", "सुबह", "शाम", "रात", "दोपहर", "बजे", "बजकर", "सोमवार", "मंगलवार",
    "बुधवार", "गुरुवार", "शुक्रवार", "शनिवार", "रविवार", "हर", "रोज़", "रोज", "रोज़ाना",
    "प्रतिदिन", "महीने", "महीना", "हफ़्ते", "हफ्ते", "सप्ताह", "साल", "दिन", "घंटे", "घंटा",
    "मिनट", "जनवरी", "फ़रवरी", "मार्च", "अप्रैल", "मई", "जून", "जुलाई", "अगस्त", "सितंबर",
    "अक्टूबर", "नवंबर", "दिसंबर",
]


def sentences(path: Path):
    with bz2.open(path, "rt", encoding="utf-8") as lines:
        for line in lines:
            parts = line.rstrip("\n").split("\t")
            if len(parts) >= 3:
                yield parts[0], " ".join(parts[2].split())


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    rng = random.Random(SEED)

    strata: dict[str, list[tuple[str, str]]] = {name: [] for name, _ in STRATA}
    for sentence_id, text in sentences(DOWNLOADS / "eng_sentences.tsv.bz2"):
        if not 12 <= len(text) <= 120 or not ENGLISH_TIME.search(text):
            continue
        for name, pattern in STRATA:
            if pattern.search(text):
                strata[name].append((sentence_id, text))
                break
    english = []
    for name, rows in strata.items():
        english.extend(rng.sample(rows, min(PER_BUCKET, len(rows))))
    rng.shuffle(english)

    hindi = []
    for sentence_id, text in sentences(DOWNLOADS / "hin_sentences.tsv.bz2"):
        words = set(re.findall(r"[ऀ-ॿ]+", text))
        if 6 <= len(text) <= 120 and words & set(HINDI_TIME):
            hindi.append((sentence_id, text))
    rng.shuffle(hindi)

    for language, rows in (("en", english), ("hi", hindi)):
        path = OUT / f"sample-{language}.jsonl"
        with path.open("w", encoding="utf-8") as handle:
            for sentence_id, text in rows:
                record = {"id": f"tatoeba-{language}-{sentence_id}", "lang": language, "text": text}
                handle.write(json.dumps(record, ensure_ascii=False) + "\n")
        print(json.dumps({"language": language, "sentences": len(rows), "path": str(path)}))


if __name__ == "__main__":
    main()
