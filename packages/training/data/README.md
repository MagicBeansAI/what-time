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

### Targeted V2 batch

`torch/generate-llm-corpus-v2.py` reproducibly expands the V2 brief into
20,000 template-authored rows; these are synthetic examples, not independent
external-model samples. `raw.manifest.json` records the seed, generator hash,
corpus hash and family/register counts. The English/Hindi/Hinglish counts
describe the carrier register; expressions deliberately mix scripts.

From `packages/training`, after building the Rust `validate-corpus` example:

```sh
uv run python torch/generate-llm-corpus-v2.py
uv run python torch/prepare-llm-corpus-v2.py
uv run python torch/test-corpus-v2.py
```

The preparation command runs the mandatory acceptance gate, reports coverage
in `data/llm/v2-validation.json`, and reserves 1,000 accepted rows for
evaluation. Minimal pairs stay together in the same split. Raw JSONL and its
grouping sidecar remain ignored; the small manifest and reports are tracked.
The commands replace this batch's raw corpus and evaluation split, so archive
an existing external corpus before generating a different batch.

`../V2_RUN.md` records the training run, measured results and remaining gaps.

## Background prose

`data/prose/` contains the filtered Tatoeba sentences the generators use as
surrounding carrier text; git-ignored, fetched via `../src/fetch-corpus.ts`
history in git. `corpus.json` is the tracked pin (URL, license, digest,
filters).
