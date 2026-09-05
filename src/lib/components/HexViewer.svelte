<script lang="ts">
  import { showErrorToast } from "$lib/errorToast";

  let { bytes }: { bytes: Uint8Array } = $props();
  let anchor = $state<number | null>(null);
  let focus = $state<number | null>(null);

  const rows = $derived(
    Array.from({ length: Math.ceil(bytes.length / 16) }, (_, index) => ({
      offset: index * 16,
      values: Array.from(bytes.slice(index * 16, index * 16 + 16)),
    })),
  );
  const selection = $derived.by(() => {
    if (anchor === null || focus === null) return [] as number[];
    const start = Math.min(anchor, focus);
    const end = Math.max(anchor, focus);
    return Array.from(bytes.slice(start, end + 1));
  });

  function select(index: number, extend: boolean) {
    if (!extend || anchor === null) anchor = index;
    focus = index;
  }

  function selected(index: number) {
    return anchor !== null && focus !== null && index >= Math.min(anchor, focus) && index <= Math.max(anchor, focus);
  }

  async function copyHex() {
    try {
      await navigator.clipboard.writeText(selection.map((value) => value.toString(16).padStart(2, "0")).join(" "));
    } catch (reason) {
      showErrorToast(reason);
    }
  }

  async function copyRaw() {
    try {
      await navigator.clipboard.writeText(new TextDecoder().decode(new Uint8Array(selection)));
    } catch (reason) {
      showErrorToast(reason);
    }
  }
</script>

<div class="hex-toolbar">
  <span>{selection.length} bytes selected</span>
  <button class="text-button" disabled={!selection.length} onclick={() => void copyHex()}>Copy hex</button>
  <button class="text-button" disabled={!selection.length} onclick={() => void copyRaw()}>Copy raw</button>
</div>
<div class="hex-view" aria-label="Hexadecimal byte viewer">
  {#each rows as row}
    <div class="hex-row">
      <span class="offset">{row.offset.toString(16).padStart(8, "0")}</span>
      <span class="values">
        {#each row.values as value, index}
          <button
            class:selected={selected(row.offset + index)}
            data-tooltip={`Byte ${row.offset + index}`}
            onclick={(event) => select(row.offset + index, event.shiftKey)}
          >{value.toString(16).padStart(2, "0")}</button>
        {/each}
      </span>
      <span class="ascii">{row.values.map((value) => (value >= 32 && value < 127 ? String.fromCharCode(value) : ".")).join("")}</span>
    </div>
  {/each}
</div>

<style>
  .hex-toolbar { display: flex; align-items: center; gap: 7px; min-height: 34px; padding: 4px 9px; color: var(--muted, #8d96a3); border-bottom: 1px solid var(--border, #282e36); background: var(--editor, #202020); font-size: var(--font-size-body); }
  .hex-toolbar span { flex: 1; }
  button { padding: 3px 7px; border: 1px solid #343b45; border-radius: 4px; color: #cbd1da; background: #171b21; cursor: pointer; }
  button:disabled { opacity: .4; }
  .hex-view { height: 100%; min-height: 160px; padding: 9px; overflow: auto; color: var(--text, #f4f6fb); background: var(--editor, #202020); font: var(--font-size-editor, 12px)/1.65 ui-monospace, SFMono-Regular, Menlo, monospace; }
  .hex-row { display: grid; grid-template-columns: 82px minmax(390px, 1fr) 140px; }
  .offset { color: var(--muted, #687181); }
  .values button { padding: 0 2px; border: 0; color: inherit; background: transparent; font: inherit; }
  .values button:nth-child(8) { margin-right: 9px; }
  .values button.selected { color: #161006; background: #f59e0b; }
  .ascii { color: var(--muted, #8ea2bd); white-space: pre; }
</style>
