"""Train the contextual tagger and write measured evaluation/checkpoint artifacts."""

from __future__ import annotations

import json
import hashlib
import subprocess
from pathlib import Path

import numpy as np
import torch
from torch.nn import functional as F

ROLE_CLASSES = 40
PADDING_ROW = 580  # the null feature row padding every token to 17

TORCH = Path(__file__).resolve().parent
ROOT = TORCH.parent



class Dataset:
    def __init__(self, prefix: Path):
        self.rows = np.fromfile(f"{prefix}.rows.bin", dtype=np.uint16).reshape(-1, 17)
        self.labels = np.fromfile(f"{prefix}.labels.bin", dtype=np.uint8)
        self.boundaries = np.fromfile(f"{prefix}.boundaries.bin", dtype=np.uint8)
        self.kinds = np.fromfile(f"{prefix}.kinds.bin", dtype=np.uint8)
        self.neighbors = np.fromfile(f"{prefix}.neighbors.bin", dtype=np.int16).reshape(
            -1, 2
        )
        self.offsets = np.fromfile(f"{prefix}.offsets.bin", dtype=np.uint32)
        self.lengths = np.diff(self.offsets)
        self.manifest = json.loads(Path(f"{prefix}.json").read_text())

    def __len__(self):
        return len(self.lengths)

    def batch(self, indices: np.ndarray, device: str):
        length = int(self.lengths[indices].max())
        length = 32 if length <= 32 else 64 if length <= 64 else 128
        rows = np.full((len(indices), length, 17), PADDING_ROW, dtype=np.int64)
        labels = np.full((len(indices), length), -100, dtype=np.int64)
        boundaries = np.zeros((len(indices), length), dtype=np.float32)
        valid = np.zeros((len(indices), length), dtype=np.bool_)
        neighbors = np.full((len(indices), length, 2), -1, dtype=np.int64)
        for destination, index in enumerate(indices):
            start, end = self.offsets[index : index + 2]
            size = int(end - start)
            rows[destination, :size] = self.rows[start:end]
            labels[destination, :size] = np.where(
                self.kinds[start:end] == 3,
                -100,
                self.labels[start:end].astype(np.int64),
            )
            boundaries[destination, :size] = self.boundaries[start:end]
            valid[destination, :size] = True
            neighbors[destination, :size] = self.neighbors[start:end]
        return tuple(
            torch.from_numpy(value).to(device)
            for value in (rows, labels, boundaries, valid, neighbors)
        )

    def batches(self, batch_size: int, rng: np.random.Generator | None = None):
        batches = []
        for lower, upper in [(0, 32), (32, 64), (64, 128)]:
            indices = np.flatnonzero((self.lengths > lower) & (self.lengths <= upper))
            if rng is not None:
                rng.shuffle(indices)
            batches.extend(
                indices[start : start + batch_size]
                for start in range(0, len(indices), batch_size)
            )
        if rng is not None:
            rng.shuffle(batches)
        return batches


def evaluate(
    model,
    dataset: Dataset,
    batch_size: int,
    device: str,
    boundary_threshold: float = 0,
) -> dict:
    model.eval()
    correct = total = sequences = exact = true_positive = false_positive = (
        false_negative
    ) = 0
    with torch.no_grad():
        for indices in dataset.batches(batch_size):
            rows, labels, boundaries, valid, neighbors = dataset.batch(indices, device)
            logits, boundary_logits = model(rows, valid, neighbors)
            roles = logits.argmax(-1)
            predicted_boundaries = boundary_logits >= boundary_threshold
            mask = labels >= 0
            right = (roles == labels) | ~mask
            boundary_right = (predicted_boundaries == boundaries.bool()) | ~mask
            correct += ((roles == labels) & mask).sum().item()
            total += mask.sum().item()
            exact += (right.all(1) & boundary_right.all(1)).sum().item()
            sequences += len(indices)
            true_positive += (
                (predicted_boundaries & boundaries.bool() & mask).sum().item()
            )
            false_positive += (
                (predicted_boundaries & ~boundaries.bool() & mask).sum().item()
            )
            false_negative += (
                (~predicted_boundaries & boundaries.bool() & mask).sum().item()
            )
    precision = true_positive / max(1, true_positive + false_positive)
    recall = true_positive / max(1, true_positive + false_negative)
    return {
        "tokens": total,
        "sequences": sequences,
        "tokenAccuracy": correct / max(1, total),
        "exactLabelAndBoundarySequence": exact / max(1, sequences),
        "boundaryPrecision": precision,
        "boundaryRecall": recall,
        "boundaryF1": 2 * precision * recall / max(1e-12, precision + recall),
        "boundaryCounts": {
            "truePositive": true_positive,
            "falsePositive": false_positive,
            "falseNegative": false_negative,
        },
    }


