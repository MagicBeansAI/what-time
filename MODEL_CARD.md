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
   tokenizer alignment, convention linting, dedupe, and a compile oracle for
   Latin-script lines). ~89% acceptance; all rejections are reported.
   Training mixes these at ≤25% with structural data as the anchor.

A 3,000-line LLM held-out split is excluded from training and evaluated
every epoch. Reserved carrier phrases stay out of training so unseen-carrier
generalization remains measurable.

## Metrics (current checkpoint)

- Gold corpora (hand-authored, all three languages): 526/526 schedule
  structures, 18/18 end-to-end occurrence cases.
- LLM held-out (unseen external lines): 0.9997 exact token-label sequences.
- Generator held-out: 0.9498 exact sequences (in-distribution; the
  structural families are the model's own generators, so this is not a
  real-user accuracy claim).
- Numeric parity: exported fixtures reproduce the quantized training
  network's logits within 2e-3 with identical argmax.

## Known limitations

- Vocabulary outside the training corpora can mislabel: तारीख/tareekh,
  unsuffixed Hinglish "ki 15" (use "ki 15th"), "roz", and Devanagari
  numerals are current known gaps; add them to the corpus brief for the
  next batch.
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
