# Corpus-generation brief v2 — targeted gap batch

This batch attacks the model's *known* misses, not broad coverage (v1 in
`LLM_CORPUS_PROMPT.md` already covers that). Hand the model the **full v1
prompt** (everything under "PROMPT (copy everything below this line)",
including the multilingual addendum) **followed by the v2 addendum below**.
Same gate applies on our side, so correctness matters more than volume.

---

## V2 ADDENDUM (append after the v1 prompt)

The v1 brief generated broad coverage. This batch targets specific gaps.
Everything else — tokenization rules, label set, hard conventions, JSONL
shape — is unchanged. Generate the sections below, ~20,000 sentences total,
split roughly evenly across English / Hindi / Hinglish where the phrasing
exists naturally.

### A. Missing vocabulary (highest priority)

These words/phrases must appear in thousands of sentences, in every
structural position that exists in the v1 families (alone, with clocks,
in recurrences, in ranges, in prose):

- **तारीख / tareek / tareekh / taareekh** — "date" as a day-of-month
  selector: `15 tareek ko`, `तारीख 15`, `agle mahine ki 20 tareek`,
  `har mahine ki 5 tareek`. The number is DOM; `tareek` itself is UNIT
  (it names the day-of-month unit); `ki`/`ko` are GLUE. Unsuffixed
  `ki 15` (no "tareek", no month) is still DOM: `reminder ki 15 ko`.
- **रोज़ / roz / roj / rozz** — daily recurrence, alone or after हर/har:
  `roz subah 7 baje`, `har roz`, `रोज़ रात को`. Label RECUR.
- **Devanagari numerals** — ० १ २ ३ ४ ५ ६ ७ ८ ९ and mixed forms like
  `८ बजे`, `१५ तारीख`, `सवा ४`. Each numeral cluster is ONE token and
  carries the role (HOUR, DOM, NUM…) exactly like ASCII digits. This is
  an explicit exception to the v1 rule "write digits as ASCII".
- **for N mins / पूरे N मिनट** — colloquial duration: `call for 10 mins`,
  `for 45 mins`, `पूरे 30 मिनट`, `bas 5 mins ka kaam`. `mins` is UNIT,
  the number is DUR, `for`/`का` GLUE.
- **की / ki as bare DOM marker** — `ki 15`, `ki 22nd` with no month or
  tareek: the number is DOM, `ki` is GLUE.

### B. Known semantic biases (disambiguation training)

Generate *minimal pairs* so the tagger learns what wins:

- **Day-part before बजे/baje**: `शाम को आठ बजे`, `shaam ko 8 baje`,
  `subah 6 baje` — DAYPART, GLUE, HOUR, MERIDIEM. But when an explicit
  am/pm or डेढ़/सवा/पौने offset is present, the offset wins:
  `शाम को सवा आठ बजे`, `shaam ko 8:30 pm`. Generate both shapes in
  equal numbers, plus contrast pairs that differ only in day-part
  (`subah ko 8 baje` vs `raat ko 8 baje`).
- **कल / kal / parso with tense cues**: for every sentence with कल/parso,
  include an explicit tense marker (आना है / आएगा / जाएँगे for future;
  आया था / गया था / था for past). कल and परसों are REL_DAY either way —
  never invent a new label. Generate equal counts of past-cued and
  future-cued.

### C. Wild real-world register (new)

- **Code-mixing inside one sentence**: Devanagari + Roman + ASCII digits
  in the same expression: `कल 8 baje meeting`, `परसों zoom call at
  सवा चार`, `agle हफ़्ते se leave`. Label each token by role regardless
  of script.
- **Casual shorthand** (still one token per whitespace-separated word,
  keep original casing): `kl 8bjE` is NOT allowed — but `kl milte hain`,
  `2mrw`, `evng`, `mn`, `tues` are allowed as single tokens labeled by
  their role (`2mrw` is REL_DAY, `tues` WEEKDAY, `evng` DAYPART).
- **Corporate shorthand**: `EOD`, `COB`, `EOW`, `EOM` (EDGE + UNIT
  semantics: `EOD` alone is EDGE,GLUE? — no: treat `EOD`/`COB` as
  a single token covering both the edge and the unit — label the single
  token `EOD`/`COB` as UNIT (end-of-period by convention). Spelled out,
  `end of day Friday` is EDGE, GLUE, UNIT, WEEKDAY as usual, and
  `end of next month` is EDGE, GLUE, DEICTIC, UNIT.
- **Distractor-heavy negatives**: sentences full of numbers and
  time-adjacent words that contain NO schedule: `invoice 2024 balance
  3500 pending`, `movie was 3 hours long tbh`. All O.

### Composition rules for this batch

- Same output shape: one JSON object per line, tokens joined with single
  spaces must reconstruct `text`, keep original casing and script.
- Still no reserved carrier phrases: "could you arrange a reminder for",
  "our rehearsal begins at", "the train leaves at", "please put this in
  my diary for", "allow extra time", "the workshop continues".
- Still under 40 tokens per sentence; no exact duplicate sentences.
- When unsure how to label something, SKIP the sentence rather than
  guess — a wrong label corrupts training; a missing sentence costs
  nothing.

Output: JSONL only, no commentary, no code fences.

---

## Receiving the data on our side

1. Save output as `packages/training/data/llm/raw.jsonl` (replace or
   append — the gate dedupes by fingerprint).
2. `cargo build --manifest-path rust/Cargo.toml --example validate-corpus`
   then `uv run --project packages/training python
   packages/training/torch/validate_llm_corpus.py`.
3. Expect a lower acceptance rate than v1 (wild register + Devanagari
   numerals are new territory for the compile oracle). If a whole section
   is rejected, read `rejected.jsonl` — a systematic reject usually means
   a label-convention bug in the generated batch, not a gate bug.
4. Retrain: `cd packages/training && pnpm train` — the export updates
   `rust/what-time/assets/weights-transformer.json` and the parity
   fixtures together; `cargo test` fails closed if anything shifts.
