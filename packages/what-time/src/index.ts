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

import initRaw, { initSync, parse as parseRaw, parse_expressions } from "../wasm/what_time_wasm.js";
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
  rrules: string[];
  truncated: boolean;
  diagnostics: Diagnostic[];
  timings: {
    tokenizeMs: number;
    inferMs: number;
    resolveMs: number;
  };
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
      if (!isNode) throw new Error("wasm module could not be loaded");
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

/** Synchronous parse for callers that already awaited {@link init}. */
export function parseSync(text: string, context: ParseContext): ParseResult {
  return run(text, context);
}
