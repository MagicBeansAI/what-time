"""Train the proof-of-concept transformer tagger and export it for Rust.

Reuses the standard supervision pipeline (train.prepare) plus the extra
families in transformer_families.py, trains on MPS when available, calibrates
the boundary threshold, quantizes to symmetric per-tensor int8, and writes:
  - packages/training/runs/transformer-poc/report.json
  - ../../../rust/what-time/assets/weights-transformer.json
  - ../../../rust/evals/active/transformer-parity.{rows,lengths,logits}.bin
Parity fixtures are computed from the quantized weights so Rust mirrors them.
"""
from __future__ import annotations

import argparse
import base64
import hashlib
import json
import math
import random
import subprocess
import time
from pathlib import Path

import numpy as np
import torch
from torch.nn import functional as F

from train import ROLE_CLASSES
from train import Dataset, evaluate, prepare, prepare_llm
from transformer import TimeTransformer
import transformer_families as families

TORCH_DIR = Path(__file__).resolve().parent
ROOT = TORCH_DIR.parent
RUST = ROOT.parent.parent / "rust"


def extra_corpus(count: int, seed: int, split: str, directory: Path) -> Dataset:
    """Render the extra families and featurize with the Rust tokenizer."""
    pyrng = torch.Generator().manual_seed(seed)
    prefix = directory / f"extra-{split}"
    path = prefix.with_suffix(".jsonl")
    with path.open("w") as output:
        for index in range(count):
            seed_value = int(torch.randint(0, 2**31 - 1, (1,), generator=pyrng).item())
            sentence = families.render(random.Random(seed_value))
            row = {
                "id": f"extra-{split}-{seed_value}-{index}",
                "template": "transformer-family",
                "text": sentence.text,
                "spans": sentence.spans,
            }
            output.write(json.dumps(row, ensure_ascii=False) + "\n")
    from train import run_featurizer

    run_featurizer(path, prefix)
    return Dataset(prefix)


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


def calibrate_threshold(model, dataset: Dataset, device: str) -> float:
    model.eval()
    best = (0.0, 0.5)
    scores = []
    with torch.no_grad():
        for indices in dataset.batches(256):
            rows, labels, boundaries, valid, _ = dataset.batch(indices, device)
            _, boundary_logits = model(rows, valid)
            mask = labels >= 0
            scores.append(
                torch.stack(
                    [boundary_logits[mask].cpu(), boundaries[mask].cpu()], dim=1
                )
            )
    if not scores:
        return 0.5
    flat = torch.cat(scores)
    for threshold in [i / 10 for i in range(0, 31)]:
        predicted = flat[:, 0] >= threshold
        actual = flat[:, 1] >= 0.5
        tp = (predicted & actual).sum().item()
        fp = (predicted & ~actual).sum().item()
        fn = (~predicted & actual).sum().item()
        precision = tp / max(1, tp + fp)
        recall = tp / max(1, tp + fn)
        f1 = 2 * precision * recall / max(1e-12, precision + recall)
        if f1 > best[0] + 1e-12:
            best = (f1, threshold)
    return round(best[1], 1)


def quantize_state_dict(model: TimeTransformer, bits: int = 8):
    """Symmetric per-tensor int8/int6; returns {name: {shape, scale, base64}}."""
    level = (1 << (bits - 1)) - 1
    tensors = {}
    state = model.state_dict()
    for name, value in state.items():
        if name == "row_map":
            continue
        value = value.detach().float()
        maximum = value.abs().max().clamp_min(1e-8)
        scale = maximum / level
        quantized = torch.round(value / scale).clamp(-level, level).to(torch.int8)
        tensors[name] = {
            "shape": list(value.shape),
            "scale": scale.item(),
            "dtype": f"int{bits}",
            "data": base64.b64encode(
                quantized.numpy().tobytes(order="C")
            ).decode("ascii"),
        }
    return tensors


