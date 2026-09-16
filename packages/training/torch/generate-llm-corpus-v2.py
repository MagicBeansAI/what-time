"""Reproducible, template-authored supervision for LLM_CORPUS_PROMPT_V2.md.

Labels are authored here, never inferred from the runtime parser. This is
synthetic expansion of the brief, not an independent external-model sample.
Related contrasts share a group so an evaluation split can keep them together.
Run the acceptance gate before using any output for training.
"""

from __future__ import annotations

import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import random

from generate_llm_corpus import ordinal_suffix
from natural import RESERVED, RESERVED_DURATION
from validate_llm_corpus import LABELS, split_tokens

ROOT = Path(__file__).resolve().parent.parent
DEVANAGARI = str.maketrans("0123456789", "०१२३४५६७८९")
# Language identifies the carrier's register; mixed scripts are deliberate.
QUOTAS = {
    "date-selector": (600, 1500, 1500),
    "daily": (1000, 1000, 1000),
    "numerals": (600, 600, 600),
    "duration": (900, 600, 600),
    "bare-dom": (400, 400, 400),
    "daypart": (1000, 1000, 1000),
    "tense": (600, 600, 600),
    "mixed": (300, 300, 300),
    "shorthand": (400, 0, 200),
    "corporate": (400, 200, 200),
    "negative": (400, 400, 400),
}
EVENTS = {
    "en": "call meeting review demo interview lesson pickup checkup recording rehearsal".split(),
    "hi": "कॉल मीटिंग समीक्षा डेमो इंटरव्यू क्लास मुलाकात जाँच रिकॉर्डिंग रिहर्सल".split(),
    "hx": "call meeting review demo interview class mulaqat checkup recording rehearsal".split(),
}
NAMES = {
    "en": "Priya Rohan Neha Kabir Meera Aman Sara Dev Anika Rahul".split(),
    "hi": "प्रिया रोहन नेहा कबीर मीरा अमन सारा देव अनिका राहुल".split(),
    "hx": "Priya Rohan Neha Kabir Meera Aman Sara Dev Anika Rahul".split(),
}
PARTS = {
    "en": ("morning", "night"),
    "hi": ("सुबह", "रात"),
    "hx": ("subah", "raat"),
}
HOURS = {
    "en": "one two three four five six seven eight nine ten eleven twelve".split(),
    "hi": "एक दो तीन चार पाँच छह सात आठ नौ दस ग्यारह बारह".split(),
    "hx": "ek do teen char paanch chhe saat aath nau das gyarah barah".split(),
}


def tagged(text: str, role: str = "O") -> list[tuple[str, str]]:
    # V2 explicitly permits the alphanumeric shorthand as a single token.
    words = [text] if text.lower() == "2mrw" else split_tokens(text)
    return [(word, role) for word in words]


def row(tokens: list[tuple[str, str]]) -> dict:
    text = " ".join(word for word, _ in tokens)
    assert 0 < len(tokens) < 40
    assert all(label in LABELS for _, label in tokens)
    assert not any(p in text.lower() for p in RESERVED + RESERVED_DURATION)
    return {"text": text, "tokens": tokens}


def wrap(rng: random.Random, language: str, expression: list, style: int | None = None) -> list:
    event, name = rng.choice(EVENTS[language]), rng.choice(NAMES[language])
    style = rng.randrange(6) if style is None else style
    frames = {
        "en": [("", ""), (f"schedule the {event}", "please"),
               (f"please book {name}'s {event}", ""),
               (f"the {event} is", "please confirm"),
               ("can we meet", f"for the {event}"),
               (f"please move the {event}", f"and notify {name}")],
        "hi": [("", ""), (f"{event}", "रख देना"),
               (f"{name} की {event}", "रखनी है"),
               (f"{event}", "कर लेना प्लीज़"),
               ("मिलना है", f"{event} के लिए"),
               (f"{event}", f"रखो और {name} को बता देना")],
        "hx": [("", ""), (f"{event}", "rakh dena"),
               (f"{name} ki {event}", "rakhni hai"),
               (f"{event}", "kar lena please"),
               ("milna hai", f"{event} ke liye"),
               (f"{event}", f"rakho aur {name} ko bata dena")],
    }
    prefix, suffix = frames[language][style]
    return tagged(prefix) + expression + tagged(suffix)


