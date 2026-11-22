# Changelog

Short and honest. Scores per build live in [`SCORES.md`](SCORES.md).

## 0.4.0

- **Cron to English**: `parse_cron` / `describe_cron` (Rust), `-x` (CLI),
  `parseCron` / `describeCron` (npm, wasm), `POST /api/cron` (local API),
  and the playground reads a pasted cron line. Exact grammar parsing, no
  model: numbers, ranges, lists, steps, names, Sunday 0/7, `@` macros, `L`,
  `5#2`/`5L`, POSIX either-day rule; a line that never fires says so.
  English descriptions only for now. The playground intro links a cron
  example, and two cron examples sit with the phrase examples.
- **Previews now match what a scheduler fires.** A sub-daily step that
  divides the day and names no clock ("every 6 hours", "every hour", "har
  do ghante mein") sits on the wall-clock grid from midnight, as cron runs
  it, instead of counting from the reference instant (which cron cannot
  express once the reference has seconds). Steps that don't divide the
  day ("every 5 hours") still count from the reference and stay RRULE-only.
  A new test runs every exported cron line through the `cron` crate the
  way Magician schedules it and requires identical fires (Kolkata and New
  York across DST).
- A recurrence whose next occurrence lies past the one-year preview
  ("annually on 29 Feb") now shows that next occurrence.
- Grammar, found by an independent 271-phrase test set (now 261 match; the
  rest are documented ambiguities and gaps):
  - "the 1st / 10th of every month", "every year on the last day of
    December", "monthly on day 25", "every 24 hours";
  - two rules joined without a second "every" ("weekdays at 9am and
    weekends at 11am", "subah 9 baje aur raat 9 baje");
  - a trailing "every Saturday" narrows the timed rule before it, and
    "weekly" in "send the weekly report every Friday" is not a second rule;
  - "except on the 25th"; "until 8pm today" no longer becomes the rule's
    clock; "tonight at 10" is 22:00;
  - label repairs for numbers after a month name ("Nov 22"), minutes after
    a colon ("7:30pm"), hours in comma lists, "everyday", and "every
    quarter" (which the retrained model had stopped labeling).
- Pinned corpora: 646 schedules, 60 cron, 86 cron-parse.

- GitHub Pages builds the playground from source on every push to `main`
  (wasm with SIMD, then the single-file page), smoke-tests that the fresh
  module turns a recurring phrase into cron, and only then deploys; the
  live site no longer depends on a rebuilt `what-time.html` being committed.

## 0.3.0

- Cron export: every result carries `crons`, five-field expressions
  (`minute hour day month weekday`, weekday names) in the caller's zone,
  next to `rrules`; the CLI prints them alone with `-c`. A rule cron cannot
  state exactly gets no expression and an `unsupported-cron` diagnostic
  instead of an approximation; start/end dates and counts get
  `cron-unbounded`. Lines that differ in one field merge (`0 8,13,20 * * *`).
- Minute recurrences (`FREQ=MINUTELY`): every 15 minutes, every half (an)
  hour, every 90 minutes, har 15 minute mein. Sub-daily steps inside a clock
  window restart each day and include both ends ("every 2 hours from 9am to
  5pm" fires at 9, 11, 1, 3, 5) and export as BYHOUR/BYMINUTE; a day part
  is a window too ("every hour in the evening").
- Recurring grammar: every third day, alternate Saturdays, every 2nd
  Saturday, last (working) day of every month, every January, every 15th,
  two rules in one sentence ("every Tuesday at 3pm and every Friday at
  5pm"), "twice a day at 9am and 9pm", once/twice a month, twice a year,
  twice daily, counts after the period (hafte mein teen baar), bare plurals
  ("Mondays and Thursdays at 7"), a clock after an exception ("weekdays
  except Wednesday at 9am"), nightly, night clocks past midnight ("2:30
  every night" is 02:30), Hindi/Hinglish frequency words and romanized
  weekday spellings.
- Model: ~575k parameters (was 144k), three blocks of width 96, plus
  word-form feature rows (word hash, prefix, suffix) trained with 50%
  dropout; trained from scratch with a new recurrence generator
  (`torch/recurrence_families.py`, English/Hindi/Hinglish). Structural
  held-out 98.7% (was 96.1%), LLM held-out 100%. CPU and GPU inference read
  the model's dimensions from the weights.
- Faster CPU inference: multi-lane f32 matrix products make a phrase 0.41 ms
  natively (was 0.67 ms with the smaller model) and 0.70 ms in wasm (was
  0.80 ms).
- **Minimum runtime raised:** the wasm module now uses WebAssembly SIMD, so
  the npm package and playground need Chrome/Edge 91+, Firefox 89+, Safari
  16.4+ or Node 16.4+ (the package declares `engines.node >=18`). Older
  engines get an error naming the requirement instead of a bare
  `CompileError`.
- `torch/fetch_prose.py` rebuilds the background prose from the pin in
  `data/corpus.json`; training without it lost ~18 points on unseen carriers.
- "every other Friday" reads as a step of two whether "other" arrives as
  NUM or GLUE (the corpus briefs and generators disagree on its label).
- Pinned corpora: 632 schedules (+37 recurring), new `cron.jsonl` (60 cases),
  zero known gaps.
- Tooling: clippy clean under Rust 1.95 (`-D warnings`), and training writes
  candidates to `runs/<run>/export` until `torch/promote.py` installs them.

## 0.2.3

- Festival vocabulary works end-to-end: Holi (both scripts), Bakrid/ईद-ul-Adha,
  Dussehra, Karwa Chauth, Gandhi Jayanti, plus the 0.2.2 set — trained via one
  balanced batch with drift-protection (weekdays, month abbreviations, har,
  Hindi tense cues, day-parts) after narrow supplements proved unstable.
- Pinned corpora at 593 + 2 playground cases, zero known gaps, zero regressions.
- "परसों सुबह" now resolves with a morning time-part, consistent with the
  English "tomorrow morning" behavior (frozen fixture updated after review).

## 0.2.2

- Holiday asset expanded to 40 entries via a curated, source-verified generator
  (`packages/training/holidays/`): Indian festivals through 2028–2030 (Holi,
  Raksha Bandhan, Ganesh Chaturthi, Dussehra, Karwa Chauth, Mahashivratri,
  predicted Eid tables) and the English-speaking world (UK bank holidays,
  Mothering Sunday, St Patrick's, Guy Fawkes; Columbus/Indigenous Peoples' and
  Veterans Day; Canada Day, Canadian Thanksgiving; Australia Day, Anzac Day).
  One command regenerates the asset; a CI-style --check keeps it from drifting.
- Adding a festival is now a data row plus corpus rows — zero compiler changes
  (policy codified in AGENTS.md).

## 0.2.1 (folded into 0.2.2)

- Quarter calendar periods: `next quarter`, `end of next quarter`, `every quarter`,
  `quarterly` — resolved on exact quarter boundaries, exported as RRULE
  `FREQ=MONTHLY;INTERVAL=3`.
- Holidays moved to a bundled data asset (`assets/holidays.json`): fixed dates,
  nth-weekday rules (Thanksgiving, Memorial Day, Labor Day…), exact Easter
  offsets (Good Friday, Easter Monday), and tabulated lunar dates (दिवाली
  2025–2030). Adding a holiday is a data row, not a code change. US and UK
  holiday vocabulary added alongside the existing global and Indian set;
  out-of-table years return a diagnostic, never a guessed date.
- Both 0.2.0 known gaps closed by retraining: ordinal-anchored Hindi recurrences
  (`हर महीने के दूसरे सोमवार को`) and verbs after fractional clocks
  (`sava char baje utth ja`). Pinned corpora now 568/568 with zero known gaps.
- Holiday vocabulary and sentence-initial EOD trained directly (no longer
  dependent on compiler rescue rules); all 20 playground examples are pinned
  regression cases. Remaining vocabulary gaps (diagnostics, not wrong
  dates): तिमाही/timahi, होली/Holi.

## 0.2.0

- Hindi/Hinglish postposed date and clock ranges: `15 tareekh se 20 tareekh tak`,
  `sava char se paune paanch tak` (both markers follow what they govern).
- `tarikh` romanization, inflected Hindi ordinals (दूसरे/तीसरे/चौथे/पाँचवे) in the compiler.
- Retrained on a 20k-line external-model corpus plus an authored supplement:
  generator held-out 95.3% → 96.1%, LLM held-out 99.45% on a harder 2k union slice.
- Playground: version badge (read from the wasm itself), new trilingual examples,
  and a fix for the initial parse silently no-op'ing on page load.
- Regression corpus wired into the release gate and grown 526 → 568 pinned cases.
- Known gaps (diagnostics, never wrong dates): `next quarter`, दिवाली and other
  movable holiday names, ordinal-anchored recurrences via the live model
  (`हर महीने के दूसरे सोमवार को` compiles but the tagger drops the ordinal).

## 0.1.0

- First public release: trilingual (English/हिन्दी/Hinglish) neural schedule
  parser — 144k-parameter int8 transformer, deterministic compiler, exact
  IANA/DST resolver, RFC 5545 export; wasm + npm, CLI, offline playground.
