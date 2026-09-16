"""Gate for externally generated corpus lines.

Reads data/llm/raw.jsonl ({text, tokens: [[token, label], ...]}), checks the
label set, the digit-suffix and tokenization conventions, rejects reserved
carrier phrases, dedupes by fingerprint, and requires the Rust oracle
compiler to check whether the labels produce
a schedule. Accepted lines are written to data/llm/accepted.jsonl in the
featurizer's input shape ({id, text, spans}); rejected ones are reported to
data/llm/rejected.jsonl for review.

Usage: uv run --project packages/training python packages/training/torch/validate_llm_corpus.py
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
import unicodedata
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
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


def _is_mark(ch: str) -> bool:
    return unicodedata.category(ch) in {"Mn", "Mc", "Me"}


def _is_letter(ch: str) -> bool:
    return ch.isalpha() or ch == "_"


def _split_word(word: str) -> list[str]:
    """Split one whitespace-delimited word.

    English matches the shared tokenizer (letters, digits, apostrophes,
    punctuation). Combining marks stay on the letter they modify so a
    Devanagari word such as हफ़्ते is one token, as the corpus brief
    requires.
    """
    tokens: list[str] = []
    i = 0
    n = len(word)
    while i < n:
        ch = word[i]
        if word[i:i + 4].lower() == "2mrw" and (
            i + 4 == n or not (word[i + 4].isalnum() or word[i + 4] == "_")
        ):
            tokens.append(word[i:i + 4])
            i += 4
        elif ch.isdigit():
            j = i + 1
            while j < n and word[j].isdigit():
                j += 1
            tokens.append(word[i:j])
            i = j
        elif _is_letter(ch):
            j = i + 1
            while j < n:
                nxt = word[j]
                if _is_letter(nxt) or _is_mark(nxt):
                    j += 1
                    continue
                if nxt in "'’" and j + 1 < n and _is_letter(word[j + 1]):
                    j += 1
                    continue
                break
            tokens.append(word[i:j])
            i = j
        elif _is_mark(ch):
            if tokens:
                tokens[-1] += ch
            else:
                tokens.append(ch)
            i += 1
        else:
            tokens.append(ch)
            i += 1
    return tokens


def split_tokens(text: str) -> list[str]:
    """Port of the shared tokenizer's split, with combining-mark attachment."""
    tokens: list[str] = []
    for word in re.findall(r"[^\s]+", text):
        tokens.extend(_split_word(word))
    return tokens


