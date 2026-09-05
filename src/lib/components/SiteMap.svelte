<script lang="ts">
  import { onMount } from "svelte";
  import { commands, isTauri } from "$lib/api";
  import { showErrorToast } from "$lib/errorToast";
  import type { HistoryEntry, ScopeEntry, SiteMapWorkspaceState } from "$lib/types";
  import { buildSiteMap, flattenSiteMap, isHostInScope, type SiteMapRow } from "$lib/siteMap";
  import Toggle from "./Toggle.svelte";

  let { entries, onSelect, onSendReplay, onDelete, workspace, onWorkspaceChange = (_state: SiteMapWorkspaceState) => {} }: {
    entries: HistoryEntry[];
    onSelect: (entry: HistoryEntry) => void;
    onSendReplay: (entry: HistoryEntry) => void;
    onDelete: (entry: HistoryEntry) => void;
    workspace?: SiteMapWorkspaceState;
    onWorkspaceChange?: (state: SiteMapWorkspaceState) => void;
  } = $props();
  let search = $state("");
  let inScopeOnly = $state(false);
  let scopeEntries = $state<ScopeEntry[]>([]);
  let scopeLoaded = $state(false);
  let collapsed = $state<Set<string>>(new Set());
  let selectedEntryId = $state<string | null>(null);
  let selectedRowKey = $state<string | null>(null);
  let menu = $state<{ x: number; y: number; label: string; url: string; endpoints: HistoryEntry[] } | null>(null);
  let workspaceInitialized = $state(false);

  $effect(() => {
    if (workspaceInitialized) return;
    if (workspace) {
      search = workspace.search;
      inScopeOnly = workspace.inScopeOnly;
      collapsed = new Set(workspace.collapsed);
      selectedEntryId = workspace.selectedEntryId;
      selectedRowKey = workspace.selectedRowKey;
    }
    workspaceInitialized = true;
  });

  $effect(() => {
    if (!workspaceInitialized) return;
    onWorkspaceChange({ search, inScopeOnly, collapsed: [...collapsed], selectedEntryId, selectedRowKey });
  });

  onMount(() => {
    if (!isTauri()) {
      scopeLoaded = true;
      return;
    }
    void commands.getScope()
      .then((snapshot) => {
        scopeEntries = snapshot.entries;
        scopeLoaded = true;
      })
      .catch(() => {
        scopeLoaded = true;
      });
  });

  const currentEntries = $derived.by(() => scopeLoaded
    ? entries.map((entry) => ({ ...entry, scoped: isHostInScope(entry.host, scopeEntries) }))
    : entries);
  const hosts = $derived.by(() => buildSiteMap(currentEntries, search, inScopeOnly));
  const rows = $derived.by(() => flattenSiteMap(hosts, collapsed, Boolean(search.trim())));

  function toggle(key: string) {
    const next = new Set(collapsed);
    if (next.has(key)) next.delete(key); else next.add(key);
    collapsed = next;
  }

  function selectRow(row: SiteMapRow) {
    selectedRowKey = row.key;
    selectedEntryId = row.kind === "endpoint" ? row.entry.id : null;
  }

  export function handleShortcut(action: string): boolean {
    if (action === "transient.close") {
      if (!menu) return false;
      menu = null;
      return true;
    }
    if (action === "siteMap.expandAll") {
      expandAll();
      return true;
    }
    if (action === "siteMap.collapseAll") {
      collapseAll();
      return true;
    }
    if (action === "siteMap.selectPrevious" || action === "siteMap.selectNext") {
      if (!rows.length) return false;
      const currentIndex = rows.findIndex((row) => row.key === selectedRowKey);
      const offset = action.endsWith("Previous") ? -1 : 1;
      const nextIndex = currentIndex < 0
        ? 0
        : Math.max(0, Math.min(rows.length - 1, currentIndex + offset));
      selectRow(rows[nextIndex]);
      return true;
    }
    if (action === "siteMap.openSelected") {
      const row = rows.find((candidate) => candidate.key === selectedRowKey);
      if (!row || row.kind !== "endpoint") return false;
      onSelect(row.entry);
      return true;
    }
    return false;
  }

  function openMenu(event: MouseEvent, label: string, url: string, endpoints: HistoryEntry[]) {
    event.preventDefault();
    menu = { x: event.clientX, y: event.clientY, label, url, endpoints };
  }

  function expandAll() {
    collapsed = new Set();
    menu = null;
  }

  function collapseAll() {
    const keys = new Set<string>();
    function collect(branches: typeof hosts) {
      for (const branch of branches) {
        keys.add(branch.key);
        collect(branch.children);
      }
    }
    collect(hosts);
    collapsed = keys;
    menu = null;
  }

  function isExpanded(row: Extract<SiteMapRow, { kind: "branch" }>) {
    return Boolean(search.trim()) || !collapsed.has(row.key);
  }
