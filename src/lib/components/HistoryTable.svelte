<script lang="ts">
  import type { HistoryEntry } from "$lib/types";
  import { formatTime } from "$lib/format";

  let {
    entries,
    selectedId = null,
    loading = false,
    onSelect,
    onSort,
    onNeedMore,
    onContext,
  }: {
    entries: HistoryEntry[];
    selectedId?: string | null;
    loading?: boolean;
    onSelect: (entry: HistoryEntry) => void;
    onSort: (column: string) => void;
    onNeedMore: () => void;
    onContext: (event: MouseEvent, entry: HistoryEntry) => void;
  } = $props();

  const rowHeight = $derived(entries.some((entry) => entry.matchSnippet) ? 44 : 32);
  const overscan = 8;
  let scrollTop = $state(0);
  let viewportHeight = $state(320);
  let viewport: HTMLDivElement;
  const start = $derived(Math.max(0, Math.floor(scrollTop / rowHeight) - overscan));
  const count = $derived(Math.ceil(viewportHeight / rowHeight) + overscan * 2);
  const visible = $derived(entries.slice(start, start + count));

  let scrollQueued = false;
  function scroll() {
    if (scrollQueued) return;
    scrollQueued = true;
    requestAnimationFrame(() => {
      scrollQueued = false;
      scrollTop = viewport.scrollTop;
      viewportHeight = viewport.clientHeight;
      if (scrollTop + viewportHeight > entries.length * rowHeight - rowHeight * 12) onNeedMore();
    });
  }

  function methodClass(method: unknown): string {
    return typeof method === "string" && method ? `method-${method.toLowerCase()}` : "method-unknown";
  }

  function handleRowKey(event: KeyboardEvent, entry: HistoryEntry) {
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      onSelect(entry);
    }
  }

  const columns = [
    ["#", "timestamp"], ["Method", "method"], ["Host", "host"], ["Path", "path"],
    ["Status", "status"], ["Length", "length"], ["MIME", "mimeType"],
    ["Time", "durationMs"], ["Timestamp", "timestamp"],
  ];
</script>

<div class="history-table" role="table" aria-label="HTTP history">
  <div class="table-header" role="row">
    {#each columns as column}
      <button role="columnheader" onclick={() => onSort(column[1])}>{column[0]} <span>↕</span></button>
    {/each}
  </div>
  <div class="viewport" bind:this={viewport} onscroll={scroll}>
    <div class="spacer" style={`height:${entries.length * rowHeight}px`}>
      <div class="visible" style={`transform:translateY(${start * rowHeight}px)`}>
        {#each visible as entry (entry.id)}
          <div
            class="table-row"
            class:selected={entry.id === selectedId}
            class:scoped={entry.scoped}
            class:error={entry.status >= 400}
            role="row"
            tabindex="0"
            aria-selected={entry.id === selectedId}
            onclick={() => onSelect(entry)}
            onkeydown={(event) => handleRowKey(event, entry)}
            oncontextmenu={(event) => { event.preventDefault(); onContext(event, entry); }}
          >
            <span role="cell">{entry.sequence}</span>
            <span role="cell" class={`method ${methodClass(entry.method)}`}>{entry.method}</span>
            <span role="cell" data-tooltip={entry.host}>{entry.host}</span>
            <span role="cell" class="path-cell" data-tooltip={entry.matchSnippet ?? entry.path}><strong>{entry.path}</strong>{#if entry.matchSnippet}<small>{entry.matchSnippet}</small>{/if}</span>
            <span role="cell" class="status">{entry.status}</span>
            <span role="cell">{entry.length.toLocaleString()}</span>
            <span role="cell" data-tooltip={entry.mimeType}>{entry.mimeType}</span>
            <span role="cell">{entry.durationMs} ms</span>
            <span role="cell">{formatTime(entry.timestamp)}</span>
          </div>
        {/each}
      </div>
    </div>
    {#if !entries.length && !loading}<div class="empty">No captured traffic matches these filters.</div>{/if}
    {#if loading}<div class="loading">Loading history…</div>{/if}
  </div>
</div>

<style>
  .history-table { display: grid; grid-template-rows: 32px minmax(0, 1fr); height: 100%; min-height: 180px; color: #b8c0cb; font-size: var(--font-size-body); }
  .table-header, .table-row { display: grid; grid-template-columns: 45px 72px minmax(130px, 1fr) minmax(190px, 1.5fr) 65px 75px 125px 72px 100px; align-items: center; }
  .table-header { border-bottom: 1px solid #303640; background: #171b21; }
  .table-header button { height: 100%; padding: 0 8px; border: 0; border-right: 1px solid #282e36; color: #8f98a5; background: transparent; text-align: left; font: inherit; cursor: pointer; }
  .table-header span { color: #525c69; }
  .viewport { position: relative; overflow: auto; background: #0c0f13; }
  .spacer { position: relative; min-width: 980px; }
  .visible { position: absolute; inset: 0 0 auto; }
  .table-row { width: 100%; height: 32px; padding: 0; border: 0; border-bottom: 1px solid #191e24; color: inherit; background: transparent; text-align: left; font: inherit; cursor: default; }
  .table-row:hover { background: #171c22; }
  .table-row:focus-visible { outline: 2px solid #f59e0b; outline-offset: -2px; }
  .table-row.selected { background: #282716; box-shadow: inset 2px 0 #f59e0b; }
  .table-row.error .status { color: #fca5a5; }
  .table-row > span { padding: 0 8px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .path-cell { display: grid; line-height: 1.2; }
  .path-cell strong, .path-cell small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .path-cell strong { font-weight: 400; }
  .path-cell small { color: #7f8996; font-size: var(--font-size-compact); }
  .method { font-weight: 700; }
  .method-get { color: #6ee7b7; } .method-post { color: #93c5fd; } .method-delete { color: #fca5a5; }
  .empty, .loading { position: sticky; left: 0; display: grid; place-items: center; height: 100%; color: #687281; }
</style>
