# Failure mining — the targeted-corpus loop

Broad corpus batches fix known gaps; mining fixes unknown ones. The loop
finds phrases the current model is *uncertain or wrong about*, and turns
them into seeds for the next external-LLM corpus batch.

## The loop

1. **Collect unlabeled phrases.** Any realistic text works: free-written
   LLM output (ask a model for "500 realistic WhatsApp messages that
   mention a date or time, no labels" — cheaper than labeled generation
   and nothing is wasted on the gate), chat excerpts, support tickets,
   your own drafts. One phrase per line, plain text or `{"text": ...}`.

2. **Mine:**
   ```sh
   ~/.cargo/bin/cargo run --manifest-path rust/Cargo.toml -q \
     -p what-time --example mine -- phrases.txt > data/mining/mined.jsonl
   ```
   Each phrase gets the min top1−top2 probability gap over its tokens
   (`Token::score`, see `testing::tagged_tokens`) plus a compile check
   through the real pipeline path. Statuses: `compile-fail` (labels
   looked temporal, nothing compiled), `spurious` (all-background text
   produced a schedule), `low-margin` (below the 0.35 default threshold,
   configurable as a second argument). Output is most-uncertain first.

3. **Format seeds:**
   ```sh
   cd packages/training
   uv run python torch/format_mining_seeds.py data/mining/mined.jsonl
   ```
   Writes `data/mining/seed-prompt.md` (top 150 by default) with the
   model's guessed labels marked unverified.

4. **External pass.** Hand an external model, in one session: the v1 brief
   (`LLM_CORPUS_PROMPT.md` PROMPT section + multilingual addendum), the
   v2 addendum (`LLM_CORPUS_PROMPT_V2.md`), and `seed-prompt.md`. It
   corrects the seeds, then writes new sentences in the same families.

5. **Gate + train as usual:** output → `data/llm/raw.jsonl` →
   `validate_llm_corpus.py` → `pnpm train`. Mined seeds deliberately
   target weak spots, so expect the batch to lift exactly the families
   the mine flagged; re-run the mine afterwards to confirm the margins
   moved.

## Reading the results

- A whole family staying `low-margin` after training usually means the
  featurizer cannot see the distinction (no lexical embeddings — see the
  model card), not that more data is needed.
- `compile-fail` clusters with the same diagnostics code point at a
  compiler grammar gap; fix `rust/what-time/src/compile.rs`, re-gate, and
  the rejected rows come back for free.
- `spurious` rows are the model hallucinating schedules into background
  text — feed them to the external model as negatives.

## Lessons

- Narrow template supplements stack badly: each one nudges neighbouring
  vocabulary loose (quarter→CLOCK_OFFSET→DAYGROUP, Friday→HOLIDAY,
  dec→UNIT across successive retrains). Prefer one wide external batch
  over several narrow patches; the corpus gate catches the damage, but
  the retrain lottery is real.

## Pin what you find

Every edge case — mined, user-reported, or spotted by hand — becomes a
permanent regression before (or alongside) its fix. `cargo test` runs the
pinned corpora, and the release workflow runs `cargo test --workspace`, so
nothing ships if a pinned case breaks.

- **Works today, must keep working** → append a line to
  `rust/evals/data/user-cases.jsonl` with the CORRECT schedule, verified
  by eyeball against the current parser output, then mirror the file to
  `packages/training/data/gold/user-cases.jsonl`. Out-of-spec input that
  must fail closed (fused tokens like `8bjE`) goes to `adversarial.jsonl`
  with `schedule: null`.
- **Broken today, correct expectation known** → same as above, plus
  register the case id in `KNOWN_GAPS` in `rust/what-time/tests/evals.rs`.
  The suite then expects it to fail: it screams if the case regresses
  further, and screams again once training fixes it (remove it from
  `KNOWN_GAPS` at that point). Expectations are authored truth — never
  edited to match current output.
- **Full resolution with timezones/DST** → `results.jsonl` (occurrences
  pinned against an explicit reference time and zone).

The corpora live in `rust/evals/data/` and are mirrored into
`packages/training/data/gold/`; keep both copies identical when editing.
