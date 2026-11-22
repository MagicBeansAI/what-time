# Project context

`what-time` is a monorepo holding one Rust implementation of a neural
schedule parser (English / Hindi / Hinglish) and the Python pipeline that
trains its model. Keep naming kebab-case and the layout as-is.

## Layout

- `rust/` — cargo workspace: `what-time` (library + full test/eval suite),
  `what-time-cli`, `what-time-wasm` (browser bindings), `site/` (static
  playground; `what-time.html` is the single-file build), and
  `what-time-web` (optional local HTTP API).
- `packages/what-time` — npm package for TS/JS apps; a thin wrapper around
  the wasm module. Published as `@magicbeansai/what-time` (scoped; publish
  with `npm publish --access public`). No parallel parser implementation
  exists or should be added: all logic lives in Rust.
- `packages/training` — uv project. Corpus generators, the LLM-corpus brief
  (`LLM_CORPUS_PROMPT.md`) and validation gate, and the transformer
  trainer that exports int8 weights into `rust/what-time/assets/`.

## Validation

```sh
cargo test --manifest-path rust/Cargo.toml   # library suite + gold corpora + parity gates
rust/site/build.sh                           # wasm + static playground rebuild
cargo build --release -p what-time --example featurize   # required before training
cd packages/training && uv run python torch/fetch_prose.py   # once per checkout: background prose
cd packages/training && pnpm train           # retrain + export weights
# or keep the export aside until it passes cargo test:
uv run python torch/train_transformer.py --run <run> --candidate && uv run python torch/promote.py <run>
```

After promoting weights, refresh every embedded copy of the wasm:
`rust/site/build.sh`, then copy `rust/site/wasm/what_time_wasm{.js,.d.ts,_bg.wasm,_bg.wasm.d.ts}`
into `packages/what-time/wasm/`.

## Rules

- One source of truth: parsing logic changes go in `rust/what-time`. The
  npm wrapper and the wasm bindings must stay thin.
- Never train on the runtime parser's own output. Supervision comes from
  the generators or gate-checked external corpora.
- `torch/validate_llm_corpus.py` takes no flags: any run rewrites
  `data/llm/accepted.jsonl` from `raw.jsonl` alone, dropping the archived and
  supplement rows merged into the training pool. Copy the pool aside before
  running it (a training run's `data/synth/<run>/llm.jsonl` holds every
  non-held-out pool row if it is ever lost).
- LLM-authored corpus data must pass `torch/validate_llm_corpus.py` before
  training and stays ≤25% of any training mix.
- The intent encoder trains with `torch/train_intent.py` from an existing
  tagger export (`--init`), which it copies unchanged. Its data lives in
  `data/intent/`: Tatoeba samples (`fetch_intent_prose.py`, pinned in
  `corpus.json` as `intentProse`), their request/mention labels, and
  `*-written.jsonl` lines with their intent. Never add evaluation-set lines
  to it, and calibrate the cut-off on held-out training rows only.
- Keep `natural.py::RESERVED` phrases out of training corpora; they back
  the unseen-carrier evaluation.
- Timezone and calendar arithmetic stays in exact Rust code, never in the
  model.
- `rust/what-time/assets/weights-transformer.json` and the
  `rust/evals/active/transformer-parity.*` fixtures move together: both are
  written by the training export, and `cargo test` fails closed if
  predictions shift. Do not hand-edit them.
- Python is always invoked through `uv` inside `packages/training`. The
  Rust toolchain lives at `~/.cargo/bin` when not on PATH.
- The repo derives from an MIT-licensed project; keep the original
  copyright line in `LICENSE` intact.

## Releasing

Follow `RELEASING.md`. Tag-driven (`git tag vX.Y.Z`); the workflow gates on
tag = cargo workspace version = package.json version, runs the full test
suite, builds wasm fresh, publishes `@magicbeansai/what-time`. Known failure
mode: publish 404 = npmjs.com org `magicbeansai` missing (create once, then
re-run failed jobs); the workflow now checks for this and says so.

## History

The first iteration was a TypeScript/WebGPU implementation with a
gated-scan model; it was fully retired after the transformer matched it on
every gold corpus. Its history remains in git, and `LICENSE` retains the
original copyright as MIT requires.