def dequantized_model(model: TimeTransformer, tensors) -> TimeTransformer:
    clone = TimeTransformer(
        feature_rows=model.feature_rows,
        d_model=model.d_model,
        heads=model.heads,
        layers=len(model.blocks),
        ffn=model.blocks[0].ff1.out_features,
    )
    state = {}
    for name, entry in tensors.items():
        raw = np.frombuffer(base64.b64decode(entry["data"]), dtype=np.int8)
        values = torch.from_numpy(raw.astype(np.float32).copy()).reshape(entry["shape"])
        state[name] = values * entry["scale"]
    clone.load_state_dict(state, strict=True)
    clone.eval()
    return clone


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--epochs", type=int, default=6)
    parser.add_argument("--samples", type=int, default=200000)
    parser.add_argument("--extra-samples", type=int, default=40000)
    parser.add_argument("--eval-samples", type=int, default=5000)
    parser.add_argument("--batch", type=int, default=256)
    parser.add_argument("--seed", type=int, default=20260912)
    parser.add_argument("--learning-rate", type=float, default=1.5e-3)
    parser.add_argument("--init", default=None,
                        help="Warm-start from a saved checkpoint (short fine-tunes)")
    parser.add_argument("--warmup-steps", type=int, default=300)
    parser.add_argument("--run", default="transformer-poc")
    parser.add_argument("--d-model", type=int, default=64)
    parser.add_argument("--layers", type=int, default=2)
    parser.add_argument("--heads", type=int, default=4)
    parser.add_argument("--ffn", type=int, default=256)
    parser.add_argument("--quantization-bits", type=int, choices=[6, 8], default=8)
    parser.add_argument("--fresh-every", type=int, default=1,
                        help="Regenerate the training corpus every N epochs")
    parser.add_argument("--qat-start", type=int, default=0,
                        help="Project weights onto the quantization grid every step from this epoch on")
    parser.add_argument(
        "--fresh-each-epoch",
        action="store_true",
        help="Draw a fresh training corpus every epoch, like the promoted pipeline",
    )
    args = parser.parse_args()

    device = "mps" if torch.backends.mps.is_available() else "cpu"
    rng = np.random.default_rng(args.seed)
    run = ROOT / "runs" / args.run
    run.mkdir(parents=True, exist_ok=True)
    directory = ROOT / "data" / "synth" / args.run
    directory.mkdir(parents=True, exist_ok=True)

    print(json.dumps({"stage": "preparing-data", "device": device}), flush=True)
    heldout = prepare("heldout", args.eval_samples, args.seed + 2, directory)
    extra_eval = extra_corpus(args.eval_samples, args.seed + 3, "eval", directory)
    llm_heldout = None
    llm_source = ROOT / "data" / "llm" / "heldout.jsonl"
    if llm_source.exists():
        prefix = directory / "llm-eval"
        subprocess.run(
            [
                str(ROOT.parent.parent / "rust" / "target" / "release" / "examples" / "featurize"),
                str(llm_source),
                str(prefix),
            ],
            check=True,
            stdout=subprocess.DEVNULL,
        )
        llm_heldout = Dataset(prefix)
    training = prepare("train", args.samples, args.seed, directory)
    extra_train = extra_corpus(args.extra_samples, args.seed + 1, "train", directory)
    training = merge(training, extra_train)
    llm_count = int(args.samples * 0.25 / 0.75)
    llm = prepare_llm(llm_count, directory, rng)
    if llm is not None:
        training = merge(training, llm)
        print(json.dumps({"stage": "llm-mix", "sequences": len(llm)}), flush=True)

    model = TimeTransformer(
        d_model=args.d_model, layers=args.layers, heads=args.heads, ffn=args.ffn
    ).to(device)
    if args.init:
        if args.init.endswith(".json"):
            # Warm-start from an exported weights file (int8), so a run can
            # resume from the deployed model even when no .pt remains.
            import base64

            export = json.loads(Path(args.init).read_text())
            state = {}
            for name, entry in export["tensors"].items():
                raw = np.frombuffer(
                    base64.b64decode(entry["data"]), dtype=np.int8
                )
                values = torch.from_numpy(raw.astype(np.float32).copy()).reshape(
                    entry["shape"]
                )
                state[name] = values * entry["scale"]
            model.load_state_dict(state)
        else:
            checkpoint = torch.load(args.init, map_location="cpu", weights_only=False)
            model.load_state_dict(checkpoint["model"])
        print(json.dumps({"stage": "warm-start", "from": args.init}), flush=True)
    parameters = sum(p.numel() for p in model.parameters())
    optimizer = torch.optim.AdamW(model.parameters(), lr=args.learning_rate)

    history = []
    best = -1.0
    step = 0
    started = time.perf_counter()
    for epoch in range(args.epochs):
        regenerate = (
            epoch > 0
            and (args.fresh_each_epoch or epoch % args.fresh_every == 0)
        )
        if regenerate:
            training = merge(
                prepare("train", args.samples, args.seed + epoch * 101, directory),
                extra_corpus(args.extra_samples, args.seed + epoch * 103, "train", directory),
            )
            llm = prepare_llm(llm_count, directory, rng)
            if llm is not None:
                training = merge(training, llm)
        model.train()
        batches = training.batches(args.batch, rng)
        losses = []
        epoch_started = time.perf_counter()
        for batch_index, indices in enumerate(batches):
            step += 1
            progress = (epoch + batch_index / len(batches)) / args.epochs
            learning_rate = (
                1e-4
                + (args.learning_rate - 1e-4)
                * (1 + math.cos(math.pi * progress))
                / 2
            ) * min(1, step / args.warmup_steps)
            for group in optimizer.param_groups:
                group["lr"] = learning_rate
            rows, labels, boundaries, valid, _ = training.batch(indices, device)
            logits, boundary_logits = model(rows, valid)
            role_loss = F.cross_entropy(
                logits.reshape(-1, ROLE_CLASSES),
                labels.reshape(-1),
                ignore_index=-100,
                label_smoothing=0.05,
            )
            boundary_loss = F.binary_cross_entropy_with_logits(
                boundary_logits, boundaries
            )
            loss = role_loss + boundary_loss
            optimizer.zero_grad(set_to_none=True)
            loss.backward()
            torch.nn.utils.clip_grad_norm_(model.parameters(), 1.0)
            optimizer.step()
            if epoch >= args.qat_start > 0:
                # Quantization-aware projection: keep every weight exactly
                # representable in the exported grid, so float training and
                # int8 inference agree by construction.
                with torch.no_grad():
                    for parameter in model.parameters():
                        level = (1 << (args.quantization_bits - 1)) - 1
                        scale = parameter.abs().max().clamp_min(1e-8) / level
                        parameter.copy_(torch.round(parameter / scale) * scale)
            losses.append(loss.item())
        threshold = calibrate_threshold(model, heldout, device)
        heldout_metrics = evaluate(model, heldout, 256, device, threshold)
        extra_metrics = evaluate(model, extra_eval, 256, device, threshold)
        llm_metrics = (
            evaluate(model, llm_heldout, 256, device, threshold)
            if llm_heldout is not None
            else None
        )
        score = heldout_metrics["exactLabelAndBoundarySequence"]
        entry = {
            "epoch": epoch,
            "loss": sum(losses) / len(losses),
            "seconds": round(time.perf_counter() - epoch_started, 1),
            "learningRate": learning_rate,
            "boundaryThreshold": threshold,
            "heldout": heldout_metrics,
            "extraFamilies": extra_metrics,
            **({"llmHeldout": llm_metrics} if llm_metrics else {}),
        }
        history.append(entry)
        print(json.dumps(entry), flush=True)
        if score > best:
            best = score
            torch.save(
                {"model": model.state_dict(), "args": vars(args)},
                run / "best.pt",
            )
    del model

    checkpoint = torch.load(run / "best.pt", map_location="cpu", weights_only=False)
    model = TimeTransformer(
        d_model=args.d_model, layers=args.layers, heads=args.heads, ffn=args.ffn
    )
    model.load_state_dict(checkpoint["model"])
    model.eval()
    threshold = calibrate_threshold(model, heldout, "cpu")
    tensors = quantize_state_dict(model, args.quantization_bits)
    quantized = dequantized_model(model, tensors)
    quantized_metrics = evaluate(quantized, heldout, 256, "cpu", threshold)
    extra_quantized = evaluate(quantized, extra_eval, 256, "cpu", threshold)

    assets = RUST / "what-time" / "assets"
    evals = RUST / "evals" / "active"
    export = {
        "format": "what-time-transformer/1",
        "featureRows": 324,
        "paddingRow": 324,
        "dModel": args.d_model,
        "layers": args.layers,
        "heads": args.heads,
        "ffn": args.ffn,
        "maxPositions": 128,
        "roleClasses": ROLE_CLASSES,
        "boundaryThreshold": threshold,
        "parameters": sum(entry["shape"][0] * int(np.prod(entry["shape"][1:])) for entry in tensors.values()),
        "sha256": None,
        "tensors": tensors,
    }
    blob = json.dumps(export, sort_keys=True)
    export["sha256"] = hashlib.sha256(blob.encode()).hexdigest()
    (assets / "weights-transformer.json").write_text(json.dumps(export))

    # Parity fixtures from the quantized weights on extra-family sequences.
    rows_out, logits_out, lengths = [], [], []
    with torch.no_grad():
        for index in range(min(64, len(extra_eval))):
            start, end = extra_eval.offsets[index], extra_eval.offsets[index + 1]
            rows = torch.from_numpy(
                extra_eval.rows[start:end].astype(np.int64)
            ).unsqueeze(0)
            valid = torch.ones(1, rows.shape[1], dtype=torch.bool)
            logits, boundary_logits = quantized(rows, valid)
            size = rows.shape[1]
            lengths.append(size)
            rows_out.append(extra_eval.rows[start:end])
            logits_out.append(
                torch.cat(
                    [logits[0], boundary_logits[0, :, None]], dim=1
                )
                .numpy()
                .astype(np.float32),
            )
    rows_flat = np.concatenate(rows_out)
    logits_flat = np.concatenate(logits_out)
    rows_flat.tofile(evals / "transformer-parity.rows.bin")
    np.array(lengths, dtype=np.uint32).tofile(evals / "transformer-parity.lengths.bin")
    logits_flat.tofile(evals / "transformer-parity.logits.bin")

    report = {
        "device": device,
        "parameters": parameters,
        "quantizedParameters": export["parameters"],
        "epochs": history,
        "bestHeldoutExact": best,
        "boundaryThreshold": threshold,
        "quantizedHeldout": quantized_metrics,
        "quantizedExtraFamilies": extra_quantized,
        "trainedAt": time.strftime("%Y-%m-%dT%H:%M:%S"),
    }
    (run / "report.json").write_text(json.dumps(report, indent=2))
    print(
        json.dumps(
            {
                "stage": "done",
                "heldoutExact": quantized_metrics["exactLabelAndBoundarySequence"],
                "extraExact": extra_quantized["exactLabelAndBoundarySequence"],
                "threshold": threshold,
            }
        ),
        flush=True,
    )


if __name__ == "__main__":
    main()