def clock(rng: random.Random, language: str, *, native: bool = False) -> list:
    hour = str(rng.randint(1, 12))
    if native:
        hour = hour.translate(DEVANAGARI)
    elif rng.random() < 0.3:
        hour = rng.choice(HOURS[language])
    part = rng.choice(PARTS[language])
    if language == "en":
        return tagged("at", "GLUE") + [(hour, "HOUR"), (rng.choice(["am", "pm"]), "MERIDIEM")]
    return [(part, "DAYPART"), ("को" if language == "hi" else "ko", "GLUE"),
            (hour, "HOUR"), ("बजे" if language == "hi" else "baje", "MERIDIEM")]


def date_selector(rng: random.Random, language: str) -> list:
    marker = "तारीख" if language == "hi" else rng.choice(["tareek", "tareekh", "taareekh"])
    ki, ko = ("की", "को") if language == "hi" else ("ki", "ko")
    day = str(rng.randint(1, 28))
    expr = [(day, "DOM"), (marker, "UNIT"), (ko, "GLUE")]
    shape = rng.randrange(6)
    if shape == 0:
        expr = [(marker, "UNIT"), (day, "DOM")]
    elif shape == 1:
        period = {"en": [("next", "DEICTIC"), ("month", "UNIT"), (ki, "GLUE")],
                  "hi": [("अगले", "DEICTIC"), ("महीने", "UNIT"), (ki, "GLUE")],
                  "hx": [("agle", "DEICTIC"), ("mahine", "UNIT"), (ki, "GLUE")]}
        expr = period[language] + expr
    elif shape == 2:
        every, month = {"en": ("every", "month"), "hi": ("हर", "महीने"), "hx": ("har", "mahine")}[language]
        expr = [(every, "RECUR"), (month, "UNIT"), (ki, "GLUE")] + expr
    elif shape == 3:
        end = str(rng.randint(int(day) + 1, 29))
        if language == "en":
            expr = [("from", "RANGE_START")] + expr[:-1] + [("to", "RANGE_END"), (end, "DOM"), (marker, "UNIT")]
        else:
            expr = expr[:-1] + [("से" if language == "hi" else "se", "RANGE_START"),
                                (end, "DOM"), (marker, "UNIT"),
                                ("तक" if language == "hi" else "tak", "RANGE_END")]
    elif shape == 4:
        month = rng.choice(["मार्च", "जून", "अगस्त"] if language == "hi" else ["March", "June", "August"])
        expr = [(month, "MONTH"), (ki, "GLUE")] + expr
    if rng.random() < 0.5:
        expr += clock(rng, language)
    return wrap(rng, language, expr)


def daily(rng: random.Random, language: str) -> list:
    daily_word = "रोज़" if language == "hi" else rng.choice(["roz", "roj", "rozz"])
    expr = [(daily_word, "RECUR")]
    if rng.random() < 0.5:
        expr = [("हर" if language == "hi" else "har", "RECUR")] + expr
    if rng.random() < 0.85:
        expr += clock(rng, language)
    return wrap(rng, language, expr)


def numerals(rng: random.Random, language: str) -> list:
    shape = rng.randrange(5)
    n = str(rng.randint(1, 28)).translate(DEVANAGARI)
    if shape == 0:
        expr = clock(rng, language, native=True)
    elif shape == 1:
        expr = [(n, "DOM"), ("तारीख", "UNIT")]
    elif shape == 2:
        h = str(rng.randint(1, 12)).translate(DEVANAGARI)
        m = rng.choice(["००", "१५", "३०", "४५", "30", "45"])
        expr = [(h, "HOUR"), (":", "GLUE"), (m, "MINUTE"), (rng.choice(["am", "pm"]), "MERIDIEM")]
    elif shape == 3:
        expr = [(rng.choice(["सवा", "पौने"]), "CLOCK_OFFSET"),
                (str(rng.randint(2, 12)).translate(DEVANAGARI), "HOUR"), ("बजे", "MERIDIEM")]
    else:
        expr = [("every", "RECUR"), (n, "NUM"), ("days", "UNIT")]
    return wrap(rng, language, expr)


