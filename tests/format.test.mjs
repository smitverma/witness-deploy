import assert from "node:assert/strict";
import test from "node:test";
import { formatClock, formatDate, formatRelativeTime, formatTime } from "../src/lib/format.ts";

test("formatRelativeTime honors injected now", () => {
  const now = new Date("2026-09-05T12:00:00Z").getTime();
  assert.equal(formatRelativeTime("2026-09-05T08:00:00Z", now), "Today");
  assert.equal(formatRelativeTime("2026-09-04T12:00:00Z", now), "Yesterday");
  assert.equal(formatRelativeTime("2026-09-02T12:00:00Z", now), "3 days ago");
  assert.equal(formatRelativeTime("not-a-date", now), "");
});

test("formatClock respects 12/24h", () => {
  const date = new Date("2026-09-05T13:05:06Z");
  const twelve = formatClock(date, false);
  const twentyFour = formatClock(date, true);
  assert.ok(twelve.length > 0);
  assert.ok(twentyFour.length > 0);
  assert.notEqual(twelve, twentyFour);
});

test("formatDate and formatTime handle strings", () => {
  assert.equal(formatDate("not-a-date"), "");
  assert.ok(formatDate("2026-09-05T00:00:00Z").length > 0);
  assert.equal(formatTime(0).length > 0, true);
  assert.equal(formatTime("not-a-date"), "");
});
