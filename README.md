<div align="center">
  <h1>what-time</h1>
  <p>
    <b>Neural schedule parsing in English, हिन्दी, and Hinglish — on-device, exact, open.</b>
  </p>
  <p>
    <img src="https://img.shields.io/badge/what--time-v0.4.1-2563EB.svg" alt="what-time v0.4.1" />
    <a href="#-license"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License: MIT" /></a>
    <img src="https://img.shields.io/badge/lang-English%20%7C%20हिन्दी%20%7C%20Hinglish-8B5CF6.svg" alt="Trilingual" />
    <a href="#-what-it-does"><img src="https://img.shields.io/badge/targets-Rust%20%7C%20WASM%20%7C%20npm%20%7C%20GPU-orange.svg" alt="Targets" /></a>
  </p>
  <p>
    <a href="packages/what-time">npm package</a>
    &nbsp;&middot;&nbsp;
    <a href="https://magicbeansai.github.io/what-time/">Live playground</a>
    &nbsp;&middot;&nbsp;
    <a href="MODEL_CARD.md">Model card</a>
  </p>
</div>

<br/>

<div align="center">
  <p>
  Turn <i>"call mom on sunday evening"</i>, <i>"कल शाम को आठ बजे"</i>, or
  <i>"har din shaam ko 8 baje"</i> into real dates, time ranges, RFC 5545
  recurrence rules, and cron expressions — with no server, no network call,
  and no data leaving the device.
  </p>
</div>

> A ~575k-parameter transformer tags each token with a semantic role; a
> deterministic compiler builds a typed schedule; an exact calendar resolver
> with a bundled IANA timezone database produces the dates. The fuzzy part is
> learned, the arithmetic is code — wrong labels reject with a diagnostic,
> never a silently wrong date.

