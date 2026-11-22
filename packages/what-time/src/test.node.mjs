/**
 * Node smoke test, run against the built wrapper: `pnpm test`.
 * Uses the nodejs-friendly import of the bundler glue (Node 20+ can
 * instantiate the module URL directly).
 */
import { test } from "node:test";
import assert from "node:assert/strict";
const { parse, parseExpressions, parseCron, describeCron } = await import("../dist/index.js");

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

test("recurring phrases export cron, or say why not", async () => {
  const window = await parse("every 15 minutes between 9am and 6pm on weekdays", context);
  assert.deepEqual(window.crons, ["*/15 9-17 * * MON-FRI", "0 18 * * MON-FRI"]);
  const hinglish = await parse("har do ghante mein", context);
  assert.deepEqual(hinglish.crons, ["0 */2 * * *"]);
  const nth = await parse("first monday of every month", context);
  assert.deepEqual(nth.crons, []);
  assert.ok(nth.diagnostics.some((value) => value.code === "unsupported-cron"));
  assert.ok(nth.rrules[0].includes("BYSETPOS=1"));
});

test("cron lines read back as English and fires", async () => {
  const result = await parseCron("*/15 9-17 * * 1-5", context);
  assert.equal(result.description, "Every 15 minutes from 9:00 AM to 5:45 PM on weekdays");
  assert.deepEqual(result.crons, ["*/15 9-17 * * MON-FRI"]);
  assert.equal(result.occurrences[0].start, "2026-09-14T09:00:00+05:30");
  assert.equal(await describeCron("@weekly"), "At midnight every Sunday");
  await assert.rejects(() => describeCron("0 0 9 * * *"), /five fields/);
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
