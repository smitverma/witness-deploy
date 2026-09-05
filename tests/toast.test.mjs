import assert from "node:assert/strict";
import test from "node:test";
import { createToastStore } from "../src/lib/toast.ts";

function snapshot(store) {
  let value;
  const unsub = store.subscribe((v) => (value = v));
  unsub();
  return value;
}

test("toast queue caps at max 3 and dedups identical messages", () => {
  const store = createToastStore({ max: 3, durationMs: 60_000 });
  store.show("one");
  store.show("two");
  store.show("three");
  store.show("four");
  let list = snapshot(store);
  assert.equal(list.length, 3);
  assert.deepEqual(list.map((t) => t.message), ["two", "three", "four"]);

  const before = snapshot(store);
  const dup = store.show("four");
  assert.equal(dup?.message, "four");
  assert.deepEqual(
    snapshot(store).map((t) => t.message),
    before.map((t) => t.message),
  );
  store.clear();
});

test("dismiss(id) removes one toast, dismiss() clears all", () => {
  const store = createToastStore({ max: 3, durationMs: 60_000 });
  const a = store.show("a");
  store.show("b");
  assert.equal(snapshot(store).length, 2);
  store.dismiss(a.id);
  assert.deepEqual(snapshot(store).map((t) => t.message), ["b"]);
  store.dismiss();
  assert.deepEqual(snapshot(store), []);
});

test("blank messages are ignored", () => {
  const store = createToastStore({ max: 3, durationMs: 60_000 });
  assert.equal(store.show("   "), undefined);
  assert.deepEqual(snapshot(store), []);
  store.clear();
});

test("auto-dismiss removes toast after duration", async () => {
  const store = createToastStore({ max: 3, durationMs: 20 });
  store.show("ephemeral");
  assert.equal(snapshot(store).length, 1);
  await new Promise((resolve) => setTimeout(resolve, 60));
  assert.deepEqual(snapshot(store), []);
});