> [!WARNING]
> **Pre-release software.** Before v1, APIs and JSON shapes may change, and
> the model's vocabulary coverage is still growing (see
> [known limitations](#️-honest-limitations)).

---

## 🚀 Quick Start

**npm / TypeScript:**

```bash
npm install @magicbeansai/what-time
```

```ts
import { parse } from "@magicbeansai/what-time";

const result = await parse("har hafte Tuesday ko gym", {
  reference: new Date().toISOString(),
  timeZone: "Asia/Kolkata",
  limit: 3,
});
// result.occurrences: [{ start: "2026-09-15T00:00:00+05:30", allDay: true }, ...]
```

**CLI (agents welcome):**

```bash
rust/target/release/what-time "call mom on sunday evening" \
  -r 2026-09-13T10:00:00Z -t Asia/Kolkata
# 2026-09-13T17:00:00+05:30 → 2026-09-13T21:00:00+05:30

echo "कल शाम को आठ बजे" | what-time -j -r 2026-09-13T10:00:00Z -t Asia/Kolkata
# exit 0, full JSON on stdout — 1 parsed / 1 nothing found / 2 bad invocation
```

**Zero-install playground:** try it live at
[magicbeansai.github.io/what-time](https://magicbeansai.github.io/what-time/)
(rebuilt from source on every push to `main`) —
or grab [`rust/site/what-time.html`](rust/site/what-time.html), one self-contained
file (wasm inlined) that runs the full trilingual model offline, straight from
disk.

---

## ✨ What it does

| Area | What you get |
| :-- | :-- |
| **Trilingual parsing** | English, Hindi (Devanagari), and Hinglish (romanized Hindi, code-switching included) through one model — `"book it for day after tomorrow"`, `"अगले महीने की 21st को"`, `"parso subah 10 baje"`. |
| **Exact resolution** | Bundled IANA timezone database, DST-correct transitions, RFC 5545 (RRULE) and cron export, exact recurrence expansion down to the minute. The model never touches dates or timezones. |
| **Every surface** | Rust library + CLI, WebAssembly module (SIMD: Chrome/Edge 91+, Firefox 89+, Safari 16.4+, Node 16.4+), npm package for TS/JS, a single-file static playground, and an optional local HTTP API. |
| **GPU batch mode** | Optional wgpu backend (`--features gpu`) dispatches batches of ≥64 windows to Metal/Vulkan — **~6–10× faster** on 10k+ phrase sweeps, with output verified identical to CPU. |
| **Request or mention** | An `intent` on every result that holds a time expression: `request` ("remind me every Monday at 9", "kal subah 7 baje uthaana") or `mention` ("I used to run every morning", "दादी रोज़ शाम को भजन गाती थीं"), with the request probability. Create schedules only for a request. |
| **Honest failures** | Ambiguity and unknown vocabulary return typed diagnostics (`code`, `message`, `severity`), never silent wrong dates. |
| **Verifiable training** | Corpus generators, a mechanical validation gate for LLM-authored data, int8 quantization with exported parity fixtures — `cargo test` fails closed if predictions shift. |

---

## 🏗️ How it works

```
"call mom on sunday evening"
   │  tokenizer (UTF-16 features; Devanagari conjuncts stay whole)
   ▼
 [neural tagger]  mom→O  sunday→WEEKDAY  evening→DAYPART    <1 ms
   │  deterministic compiler
   ▼
 schedule: weekday[SU] + day-part evening (typed JSON, no dates yet)
   │  exact resolver (jiff + bundled tzdb, Asia/Kolkata)
   ▼
 2026-09-13T17:00:00+05:30 → 2026-09-13T21:00:00+05:30
```

1. **Tokenization** — one scan emits sparse feature rows (character shape,
   hashes, digit buckets) over UTF-16 units.
2. **The model** — a three-block, four-head transformer encoder (int8-quantized,
   ~575k parameters) classifies each token into one of 35 semantic roles plus
   a clause-boundary score. Trained on generated structural supervision plus
   gate-checked LLM-authored English/Hindi/Hinglish corpora.
3. **Compilation** — roles compile into a typed `Schedule`: date anchors,
   clocks, ranges, recurrences, durations. The compiler never consults the
   reference date.
4. **Resolution** — the resolver applies timezone and DST rules to produce
   occurrences and RFC 5545 rules.
5. **Intent** — a second, smaller encoder (two blocks, ~223k parameters) over
   the same features reads the whole sentence and scores whether it asks for
   something to be scheduled. It runs only when the tagger found a time
   expression; a text that is nothing but a schedule phrase is a request.

Full details: [`MODEL_CARD.md`](MODEL_CARD.md) — training data, metrics,
and known limitations.

---

## 📁 Repository layout

| Path | What lives there |
| :-- | :-- |
| `rust/what-time` | The library: tokenizer, transformer inference (CPU + optional GPU), compiler, resolver, RRULE export, full test/eval suite. |
| `rust/what-time-cli` | The `what-time` binary — agent-friendly exit codes, stdin, JSON mode. |
| `rust/what-time-wasm` | WebAssembly bindings (browser + bundlers + Node). |
| `rust/what-time-web` | Optional local HTTP API (`POST /api/parse`). |
| `rust/site` | The static playground; `what-time.html` is the single-file build, `build.sh` regenerates. |
| `packages/what-time` | The npm package, published as `@magicbeansai/what-time` — a thin typed wrapper around the wasm. No parallel implementation exists or will be added. |
| `packages/training` | Training pipeline (Python via uv): corpus generators, the LLM-corpus brief and validation gate, the transformer trainer that exports int8 weights into `rust/`. |

---

## 🛠️ Development

```sh
cargo test --manifest-path rust/Cargo.toml    # full suite + gold corpora + parity gates
cargo build --release -p what-time --example featurize   # required before training
cd packages/training && pnpm train            # retrain + export weights
rust/site/build.sh                            # rebuild wasm + static playground
cargo run --release -p what-time --example bench-bulk --features gpu -- 10000  # GPU bulk bench
```

- To release: follow [RELEASING.md](RELEASING.md) — tag-driven, gated, with
  the one-time npm org setup documented.
- Python is always invoked through `uv` inside `packages/training`.
- The Rust toolchain lives at `~/.cargo/bin` when not on `PATH`.
- Weights and parity fixtures move together via the training export; the test
  suite fails closed if predictions shift. Do not hand-edit them.

---

## 🤖 Using it from agents

The CLI is designed for programmatic use:

```bash
what-time "<phrase>" -j -r <ISO instant> -t <IANA zone> -l <limit>
```

Exit codes: **0** parsed · **1** no schedule found · **2** bad invocation.
JSON mode prints occurrences, rrules, crons, diagnostics, and timings — the
same shape the npm package returns. Pipe phrases on stdin when quoting gets
hairy (including Devanagari).

`-c` prints only the cron expressions, one per line, in the `-t` zone:

```bash
what-time -c "every 15 minutes between 9am and 6pm on weekdays" -t Asia/Kolkata
# */15 9-17 * * MON-FRI
# 0 18 * * MON-FRI
```

`-x` goes the other way: a cron line read back in English, with its fires,
RRULE and the normalized line. No model is involved — cron is a formal
grammar, so reading it is exact.

```bash
what-time -x "*/15 9-17 * * 1-5" -t Asia/Kolkata -l 2
# Every 15 minutes from 9:00 AM to 5:45 PM on weekdays
# 2026-11-23T09:00:00+05:30
# 2026-11-23T09:15:00+05:30
# … RRULE, then CRON:*/15 9-17 * * MON-FRI
```

It reads numbers, ranges, lists, steps (`*/15`, `1-10/3`, `5/10`), month and
weekday names, Sunday as 0 or 7, `@hourly`…`@yearly`, and the `L` (last day)
and `5#2` / `5L` (nth / last weekday) extensions; when day-of-month and
day-of-week are both restricted, a day matches when either does (POSIX).
English only for now.

Cron is the common five-field dialect with weekday names. A rule it cannot
state exactly — every other week, the first Monday of a month, the last day
of a month — prints nothing, exits 1, and says why on stderr; the RRULE in
JSON mode still carries it. A sub-daily step that does not divide the day
("every 5 hours", "every 7 minutes") also gets an `intervals` entry — fire at
`start`, then every `everySeconds` — for schedulers with interval triggers.
The fires sit on a fixed grid from `start`, so a scheduler should anchor
them there rather than count each one from the last run; in Magician that
is `{"kind": {"Interval": {"seconds": everySeconds}}, "starts_at": start,
"ends_at": until, "max_runs": count}`. Start dates, end dates and counts do
not fit in cron either, so those print with a `cron-unbounded` warning; the
RRULE's `DTSTART` / `UNTIL` / `COUNT` carry them.

---

## ⚠️ Honest limitations

- **Trained phrasing, not all phrasing.** Rare vocabulary can mislabel
  (current known gaps: English "Diwali", तिमाही, होली). Known
  gaps return diagnostics rather than wrong dates, and each is queued for the
  next corpus batch.
- **कल / kal** (yesterday or tomorrow) resolves to **tomorrow** unless a
  past-tense cue appears — a documented default, not a guess.
- **Real-user accuracy is unmeasured.** In-distribution scores are excellent
  (568 pinned corpus cases, zero known gaps, LLM held-out 0.995) but generated corpora share
  training families.
- **Intent is a probability, not a verdict.** On an independent set where
  every line contains a time word, gating on `intent` cut wrongly-read
  non-schedules from 76 to 31 of 150 and lost 3 of 141 real schedules. Past
  and duration chatter in English still often reads as a request ("my boss
  emailed me at 11pm last night"), and a long task description can lower a
  request's score. The label uses a high-recall cut-off; `intent.score` is
  there for a stricter one. Confirm before creating a schedule from free
  text.
- **GPU mode pays only in bulk.** Single phrases are fastest on CPU (~0.4 ms);
  the GPU path activates at ≥64 phrases per call.
- **Not for high-stakes scheduling** — legal, medical, or billing dates need
  verified input.

---

## 📄 License

MIT. This project derives from an MIT-licensed predecessor; the original
copyright notice is retained in [`LICENSE`](LICENSE) as the license requires.
