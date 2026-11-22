/**
 * what-time — a neural parser for English, Hindi, and Hinglish time
 * expressions. This package is a thin, typed wrapper around the
 * WebAssembly build of the Rust implementation: the same tokenizer,
 * transformer, compiler, and exact calendar resolver the CLI and the
 * playground use. There is no parallel JavaScript implementation and no
 * network call — parsing runs entirely in-process.
 *
 * ```ts
 * import { parse } from "what-time";
 *
 * const result = await parse("कल शाम को आठ बजे मीटिंग", {
 *   reference: new Date().toISOString(),
 *   timeZone: "Asia/Kolkata",
 *   limit: 3,
 * });
 * console.log(result.occurrences[0].start); // "2026-09-14T20:00:00+05:30"
 * ```
 */

import initRaw, {
  describe_cron,
  initSync,
  parse as parseRaw,
  parse_cron,
  parse_expressions,
} from "../wasm/what_time_wasm.js";
import { VERSION } from "./version.js";

/** The npm package version (generated from package.json by pnpm build). */
export { VERSION };

export interface ParseContext {
  /** ISO instant with Z or an offset; the "now" the text is read against. */
  reference: string;
  /** IANA zone name. Unknown zones reject rather than falling back. */
  timeZone: string;
  /** Maximum previewed occurrences, 1..=1000. */
  limit?: number;
  /** Ambiguous numeric dates read month-first (default) or day-first. */
  dateOrder?: "MDY" | "DMY";
}

export interface TimeRange {
  start: string;
  end?: string;
  allDay?: boolean;
}

export interface Diagnostic {
  code: string;
  message: string;
  severity: "error" | "warning";
  start?: number;
  end?: number;
}

export interface ParseResult {
  occurrences: TimeRange[];
  /** RFC 5545 rules, one `DTSTART…\nRRULE:…` block per recurring rule. */
  rrules: string[];
  /**
   * Five-field cron expressions (`minute hour day-of-month month
   * day-of-week`, weekday names) in the caller's time zone, for every
   * recurring rule cron can state exactly. A rule it cannot state adds an
   * `unsupported-cron` diagnostic and stays available in `rrules`.
   */
  crons: string[];
  /**
   * Elapsed-time steps for sub-daily rules cron cannot state ("every 5
   * hours"): fire at `start`, then every `everySeconds`. For schedulers
   * with interval triggers.
   */
  intervals: { everySeconds: number; start: string; until?: string; count?: number }[];
  /**
   * Whether the text asks for something to be scheduled ("request") or only
   * talks about time ("mention": past events, habits, facts, titles,
   * idioms). `score` is the request probability. The time expressions are
   * reported either way; create a schedule only for a request. Absent when
   * the bundled model has no intent head.
   */
  intent?: { label: "request" | "mention"; score: number };
  truncated: boolean;
  diagnostics: Diagnostic[];
  timings: {
    tokenizeMs: number;
    inferMs: number;
    resolveMs: number;
  };
}

/** A cron line read back into a schedule. */
export interface CronResult {
  /** The line as given, trimmed. */
  expression: string;
  /** English: "Every 15 minutes from 9:00 AM to 5:45 PM on weekdays". */
  description: string;
  /** The typed schedule, in the same shape `parseExpressions` returns. */
  schedule: unknown;
  /** When it fires from `reference`, in `timeZone`. */
  occurrences: TimeRange[];
  rrules: string[];
  /** The line re-exported by the cron writer (`1-5` becomes `MON-FRI`). */
  crons: string[];
  truncated: boolean;
  /** `never-fires` for a valid line no date matches (30 February). */
  diagnostics: Diagnostic[];
}

export interface Expression {
  text: string;
  start: number;
  end: number;
  confidence: number;
  schedule: unknown;
  diagnostics: Diagnostic[];
}

let ready: Promise<void> | null = null;

