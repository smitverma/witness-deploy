<script lang="ts">
  import Toggle from "./Toggle.svelte";
  import type { HistoryFilter } from "$lib/types";

  let {
    filter,
    onChange,
    onClearHistory,
  }: {
    filter: HistoryFilter;
    onChange: () => void;
    onClearHistory: () => void;
  } = $props();
  let statusMode = $state("all");

  function setStatus(value: string) {
    statusMode = value;
    if (value === "custom") return;
    const ranges: Record<string, [number | null, number | null]> = {
      all: [null, null], "2xx": [200, 299], "3xx": [300, 399],
      "4xx": [400, 499], "5xx": [500, 599],
    };
    [filter.statusMin, filter.statusMax] = ranges[value] ?? [null, null];
    onChange();
  }

  function clear() {
    statusMode = "all";
    Object.assign(filter, {
      method: null, host: null, statusMin: null, statusMax: null,
      mimeType: null, search: null, inScopeOnly: false,
    });
    onChange();
  }
</script>

<div class="filter-bar" aria-label="History filters">
  <select aria-label="Filter by method" bind:value={filter.method} onchange={onChange}>
    <option value={null}>All methods</option>
    {#each ["GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS", "HEAD"] as method}
      <option value={method}>{method}</option>
    {/each}
  </select>
  <input aria-label="Filter by host" placeholder="Host contains…" bind:value={filter.host} oninput={onChange} />
  <div class="status-filter">
    <select aria-label="Filter by status" bind:value={statusMode} onchange={(event) => setStatus(event.currentTarget.value)}>
      <option value="all">All statuses</option>
      <option value="2xx">2xx Success</option>
      <option value="3xx">3xx Redirect</option>
      <option value="4xx">4xx Client error</option>
      <option value="5xx">5xx Server error</option>
      <option value="custom">Custom range…</option>
    </select>
    {#if statusMode === "custom"}
      <div class="custom-range">
        <input aria-label="Minimum status" type="number" min="100" max="599" placeholder="100" bind:value={filter.statusMin} oninput={onChange} />
        <span>–</span>
        <input aria-label="Maximum status" type="number" min="100" max="599" placeholder="599" bind:value={filter.statusMax} oninput={onChange} />
      </div>
    {/if}
  </div>
  <select aria-label="Filter by MIME type" bind:value={filter.mimeType} onchange={onChange}>
    <option value={null}>All content types</option>
    <option value="text/html">HTML</option>
    <option value="application/json">JSON</option>
    <option value="xml">XML</option>
    <option value="javascript">JavaScript</option>
    <option value="text/css">CSS</option>
    <option value="image/">Image</option>
  </select>
  <div class="search-wrap">
    <span aria-hidden="true">⌕</span>
    <input aria-label="Search URL and headers" placeholder="Search URL / headers…" bind:value={filter.search} oninput={onChange} />
  </div>
  <label class="scope-filter" data-tooltip="Show only history entries captured in scope">
    <span>Show in scope</span>
    <Toggle
      checked={filter.inScopeOnly}
      ariaLabel="Show only in-scope history entries"
      onchange={(event) => { filter.inScopeOnly = event.currentTarget.checked; onChange(); }}
    />
  </label>
  <button class="text-button" onclick={clear}>Clear filters</button>
  <button class="text-button danger" onclick={onClearHistory}>Clear history</button>
</div>

<style>
  .filter-bar { display: grid; grid-template-columns: 115px minmax(120px, .7fr) 125px 135px minmax(140px, .7fr) max-content auto auto; gap: 7px; padding: 8px; border-bottom: 1px solid #282e36; background: #11151a; }
  input, select, button { min-width: 0; height: 30px; padding: 0 8px; border: 1px solid #303741; border-radius: 4px; color: #cdd3dc; background: #0c0f13; font: var(--font-size-body) system-ui, sans-serif; }
  button { cursor: pointer; background: #191e25; }
  button.danger { color: #fca5a5; }
  .search-wrap { position: relative; }
  .search-wrap span { position: absolute; left: 8px; top: 5px; color: #77818f; font-size: var(--font-size-heading); }
  .search-wrap input { width: 100%; padding-left: 27px; }
  .scope-filter { display: inline-flex; align-items: center; justify-content: space-between; gap: 8px; min-width: 118px; height: 30px; padding: 0 8px; border: 0; border-radius: 4px; color: #cdd3dc; background: transparent; font: var(--font-size-body) system-ui, sans-serif; white-space: nowrap; cursor: pointer; }
  .scope-filter :global(.toggle-control) { flex: 0 0 30px; }
  .scope-filter:focus-within { outline: 2px solid var(--accent, #f59e0b); outline-offset: 1px; }
  .status-filter { position: relative; }
  .status-filter > select { width: 100%; }
  .custom-range { position: absolute; z-index: 8; top: 34px; left: 0; display: grid; grid-template-columns: 72px auto 72px; align-items: center; gap: 4px; padding: 7px; border: 1px solid #3a424e; border-radius: 5px; background: #171b21; box-shadow: 0 8px 20px #0008; }
  .custom-range input { width: 72px; }
</style>