def duration(rng: random.Random, language: str) -> list:
    n = str(rng.choice([5, 10, 15, 20, 25, 30, 40, 45, 50, 60, 75, 90]))
    if language == "hi":
        expr = [("पूरे", "GLUE"), (n, "DUR"), ("मिनट", "UNIT")]
    elif language == "hx" and rng.random() < 0.5:
        expr = tagged("bas") + [(n, "DUR"), ("mins", "UNIT"), ("ka", "GLUE")] + tagged("kaam")
    else:
        expr = [("for", "GLUE"), (n, "DUR"), ("mins", "UNIT")]
    if rng.random() < 0.45:
        expr = clock(rng, language) + expr
    return wrap(rng, language, expr)


def bare_dom(rng: random.Random, language: str) -> list:
    n = rng.randint(1, 28)
    expr = [("की" if language == "hi" else "ki", "GLUE"), (str(n), "DOM")]
    if language != "hi" and rng.random() < 0.5:
        expr += [(ordinal_suffix(n), "GLUE")]
    expr += [("को" if language == "hi" else "ko", "GLUE")]
    if rng.random() < 0.5:
        expr += clock(rng, language)
    return wrap(rng, language, expr, rng.choice([1, 2, 3, 5]))


def daypart(rng: random.Random, language: str) -> list[list]:
    # Two day-parts crossed with implicit/explicit clocks: four related rows.
    # The explicit am/pm is fixed, so the contrast teaches its precedence.
    h = str(rng.randint(3, 11)) if rng.random() < 0.5 else rng.choice(HOURS[language][2:11])
    explicit = rng.choice(["meridiem", "quarter", "half"])
    seed = rng.randrange(2**31)
    variants = []
    for part in PARTS[language]:
        glue = "at" if language == "en" else "को" if language == "hi" else "ko"
        base = [(part, "DAYPART"), (glue, "GLUE")]
        marker = "o'clock" if language == "en" else "बजे" if language == "hi" else "baje"
        implicit = base + [(h, "HOUR"), (marker, "MERIDIEM")]
        if explicit == "meridiem":
            other = base + [(h, "HOUR"), (":", "GLUE"), ("30", "MINUTE"), ("pm", "MERIDIEM")]
            # Colons require numeric hours.
            if not h.isdigit():
                other = base + [(h, "HOUR"), ("pm", "MERIDIEM")]
        elif explicit == "half" and language != "en":
            other = base + [("डेढ़" if language == "hi" else "dedh", "CLOCK_OFFSET"), (marker, "MERIDIEM")]
        else:
            offset = "quarter past" if language == "en" else "सवा" if language == "hi" else "sava"
            other = base + tagged(offset, "CLOCK_OFFSET") + [(h, "HOUR"), (marker, "MERIDIEM")]
        variants.extend([wrap(random.Random(seed), language, implicit, 2),
                         wrap(random.Random(seed), language, other, 2)])
    return variants


def tense(rng: random.Random, language: str) -> list[list]:
    # Past/future partners differ only in the tense cue. Even English carriers
    # retain कल/परसों so this is supervision for the ambiguity, not a new label.
    rel = rng.choice(["कल", "परसों"] if language != "hx" else ["kal", "parso"])
    name, event = rng.choice(NAMES[language]), rng.choice(EVENTS[language])
    expr = [(rel, "REL_DAY")] + clock(rng, language)
    prefix = tagged(f"{name} {event}")
    past, future = {"en": ("आया था", "आएगा"), "hi": ("आया था", "आएगा"),
                    "hx": ("aaya tha", "aayega")}[language]
    return [prefix + expr + tagged(cue) for cue in (past, future)]


def mixed(rng: random.Random, language: str) -> list:
    expr = rng.choice([
        [("अगले", "DEICTIC"), ("week", "UNIT"), ("सोमवार", "WEEKDAY"), ("ko", "GLUE")],
        [("agle", "DEICTIC"), ("हफ़्ते", "UNIT"), ("Tuesday", "WEEKDAY"), ("को", "GLUE")],
        [("हर", "RECUR"), ("month", "UNIT"), ("की", "GLUE"),
         (str(rng.randint(1, 28)), "DOM"), ("tareekh", "UNIT")],
    ])
    expr += [("शाम", "DAYPART"), ("ko", "GLUE"), (str(rng.randint(3, 11)), "HOUR"), ("baje", "MERIDIEM")]
    return wrap(rng, language, expr, rng.choice([1, 2, 3, 5]))


