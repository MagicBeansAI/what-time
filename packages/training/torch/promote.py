"""Copy a candidate export (train_transformer.py --candidate) into the Rust
tree: the weights and the three parity fixtures move together, as the
`cargo test` parity gate requires.

Usage: uv run python torch/promote.py <run>
"""
from __future__ import annotations

import shutil
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RUST = ROOT.parent.parent / "rust"
FIXTURES = ["rows", "lengths", "logits"]


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: promote.py <run>")
    export = ROOT / "runs" / sys.argv[1] / "export"
    weights = export / "weights-transformer.json"
    fixtures = [export / f"transformer-parity.{name}.bin" for name in FIXTURES]
    missing = [path for path in [weights, *fixtures] if not path.exists()]
    if missing:
        raise SystemExit(f"not a complete candidate export: missing {missing[0]}")
    shutil.copy2(weights, RUST / "what-time" / "assets" / weights.name)
    for path in fixtures:
        shutil.copy2(path, RUST / "evals" / "active" / path.name)
    print(f"promoted {sys.argv[1]}: run cargo test --manifest-path rust/Cargo.toml")


if __name__ == "__main__":
    main()