/** The smallest module using a SIMD instruction: `(func (result v128) i32.const 0 i8x16.splat)`. */
const SIMD_PROBE = new Uint8Array([
  0, 97, 115, 109, 1, 0, 0, 0, 1, 5, 1, 96, 0, 1, 123, 3, 2, 1, 0, 10, 8, 1, 6, 0, 65, 0, 253, 15, 11,
]);

/**
 * Loads and instantiates the wasm module once. In browsers and bundlers
 * the glue fetches its own module URL; under Node (SSR, tests) there is no
 * fetch for file URLs, so the module is instantiated synchronously from
 * disk instead.
 */
export function init(): Promise<void> {
  ready ??= (async () => {
    try {
      await initRaw();
    } catch {
      // Node (SSR, tests): no fetch for file URLs. The specifiers below are
      // built at runtime so browser bundlers never try to resolve them.
      const isNode =
        typeof process !== "undefined" && process.versions?.node !== undefined;
      if (!isNode) {
        throw new Error(
          WebAssembly.validate(SIMD_PROBE)
            ? "wasm module could not be loaded"
            : "what-time needs WebAssembly SIMD (Chrome/Edge 91+, Firefox 89+, Safari 16.4+, Node 16.4+)",
        );
      }
      const fsModule = "node:fs/prom" + "ises";
      const urlModule = "node:u" + "rl";
      const { readFile } = (await import(/* @vite-ignore */ fsModule)) as typeof import("node:fs/promises");
      const { fileURLToPath } = (await import(/* @vite-ignore */ urlModule)) as typeof import("node:url");
      const bytes = await readFile(
        fileURLToPath(new URL("../wasm/what_time_wasm_bg.wasm", import.meta.url)),
      );
      initSync(new WebAssembly.Module(new Uint8Array(bytes)));
    }
  })();
  return ready;
}

function run(text: string, context: ParseContext): ParseResult {
  const json = parseRaw(
    text,
    context.reference,
    context.timeZone,
    BigInt(Math.min(1000, Math.max(1, Math.trunc(context.limit ?? 30)))),
    context.dateOrder ?? "MDY",
  );
  const parsed = JSON.parse(json) as ParseResult | { error: string };
  if ("error" in parsed) {
    throw new TypeError(parsed.error);
  }
  return parsed;
}

/**
 * Parse one schedule expression into occurrences, RFC 5545 rules, and
 * diagnostics. Safe to call concurrently; the wasm module loads once.
 */
export async function parse(text: string, context: ParseContext): Promise<ParseResult> {
  await init();
  return run(text, context);
}

/**
 * Model-level view of one input: expression spans, per-expression
 * confidence, schedule JSON, and expression-level diagnostics.
 */
export async function parseExpressions(text: string): Promise<Expression[]> {
  await init();
  return JSON.parse(parse_expressions(text)) as Expression[];
}

function cronJson<T>(json: string): T {
  const parsed = JSON.parse(json) as T | { error: string };
  if (parsed && typeof parsed === "object" && "error" in parsed) {
    throw new TypeError((parsed as { error: string }).error);
  }
  return parsed as T;
}

/**
 * Read a five-field cron line (or `@daily`-style macro): an English
 * description plus when it fires from `context.reference` in
 * `context.timeZone`, with RRULE and normalized cron exports. When
 * day-of-month and day-of-week are both restricted, a day matches when
 * either does (POSIX). Throws a TypeError naming what is wrong with an
 * invalid line.
 */
export async function parseCron(
  expression: string,
  context: Omit<ParseContext, "dateOrder">,
): Promise<CronResult> {
  await init();
  return cronJson<CronResult>(
    parse_cron(
      expression,
      context.reference,
      context.timeZone,
      BigInt(Math.min(1000, Math.max(1, Math.trunc(context.limit ?? 30)))),
    ),
  );
}

/** English description of a cron line; throws a TypeError if it is invalid. */
export async function describeCron(expression: string): Promise<string> {
  await init();
  return cronJson<{ description: string }>(describe_cron(expression)).description;
}

/** Synchronous parse for callers that already awaited {@link init}. */
export function parseSync(text: string, context: ParseContext): ParseResult {
  return run(text, context);
}
