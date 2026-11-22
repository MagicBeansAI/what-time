"""Fetch and filter the background prose pinned in `data/corpus.json`.

Writes `data/prose/sentences.txt`: ordinary English sentences with no time
expression in them, which the generators use as carrier text and as
all-O contrast. The sentences teach the tagger what ordinary words look
like, so a word it never saw in a schedule ("diary", "rehearsal") stays O.

The filter follows the pin: a length window, no number words beyond a/an,
no time vocabulary (weekdays, months, units, relative days, the pin's extra
words), and no time-shaped numbers. Selection is a seeded sample, so a
given export always yields the same file. Tatoeba republishes its exports,
so the download's digest can differ from the pin; the digest actually used
is recorded next to the output.

Usage (from packages/training): uv run python torch/fetch_prose.py
"""
from __future__ import annotations

import bz2
import hashlib
import json
import random
import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PIN = ROOT / "data" / "corpus.json"
DOWNLOADS = ROOT / "data" / "downloads"
OUTPUT = ROOT / "data" / "prose" / "sentences.txt"
SEED = 20260911

DAYS = ["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"]
DAY_SHORT = ["mon", "tue", "tues", "wed", "thu", "thur", "thurs", "fri", "sat", "sun"]
MONTHS = [
    "january", "february", "march", "april", "may", "june", "july", "august",
    "september", "october", "november", "december",
]
MONTH_SHORT = ["jan", "feb", "mar", "apr", "jun", "jul", "aug", "sep", "sept", "oct", "nov", "dec"]
UNITS = [
    "sec", "secs", "seconds", "minute", "minutes", "min", "mins", "hour", "hours", "hr",
    "hrs", "day", "days", "week", "weeks", "month", "months", "year", "years",
    "quarter", "quarters", "fortnights",
]
RELATIVE = ["yesterday", "tomorrow", "tonight", "today", "weekends", "weekdays"]
HOLIDAYS = ["christmas", "easter", "halloween", "thanksgiving", "diwali", "holi", "eid"]
NUMBER_WORDS = (
    "zero one two three four five six seven eight nine ten eleven twelve thirteen "
    "fourteen fifteen sixteen seventeen eighteen nineteen twenty thirty forty fifty "
    "sixty seventy eighty ninety hundred thousand million dozen couple half once "
    "twice thrice first third fourth fifth sixth seventh eighth ninth tenth eleventh "
    "twelfth twentieth thirtieth"
).split()
# Lower-case "may" and "march" are a modal and a verb; only the capitalized
# word away from the start of the sentence names the month.
CASED_MONTHS = {"may", "march"}


def download(url: str) -> Path:
    DOWNLOADS.mkdir(parents=True, exist_ok=True)
    target = DOWNLOADS / url.rsplit("/", 1)[-1]
    if not target.exists():
        subprocess.run(["curl", "-fsSL", "--retry", "3", "-o", str(target), url], check=True)
    return target


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main() -> None:
    pin = json.loads(PIN.read_text())["prose"]
    rules = pin["filter"]
    archive = download(pin["url"])
    digest = sha256(archive)

    banned = set(DAYS + DAY_SHORT + MONTHS + MONTH_SHORT + UNITS + RELATIVE + HOLIDAYS)
    banned |= {word for word in NUMBER_WORDS if word not in rules["allowedQuantityWords"]}
    banned |= {word.lower() for word in rules["additionalTimeWords"]}
    banned -= CASED_MONTHS
    shaped = [re.compile(pattern, re.IGNORECASE) for pattern in rules["timeShapedNumberPatterns"]]
    word_pattern = re.compile(r"[A-Za-z]+(?:'[A-Za-z]+)?")

    kept: list[str] = []
    seen: set[str] = set()
    with bz2.open(archive, "rt", encoding="utf-8") as lines:
        for line in lines:
            parts = line.rstrip("\n").split("\t")
            if len(parts) < 3:
                continue
            text = " ".join(parts[2].split())
            if not rules["minimumCharacters"] <= len(text) <= rules["maximumCharacters"]:
                continue
            words = word_pattern.findall(text)
            if not rules["minimumWords"] <= len(words) <= rules["maximumWords"]:
                continue
            if any(word.lower() in banned for word in words):
                continue
            if any(
                word.lower() in CASED_MONTHS and word[0].isupper() and index > 0
                for index, word in enumerate(words)
            ):
                continue
            if any(pattern.search(text) for pattern in shaped):
                continue
            if not text.isascii() or text.lower() in seen:
                continue
            seen.add(text.lower())
            kept.append(text)

    rng = random.Random(SEED)
    limit = pin["selection"]["limit"]
    chosen = sorted(rng.sample(kept, min(limit, len(kept))))
    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT.write_text("\n".join(chosen) + "\n", encoding="utf-8")
    manifest = {
        "url": pin["url"],
        "downloadSha256": digest,
        "pinnedSha256": pin["sha256"],
        "matchesPin": digest == pin["sha256"],
        "eligible": len(kept),
        "selected": len(chosen),
        "seed": SEED,
        "outputSha256": sha256(OUTPUT),
    }
    OUTPUT.with_name("sentences.manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(json.dumps(manifest))


if __name__ == "__main__":
    main()
