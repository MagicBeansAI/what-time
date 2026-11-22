# what-time model card

## Model

The active model is a ~575k-parameter transformer encoder (three
pre-LayerNorm blocks, width 96, four heads, feed-forward width 384, learned
positions, mean-pooled context head), stored with symmetric per-tensor int8
quantization
(`rust/what-time/assets/weights-transformer.json`). Inference is a pure-Rust
forward pass (`rust/what-time/src/model/transformer.rs`) mirrored op-for-op
from the training network; exported parity fixtures gate logit drift below
2e-3.

Each token enters as up to 20 summed feature rows from a 2,116-row table:
the shape rows (character classes, length, casing, number bucket, an 8-bit
word hash, neighbor punctuation, position flags) plus three word-form rows —
ten more bits of the word hash and hashes of the three-letter prefix and
suffix, which stay close across romanizations (somvar/somwar). Training hides
the word-form rows of each token at random half the time; without that the
model trusts the hash bucket of a word it has never seen, which belongs to
whatever trained word shares it.

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

## Metrics (current checkpoint — `lex-on`, shipped as v0.3.0)

Per-build history lives in [`SCORES.md`](SCORES.md).

- Pinned corpora: 632/632 schedule cases (zero known gaps, 37 new
  recurring-schedule pins), 60/60 cron cases end to end, 15/15 multilingual
  fixtures, 22/22 end-to-end occurrence cases.
- Recurrence families (`torch/recurrence_families.py`, never-trained
  seeds): 1.0000 exact on 4,701 rows.
- LLM held-out: 1.0000 exact token-label/boundary sequences on a 2,000-row
  union slice (1,000 of them from a never-trained external-model corpus).
- Structural generator held-out: 0.9868 on 5,000 rows (0.9606 before). The
  slice wraps expressions in carrier sentences reserved from training ("the
  train leaves at", "the workshop continues"), so it measures unseen
  carriers.
- Word-form rows ablation, same data and seed: without them 0.9858
  structural, 0.9938 extra, 0.9985 recurrence, so they add a little on every
  slice and stay.
- Extra families: 0.9946 on 4,983 rows. These are synthetic,
  generator-related evaluations, not real-user accuracy claims.
- Numeric parity: exported fixtures reproduce the quantized training
  network's logits within 2e-3 with identical argmax.

## Known limitations

- Seconds and seasons are not schedules: "every 10 seconds" and "every
  summer" return diagnostics. Cron has no seconds field either.
- Cron is exported only when it is exact. Every other week or every N days,
  the nth weekday of a month and the last day of a month stay RRULE-only
  with an `unsupported-cron` diagnostic, and cron cannot carry a start or
  end date or a count (`cron-unbounded`).

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
`cargo test` fails closed if predictions shift. `--candidate` keeps them in
`runs/<run>/export` until `torch/promote.py <run>` copies them in. The current
checkpoint trained from scratch (the architecture changed, so no warm start).
A first attempt reached only 0.79 structural: the background prose was
missing from the checkout, and the recurrence generator labeled a carrier's
closing connector GLUE and "per"/"half an" differently from the other
generators. Fixing those, not the architecture, closed the gap:

```sh
uv run python torch/fetch_prose.py      # background prose, once per checkout
uv run python torch/train_transformer.py --run lex-on --candidate --epochs 12
uv run python torch/promote.py lex-on
```
