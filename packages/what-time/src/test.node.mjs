/**
 * Node smoke test, run against the built wrapper: `pnpm test`.
 * Uses the nodejs-friendly import of the bundler glue (Node 20+ can
 * instantiate the module URL directly).
 */
import { test } from "node:test";
import assert from "node:assert/strict";
const { parse, parseExpressions } = await import("../dist/index.js");

const context = {
  reference: "2026-09-13T10:00:00Z",
  timeZone: "Asia/Kolkata",
  limit: 3,
};

test("english parses to occurrences", async () => {
  const result = await parse("game night every Friday at 7:30pm", context);
  assert.equal(result.occurrences.length, 3);
  assert.equal(result.occurrences[0].start, "2026-09-18T19:30:00+05:30");
  assert.ok(result.rrules[0].includes("FREQ=WEEKLY"));
});

test("hindi day-part biases the hour to pm", async () => {
  const result = await parse("कल शाम को आठ बजे मीटिंग", context);
  assert.equal(result.occurrences[0].start, "2026-09-14T20:00:00+05:30");
});

test("hinglish daily recurrence", async () => {
  const result = await parse("har din shaam ko 8 baje", context);
  assert.equal(result.occurrences[0].start, "2026-09-13T20:00:00+05:30");
  assert.equal(result.occurrences.length, 3);
});

test("invalid timezone rejects", async () => {
  await assert.rejects(
    parse("tomorrow at 9am", { ...context, timeZone: "Not/AZone" }),
    TypeError,
  );
});

test("parseExpressions exposes confidence and schedule", async () => {
  const expressions = await parseExpressions("24th august last year");
  assert.equal(expressions.length, 1);
  assert.ok(expressions[0].confidence > 0.9);
});

test("limit clamps and date order passes through", async () => {
  const result = await parse("03/04/2027", { ...context, limit: 1, dateOrder: "DMY" });
  assert.equal(result.occurrences.length, 1);
  assert.equal(result.occurrences[0].start.slice(0, 10), "2027-04-03");
});
