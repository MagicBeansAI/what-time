# what-time model card

## Model

The active model is a ~144k-parameter transformer encoder (two pre-LayerNorm
blocks, four heads, feed-forward width 256, learned positions, mean-pooled
context head), stored with symmetric per-tensor int8 quantization
(`rust/what-time/assets/weights-transformer.json`). Inference is a pure-Rust
forward pass (`rust/what-time/src/model/transformer.rs`) mirrored op-for-op
from the training network; exported parity fixtures gate logit drift below
2e-3.

The model predicts one of 35 semantic roles per token plus a clause-boundary
score. Timezones, calendar arithmetic, DST, and the reference instant are
handled after the model runs, in exact Rust code. A retired gated-scan
predecessor matched no better than this model on any gold corpus and was
removed.

## Intended use

Short English, Hindi (Devanagari), and Hinglish (romanized Hindi)
scheduling phrases in reminders, forms, chat, and assistants — on-device or
in-browser (WebAssembly). Not suitable for document-scale extraction, legal
or medical scheduling, or any case where a silently wrong date has real
consequences. कल/parso (yesterday-vs-tomorrow) resolves by documented
default: future reading unless a past-tense cue appears in the sentence.

## Training data

Two sources, both generator-verifiable:

1. **Structural supervision** — the Python generators in
   `packages/training/torch/` (phrase families, augmentation, background
   prose from filtered Tatoeba sentences). Labels come from the generator's
   structure, never from the runtime parser.
2. **LLM-authored corpora** — ~180k English/Hindi/Hinglish lines generated
   by an external model against `packages/training/LLM_CORPUS_PROMPT.md`
   (label taxonomy, hard conventions, reserved-phrase exclusions), then
   filtered by a mechanical gate (`torch/validate_llm_corpus.py`: label set,
   tokenizer alignment, convention linting, dedupe, and a compile oracle).
   The original batch had ~89% acceptance; all rejections are reported.
   Training mixes these at ≤25% with structural data as the anchor.

The V2 update uses 20,000 reproducible, template-authored rows following the
targeted English/Hindi/Hinglish brief. These are synthetic expansions, not
independent external-model samples. The gate accepts 19,501 rows (97.505%);
1,000 are reserved for evaluation, keeping contrast groups together, and
18,501 enter training. The original V1 corpus and its 3,000-row held-out slice
were not available in this checkout, so that earlier evaluation is not
repeated. Reserved carrier phrases remain excluded from training.

See [`packages/training/V2_RUN.md`](packages/training/V2_RUN.md) for the
commands, measured comparison, training proportions and remaining gaps.

## Metrics (current checkpoint — `quarter-gaps-r2`, shipped as v0.2.2)

Per-build history lives in [`SCORES.md`](SCORES.md).

- Pinned corpora: 568/568 schedule cases (zero known gaps), 15/15
  multilingual fixtures, 22/22 end-to-end occurrence cases.
- LLM held-out: 0.9955 exact token-label/boundary sequences on a 2,000-row
  union slice (1,000 of them from a never-trained external-model corpus).
- Generator held-out: 0.9606 on 5,000 rows; extra families: 0.9950 on
  4,983 rows. These are synthetic, generator-related evaluations, not
  real-user accuracy claims.
- Numeric parity: exported fixtures reproduce the quantized training
  network's logits within 2e-3 with identical argmax.

## Known limitations

- Coverage includes तारीख/tarikh (all common romanizations), bare "ki 15",
  daily roz/roj/rozz, Devanagari numerals, mins durations, postposed
  से…तक date and clock ranges, and common shorthand. Unseen contexts and
  vocabulary can still mislabel.
- Quarter periods and tabulated दिवाली (2025–2030) are covered; English
  "Diwali", तिमाही/timahi and होली/Holi return diagnostics until corpus
  rows exist for them.
- Ambiguous "mn" was omitted from training.
- EOD/COB/EOW/EOM select the end of a calendar period; they do not imply a
  configured business closing hour.
- A day-part before an o'clock-style marker ("शाम को आठ बजे") biases the
  hour; an explicit am/pm always wins.
- English-only phrasings outside the generator families (terse forms,
  unusual locales) are unmeasured on real user language.
- Quantization is int8 per-tensor; measured loss against float weights is
  within eval noise.

## Retraining

`packages/training`: `pnpm train` (or `uv run python torch/train_transformer.py`).
Requires the Rust featurizer (`cargo build --release -p what-time --example
featurize`) so training tokenization matches runtime tokenization exactly.
The export writes the weights JSON and parity fixtures into `rust/` and
`cargo test` fails closed if predictions shift.
