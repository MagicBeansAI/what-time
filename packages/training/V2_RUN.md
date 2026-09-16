# V2 corpus update — 2026-09-16

Generated 20,000 unique, template-authored English/Hindi/Hinglish examples
from the V1 conventions and V2 addendum. Labels are authored in the
generator, never taken from runtime predictions. This is a reproducible
synthetic batch, not an independent sample from an external model.

The carrier registers are English 6,600, Hindi 6,600 and Hinglish 6,800;
mixed scripts within expressions are intentional. The batch includes
3,000 day-part contrast rows and 1,800 balanced past/future tense rows.
Short standalone forms and recurrences with day-parts but no clock are
included explicitly.

## Data and validation

- `data/llm/raw.jsonl`: 20,000 rows, each under 40 tokens, with text exactly
  reconstructed by joining its tokens with single spaces.
- `accepted.jsonl`: 19,501 rows (97.505%).
- `rejected.jsonl`: 499 rows, all Hindi/Hinglish postposed date ranges
  (`X tareekh se Y tareekh tak`). The gate retains diagnostic details.
- `heldout.jsonl`: 1,000 accepted rows, excluded from training. Whole
  contrast groups stay together; 18,501 accepted rows remain for training.
- No reserved carrier phrases or exact duplicates. `mn` is omitted because
  the brief does not give it an unambiguous meaning or label.

The small tracked files [raw.manifest.json](data/llm/raw.manifest.json) and
[v2-validation.json](data/llm/v2-validation.json) record hashes, counts and
coverage. JSONL data stays git-ignored and can be regenerated:

```sh
~/.cargo/bin/cargo build --manifest-path rust/Cargo.toml --example validate-corpus
~/.cargo/bin/cargo build --manifest-path rust/Cargo.toml --release -p what-time --example featurize
cd packages/training
uv run python torch/generate-llm-corpus-v2.py
uv run python torch/prepare-llm-corpus-v2.py
uv run python torch/test-corpus-v2.py
```

These commands replace the batch and its evaluation slice. The source brief
is retained unchanged. `gen:llm:v2` and `prepare:llm:v2` are also available as
pnpm scripts.

## Runtime changes needed by the brief

The initial compile gate accepted only 10,495 rows. The Rust implementation
now recognizes Devanagari numeral clusters, preserves their source offsets,
and gives them the same numeric features as ASCII digits. It supports
date-selector units, the additional daily-recurrence spellings, quantity
labels on durations, and the specified casual/corporate shorthand.

Fractional clocks retain precedence over a preceding day-part. Hindi tense
resolution now sees background cues such as `आया था` after expression
extraction, with token boundaries preventing accidental matches in names.
Calendar and timezone arithmetic remain in Rust. The acceptance gate also
fails closed if its oracle crashes or produces an incomplete report, and
writes correct UTF-16 span offsets for text containing supplementary Unicode.

## Training and results

Training warm-started from the original int8 weights and ran on MPS. Four
stages retained the best generated-evaluation checkpoint at each stage:

| Run | Epochs | Structural rows/epoch | Extra rows requested/epoch |
| --- | ---: | ---: | ---: |
| `corpus-v2` | 3 | 60,000 | 40,000 |
| `corpus-v2-final` | 6 | 200,000 | 40,000 |
| `corpus-v2-promoted` | 3 | 200,000 | 10,000 |
| `corpus-v2-complete` | 2 | 200,000 | 10,000 |

All used learning rate `0.0001`, 30 warmup steps and the trainer's default
seed `20260912`. Each included at most 18,501 gated V2 training rows. The
largest V2 share stayed below 15.7%; the final stage used about 8.1%, below the
25% ceiling. No runtime-predicted labels entered training.
Actual final-epoch sequence counts and evaluation-overlap checks for each
stage are recorded in [v2-training-mix.json](data/llm/v2-training-mix.json).

The later corpus refinements preserved row order and contrast-group sizes.
Before each refinement, the new evaluation texts were checked against prior
training texts: overlap was zero. The older V1 corpus and its historical
3,000-row evaluation slice were absent and could not be re-evaluated.

Final-stage invocation, from `packages/training`:

```sh
uv run python torch/train_transformer.py \
  --run corpus-v2-complete \
  --init ../../rust/what-time/assets/weights-transformer.json \
  --epochs 2 --samples 200000 --extra-samples 10000 \
  --learning-rate 0.0001 --warmup-steps 30
```

