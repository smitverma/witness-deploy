import assert from "node:assert/strict";
import test from "node:test";
import {
  createPayloadWarehouse,
  generatePayloads,
  findTestPositions,
  planPayloadRows,
  processPayloads,
  renderTestRequestValues,
} from "../src/lib/intruder.ts";

test("number payloads support sequential ranges and count limits", () => {
  const warehouse = createPayloadWarehouse(new Date("2026-07-29T00:00:00Z"));
  warehouse.type = "numbers";
  Object.assign(warehouse.numbers, { from: "0", to: "10", step: "2", count: "3" });
  assert.deepEqual(generatePayloads(warehouse).payloads, ["0", "2", "4"]);
});

test("null payloads support finite and continuous modes", () => {
  const warehouse = createPayloadWarehouse();
  warehouse.type = "null";
  warehouse.nullPayload.count = "3";
  assert.deepEqual(generatePayloads(warehouse), {
    payloads: ["", "", ""],
    repeatIndefinitely: false,
  });
  warehouse.nullPayload.mode = "infinite";
  assert.equal(generatePayloads(warehouse).repeatIndefinitely, true);
});

test("character generator emits every permutation in length order", () => {
  const warehouse = createPayloadWarehouse();
  warehouse.type = "bruteForce";
  Object.assign(warehouse.bruteForce, {
    characterSet: "ab",
    minLength: "1",
    maxLength: "2",
  });
  assert.deepEqual(generatePayloads(warehouse).payloads, [
    "a", "b", "aa", "ab", "ba", "bb",
  ]);
});

test("dates honor range, step, and format", () => {
  const warehouse = createPayloadWarehouse();
  warehouse.type = "dates";
  Object.assign(warehouse.dates, {
    from: "2026-07-28",
    to: "2026-07-30",
    step: "1",
    unit: "days",
    formatMode: "preset",
    format: "MM/DD/YYYY",
  });
  assert.deepEqual(generatePayloads(warehouse).payloads, [
    "07/28/2026", "07/29/2026", "07/30/2026",
  ]);
});

test("character substitution includes original and substituted variants", () => {
  const warehouse = createPayloadWarehouse();
  warehouse.type = "characterSubstitution";
  warehouse.characterSubstitution.itemsText = "a";
  assert.deepEqual(generatePayloads(warehouse).payloads, ["a", "4"]);
});

test("payload processing applies enabled rules in order", async () => {
  const base = {
    enabled: true,
    value: "",
    match: "",
    replacement: "",
    useRegex: false,
    caseSensitive: true,
    start: "0",
    length: "1",
    operation: "",
  };
  const values = await processPayloads(["admin"], [
    { ...base, id: "1", type: "addPrefix", value: "user-" },
    { ...base, id: "2", type: "matchReplace", match: "admin", replacement: "root" },
    { ...base, id: "3", type: "modifyCase", operation: "upper" },
  ]);
  assert.deepEqual(values, ["USER-ROOT"]);
});

test("spread mode places one value into every position", () => {
  const plan = planPayloadRows(
    "spread",
    [{ payloads: ["one", "two"], repeatIndefinitely: false }],
    3,
  );
  assert.deepEqual(plan.rows, [
    ["one", "one", "one"],
    ["two", "two", "two"],
  ]);
});

test("map mode advances position sets together", () => {
  const plan = planPayloadRows(
    "map",
    [
      { payloads: ["a", "b", "c"], repeatIndefinitely: false },
      { payloads: ["1", "2"], repeatIndefinitely: false },
    ],
    2,
  );
  assert.deepEqual(plan.rows, [["a", "1"], ["b", "2"]]);
});

test("map mode ends with its shortest finite set", () => {
  const plan = planPayloadRows(
    "map",
    [
      { payloads: ["a", "b"], repeatIndefinitely: false },
      { payloads: [""], repeatIndefinitely: true },
    ],
    2,
  );
  assert.equal(plan.repeatIndefinitely, false);
  assert.deepEqual(plan.rows, [["a", ""], ["b", ""]]);
});

test("combine mode produces the Cartesian product", () => {
  const plan = planPayloadRows(
    "combine",
    [
      { payloads: ["a", "b"], repeatIndefinitely: false },
      { payloads: ["1", "2"], repeatIndefinitely: false },
    ],
    2,
  );
  assert.deepEqual(plan.rows, [
    ["a", "1"], ["a", "2"], ["b", "1"], ["b", "2"],
  ]);
});

test("multiple values render into their corresponding positions", () => {
  const template = "GET /?first=§x§&second=§y§ HTTP/1.1\r\nHost: example.test\r\n\r\n";
  const rendered = renderTestRequestValues(
    template,
    findTestPositions(template),
    ["one", "two"],
  );
  assert.match(rendered, /first=one&second=two/);
});
