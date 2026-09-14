# Training data

`data/gold/` contains tracked, authored evaluation fixtures — the source the
Rust workspace's `rust/evals/data/` corpora derive from. Expected schedules
come from hand-authored specifications, never from model predictions, so the
corpora guide development rather than serve as untouched test data. Records
are JSONL with an `id`, `text`, and expected `schedule` (a `null` schedule
means no schedule is expected).

| file | contents |
| -- | -- |
| `grammar.jsonl` | the authored grammar surface (single expressions) |
| `grammar-variations.jsonl` | mechanical variations of grammar (casing, spacing, abbreviations) |
| `adversarial.jsonl` | deliberately tricky compositions |
| `negatives.jsonl` | non-temporal text, including words that are time words in other contexts — `null` schedule is the correct answer |
| `prose.jsonl` | carrier sentences wrapping expressions |
| `labels.jsonl` | per-token label and clause-boundary annotations (compiler-oracle ground truth) |
| `results.jsonl` | end-to-end occurrences with resolution contexts |
| `user-cases.jsonl` | regression cases from real usage |

The multilingual expectations live in `rust/evals/data/multilingual.jsonl`
and are frozen by the Rust-side `freeze-multilingual` example after eyeball
verification.

## Generated data (never committed)

`data/synth/` holds the sampled training, validation, and heldout splits per
run plus the featurized `.bin` shards the trainer reads. It is git-ignored
and regenerated on demand:

```sh
cd packages/training
uv run python torch/generate.py --count 400000 --split train --seed N --out data/synth/<run>/train.jsonl
```

Supervision comes from the generators' semantic slots, never from the runtime
parser. Each split records a `.manifest.json` with the generator's own sha256
and a `.fingerprints.json` of structural signatures, which keeps
train/validation/heldout splits disjoint.

## LLM-authored corpus

`data/llm/` holds external-model-generated lines (English/Hindi/Hinglish) —
see `../LLM_CORPUS_PROMPT.md` for the brief and
`../torch/validate_llm_corpus.py` for the acceptance gate. `raw.jsonl` is
what the external model produced; `accepted.jsonl` is what passed the gate;
`heldout.jsonl` is a 3,000-line evaluation slice excluded from training.
All are git-ignored (regenerable only by the external model, so archive them
out-of-band if reproducibility matters).

## Background prose

`data/prose/` contains the filtered Tatoeba sentences the generators use as
surrounding carrier text; git-ignored, fetched via `../src/fetch-corpus.ts`
history in git. `corpus.json` is the tracked pin (URL, license, digest,
filters).