def shorthand(rng: random.Random, language: str) -> list:
    kind = rng.randrange(4)
    if kind == 0:
        expr = tagged(rng.choice(["2mrw", "2MRW"]), "REL_DAY") + clock(rng, language)
    elif kind == 1:
        expr = [("kl", "REL_DAY")] + clock(rng, language) + tagged("aana hai")
    elif kind == 2:
        expr = [("evng", "DAYPART"), ("at", "GLUE"), (str(rng.randint(3, 11)), "HOUR"), ("o'clock", "MERIDIEM")]
    else:
        expr = [(rng.choice(["tues", "Tues"]), "WEEKDAY")] + clock(rng, language)
    # "mn" has no unambiguous role in the brief; deliberately omit it.
    return wrap(rng, language, expr, rng.choice([1, 2, 3, 5]))


def corporate(rng: random.Random, language: str) -> list:
    word = rng.choice(["EOD", "COB", "EOW", "EOM"])
    if rng.random() < 0.5:
        expr = [(word, "UNIT")]
    else:
        unit = {"EOD": "day", "COB": "day", "EOW": "week", "EOM": "month"}[word]
        expr = [("end", "EDGE"), ("of", "GLUE")]
        if rng.random() < 0.5:
            expr += [("next", "DEICTIC")]
        expr += [(unit, "UNIT")]
    return wrap(rng, language, expr, rng.choice([1, 2, 3, 5]))


def negative(rng: random.Random, language: str) -> list:
    n, balance = rng.randint(1000, 9999), rng.randint(100, 9000)
    name = rng.choice(NAMES[language])
    choices = {
        "en": [f"invoice {n} balance {balance} pending", f"{name} said the movie was {rng.randint(2, 4)} hours long tbh",
               f"{name} copied {n} rows into the minutes document", f"ticket {n} has priority {rng.randint(1, 5)} in the calendar app"],
        "hi": [f"बिल {n} में {balance} रुपये बाकी हैं", f"{name} ने बताया फिल्म {rng.randint(2, 4)} घंटे लंबी थी",
               f"{name} ने कैलेंडर ऐप में त्रुटि {n} देखी", f"घड़ी का मॉडल {n} है और कीमत {balance} रुपये है"],
        "hx": [f"invoice {n} ka balance {balance} pending hai", f"{name} bola movie {rng.randint(2, 4)} hours lambi thi",
               f"{name} ne calendar app ka bug {n} report kiya", f"clock ka model {n} hai price {balance} hai"],
    }
    return tagged(rng.choice(choices[language]))


FACTORIES = {"date-selector": date_selector, "daily": daily, "numerals": numerals,
             "duration": duration, "bare-dom": bare_dom, "daypart": daypart,
             "tense": tense, "mixed": mixed, "shorthand": shorthand,
             "corporate": corporate, "negative": negative}


