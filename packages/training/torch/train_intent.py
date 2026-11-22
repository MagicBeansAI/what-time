"""Train the sentence-intent encoder.

The tagger answers "where are the time expressions?"; the intent encoder
answers "is the user asking for something to be scheduled?" ("request")
or only talking about time ("mention": past events, habits, facts, titles,
idioms). It is a small encoder of its own (`IntentEncoder`) over the same
feature rows, trained here alone and exported beside an existing tagger:
the tagger's tensors are copied unchanged, so its labels, boundaries and
parity fixtures stay exactly as they were. (A head on the frozen tagger's
token states was tried first and missed even "remind me every 2 hours":
those states carry little sentence-level meaning.)

Data, both labels from the same kinds of source so the head cannot learn
writing style instead of intent:
- natural sentences with time words (Tatoeba English and Hindi, sampled by
  `fetch_intent_prose.py`), labelled request/mention by readers who did not
  see the parser (`data/intent/labels-*.jsonl`); "unsure" rows are dropped;
- chat lines written with their intent (`*-written.jsonl`): Hinglish, since
  Tatoeba has none, and Hindi, since Tatoeba Hindi has few requests;
- generated rows from the tagger's own generators: a row with any time span
  is a request (a schedule phrase, bare or carried), an all-O row (prose,
  negatives) a mention.
Natural rows are split 90/10 by a stable hash of their id; the 10% is the
held-out report. Natural training rows are seen in three casings (as
written, lower case, without final punctuation), so case and punctuation
carry no signal.

Usage (from packages/training):
  uv run python torch/train_intent.py --init runs/neg-r4/export/weights-transformer.json --run intent-r1
Then `uv run python torch/promote.py intent-r1`.
"""
from __future__ import annotations

import argparse
import base64
import hashlib
import json
import random
import shutil
import subprocess
import time
from pathlib import Path

import numpy as np
import torch
from torch.nn import functional as F

import recurrence_families
from features import LEXICAL_ROWS, PADDING_ROW
from train import ROOT, Dataset, run_featurizer
from train_transformer import quantize_state_dict
from transformer import INTENT_HIDDEN, IntentEncoder

INTENT = ROOT / "data" / "intent"
RUST = ROOT.parent.parent / "rust"


def natural_rows() -> list[dict]:
    rows = []
    for sample in sorted(INTENT.glob("sample-*.jsonl")):
        language = sample.stem.split("-", 1)[1]
        texts = {}
        for line in sample.read_text(encoding="utf-8").splitlines():
            record = json.loads(line)
            texts[record["id"]] = record["text"]
        for labels in sorted(INTENT.glob(f"labels-{language}*.jsonl")):
            for line in labels.read_text(encoding="utf-8").splitlines():
                if not line.strip():
                    continue
                record = json.loads(line)
                if record["intent"] in ("request", "mention") and record["id"] in texts:
                    rows.append(
                        {
                            "id": record["id"],
                            "lang": language,
                            "text": texts[record["id"]],
                            "intent": int(record["intent"] == "request"),
                        }
                    )
    # Lines written with their intent: Hinglish (Tatoeba has none) and more
    # Hindi requests (Tatoeba Hindi is nine parts mention to one).
    for written in sorted(INTENT.glob("*-written.jsonl")):
        for line in written.read_text(encoding="utf-8").splitlines():
            if line.strip():
                record = json.loads(line)
                if record.get("intent") in ("request", "mention"):
                    rows.append(
                        {
                            "id": record["id"],
                            "lang": record.get("lang", "hinglish"),
                            "text": record["text"],
                            "intent": int(record["intent"] == "request"),
                        }
                    )
    return rows


def heldout(row_id: str) -> bool:
    return int(hashlib.sha256(row_id.encode()).hexdigest(), 16) % 10 == 0


def casings(text: str) -> list[str]:
    stripped = text.rstrip(".!?। ")
    return list(dict.fromkeys([text, text.lower(), stripped or text]))


