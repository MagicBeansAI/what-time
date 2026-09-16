<div align="center">
  <h1>@magicbeansai/what-time</h1>
  <p>
    <b>Turn "har hafte Tuesday ko gym" into real dates — on-device, in three languages.</b>
  </p>
  <p>
    <a href="#-install"><img src="https://img.shields.io/badge/what--time-v0.2.1-2563EB.svg" alt="@magicbeansai/what-time v0.2.1" /></a>
    <a href="#license"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License: MIT" /></a>
    <img src="https://img.shields.io/badge/lang-English%20%7C%20हिन्दी%20%7C%20Hinglish-8B5CF6.svg" alt="English, Hindi, Hinglish" />
    <img src="https://img.shields.io/badge/runtime-WebAssembly%20%2B%20TypeScript-6DA55F.svg" alt="WebAssembly + TypeScript" />
  </p>
</div>

<br/>

**@magicbeansai/what-time** parses natural-language schedule expressions — *"tomorrow at
9am"*, *"कल शाम को आठ बजे"*, *"har din shaam ko 8 baje"* — into concrete
occurrences, time ranges, and RFC 5545 recurrence rules. A ~144k-parameter
transformer tags each token with a semantic role; a deterministic compiler and
an exact calendar resolver (bundled IANA timezone database, DST-correct) turn
those roles into dates.

Everything runs in-process via WebAssembly. No server, no network call after
install, no data leaving the device. The whole pipeline is one Rust
implementation; this package is its typed JavaScript surface — there is no
parallel JS parser to drift out of sync.

```ts
import { parse } from "@magicbeansai/what-time";

const result = await parse("कल शाम को आठ बजे मीटिंग", {
  reference: new Date().toISOString(),
  timeZone: "Asia/Kolkata",
  limit: 3,
});

result.occurrences[0].start; // "2026-09-14T20:00:00+05:30"
result.rrules;               // [] — single event
```

---

## 📦 Install

```bash
npm install @magicbeansai/what-time
```

- **ESM-only** (`"type": "module"`). Works with every modern bundler
  (Vite, webpack, esbuild) and in Node 18+ / SSR.
- The wasm module (**~460 KB gzipped**, most of it the bundled IANA timezone
  database, so timezone resolution is exact **and offline**) lazy-loads on the
  first `parse` call and is cached after that.
- No runtime dependencies.

> [!NOTE]
> **Pre-release software (v0.x).** The API surface is small and stable, but
> breaking changes are possible before v1. Pin exact versions for now.

---

## 🚀 Usage

### Parse a phrase

```ts
import { parse } from "@magicbeansai/what-time";

const result = await parse("game night every Friday at 7:30pm", {
  reference: new Date().toISOString(), // required — the "now" text is read against
  timeZone: "Asia/Kolkata",            // required — any IANA zone name
  limit: 3,                            // optional — max occurrences (default 30, max 1000)
  dateOrder: "MDY",                    // optional — "MDY" (default) or "DMY" for 03/04/2027
});

result.occurrences;
// [{ start: "2026-09-18T19:30:00+05:30", end: undefined, allDay: false }, ...]

result.rrules;
// ["DTSTART;TZID=Asia/Kolkata:20260918T193000", "RRULE:FREQ=WEEKLY;INTERVAL=1;BYDAY=FR"]

result.diagnostics;
// [] — or [{ code, message, severity, start, end }] explaining any rejection

result.truncated; // true if `limit` cut the preview short
```

An invalid timezone rejects rather than guessing:

```ts
await parse("tomorrow at 9am", { reference, timeZone: "Not/AZone" });
// throws TypeError
```

### Inspect what the model saw

`parseExpressions` exposes the model-level view: expression spans, per-span
confidence, and the intermediate schedule JSON.

```ts
import { parseExpressions } from "@magicbeansai/what-time";

const [expression] = await parseExpressions("24th august last year");
expression.text;        // "24th august last year"
expression.confidence;  // 0.947
expression.schedule;    // { clauses: [{ date: { kind: "periodCalendar", ... } }] }
```

### Avoid the async dance

The first call loads the wasm. If you need synchronous parsing later (hot
paths, loops), await `init()` once:

```ts
import { init, parseSync } from "@magicbeansai/what-time";

await init();
const result = parseSync("call mom on sunday evening", context); // synchronous
```

---

## 🌍 Three languages, one call

The same call handles all three, including code-switching inside one sentence:

| Language | Example | Resolves to |
| :-- | :-- | :-- |
| English | `call mom on sunday evening` | Sun, 5:00–9:00 PM |
| हिन्दी | `कल शाम को आठ बजे मीटिंग` | tomorrow, 8:00 PM |
| Hinglish | `har din shaam ko 8 baje` | daily, 8:00 PM |
| Mixed | `agle week ka plan banate hain` | next week |

Ambiguous forms follow documented defaults (the full list is in the
repository's `MODEL_CARD.md`):

- **कल / kal** (yesterday *or* tomorrow) → **tomorrow**, unless a past-tense
  cue appears in the sentence.
- **Timezone and calendar math never enter the model** — DST transitions and
  recurrence expansion are computed exactly, in code.

---

## 📖 API

### `parse(text, context): Promise<ParseResult>`

| Field | Type | Notes |
| :-- | :-- | :-- |
| `text` | `string` | The phrase to parse. |
| `context.reference` | `string` | ISO instant with `Z` or offset. |
| `context.timeZone` | `string` | IANA zone name; unknown zones throw. |
| `context.limit?` | `number` | Max previewed occurrences, 1–1000. |
| `context.dateOrder?` | `"MDY" \| "DMY"` | Reading of ambiguous numeric dates. |

Returns `ParseResult`:

| Field | Type | Notes |
| :-- | :-- | :-- |
| `occurrences` | `TimeRange[]` | `{ start, end?, allDay?, open? }` — ISO strings. |
| `rrules` | `string[]` | RFC 5545 lines for recurring results. |
| `diagnostics` | `Diagnostic[]` | `{ code, message, severity, start?, end? }`. |
| `truncated` | `boolean` | Whether `limit` cut the preview. |
| `timings` | `object` | `tokenizeMs` / `inferMs` / `resolveMs` (0 in wasm builds). |

### `parseExpressions(text): Promise<Expression[]>`

Per-expression `{ text, start, end, confidence, schedule, diagnostics }`.

### `init(): Promise<void>`

Loads the wasm module once. Safe to call repeatedly.

### `parseSync(text, context): ParseResult`

Synchronous parse — only valid after `await init()`.

---

## ⚡ Performance

- Single phrase: **~1 ms** on a modern laptop (in-process wasm).
- Bulk: the underlying Rust engine also ships a GPU backend (wgpu/Metal/
  Vulkan) that batches ~10× faster for thousands of phrases; the JS wrapper
  exposes this via `parseMany` in a follow-up release.

---

## ⚠️ Honest limitations

- **Trained phrasing, not all phrasing.** Unusual vocabulary can mislabel
  (e.g. `next quarter`; movable holidays like दिवाली). Rejections come
  back as diagnostics, never silent wrong dates.
- **Three languages by design.** Other languages produce diagnostics, not
  guesses.
- **Not for high-stakes scheduling.** Legal, medical, or billing dates need
  verified input.
- **Day-part semantics:** an explicit `am`/`pm` always wins over a day-part
  (`night 9:30` → 9:30 PM; `9:30 pm` stays 9:30 PM regardless of position).

---

## 🧪 Try it without installing

Try it live, no install: **[magicbeansai.github.io/what-time](https://magicbeansai.github.io/what-time/)**
(wasm inlined, runs offline after first load; the same page ships as a single
[`what-time.html`](https://github.com/MagicBeansAI/what-time/blob/main/rust/site/what-time.html) file).

---

## 🤝 Contributing

The parser lives in Rust ([`rust/what-time`](https://github.com/MagicBeansAI/what-time/tree/main/rust/what-time)); the training pipeline in Python
([`packages/training`](https://github.com/MagicBeansAI/what-time/tree/main/packages/training)). This wrapper must stay thin — bug fixes belong
upstream, not in JS. See the [repository README](https://github.com/MagicBeansAI/what-time) for the full layout.

---

## 📄 License

MIT. This package derives from an MIT-licensed predecessor; the original
copyright notice is retained as the license requires.