def include_short_forms(rows: list[dict], metadata: list[dict]) -> None:
    """Reserve single-row slots for terse forms that prose expansion can miss.

    Replacements preserve row order, family quotas and contrast-group sizes.
    They do not consume RNG draws or move the existing evaluation groups.
    """
    seeds = {
        ("numerals", "hi"): [
            [(offset, "CLOCK_OFFSET"), (str(hour).translate(DEVANAGARI), "HOUR")]
            for offset in ("सवा", "पौने") for hour in range(1, 13)
        ],
        ("shorthand", "en"): [[(word, role)] for word, role in
                               [("2mrw", "REL_DAY"), ("2MRW", "REL_DAY"),
                                ("evng", "DAYPART"), ("tues", "WEEKDAY")]],
        ("shorthand", "hx"): [[("kl", "REL_DAY")] + tagged(cue)
                               for cue in ("aana hai", "aaya tha")],
        ("corporate", "en"): [[(word, "UNIT")] for word in ("EOD", "COB", "EOW", "EOM")],
        ("duration", "en"): [tagged("call") + [("for", "GLUE"), ("10", "DUR"), ("mins", "UNIT")]],
        ("negative", "en"): [tagged(text) for text in
                              ("invoice 2024 balance 3500 pending", "movie was 3 hours long tbh")],
    }
    seen = {record["text"] for record in rows}

    def replace(index: int, record: dict) -> None:
        seen.remove(rows[index]["text"])
        rows[index] = record
        seen.add(record["text"])
        metadata[index]["text"] = record["text"]
        metadata[index]["group"] = hashlib.sha256(record["text"].encode()).hexdigest()[:20]

    for (family, language), examples in seeds.items():
        positions = iter(i for i, meta in enumerate(metadata)
                         if meta["family"] == family and meta["language"] == language)
        for tokens in examples:
            record = row(tokens)
            if record["text"] in seen:
                continue
            index = next(positions)
            replace(index, record)
    # Teach these words without a following clock or बजे giving away the role.
    # Keep half of the original contexts as a contrast, with stable row IDs.
    for index, meta in enumerate(metadata):
        if index % 2:
            continue
        tokens = rows[index]["tokens"]
        if meta["family"] == "daily" and meta["language"] in {"hi", "hx"}:
            # Daily recurrence also occurs with just a day-part: रोज़ शाम को.
            if not any(label == "DAYPART" for _, label in tokens):
                continue
            parts = ["सुबह", "दोपहर", "शाम", "रात"] if meta["language"] == "hi" else ["subah", "dopahar", "shaam", "raat"]
            tokens = [(parts[(index // 2) % 4] if label == "DAYPART" else w, label)
                      for w, label in tokens if label not in {"HOUR", "MERIDIEM"}]
        elif meta["family"] == "shorthand" and any(w.lower() == "2mrw" for w, _ in tokens):
            tokens = [(w, label) for w, label in tokens if label in {"O", "REL_DAY"}]
        elif meta["family"] == "numerals" and any(label == "CLOCK_OFFSET" for _, label in tokens):
            tokens = [(w, label) for w, label in tokens if label != "MERIDIEM"]
        else:
            continue
        record = row(tokens)
        if record["text"] not in seen:
            replace(index, record)


def generate(seed: int) -> tuple[list[dict], list[dict], dict]:
    rng = random.Random(seed)
    rows, metadata = [], []
    seen: set[str] = set()
    counts = Counter()
    for family, quotas in QUOTAS.items():
        for language, target in zip(("en", "hi", "hx"), quotas):
            count = attempts = 0
            while count < target:
                attempts += 1
                if attempts > max(5000, target * 200):
                    raise RuntimeError(f"exhausted {family}/{language}: {count}/{target}")
                sample = FACTORIES[family](rng, language)
                group = sample if family in {"daypart", "tense"} else [sample]
                batch = [row(tokens) for tokens in group]
                texts = [r["text"] for r in batch]
                if len(set(texts)) != len(texts) or any(t in seen for t in texts):
                    continue
                group_id = hashlib.sha256("\n".join(texts).encode()).hexdigest()[:20]
                for record in batch:
                    rows.append(record)
                    metadata.append({"line": len(rows), "family": family, "language": language,
                                     "group": group_id, "text": record["text"]})
                seen.update(texts)
                count += len(batch)
            counts[f"{family}/{language}"] = count
    include_short_forms(rows, metadata)
    return rows, metadata, dict(counts)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--seed", type=int, default=20260916)
    parser.add_argument("--out", type=Path, default=ROOT / "data/llm/raw.jsonl")
    args = parser.parse_args()
    rows, metadata, counts = generate(args.seed)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    for path, records in [(args.out, rows), (args.out.with_suffix(".groups.jsonl"), metadata)]:
        path.write_text("".join(json.dumps(r, ensure_ascii=False) + "\n" for r in records), encoding="utf-8")
    manifest = {"source": "template-authored-v2", "seed": args.seed, "rows": len(rows),
                "generatorSha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                "rawSha256": hashlib.sha256(args.out.read_bytes()).hexdigest(), "byFamilyAndLanguage": counts,
                "omitted": {"mn": "No unambiguous expansion or role specified in the brief."}}
    args.out.with_suffix(".manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(json.dumps(manifest, indent=2))


if __name__ == "__main__":
    main()