</script>

<svelte:window onclick={() => (menu = null)} />

<section class="site-map">
  <header>
    <div><p>SITE MAP</p><h1>Discovered endpoints</h1></div>
    <div class="header-actions">
      <button class="icon-button" type="button" data-tooltip="Expand all" aria-label="Expand all" onclick={expandAll}>
        <svg viewBox="0 0 24 24" aria-hidden="true" focusable="false">
          <path d="M14 10L21 3M21 3H16.5M21 3V7.5M10 14L3 21M3 21H7.5M3 21L3 16.5" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" />
        </svg>
      </button>
      <button class="icon-button" type="button" data-tooltip="Collapse all" aria-label="Collapse all" onclick={collapseAll}>
        <svg viewBox="0 0 24 24" aria-hidden="true" focusable="false">
          <path
            d="M14 10L21 3M14 10V6.5M14 10H17.5M10 14L3 21M10 14V17.5M10 14H6.5"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
          />
        </svg>
      </button>
      <label class="scope-toggle" data-tooltip="Show only in-scope URLs">
        <Toggle bind:checked={inScopeOnly} ariaLabel="In Scope Only" />
        <span>In Scope Only</span>
      </label>
      <input bind:value={search} placeholder="Filter tree…" aria-label="Search site map" />
    </div>
  </header>
  <div class="tree" role="tree" aria-label="Discovered endpoints">
    {#each rows as row (row.key)}
      {#if row.kind === "branch"}
        <button
          class:host-node={row.node.kind === "host"}
          class:selected={selectedRowKey === row.key}
          class="tree-row branch-node"
          role="treeitem"
          aria-level={row.depth + 1}
          aria-selected={selectedRowKey === row.key}
          aria-expanded={isExpanded(row)}
          style={'--depth:' + row.depth}
          onclick={() => { selectRow(row); toggle(row.key); }}
          oncontextmenu={(event) => openMenu(event, row.label, row.node.url, row.node.entries)}
        >
          <span class="chevron" aria-hidden="true">{isExpanded(row) ? "▾" : "▸"}</span>
          <span class="node-label">{row.label}</span>
          <em>{row.node.entries.length}</em>
        </button>
      {:else}
        <div
          class:selected={selectedRowKey === row.key}
          class="tree-row endpoint"
          role="treeitem"
          aria-level={row.depth + 1}
          aria-selected={selectedRowKey === row.key}
          tabindex="-1"
          style={'--depth:' + row.depth}
          oncontextmenu={(event) => openMenu(event, row.label, row.entry.url, [row.entry])}
          onclick={() => selectRow(row)}
          onkeydown={(event) => {
            if (event.key === "Enter" || event.key === " ") {
              event.preventDefault();
              selectRow(row);
            }
          }}
        >
          <span class={'method method-' + row.entry.method.toLowerCase()}>{row.entry.method}</span>
          <button class="path" onclick={() => onSelect(row.entry)}>{row.label}</button>
          <button class="send" data-tooltip="Send to Replay" aria-label={'Send ' + row.label + ' to Replay'} onclick={() => onSendReplay(row.entry)}>↗</button>
          <span>{row.entry.status}</span>
        </div>
      {/if}
    {:else}
      <div class="empty">No captured endpoints match this filter.</div>
    {/each}
  </div>
</section>

{#if menu}
  <div class="node-menu" style={'left:' + menu.x + 'px;top:' + menu.y + 'px'} role="menu" tabindex="-1">
    <strong>{menu.label}</strong>
    <button class="text-button" onclick={() => { if (menu?.endpoints[0]) onSendReplay(menu.endpoints[0]); menu = null; }}>Send to Replay</button>
    <button class="text-button" onclick={() => { if (menu) void navigator.clipboard.writeText(menu.url).catch(showErrorToast); menu = null; }}>Copy URL</button>
    <button class="text-button" onclick={expandAll}>Expand all</button>
    <button class="text-button danger delete" onclick={() => { for (const entry of menu?.endpoints ?? []) onDelete(entry); menu = null; }}>Delete from History</button>
  </div>
{/if}

<style>
  .site-map { display: grid; grid-template-rows: 72px minmax(0, 1fr); width: min(960px, 100%); height: 100%; margin: auto; padding: 0 22px 18px; color: #dce1e8; }
  header { display: flex; align-items: center; justify-content: space-between; }
  header p { margin: 0 0 4px; color: #f59e0b; font-size: var(--font-size-compact); font-weight: 800; letter-spacing: .14em; }
  h1 { margin: 0; font-size: var(--font-size-title); }
  .header-actions { display: flex; align-items: center; justify-content: flex-end; gap: 7px; }
  header input { width: 260px; height: 31px; padding: 0 9px; border: 1px solid #343c47; border-radius: 4px; color: #cdd4dd; background: #101419; }
  .icon-button { position: relative; display: grid; place-items: center; width: 31px; height: 31px; padding: 0; border: 1px solid #343c47; border-radius: 4px; color: #adb6c1; background: #171c22; }
  .icon-button:hover { color: #e6ebf2; background: #222932; }
  .icon-button:focus-visible { outline: 2px solid #f59e0b; outline-offset: 2px; }
  .icon-button svg { width: 16px; height: 16px; fill: none; stroke: currentColor; stroke-linecap: round; stroke-linejoin: round; stroke-width: var(--svgbuttonstrokewidth, 1.5); }
  .scope-toggle { display: inline-flex; position: relative; align-items: center; gap: 6px; height: 31px; color: #aeb7c2; font-size: var(--font-size-compact); white-space: nowrap; cursor: pointer; }
  .scope-toggle :global(.toggle-control input:focus-visible),
  .scope-toggle :global(.toggle-control input:focus-visible + .toggle-track) {
    outline: none !important;
    outline-offset: 0;
  }
  .tree { overflow: auto; border: 1px solid #29303a; border-radius: 6px; background: #0c0f13; }
  button { color: inherit; background: transparent; border: 0; cursor: pointer; }
  .tree-row { position: relative; width: 100%; border-bottom: 1px solid #20262d; }
  .branch-node { display: grid; grid-template-columns: 20px minmax(0, 1fr) auto; align-items: center; height: 38px; padding: 0 12px 0 calc(12px + var(--depth) * 20px); text-align: left; }
  .branch-node:not(.host-node)::before, .endpoint::before { position: absolute; top: 0; bottom: 0; left: calc(12px + (var(--depth) - 1) * 20px); border-left: 1px solid #303742; content: ""; }
  .branch-node:hover, .endpoint:hover { background: #171c22; }
  .tree-row.selected { background: #282716; box-shadow: inset 2px 0 #f59e0b; }
  .chevron { color: #8c98a7; font-size: var(--font-size-compact); }
  .node-label { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .host-node .node-label { color: #e0e5ec; font-weight: 700; }
  .branch-node:not(.host-node) .node-label { color: #aeb7c2; }
  .branch-node em { color: #77818f; font-style: normal; font-size: var(--font-size-compact); }
  .endpoint { display: grid; grid-template-columns: 56px minmax(0, 1fr) 35px 45px; align-items: center; height: 32px; padding: 0 10px 0 calc(31px + var(--depth) * 20px); color: #909aa7; }
  .endpoint .method { width: 45px; padding: 2px; border-radius: 3px; color: #cbd2dc; background: #30343a; font-size: var(--font-size-compact); font-weight: 800; text-align: center; }
  .endpoint .method-get { color: #6ee7b7; background: #15382c; }
  .endpoint .method-post { color: #93c5fd; background: #172c46; }
  .endpoint .method-delete { color: #fca5a5; background: #3b1d23; }
  .path { min-width: 0; overflow: hidden; color: #c8d0da; text-align: left; text-overflow: ellipsis; white-space: nowrap; }
  .send { color: #fbbf24; }
  .empty { display: grid; place-items: center; height: 200px; color: #727d8a; }
  .node-menu { position: fixed; z-index: 45; display: grid; min-width: 180px; padding: 5px; border: 1px solid #39414c; border-radius: 6px; color: #cbd2dc; background: #171b21; box-shadow: 0 14px 36px #000b; }
  .node-menu strong { padding: 6px 8px; overflow: hidden; color: #8d97a4; text-overflow: ellipsis; white-space: nowrap; font-size: var(--font-size-compact); }
  .node-menu button { padding: 7px 8px; border-radius: 3px; text-align: left; }
  .node-menu button:hover { background: #282e36; }
  .node-menu button.delete { color: #fca5a5; }
</style>