def synthetic_rows(count: int, seed: int, directory: Path) -> list[dict]:
    """Half from generate.py (structural phrases, carriers, prose), half from
    the recurrence and contrast families (negatives included)."""
    path = directory / f"generated-{seed}.jsonl"
    subprocess.run(
        [
            "uv", "run", "--project", str(ROOT), "python", str(ROOT / "torch" / "generate.py"),
            "--count", str(count // 2), "--seed", str(seed), "--split", "train", "--out", str(path),
        ],
        cwd=ROOT,
        check=True,
        stdout=subprocess.DEVNULL,
    )
    rows = []
    for line in path.read_text(encoding="utf-8").splitlines():
        record = json.loads(line)
        labeled = any(span.get("label", "O") != "O" for span in record["spans"])
        rows.append({"id": record["id"], "text": record["text"], "intent": int(labeled)})
    rng = random.Random(seed)
    for index in range(count - count // 2):
        sentence = recurrence_families.render(random.Random(rng.randrange(2**31)))
        labeled = any(span["label"] != "O" for span in sentence.spans)
        rows.append({"id": f"family-{seed}-{index}", "text": sentence.text, "intent": int(labeled)})
    return rows


def featurize(rows: list[dict], prefix: Path) -> tuple[Dataset, np.ndarray]:
    path = prefix.with_suffix(".jsonl")
    with path.open("w", encoding="utf-8") as handle:
        for row in rows:
            handle.write(
                # The tagger is frozen, so token labels are not trained here;
                # one background span covers the text, as the featurizer
                # requires every token to sit in a span.
                json.dumps(
                    {
                        "id": row["id"],
                        "text": row["text"],
                        "spans": [
                            {"start": 0, "end": len(row["text"].encode("utf-16-le")) // 2, "label": "O"}
                        ],
                    },
                    ensure_ascii=False,
                )
                + "\n"
            )
    run_featurizer(path, prefix)
    dataset = Dataset(prefix)
    if len(dataset) != len(rows):
        raise SystemExit(f"featurizer returned {len(dataset)} sequences for {len(rows)} rows")
    return dataset, np.array([row["intent"] for row in rows], dtype=np.float32)


@torch.no_grad()
def intent_scores(model, dataset: Dataset, device: str) -> np.ndarray:
    model.eval()
    scores = np.zeros(len(dataset), dtype=np.float32)
    for indices in dataset.batches(256):
        rows, _, _, valid, _ = dataset.batch(indices, device)
        scores[indices] = torch.sigmoid(model(rows, valid)).float().cpu().numpy()
    return scores


def report(scores: np.ndarray, targets: np.ndarray, threshold: float) -> dict:
    predicted = scores >= threshold
    actual = targets >= 0.5
    true_positive = int((predicted & actual).sum())
    precision = true_positive / max(1, int(predicted.sum()))
    recall = true_positive / max(1, int(actual.sum()))
    return {
        "rows": int(len(targets)),
        "accuracy": round(float((predicted == actual).mean()), 4),
        "requestPrecision": round(precision, 4),
        "requestRecall": round(recall, 4),
        "mentionRecall": round(float((~predicted & ~actual).sum() / max(1, int((~actual).sum()))), 4),
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--init", required=True, type=Path)
    parser.add_argument("--run", required=True)
    parser.add_argument("--epochs", type=int, default=12)
    parser.add_argument("--d-model", type=int, default=64)
    parser.add_argument("--layers", type=int, default=2)
    parser.add_argument("--ffn", type=int, default=128)
    parser.add_argument("--lexical-dropout", type=float, default=0.3)
    parser.add_argument("--synthetic", type=int, default=24000)
    parser.add_argument("--batch", type=int, default=128)
    parser.add_argument("--learning-rate", type=float, default=2e-3)
    parser.add_argument("--seed", type=int, default=20261122)
    parser.add_argument("--synthetic-weight", type=float, default=0.3,
                        help="Weight of the generated rows' loss next to the natural rows'")
    args = parser.parse_args()

    device = "mps" if torch.backends.mps.is_available() else "cpu"
    torch.manual_seed(args.seed)
    rng = np.random.default_rng(args.seed)
    run = ROOT / "runs" / args.run
    directory = ROOT / "data" / "synth" / args.run
    for folder in (run, directory):
        folder.mkdir(parents=True, exist_ok=True)

    document = json.loads(args.init.read_text())
    document["tensors"] = {
        name: entry for name, entry in document["tensors"].items() if not name.startswith("intent")
    }
    model = IntentEncoder(d_model=args.d_model, layers=args.layers, ffn=args.ffn).to(device)

    natural = natural_rows()
    train_natural = [row for row in natural if not heldout(row["id"])]
    test_natural = [row for row in natural if heldout(row["id"])]
    counts = {
        language: {
            "request": sum(1 for r in natural if r.get("lang") == language and r["intent"]),
            "mention": sum(1 for r in natural if r.get("lang") == language and not r["intent"]),
        }
        for language in ("en", "hi", "hinglish")
    }
    print(json.dumps({"stage": "natural", "rows": len(natural), "byLanguage": counts}), flush=True)
    test_set, test_targets = featurize(test_natural, directory / "natural-heldout")
    synthetic_test, synthetic_targets = featurize(
        synthetic_rows(4000, args.seed + 999, directory), directory / "synthetic-heldout"
    )

    optimizer = torch.optim.AdamW(model.parameters(), lr=args.learning_rate, weight_decay=0.01)
    history, best, started = [], -1.0, time.perf_counter()
    for epoch in range(args.epochs):
        rows = [
            {"id": f"{row['id']}#{variant}", "text": text, "intent": row["intent"], "natural": True}
            for row in train_natural
            for variant, text in enumerate(casings(row["text"]))
        ]
        rows += synthetic_rows(args.synthetic, args.seed + epoch * 131, directory)
        dataset, targets = featurize(rows, directory / f"train-{epoch}")
        natural_mask = np.array([bool(row.get("natural")) for row in rows])
        # Class balance per source: natural requests are a minority among
        # natural rows, generated rows are mostly requests. Balancing each
        # source on its own keeps the head from reading the source (writing
        # style) as the label.
        sample_weight = np.zeros(len(rows), dtype=np.float32)
        for mask, scale in ((natural_mask, 1.0), (~natural_mask, args.synthetic_weight)):
            share = float(targets[mask].mean()) if mask.any() else 0.5
            share = min(max(share, 1e-3), 1 - 1e-3)
            sample_weight[mask] = np.where(targets[mask] > 0.5, 0.5 / share, 0.5 / (1 - share)) * scale
        sample_weight *= len(rows) / sample_weight.sum()
        model.train()
        losses = []
        for indices in dataset.batches(args.batch, rng):
            batch_rows, _, _, valid, _ = dataset.batch(indices, device)
            if args.lexical_dropout > 0:
                # As for the tagger: hide word-form rows at random so an
                # unseen word's hashed rows are not trusted blindly.
                lexical = (batch_rows >= LEXICAL_ROWS.start) & (batch_rows < LEXICAL_ROWS.stop)
                hidden = torch.rand(batch_rows.shape[:2], device=batch_rows.device) < args.lexical_dropout
                batch_rows = torch.where(lexical & hidden.unsqueeze(-1), PADDING_ROW, batch_rows)
            logits = model(batch_rows, valid)
            target = torch.from_numpy(targets[indices]).to(device)
            weight = torch.from_numpy(sample_weight[indices]).to(device)
            loss = (F.binary_cross_entropy_with_logits(logits, target, reduction="none") * weight).mean()
            optimizer.zero_grad(set_to_none=True)
            loss.backward()
            optimizer.step()
            losses.append(loss.item())
        natural_metrics = report(intent_scores(model, test_set, device), test_targets, 0.5)
        synthetic_metrics = report(intent_scores(model, synthetic_test, device), synthetic_targets, 0.5)
        # Keep the epoch that best separates natural requests from mentions
        # while still reading generated schedules as requests.
        score = (
            natural_metrics["requestRecall"] + natural_metrics["mentionRecall"]
        ) / 2 * 0.7 + synthetic_metrics["accuracy"] * 0.3
        entry = {
            "epoch": epoch,
            "loss": round(sum(losses) / len(losses), 4),
            "trainRows": len(rows),
            "naturalHeldout": natural_metrics,
            "syntheticHeldout": synthetic_metrics,
            "seconds": round(time.perf_counter() - started, 1),
        }
        history.append(entry)
        print(json.dumps(entry), flush=True)
        if score > best:
            best = score
            torch.save({"model": model.state_dict(), "args": {**vars(args), "init": str(args.init)}}, run / "best.pt")

    model.load_state_dict(torch.load(run / "best.pt", map_location="cpu", weights_only=False)["model"])
    model.to("cpu").eval()
    intent_tensors = quantize_state_dict(model, 8)
    quantized = IntentEncoder(d_model=args.d_model, layers=args.layers, ffn=args.ffn)
    state = {}
    for name, entry in intent_tensors.items():
        raw = np.frombuffer(base64.b64decode(entry["data"]), dtype=np.int8)
        state[name] = torch.from_numpy(raw.astype(np.float32).copy()).reshape(entry["shape"]) * entry["scale"]
    quantized.load_state_dict(state)
    quantized.eval()
    # The cut-off: missing a real request costs more than letting a mention
    # through (a caller can still confirm), so keep 95% of held-out requests
    # and reject as many mentions as that allows. Calibrated on the held-out
    # training sources only, never on an evaluation set.
    natural_scores = intent_scores(quantized, test_set, "cpu")
    synthetic_scores = intent_scores(quantized, synthetic_test, "cpu")
    # Natural requests set the cut-off: the generated ones sit far above
    # any reasonable threshold and would hide where natural ones fall.
    requests = np.sort(natural_scores[test_targets >= 0.5])
    threshold = float(requests[int(len(requests) * 0.05)]) if len(requests) else 0.5
    threshold = min(threshold, 0.5)
    natural_quantized = report(natural_scores, test_targets, threshold)
    synthetic_quantized = report(synthetic_scores, synthetic_targets, threshold)

    export_directory = run / "export"
    export_directory.mkdir(parents=True, exist_ok=True)
    export = {key: value for key, value in document.items() if key not in ("tensors", "sha256")}
    tensors = dict(document["tensors"])
    tensors.update({f"intent.{name}": entry for name, entry in intent_tensors.items()})
    export["intentEncoder"] = {
        "dModel": args.d_model,
        "layers": args.layers,
        "heads": 4,
        "ffn": args.ffn,
        "hidden": INTENT_HIDDEN,
        "threshold": round(threshold, 4),
    }
    export["parameters"] = sum(
        entry["shape"][0] * int(np.prod(entry["shape"][1:])) for entry in tensors.values()
    )
    export["sha256"] = None
    export["tensors"] = tensors
    export["sha256"] = hashlib.sha256(json.dumps(export, sort_keys=True).encode()).hexdigest()
    (export_directory / "weights-transformer.json").write_text(json.dumps(export))

    # The tagger fixtures are unchanged; add the intent logit for each of
    # their sequences.
    source = args.init.parent
    for name in ("rows", "lengths", "logits"):
        candidate = source / f"transformer-parity.{name}.bin"
        if not candidate.exists():
            candidate = RUST / "evals" / "active" / f"transformer-parity.{name}.bin"
        shutil.copy2(candidate, export_directory / f"transformer-parity.{name}.bin")
    from train import ROWS_PER_TOKEN

    fixture_rows = np.fromfile(export_directory / "transformer-parity.rows.bin", dtype=np.uint16).reshape(-1, ROWS_PER_TOKEN)
    lengths = np.fromfile(export_directory / "transformer-parity.lengths.bin", dtype=np.uint32)
    intent_out, offset = [], 0
    with torch.no_grad():
        for length in lengths:
            rows = torch.from_numpy(fixture_rows[offset : offset + length].astype(np.int64)).unsqueeze(0)
            valid = torch.ones(1, int(length), dtype=torch.bool)
            intent_out.append(quantized(rows, valid)[0].item())
            offset += int(length)
    np.array(intent_out, dtype=np.float32).tofile(export_directory / "transformer-parity.intent.bin")

    result = {
        "init": str(args.init),
        "natural": counts,
        "epochs": history,
        "threshold": threshold,
        "quantizedNaturalHeldout": natural_quantized,
        "quantizedSyntheticHeldout": synthetic_quantized,
        "trainedAt": time.strftime("%Y-%m-%dT%H:%M:%S"),
    }
    (run / "report.json").write_text(json.dumps(result, indent=2))
    print(json.dumps({"stage": "done", "threshold": round(threshold, 4), "natural": natural_quantized, "synthetic": synthetic_quantized}), flush=True)


if __name__ == "__main__":
    main()
