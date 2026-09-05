<script lang="ts">
  import { onDestroy } from "svelte";
  import { commands } from "$lib/api";
  import { showErrorToast } from "$lib/errorToast";
  import type { ComparerWorkspaceState, DiffResult } from "$lib/types";

  let {
    workspace,
    initialLeft = "",
    initialRight = "",
    onStateChange = (_state: ComparerWorkspaceState) => {},
  }: {
    workspace?: ComparerWorkspaceState;
    initialLeft?: string;
    initialRight?: string;
    onStateChange?: (state: ComparerWorkspaceState) => void;
  } = $props();

  let left = $state("");
  let right = $state("");
  let granularity = $state("character");
  let layout = $state<"side" | "stacked">("side");
  let result = $state<DiffResult>({ chunks: [], additions: 0, deletions: 0, unchanged: 0 });
  let timer: ReturnType<typeof setTimeout> | undefined;
  let leftPane: HTMLDivElement;
  let rightPane: HTMLDivElement;
  let syncing = false;
  let appliedInitialLeft = "";
  let appliedInitialRight = "";
  let workspaceInitialized = $state(false);
  // Cancellation token: ignore stale compareText responses.
  let compareVersion = 0;

  onDestroy(() => {
    if (timer) clearTimeout(timer);
    timer = undefined;
    compareVersion += 1;
  });

  $effect(() => {
    if (workspaceInitialized) return;
    if (workspace) {
      left = workspace.left;
      right = workspace.right;
      granularity = workspace.granularity;
      layout = workspace.layout;
    }
    workspaceInitialized = true;
    if (workspace) schedule();
  });

  $effect(() => {
    if (!workspaceInitialized) return;
    onStateChange({ left, right, granularity: granularity as ComparerWorkspaceState["granularity"], layout });
  });

  $effect(() => {
    if (initialLeft && initialLeft !== appliedInitialLeft) {
      appliedInitialLeft = initialLeft;
      left = initialLeft;
      schedule();
    }
    if (initialRight && initialRight !== appliedInitialRight) {
      appliedInitialRight = initialRight;
      right = initialRight;
      schedule();
    }
  });

  function schedule() {
    if (timer) clearTimeout(timer);
    const version = ++compareVersion;
    const snapshotLeft = left;
    const snapshotRight = right;
    const snapshotGranularity = granularity;
    timer = setTimeout(async () => {
      try {
        const value = await commands.compareText(snapshotLeft, snapshotRight, snapshotGranularity);
        if (version === compareVersion) result = value;
      } catch (reason) {
        if (version === compareVersion) showErrorToast(reason);
      }
    }, 120);
  }

  function sync(source: HTMLDivElement, target: HTMLDivElement) {
    if (syncing) return;
    syncing = true;
    target.scrollTop = source.scrollTop;
    target.scrollLeft = source.scrollLeft;
    requestAnimationFrame(() => (syncing = false));
  }

  async function paste(target: "left" | "right") {
    let value: string;
    try {
      value = await navigator.clipboard.readText();
    } catch (reason) {
      showErrorToast(reason);
      return;
    }
    if (target === "left") left = value; else right = value;
    schedule();
  }

  export function handleShortcut(action: string): boolean {
    if (action === "comparer.focusLeft" || action === "comparer.focusRight") {
      const textareas = [...document.querySelectorAll<HTMLTextAreaElement>(".comparer-tool textarea")];
      textareas[action.endsWith("Left") ? 0 : 1]?.focus();
      return true;
    }
    if (action === "comparer.recompute") {
      if (!left && !right) return false;
      const version = ++compareVersion;
      void commands.compareText(left, right, granularity).then(
        (value) => {
          if (version === compareVersion) result = value;
        },
        (reason) => {
          if (version === compareVersion) showErrorToast(reason);
        },
      );
      return true;
    }
    if (action === "comparer.clear") {
      left = "";
      right = "";
      schedule();
      return true;
    }
    if (action === "comparer.toggleLayout") {
      layout = layout === "side" ? "stacked" : "side";
      return true;
    }
    return false;
  }
</script>

