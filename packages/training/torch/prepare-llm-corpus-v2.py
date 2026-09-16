"""Gate V2 rows, keep contrast groups together, and report section coverage.

Run generate-llm-corpus-v2.py first. This replaces the evaluation slice for
this batch; archive an existing corpus before starting a different batch.
"""

from collections import Counter, defaultdict
import hashlib
import json
from pathlib import Path
import random
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
DATA = ROOT / "data/llm"


def records(path):
    return [json.loads(line) for line in path.read_text().splitlines() if line.strip()]


def split_groups(rows, metadata, count=1000, seed=20260916):
    groups = defaultdict(list)
    for row in rows:
        groups[metadata[row["text"]]["group"]].append(row)
    keys = list(groups)
    random.Random(seed).shuffle(keys)
    heldout = []
    for key in keys:
        if len(heldout) + len(groups[key]) <= count:
            heldout.extend(groups[key])
        if len(heldout) == count:
            break
    if len(heldout) != count:
        raise ValueError(f"could only reserve {len(heldout)}/{count} rows without splitting a group")
    return heldout


def main():
    manifest = json.loads((DATA / "raw.manifest.json").read_text())
    if hashlib.sha256((DATA / "raw.jsonl").read_bytes()).hexdigest() != manifest["rawSha256"]:
        raise ValueError("raw corpus differs from the generator manifest")
    metadata = {row["text"]: row for row in records(DATA / "raw.groups.jsonl")}
    if len(metadata) != manifest["rows"]:
        raise ValueError("missing or duplicated grouping metadata")
    subprocess.run([sys.executable, str(ROOT / "torch/validate_llm_corpus.py")], check=True)
    accepted = records(DATA / "accepted.jsonl")
    rejected = records(DATA / "rejected.jsonl")
    heldout = split_groups(accepted, metadata)
    (DATA / "heldout.jsonl").write_text("".join(json.dumps(row, ensure_ascii=False) + "\n" for row in heldout))
    held_ids = {row["id"] for row in heldout}
    coverage = defaultdict(Counter)
    for row in accepted:
        coverage[metadata[row["text"]]["family"]]["accepted"] += 1
        coverage[metadata[row["text"]]["family"]]["heldout" if row["id"] in held_ids else "training"] += 1
    for row in rejected:
        coverage[metadata[row["text"]]["family"]]["rejected"] += 1
    report = {"raw": manifest["rows"], "accepted": len(accepted), "rejected": len(rejected),
              "heldout": len(heldout), "training": len(accepted) - len(heldout),
              "heldoutSeed": 20260916, "splitUnit": "contrast-group", "byFamily": dict(coverage)}
    (DATA / "v2-validation.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
