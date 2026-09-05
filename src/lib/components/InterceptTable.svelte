<script lang="ts">
  import type { InterceptEntry } from "$lib/types";
  import { formatTime } from "$lib/format";

  let {
    entries,
    selectedId = null,
    onSelect,
  }: {
    entries: InterceptEntry[];
    selectedId?: string | null;
    onSelect: (entry: InterceptEntry) => void;
  } = $props();

  function methodClass(method: unknown): string {
    return typeof method === "string" && method ? `method-${method.toLowerCase()}` : "method-unknown";
  }

  function handleRowKey(event: KeyboardEvent, entry: InterceptEntry) {
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      onSelect(entry);
    }
  }
</script>

<div class="intercept-table" role="table" aria-label="Intercepted HTTP messages">
  <div class="table-header" role="row">
    <span role="columnheader">Time</span>
    <span role="columnheader">Type</span>
    <span role="columnheader">Direction</span>
    <span role="columnheader">Method</span>
    <span role="columnheader">URL</span>
    <span role="columnheader">Status code</span>
    <span role="columnheader">Length</span>
  </div>
  <div class="table-body" role="rowgroup">
    {#each entries as entry (entry.id)}
      <div
        class="table-row"
        class:selected={entry.id === selectedId}
        role="row"
        tabindex="0"
        aria-selected={entry.id === selectedId}
        onclick={() => onSelect(entry)}
        onkeydown={(event) => handleRowKey(event, entry)}
      >
        <span role="cell">{formatTime(entry.receivedAt)}</span>
        <span role="cell">HTTP</span>
        <span role="cell" class="direction"><b aria-hidden="true">{entry.kind === "request" ? "→" : "←"}</b> {entry.kind === "request" ? "Request" : "Response"}</span>
        <span role="cell" class={`method ${methodClass(entry.method)}`}>{entry.method}</span>
        <span role="cell" class="url" data-tooltip={entry.url}>{entry.url}</span>
        <span role="cell" class:error={entry.status !== null && entry.status >= 400}>{entry.status ?? "—"}</span>
        <span role="cell">{entry.length.toLocaleString()}</span>
      </div>
    {/each}
    {#if !entries.length}
      <div class="empty">No messages are waiting for interception.</div>
    {/if}
  </div>
</div>

<style>
  .intercept-table {
    display: grid;
    grid-template-rows: 29px minmax(0, 1fr);
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    color: var(--muted);
    background: var(--surface);
    font-size: var(--font-size-compact);
  }
  .table-header, .table-row {
    display: grid;
    grid-template-columns: 112px 68px 100px 80px minmax(280px, 1fr) 88px 80px;
    align-items: center;
    min-width: 900px;
  }
  .table-header {
    color: var(--muted);
    border-bottom: 1px solid var(--border-strong);
    background: var(--surface-2);
    font-weight: 650;
  }
  .table-header span, .table-row span {
    min-width: 0;
    padding: 0 8px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .table-header span {
    height: 100%;
    border-right: 1px solid var(--border);
    line-height: 29px;
  }
  .table-body {
    position: relative;
    min-height: 0;
    overflow: auto;
    background: var(--bg);
  }
  .table-row {
    width: 100%;
    height: 27px;
    padding: 0;
    color: var(--text);
    border: 0;
    border-bottom: 1px solid var(--border);
    background: transparent;
    text-align: left;
    font: inherit;
    cursor: default;
  }
  .table-row:hover { background: var(--surface-2); }
  .table-row:focus-visible { outline: 2px solid var(--accent); outline-offset: -2px; }
  .table-row.selected {
    color: var(--text);
    background: var(--accent-soft);
    box-shadow: inset 2px 0 var(--accent);
  }
  .direction { color: var(--muted); }
  .direction b { margin-right: 5px; color: var(--accent); font-size: var(--font-size-body); }
  .method { font-weight: 750; }
  .method-get { color: #23b889; }
  .method-post { color: #5794ef; }
  .method-delete { color: var(--danger); }
  .error { color: var(--danger); }
  .empty {
    display: grid;
    place-items: center;
    min-width: 900px;
    height: 100%;
    min-height: 70px;
    color: var(--muted);
  }
</style>
