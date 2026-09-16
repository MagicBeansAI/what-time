"""Turn mined low-confidence phrases into a seed prompt for an external model.

Input: JSONL produced by `cargo run --example mine` (most uncertain first).
Output: a markdown prompt to hand to an external model alongside the v1 brief
(`LLM_CORPUS_PROMPT.md`) and the v2 addendum (`LLM_CORPUS_PROMPT_V2.md`).
The mined phrases carry the current model's guessed labels, which are
explicitly marked as unverified — the external model decides the correct
labels per the brief, then writes new sentences in the same families.

Run from packages/training:
  uv run python torch/format_mining_seeds.py <mined.jsonl> [out.md] [--limit N]
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

HEADER = """# Mined seed batch — targeted at the current model's failures

You previously received the corpus brief (tokenization, the 35-label set,
hard conventions) and the v2 addendum. This batch is different: the phrases
below are ones the current model tags with low confidence, or whose labels
fail to compile into a schedule. Each shows the model's *guessed* labels —
they may well be wrong. Your job:

1. For each seed phrase, work out the CORRECT labels under the brief. If
   the phrase is genuinely unparseable or ambiguous under the brief's
   rules, mark it "skip" and move on.
2. Then generate 50–100 NEW sentences per family these seeds belong to —
   same construction pattern, varied vocabulary, register, and carriers.
   Label them correctly from scratch; do not copy the model's guesses.
3. Output JSONL only, one object per line, exactly the brief's shape:
   {"text": ..., "tokens": [[token, label], ...]}
   Include your corrected versions of the seed phrases as the first lines.

All brief rules still apply: no reserved carrier phrases ("could you
arrange a reminder for", "our rehearsal begins at", "the train leaves at",
"please put this in my diary for", "allow extra time", "the workshop
continues"), under 40 tokens, no duplicates, skip rather than guess.
"""


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mined", type=Path, help="JSONL from `mine` example")
    parser.add_argument("out", nargs="?", type=Path, default=Path("data/mining/seed-prompt.md"))
    parser.add_argument("--limit", type=int, default=150, help="max seed phrases to include")
    args = parser.parse_args()

    entries = []
    for line in args.mined.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line:
            entries.append(json.loads(line))
    entries = entries[: args.limit]
    if not entries:
        raise SystemExit(f"no mined entries in {args.mined}")

    sections: dict[str, list[dict]] = {}
    for entry in entries:
        sections.setdefault(entry.get("status", "?"), []).append(entry)

    parts = [HEADER]
    order = ["compile-fail", "spurious", "low-margin"]
    titles = {
        "compile-fail": "## Labels looked temporal but compiled to nothing",
        "spurious": "## All-background text produced a schedule",
        "low-margin": "## Low-confidence tagging (model unsure)",
    }
    for status in order:
        group = sections.get(status)
        if not group:
            continue
        parts.append(titles[status])
        parts.append("")
        for entry in group:
            labels = " ".join(
                f"{token}[{label}]" for token, label, _ in entry.get("tokens", [])
            )
            diagnostics = ", ".join(entry.get("diagnostics") or []) or "none"
            parts.append(
                f"- `{entry['text']}`\n"
                f"  model guess: {labels}\n"
                f"  diagnostics: {diagnostics}"
            )
        parts.append("")

    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text("\n".join(parts), encoding="utf-8")
    counts = {status: len(group) for status, group in sections.items()}
    print(f"wrote {args.out} with {len(entries)} seeds: {counts}")


if __name__ == "__main__":
    main()