def span_offsets(text: str) -> list[tuple[int, int, str]]:
    """UTF-16 code-unit [start, end) offsets for each non-space token."""
    spans = []
    position = 0
    for token in split_tokens(text):
        start = text.index(token, position)
        end = start + len(token)
        spans.append((len(text[:start].encode("utf-16-le")) // 2,
                      len(text[:end].encode("utf-16-le")) // 2, token))
        position = end
    return spans


def oracle_failures(result: subprocess.CompletedProcess, count: int) -> dict[int, str]:
    """Fail closed on crashes or incomplete oracle output, not just bad rows."""
    summary = re.fullmatch(r"validated (\d+) lines, (\d+) failures \([\d.]+%\)", result.stdout.strip())
    if result.returncode not in (0, 1) or summary is None:
        raise RuntimeError(f"corpus oracle did not complete: {result.stderr.strip()}")
    failed = {}
    mismatched = 0
    for line in result.stderr.splitlines():
        match = re.fullmatch(r"line (\d+): (.+)", line)
        if not match:
            raise RuntimeError(f"unexpected oracle diagnostic: {line}")
        index = int(match[1]) - 1
        if index in failed or not 0 <= index < count:
            raise RuntimeError(f"invalid oracle row index: {line}")
        failed[index] = match[2]
        if "labels for" in match[2] or match[2].startswith("bad json:"):
            mismatched += 1
    if int(summary[2]) != len(failed) or int(summary[1]) + mismatched != count:
        raise RuntimeError("incomplete corpus oracle report")
    return failed


def main() -> int:
    source = ROOT / "data" / "llm" / "raw.jsonl"
    if not source.exists():
        print(f"missing {source}", file=sys.stderr)
        return 2
    out_dir = ROOT / "data" / "llm"
    accepted_path = out_dir / "accepted.jsonl"
    rejected_path = out_dir / "rejected.jsonl"
    out_dir.mkdir(parents=True, exist_ok=True)
    # Never leave a previous successful batch available after a failed gate.
    accepted_path.unlink(missing_ok=True)
    rejected_path.write_text("")
    seen: set[str] = set()
    accepted = rejected = 0
    reasons: dict[str, int] = {}
    accepted_rows: list[dict] = []

    for number, line in enumerate(source.read_text().splitlines(), start=1):
        if not line.strip():
            continue
        try:
            row = json.loads(line)
            text = row["text"]
            tokens = [(str(t), str(l)) for t, l in row["tokens"]]
        except Exception as error:
            rejected += 1
            reasons["malformed"] = reasons.get("malformed", 0) + 1
            out_dir.mkdir(parents=True, exist_ok=True)
            with rejected_path.open("a") as sink:
                sink.write(json.dumps({"line": number, "reason": f"malformed: {error}"}) + "\n")
            continue

        def fail(reason: str) -> None:
            nonlocal rejected
            rejected += 1
            reasons[reason] = reasons.get(reason, 0) + 1
            with rejected_path.open("a") as sink:
                sink.write(json.dumps({"line": number, "reason": reason, "text": text}) + "\n")

        if " ".join(t for t, _ in tokens) != " ".join(split_tokens(text)):
            fail("tokenization-mismatch")
            continue
        if any(label not in LABELS for _, label in tokens):
            fail("unknown-label")
            continue
        lowered = [t.lower() for t, _ in tokens]
        if any(
            lowered[i] in SUFFIXES and tokens[i][1] != "GLUE"
            for i in range(len(tokens))
        ):
            fail("suffix-not-glue")
            continue
        lowered_text = text.lower()
        if any(reserved in lowered_text for reserved in RESERVED):
            fail("reserved-phrase")
            continue
        expects_time = any(label not in ("O", "GLUE") for _, label in tokens)
        if not expects_time and len(tokens) < 3:
            fail("trivial-negative")
            continue
        fingerprint = json.dumps([text, [l for _, l in tokens]])
        if fingerprint in seen:
            fail("duplicate")
            continue
        seen.add(fingerprint)

        spans = []
        offsets = span_offsets(text)
        for (start, end, token), (_, label) in zip(offsets, tokens):
            spans.append({"start": start, "end": end, "label": label, "clauseStart": False})
        accepted += 1
        accepted_rows.append(
            {"id": f"llm-{number}", "text": text, "spans": spans}
        )

    compile_failures = 0
    cargo = Path(__file__).resolve().parents[3] / "rust"
    binary = cargo / "target" / "debug" / "examples" / "validate-corpus"
    if not binary.exists():
        print(f"missing oracle {binary}; cargo build --example validate-corpus", file=sys.stderr)
        return 2
    if accepted_rows:
        staged = out_dir / "staged.jsonl"
        with staged.open("w") as sink:
            for row in accepted_rows:
                sink.write(
                    json.dumps(
                        {"text": row["text"], "labels": [s["label"] for s in row["spans"]]}
                    )
                    + "\n"
                )
        result = subprocess.run(
            [str(binary), str(staged)], capture_output=True, text=True
        )
        try:
            failed_index = oracle_failures(result, len(accepted_rows))
        except RuntimeError as error:
            print(str(error), file=sys.stderr)
            return 2
        compile_failures = len(failed_index)
        if result.stdout.strip():
            print(result.stdout.strip())
        kept: list[dict] = []
        for index, row in enumerate(accepted_rows):
            if index in failed_index:
                rejected += 1
                reasons["compile"] = reasons.get("compile", 0) + 1
                with rejected_path.open("a") as sink:
                    sink.write(
                        json.dumps({"reason": "compile", "text": row["text"],
                                    "detail": failed_index[index]}, ensure_ascii=False)
                        + "\n"
                    )
            else:
                kept.append(row)
        accepted_rows = kept
        accepted = len(accepted_rows)

    with accepted_path.open("w") as sink:
        for row in accepted_rows:
            sink.write(json.dumps(row, ensure_ascii=False) + "\n")

    total = accepted + rejected
    print(json.dumps({
        "total": total,
        "accepted": accepted,
        "rejected": rejected,
        "rejectedByReason": reasons,
        "compileFailures": compile_failures,
    }, indent=2))
    return 0 if accepted > 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())
