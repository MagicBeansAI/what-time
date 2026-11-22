/* tslint:disable */
/* eslint-disable */

/**
 * An English description of a cron line: `{"description": ...}`, or
 * `{"error": ...}` for an invalid line.
 */
export function describe_cron(expression: string): string;

/**
 * Full parse: occurrences, diagnostics, rrules, crons, timings.
 */
export function parse(text: string, reference: string, time_zone: string, limit: bigint, date_order: string): string;

/**
 * A cron line read back: English description, schedule, fires from the
 * reference, RRULE and normalized cron (the `CronResult` JSON shape), or
 * `{"error": ...}` for an invalid line.
 */
export function parse_cron(expression: string, reference: string, time_zone: string, limit: bigint): string;

/**
 * Model-level schedules: expression spans, confidence, schedule JSON.
 */
export function parse_expressions(text: string): string;

/**
 * The library version embedded in this wasm module.
 */
export function version(): string;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly describe_cron: (a: number, b: number) => [number, number];
    readonly parse: (a: number, b: number, c: number, d: number, e: number, f: number, g: bigint, h: number, i: number) => [number, number];
    readonly parse_cron: (a: number, b: number, c: number, d: number, e: number, f: number, g: bigint) => [number, number];
    readonly parse_expressions: (a: number, b: number) => [number, number];
    readonly version: () => [number, number];
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