<section class="comparer-tool">
  <header>
    <div class="controls">
      <select bind:value={granularity} onchange={schedule} aria-label="Diff granularity"><option value="character">Characters</option><option value="line">Lines</option><option value="word">Words</option></select>
      <button class="text-button" style="padding-right:0;" onclick={() => { left = ""; right = ""; schedule(); }}>Clear</button>
      <button class="text-button layout-toggle" style="padding:0; margin: 0" onclick={() => (layout = layout === "side" ? "stacked" : "side")}>{layout === "side" ? "Stack" : "Parallel"}</button>
    </div>
  </header>
  <div class="inputs" class:stacked={layout === "stacked"}>
    <label><span>Left <button class="text-button compact" onclick={() => void paste("left")}>Paste</button></span><textarea bind:value={left} oninput={schedule}></textarea></label>
    <label><span>Right <button class="text-button compact" onclick={() => void paste("right")}>Paste</button></span><textarea bind:value={right} oninput={schedule}></textarea></label>
  </div>
  <div class="stats"><span class="add">+{result.additions} additions</span><span class="delete">−{result.deletions} deletions</span><span>{result.unchanged} unchanged</span></div>
  <div class="diff" class:stacked={layout === "stacked"}>
    <div bind:this={leftPane} onscroll={() => sync(leftPane, rightPane)}>{#each result.chunks as chunk}{#if chunk.kind !== "insert"}<span class:delete={chunk.kind === "delete"}>{chunk.text}</span>{/if}{/each}</div>
    <div bind:this={rightPane} onscroll={() => sync(rightPane, leftPane)}>{#each result.chunks as chunk}{#if chunk.kind !== "delete"}<span class:add={chunk.kind === "insert"}>{chunk.text}</span>{/if}{/each}</div>
  </div>
</section>

<style>
  .comparer-tool { display: grid; grid-template-rows: 52px minmax(0, 1fr) 30px minmax(0, 1fr); gap: 8px; height: 100%; min-height: 0; padding: 0 12px 12px; overflow: hidden; color: #dce1e8; }
  header { display: flex; align-items: center; justify-content: space-between; }
  .controls { display: flex; gap: 6px; } select, button { height: 29px; padding: 0 8px; border: 1px solid var(--border-strong, #343c47); border-radius: 4px; color: #cbd2dc; background: var(--surface-2, #191e25); cursor: pointer; }
  .layout-toggle { box-sizing: border-box; width: 56px; }
  .inputs, .diff { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; height: 100%; min-height: 0; align-self: stretch; } .stacked { grid-template-columns: 1fr; grid-template-rows: 1fr 1fr; }
  .inputs label { display: grid; grid-template-rows: 29px minmax(0, 1fr); width: 100%; height: 100%; min-width: 0; min-height: 0; overflow: hidden; color: #949eaa; border: 1px solid var(--border, #29303a); border-radius: 5px; background: var(--surface, #12161b); font-size: var(--font-size-body); }
  .inputs label > span { display: flex; align-items: center; justify-content: space-between; padding: 0 6px 0 9px; border-bottom: 1px solid var(--border, #29303a); background: var(--surface-2, #191e25); }
  .inputs label > span button { height: 23px; min-height: 23px; border-color: transparent; background: transparent; }
  .inputs label > span button:hover { border-color: var(--border-strong, #343c47); background: var(--surface-3, #242b34); }
  textarea { width: 100%; height: 100%; min-height: 0; padding: 10px; resize: none; border: 0; border-radius: 0; color: #dfe4eb; background: #0c0f13; font: var(--font-size-body)/1.5 ui-monospace, monospace; }
  .stats { display: flex; align-items: center; gap: 18px; padding: 0 10px; color: #8993a0; border: 1px solid var(--border, #29303a); border-radius: 4px; background: var(--surface, #12161b); font-size: var(--font-size-compact); } .stats .add { color: #6ee7b7; } .stats .delete { color: #fca5a5; }
  .diff > div { width: 100%; height: 100%; min-height: 0; padding: 10px; overflow: auto; border: 1px solid var(--border, #29303a); border-radius: 5px; color: #cbd2dc; background: #0c0f13; white-space: pre-wrap; overflow-wrap: anywhere; font: var(--font-size-body)/1.55 ui-monospace, monospace; }
  .diff span.add { color: #d1fae5; background: #14532d; } .diff span.delete { color: #fee2e2; background: #7f1d1d; text-decoration: line-through; }
</style>
