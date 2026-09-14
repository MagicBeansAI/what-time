# what-time

A compact neural parser for English schedules. It converts natural-language
text into dates, time ranges, and RFC 5545 recurrence rules, running entirely
locally — no server, no network calls.

```
what-time --reference "2026-09-09T12:00:00+06:00" --time-zone "Asia/Dhaka" \
    "Sat Sun 1pm-8pm Mon 10pm-12am"
```

```
2026-09-12T13:00:00+06:00 → 2026-09-12T20:00:00+06:00
2026-09-13T13:00:00+06:00 → 2026-09-13T20:00:00+06:00
2026-09-14T22:00:00+06:00 → 2026-09-15T00:00:00+06:00
```

## Layout

| Crate | Purpose |
| --- | --- |
| `what-time` | Library: tokenizer, int6 model inference, schedule compiler, calendar resolver |
| `what-time-cli` | Command-line interface |
| `what-time-web` | Local web UI for interactive testing |

```
what-time/src/
  tokenizer.rs   scanning + sparse feature rows (UTF-16 hashing)
  lexicon.rs     English weekday/month/number/unit tables
  labels.rs      the 35 model roles
  model/         int6 weight decode + CPU forward pass (f64 math, f32 stores)
  tagger.rs      inference windows (96-token stride, 16-token context)
  compile.rs     roles → typed Schedule (the largest module)
  quantity.rs    spoken-number and duration readers
  zoned.rs       civil date arithmetic + DST-safe local↔instant resolution (jiff)
  calendar.rs    date specifications → local periods
  clock.rs       clock times, day parts, seconds-of-day
  occurrence.rs  clauses + periods → occurrences
  exclusions.rs  recurrence exception filters
  recurrence.rs  bounded recurrence expansion
  rrule.rs       RFC 5545 export
  resolve.rs     schedules → instants / previews / rules
  schedule.rs    parser facade

evals/
  data/          gold corpora (JSONL): end-to-end results, schedules, oracle labels
  active/        reference fixtures for numeric model parity (binary)
```

## How it works

1. **Tokenization** — one CPU scan splits the input into tokens and emits a
   sparse feature row per token: character shape, casing, digit and
   punctuation class, length bucket, lexicon membership, and hashes of
   neighboring tokens.
2. **The model** — a ~144k-parameter transformer encoder (two 4-head
   blocks, `model/transformer.rs`), int8-quantized, trained on the standard
   generator supervision plus LLM-authored English/Hindi/Hinglish corpora.
   It classifies tokens into 35
   named semantic roles (clock hours, weekdays, range separators, …) plus a
   boundary score that splits the input into independent expressions.
3. **Compilation** — predicted roles compile into a typed `Schedule`: date
   anchors, clock points, ranges, quantities, recurrence rules, exclusions.
   The compiler never consults the reference date.
4. **Resolution** — the resolver turns schedules into instants. Calendar days
   and weeks preserve wall-clock time across DST; hours and minutes add
   elapsed time. Recurrence produces a bounded preview plus RFC 5545
   properties.

The caller provides the reference instant and timezone; context never enters
the model. An unknown timezone rejects the call rather than falling back.

## Usage

### Library

```rust
use what_time::{parse, ParseContext};

let context = ParseContext {
    reference: "2026-09-09T12:00:00+06:00".into(),
    time_zone: "Asia/Dhaka".into(),
    limit: Some(30),
    ..Default::default()
};
let result = parse("tomorrow at 3pm", &context)?;
for occurrence in &result.occurrences {
    println!("{} (all day: {})", occurrence.start, occurrence.all_day);
}
```

`Parser::new(ParserOptions { .. })` provides a reusable instance;
`parse`/`parse_many` are shared-default shortcuts. `resolve(schedule,
options)` resolves an already-compiled schedule. Cleanup is ownership-based —
there is no `dispose()`.

### CLI

```
what-time [OPTIONS] <TEXT>
  -r, --reference <INSTANT>    ISO instant with Z or an offset [default: now]
  -t, --time-zone <ZONE>       IANA time zone name [default: UTC]
  -l, --limit <COUNT>          Maximum previewed occurrences, 1..=1000 [default: 30]
  -j, --json                   Print the full parse result as JSON
```

### Web playground (static, runs in the browser)

`site/` is a fully static playground: the parser is compiled to
WebAssembly (`what-time-wasm`), so the trilingual transformer model, weights,
and the bundled IANA timezone database all run client-side — no server.
Serve the directory from any static host:

```sh
python3 -m http.server --directory site 8788
```

Rebuild the wasm after changing the library or weights: `site/build.sh`
(needs `wasm32-unknown-unknown` and a matching `wasm-bindgen-cli`). The
wasm module exposes `parse(text, reference, timeZone, limit, dateOrder)`
and `parse_expressions(text)`, returning the same JSON the library emits.
A traditional HTTP API remains available via the `what-time-web` crate if
you need curl-able parsing, but the playground itself is serverless.