def merge(first: Dataset, second: Dataset) -> Dataset:
    merged = Dataset.__new__(Dataset)
    merged.rows = np.concatenate([first.rows, second.rows])
    merged.labels = np.concatenate([first.labels, second.labels])
    merged.boundaries = np.concatenate([first.boundaries, second.boundaries])
    merged.kinds = np.concatenate([first.kinds, second.kinds])
    merged.neighbors = np.concatenate([first.neighbors, second.neighbors])
    merged.offsets = np.concatenate(
        [first.offsets, second.offsets[1:] + first.offsets[-1]]
    )
    merged.lengths = np.diff(merged.offsets)
    merged.manifest = {"merged": True}
    return merged


def run_featurizer(jsonl: Path, prefix: Path) -> None:
    """Featurize with the Rust runtime tokenizer via the workspace example."""
    binary = (
        ROOT.parent.parent
        / "rust"
        / "target"
        / "release"
        / "examples"
        / "featurize"
    )
    if not binary.exists():
        raise SystemExit(
            f"Rust featurizer missing: {binary}.\n"
            "Build it once with: cargo build --release -p what-time --example featurize"
        )
    subprocess.run(
        [str(binary), str(jsonl), str(prefix)],
        check=True,
        stdout=subprocess.DEVNULL,
    )


def prepare_llm(count: int, directory: Path, rng: np.random.Generator) -> Dataset | None:
    source = ROOT / "data" / "llm" / "accepted.jsonl"
    if count <= 0 or not source.exists():
        return None
    lines = [line for line in source.read_text(encoding="utf-8").splitlines() if line.strip()]
    excluded = (ROOT / "data" / "llm" / "heldout.jsonl")
    if excluded.exists():
        held = {
            json.loads(line)["id"]
            for line in excluded.read_text(encoding="utf-8").splitlines()
            if line.strip()
        }
        lines = [line for line in lines if json.loads(line)["id"] not in held]
    if not lines:
        return None
    rng.shuffle(lines)
    chosen = lines[:count]
    path = directory / "llm.jsonl"
    path.write_text("\n".join(chosen) + "\n", encoding="utf-8")
    prefix = directory / "llm"
    run_featurizer(path, prefix)
    return Dataset(prefix)


def prepare(split: str, count: int, seed: int, directory: Path) -> Dataset:
    prefix = directory / split
    command = [
        "uv",
        "run",
        "--project",
        str(ROOT),
        "python",
        str(TORCH / "generate.py"),
        "--count",
        str(count),
        "--seed",
        str(seed),
        "--split",
        split,
        "--out",
        f"{prefix}.jsonl",
    ]
    reserved = directory / "heldout.fingerprints.json"
    if split != "heldout" and reserved.exists():
        command.extend(["--exclude", str(reserved)])
    subprocess.run(command, cwd=ROOT, check=True, stdout=subprocess.DEVNULL)
    run_featurizer(f"{prefix}.jsonl", prefix)
    return Dataset(prefix)