The checked-in weights and all three parity fixtures were written together
by the training exporter. The table below compares the original and final
**quantized** weights on identical final evaluation rows:

| Evaluation | Rows | Original exact sequences | Final exact sequences |
| --- | ---: | ---: | ---: |
| Targeted V2 | 1,000 | 25.50% | 99.80% |
| Structural generator | 5,000 | 94.14% | 95.34% |
| Extra generated families | 4,982 | 68.97% | 99.28% |

Full token/boundary metrics and model hashes are in
[v2-metrics.json](data/llm/v2-metrics.json). These are synthetic evaluations
with shared generator structures; they do not measure unrestricted real-user
accuracy. The first training pass reduced broad accuracy, which motivated
the larger structural mix. A later multilingual regression motivated the
standalone day-part recurrence examples before the final export.

## Verification

The full Rust workspace suite passes, including all 526 schedule gold cases,
15 multilingual fixtures, 18 occurrence cases, numeric parity, and 20 new
V2 phrase regressions. Five Python checks cover corpus shape, reserved phrases,
contrast grouping, Unicode tokenization/spans and oracle failure handling.
The browser playground, its embedded single-file build and the npm wasm
module are regenerated from the same final Rust/model source.
The npm wrapper builds and passes all six existing tests; six additional V2
schedule checks pass through the rebuilt WebAssembly module. The embedded
HTML, playground and npm WebAssembly bytes match exactly.

The 499 rejected range examples remain available for inspection and are not
part of the training mix. EOD/COB mean the end of a calendar day by the brief's
convention, not a specific business closing hour.

## Post-run fix: postposed date ranges reclaimed

The 499 rejections were one family: Hindi/Hinglish postposed date ranges
(`20 तारीख से 24 तारीख तक`) — both markers follow the date they govern,
unlike English "from X until Y". The compiler now treats a trailing
तक/tak with calendar material before it as a complete range, and a से
arriving after a filled start calendar opens the end calendar. All
20,000 rows pass the gate (`rejected.jsonl` is empty); the batch was
re-staged with the same heldout seed, so the next training round sees
19,000 V2 rows instead of 18,501. The numbers above describe the
original run and were not re-measured. See the regression test
`compiles_a_postposed_hindi_date_range` in `rust/what-time/tests/compile_cases.rs`.

## External batch (2026-09-16, post-mining)

An external model authored 20,000 labeled rows (informed by 34 mined
weaknesses; `MINING.md` documents the loop). Gate results after two
compiler-side repairs:

- `tarikh` (single-a romanization of तारीख) added to the date-selector
  word list — 2,329 rows reclaimed.
- Postposed clock ranges (`sava char se paune paanch tak`): a trailing
  तक/tak with an earlier से links two clocks, not just two dates — 338
  rows reclaimed.
- Inflected Hindi ordinals (दूसरे/तीसरे/चौथे/पाँचवे + romanizations)
  added to `spoken_quantity` — 45 rows reclaimed.

Accepted 21,250/21,555 (a 1,555-row authored day-group supplement from
`torch/supplement-daygroup.py` is included; 142 of its rows were rejected
for un-split `10:30` tokens — the gate catching our own convention slip).
Known remaining rejects, documented for the next round: `next quarter`
needs a `Unit::quarter` variant plus resolve arithmetic (245 rows), and
दिवाली needs a holiday-date table (62 rows).

The pool merges the external batch, the supplement, and the archived V2
batch (`data/llm/archive-v2/`, ids prefixed `v2-`) — 40,250 accepted, a
2,000-row union heldout, 39,250 training rows.

Training (`llm-ext-r5`, warm-start from the V2 export, 3 epochs, 200k
structural + 15k LLM/epoch, lr 1e-4): structural heldout 96.06% exact
(95.34% before), extra families 99.50%, LLM heldout 99.45%. Bare
"weekdays"/"weekends" had drifted to RECUR/HOLIDAY after the first
retrains — the v1 LLM corpus that originally taught that family is gone,
and neither V2 nor the external batch contains a single weekday word.
Repairs that made the family stick: a dedicated `family-bare-daygroup`
injection (3%) and whole-sentence uppercase augmentation (5%) in
`generate.py`, plus the supplement above. The full Rust suite, npm
tests, and both wasm builds are green; weights, parity fixtures, and
wasm moved together.
