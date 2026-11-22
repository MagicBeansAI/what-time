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

## Metrics (current checkpoint — `neg-r4`, shipped as v0.5.0)

Per-build history lives in [`SCORES.md`](SCORES.md).

- Pinned corpora: 685/685 schedule cases (zero known gaps), 68/68 cron
  cases end to end, 86/86 cron lines read back, 15/15 multilingual
  fixtures, 22/22 end-to-end occurrence cases, and 114 phrase/zone pairs
  whose exported cron fires identically under the `cron` crate.
- Recurrence families (`torch/recurrence_families.py`, never-trained
  seeds): 0.9998 exact on 4,701 rows, negatives included.
- LLM held-out: 0.9990 exact token-label/boundary sequences on a 2,000-row
  union slice (1,000 of them from a never-trained external-model corpus).
- Structural generator held-out: 0.9850 on 5,000 rows (0.9868 for the
  v0.3.0 weights). The slice wraps expressions in carrier sentences
  reserved from training ("the train leaves at", "the workshop
  continues"), so it measures unseen carriers.
- Negatives: training now includes a family of schedule words that name
  nothing (publication titles, past events without a date, preferences,
  "every"/"har" meaning "each"), paired with real schedules that use the
  same words. On a held-out contrast check written apart from the
  generator, 22 of 27 non-schedules return nothing (14 for the v0.3.0
  weights) and 10 of 10 real schedules still parse. A sentence naming a
  real day, even a past one ("last night was cold"), is a date expression,
  not a negative.
- Independent evaluation (271 phrases written without seeing the parser):
  268 match on the first four occurrences (261 for the v0.3.0 weights).
  The three left are readings the author marked ambiguous: "every 5 hours"
  and "every 7 minutes" (the author read them per day, as cron's `*/5`
  would; what-time counts on from now and exports an interval, since cron
  cannot keep that rhythm) and "on the last day of the month" (read as one
  date, not a monthly rule).
- A second independent contrast set (197 lines, written by a separate
  author who saw neither the parser nor its training data, and kept out of
  training): 102/110 non-schedules return nothing, 62/62 recurring
  schedules and 25/25 one-off dates parse correctly. The v0.4.1 weights,
  with the same compiler, score 96, 60 and 25.
- A rejected candidate shows why negatives need pairing: one whose
  negatives used "every/har" before people ("har koi", "every student")
  without matching schedules learned to ignore "every quarter", "each
  month" and "har ghante" (recurring recall fell to 55/62). The shipped
  model trains every such negative against two real schedules with the
  same marker.
- Word-form rows ablation, same data and seed: without them 0.9858
  structural, 0.9938 extra, 0.9985 recurrence, so they add a little on every
  slice and stay.
- Extra families: 0.9946 on 4,983 rows. These are synthetic,
  generator-related evaluations, not real-user accuracy claims.
- Numeric parity: exported fixtures reproduce the quantized training
  network's logits within 2e-3 with identical argmax.

## Intent encoder (`intent-r4`, shipped as v0.5.0)

A second, smaller encoder decides whether a text asks for something to be
scheduled (`request`) or only talks about time (`mention`: past events,
habits, facts, titles, idioms). The tagger finds time expressions; it cannot
make that call, and template negatives did not teach it to (they fixed only
the patterns they copied).

- Architecture: two pre-LayerNorm blocks, width 64, four heads, feed-forward
  128, over the tagger's own feature rows (word-form rows included, with 30%
  training dropout); final states pooled by mean, max and one learned
  attention query; a 64-unit tanh layer; one logit. ~223k parameters, int8,
  exported with an `intent.` prefix beside the tagger's tensors, which are
  copied unchanged (labels, boundaries and parity fixtures are identical).
  Rust runs it through the tagger's encoder pass; parity with PyTorch is
  2.4e-6 on the fixture sequences. A head on the frozen tagger's token
  states was tried first and missed even "remind me every 2 hours".
- Data: 7,025 natural sentences with time words from Tatoeba (6,000
  English, 1,025 Hindi, CC-BY 2.0 FR), labelled request/mention by readers
  who saw neither the parser nor its data (`data/intent/labels-*.jsonl`;
  "unsure" dropped); 900 Hinglish and 800 Hindi chat lines written with
  their intent (Tatoeba has no Hinglish and few Hindi requests); generated
  rows (a row with a time span is a request, an all-O row a mention).
  Classes are balanced per source, so the head cannot read the source's
  writing style as the label.
- Cut-off: the request probability that keeps 95% of held-out natural
  requests (0.0553); at it, 79% of held-out natural mentions are rejected.
  Generated held-out rows: 99.6%.
- Independent evaluation (300 lines written for this, every one containing
  a time word, scored once): with the tagger alone, 74/150 non-schedules
  return nothing and 141/150 real schedules parse; gated on intent, 119/150
  and 138/150. By language, non-schedules: English 37→58/80, Hindi
  16→29/30, Hinglish 21→32/40.
- It runs only when the tagger labelled a time word. With a time
  expression, a single parse takes 0.69 ms (0.40 ms without the encoder).
- Limits: English past and duration chatter often still scores as a request
  ("My boss emailed me at 11pm last night", "It took me 45 minutes to find
  parking yesterday"); a long task description after the time lowers a
  request's score ("remind me in 30 minutes to take the noodles off the
  stove" is near the cut-off). The labels for the natural sentences and the
  written lines come from language models, as does the evaluation set, so
  real users may differ. A text that is nothing but a time expression is
  always a request.

## Known limitations


- Mentions read as schedules. The tagger finds time expressions; it does
  not decide whether the user is asking for one. On a third independent set
  (240 lines, written after this model was trained, scored once before any
  change), 98 of 130 non-schedules return nothing; 65/70 recurring and
  39/40 one-off phrases parse. The misses are past habits ("I used to jog
  every morning", "papa roz subah walk pe jaate the"), titles and names
  ("Last Christmas", "Monday.com", "दैनिक जागरण", "naam April hai"),
  numbers shaped like times ("pages 11 to 45", "24/7"), and real dates in
  chatter ("too easy today"). A candidate trained on template negatives for
  titles, subjects and idioms (`neg-r5`) gained 3 of those 130 and lost two
  pinned schedules, so it was not shipped. Callers that create schedules
  from free text should confirm with the user; telling a request from a
  mention needs a sentence-level intent signal, which this model lacks.
- Model labelling misses remain in some sentences ("water the plants on
  alternate days, mornings around 7" leaves "alternate" and "mornings"
  unlabelled; "Hourly reminders …" leaves "Hourly" unlabelled), so those
  phrases return nothing; the same words parse in plainer sentences.

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
