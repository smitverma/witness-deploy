import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const pageSource = await readFile(
  new URL("../src/routes/+page.svelte", import.meta.url),
  "utf8",
);

test("Forge stays mounted while another workspace tab is active", () => {
  const hostStart = pageSource.indexOf('class="forge-panel-host"');
  const workspaceSwitchStart = pageSource.indexOf(
    '{#if activeTab === "Proxy"}',
  );
  const forgeComponentCount = (pageSource.match(/<AiController\b/g) ?? [])
    .length;

  assert.notEqual(hostStart, -1, "Forge needs a persistent panel host");
  assert.ok(
    hostStart < workspaceSwitchStart,
    "Forge must be mounted outside the active workspace switch",
  );
  assert.match(
    pageSource.slice(hostStart, workspaceSwitchStart),
    /hidden=\{activeTab !== "AI"\}/,
  );
  assert.match(
    pageSource.slice(hostStart - 80, hostStart),
    /snapshot && workspaceReady/,
    "Forge must mount after the saved workspace has been restored",
  );
  assert.equal(
    forgeComponentCount,
    1,
    "the page should own one Forge component instance",
  );
  assert.doesNotMatch(
    pageSource,
    /\{:else if activeTab === "AI"[^\n]*\}\s*<AiController/,
    "Forge must not be conditionally mounted by the active tab",
  );
});
