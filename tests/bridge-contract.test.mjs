import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const libRs = readFileSync(path.join(root, "src-tauri", "src", "lib.rs"), "utf8");
const apiTs = readFileSync(path.join(root, "src", "lib", "api.ts"), "utf8");

function rustCommands(src) {
  const m = src.match(/generate_handler!\s*\[([\s\S]*?)\]/);
  assert.ok(m, "generate_handler! block not found");
  return m[1]
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean)
    .map((s) => s.replace(/^ui_bridge::/, ""))
    .filter((s) => /^[a-z0-9_]+$/i.test(s));
}

function invokeNames(src) {
  const names = new Set();
  for (const m of src.matchAll(/invoke<[^>]*>\s*\(\s*"([^"]+)"/g)) names.add(m[1]);
  return [...names].sort();
}

test("every Rust command is reachable from api.ts", () => {
  const rust = rustCommands(libRs);
  const js = new Set(invokeNames(apiTs));
  const missing = rust.filter((c) => !js.has(c));
  assert.deepEqual(missing, [], `api.ts missing invokes: ${missing.join(", ")}`);
});

test("every invoke() targets a registered Rust command", () => {
  const rust = new Set(rustCommands(libRs));
  const js = invokeNames(apiTs);
  const extra = js.filter((c) => !rust.has(c));
  assert.deepEqual(extra, [], `invoke() without Rust handler: ${extra.join(", ")}`);
});

test("command count sanity (catches silent drops)", () => {
  const rust = rustCommands(libRs);
  assert.ok(rust.length >= 60, `expected >=60 commands, got ${rust.length}`);
});
