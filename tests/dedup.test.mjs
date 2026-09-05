import assert from "node:assert/strict";
import test from "node:test";
import { highlightSearchText } from "../src/lib/text-search.ts";
import { buildTabBarEntries } from "../src/lib/tab-groups.ts";

test("highlightSearchText splits matches case-insensitively", () => {
  const parts = highlightSearchText("Hello hello world", "hello");
  assert.deepEqual(parts, [
    { text: "Hello", match: true },
    { text: " ", match: false },
    { text: "hello", match: true },
    { text: " world", match: false },
  ]);
});

test("highlightSearchText returns whole value when query blank", () => {
  assert.deepEqual(highlightSearchText("abc", "   "), [{ text: "abc", match: false }]);
});

test("buildTabBarEntries groups tabs by known groups", () => {
  const groups = [
    { id: "g1", name: "One", color: "#fff", collapsed: false },
    { id: "g2", name: "Two", color: "#000", collapsed: true },
  ];
  const tabs = [
    { id: 1, groupId: null },
    { id: 2, groupId: "g1" },
    { id: 3, groupId: "g1" },
    { id: 4, groupId: "missing" },
  ];
  const entries = buildTabBarEntries(tabs, groups);
  assert.equal(entries.length, 3);
  assert.equal(entries[0].kind, "tab");
  assert.equal(entries[1].kind, "group");
  if (entries[1].kind === "group") {
    assert.equal(entries[1].group.id, "g1");
    assert.deepEqual(entries[1].tabs.map((t) => t.id), [2, 3]);
  }
  assert.equal(entries[2].kind, "tab");
});
