import assert from "node:assert/strict";
import test from "node:test";

import {
  SHORTCUTS,
  assertValidShortcutRegistry,
  defaultShortcutModifier,
  formatShortcut,
  matchesShortcut,
  normalizeEventKey,
  normalizeShortcutModifier,
  resolveShortcut,
  validateShortcutRegistry,
} from "../src/lib/keyboard-shortcuts.ts";

// Registry validation is test-only (no import side-effect throw in the lib).
assertValidShortcutRegistry();

const shortcut = (id) => SHORTCUTS.find((definition) => definition.id === id);

function keyEvent(overrides = {}) {
  return {
    key: "f",
    code: "KeyF",
    shiftKey: false,
    altKey: false,
    metaKey: false,
    ctrlKey: false,
    isComposing: false,
    repeat: false,
    target: null,
    ...overrides,
  };
}

test("the shortcut registry is stable and collision-free", () => {
  assert.deepEqual(validateShortcutRegistry(), []);
  assert.equal(SHORTCUTS.some((definition) => definition.primary && definition.key === "Tab"), false);
  for (const id of [
    "proxy.sendFuzz",
    "history.sendFuzz",
    "siteMap.sendFuzz",
    "replay.sendToFuzz",
    "fuzz.sendToFuzz",
    "organizer.sendFuzz",
  ]) {
    const definition = shortcut(id);
    assert.ok(definition);
    assert.equal(definition.key, "i");
    assert.equal(definition.shift, undefined);
  }
  assert.equal(shortcut("proxy.sendReplay").key, "r");
  assert.equal(shortcut("proxy.saveOrganizer").key, "o");
  for (const definition of SHORTCUTS) {
    assert.ok(definition.id);
    assert.ok(definition.action);
    assert.ok(definition.label);
    assert.ok(definition.scope);
    assert.ok(definition.key);
    assert.ok(definition.availability);
  }
});

test("platform defaults and display formatting follow the selected modifier", () => {
  assert.equal(defaultShortcutModifier("macos"), "command");
  assert.equal(defaultShortcutModifier("windows"), "control");
  assert.equal(defaultShortcutModifier("linux"), "control");
  assert.equal(normalizeShortcutModifier("command", "macos"), "command");
  assert.equal(normalizeShortcutModifier("invalid", "macos"), "command");
  assert.equal(normalizeShortcutModifier(undefined, "macos"), "command");
  assert.equal(normalizeShortcutModifier("command", "linux"), "control");

  const replaySend = shortcut("replay.send");
  const dropAll = shortcut("proxy.dropAll");
  assert.equal(formatShortcut(replaySend, "macos", "command"), "⌘F");
  assert.equal(formatShortcut(replaySend, "macos", "control"), "⌃F");
  assert.equal(formatShortcut(replaySend, "windows", "control"), "Ctrl+F");
  assert.equal(formatShortcut(dropAll, "macos", "command"), "⌘⇧D");
  assert.equal(formatShortcut(dropAll, "linux", "control"), "Ctrl+Shift+D");
  assert.equal(formatShortcut(shortcut("fuzz.launch"), "linux", "control"), "Ctrl+Enter");
  assert.equal(formatShortcut(shortcut("decoder.clear"), "windows", "control"), "Ctrl+Backspace");
});

test("macOS Command preference requires Meta and rejects Control or extra modifiers", () => {
  const definition = shortcut("replay.send");
  assert.equal(matchesShortcut(keyEvent({ metaKey: true }), definition, "macos", "command"), true);
  assert.equal(matchesShortcut(keyEvent({ ctrlKey: true }), definition, "macos", "command"), false);
  assert.equal(matchesShortcut(keyEvent({ metaKey: true, ctrlKey: true }), definition, "macos", "command"), false);
  assert.equal(matchesShortcut(keyEvent({ metaKey: true, shiftKey: true }), definition, "macos", "command"), false);
});

test("Control preference and non-macOS platforms only accept Control", () => {
  const definition = shortcut("replay.send");
  assert.equal(matchesShortcut(keyEvent({ ctrlKey: true }), definition, "macos", "control"), true);
  assert.equal(matchesShortcut(keyEvent({ metaKey: true }), definition, "macos", "control"), false);
  assert.equal(matchesShortcut(keyEvent({ ctrlKey: true }), definition, "windows", "control"), true);
  assert.equal(matchesShortcut(keyEvent({ metaKey: true }), definition, "linux", "control"), false);
});

test("repeat, IME, punctuation, and scope precedence are deterministic", () => {
  const save = shortcut("global.project.save");
  const previous = shortcut("history.selectPrevious");
  assert.equal(matchesShortcut(keyEvent({ key: "s", code: "KeyS", metaKey: true, repeat: true }), save, "macos", "command"), false);
  assert.equal(matchesShortcut(keyEvent({ key: "s", code: "KeyS", metaKey: true, isComposing: true }), save, "macos", "command"), false);
  assert.equal(matchesShortcut(keyEvent({ key: "ArrowUp", code: "ArrowUp", repeat: true }), previous, "linux", "control"), true);
  assert.equal(normalizeEventKey({ key: "?", code: "Slash" }), "/");

  const tabAction = {
    id: "tab.test",
    action: "tab.action",
    scope: "Replay",
    key: "x",
    label: "Tab action",
    description: "Tab action",
    availability: "Always",
  };
  const globalAction = { ...tabAction, id: "global.test", action: "global.action", scope: "global" };
  const event = keyEvent({ key: "x", code: "KeyX" });
  assert.equal(resolveShortcut(event, ["Replay", "global"], "linux", "control", [tabAction, globalAction])?.action, "tab.action");
  assert.equal(resolveShortcut(event, ["global", "Replay"], "linux", "control", [tabAction, globalAction])?.action, "global.action");
});
