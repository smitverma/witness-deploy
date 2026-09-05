<script lang="ts">
  import { commands } from "$lib/api";
  import ConfirmDialog from "./ConfirmDialog.svelte";
  import type { ScopeEntry, ScopeSnapshot } from "$lib/types";
  import Toggle from "./Toggle.svelte";
  import RecycleBinIcon from "./RecycleBinIcon.svelte";

  let {
    projectOpen,
    onError,
  }: {
    projectOpen: boolean;
    onError: (reason: unknown) => void;
  } = $props();
  let scope = $state<ScopeSnapshot>({ entries: [] });
  let pattern = $state("");
  let regex = $state(false);
  let includeSubdomains = $state(false);
  let editingId = $state<number | null>(null);
  let editingIsInScope = $state(true);
  let editPattern = $state("");
  let editRegex = $state(false);
  let editIncludeSubdomains = $state(false);
  let loadedForProject = false;
  let selectedId = $state<number | null>(null);
  let deleteScopeId = $state<number | null>(null);

  $effect(() => {
    if (projectOpen && !loadedForProject) {
      loadedForProject = true;
      void commands
        .getScope()
        .then((value) => {
          scope = value;
          selectedId = value.entries[0]?.id ?? null;
        })
        .catch(onError);
    }
    if (!projectOpen) {
      loadedForProject = false;
      selectedId = null;
    }
  });

  async function add(isInScope: boolean) {
    if (!pattern.trim()) return;
    try {
      const entry = await commands.addScopeEntry(
        pattern.trim(),
        regex,
        includeSubdomains,
        isInScope,
      );
      if (!scope.entries.some((item) => item.id === entry.id))
        scope.entries = [...scope.entries, entry];
      pattern = "";
    } catch (reason) {
      onError(reason);
    }
  }

  async function saveEdit() {
    if (editingId === null || !editPattern.trim()) return;
    try {
      const entry = await commands.updateScopeEntry(
        editingId,
        editPattern.trim(),
        editRegex,
        editIncludeSubdomains,
        editingIsInScope,
      );
      scope = { entries: scope.entries.map((item) => item.id === entry.id ? entry : item) };
      cancelEdit();
    } catch (reason) {
      onError(reason);
    }
  }

  function startEdit(entry: ScopeEntry) {
    selectedId = entry.id;
    editingId = entry.id;
    editingIsInScope = entry.isInScope;
    editPattern = entry.pattern;
    editRegex = entry.isRegex;
    editIncludeSubdomains = entry.includeSubdomains;
  }

  function cancelEdit() {
    editingId = null;
    editingIsInScope = true;
    editPattern = "";
    editRegex = false;
    editIncludeSubdomains = false;
  }

  function remove(id: number) {
    const entry = scope.entries.find((item) => item.id === id);
    if (!entry) return;
    deleteScopeId = id;
  }

  async function confirmRemove() {
    const id = deleteScopeId;
    deleteScopeId = null;
    if (id === null) return;
    try {
      if (await commands.removeScopeEntry(id)) {
        scope.entries = scope.entries.filter((entry) => entry.id !== id);
        if (selectedId === id) selectedId = scope.entries[0]?.id ?? null;
        if (editingId === id) cancelEdit();
      }
    } catch (reason) {
      onError(reason);
    }
  }

  export function handleShortcut(action: string): boolean {
    if (!projectOpen) return false;
    if (action === "transient.close") {
      if (editingId === null) return false;
      cancelEdit();
      return true;
    }
    if (action === "scope.focusFilter" || action === "scope.create") {
      document.querySelector<HTMLInputElement>('.scope-tool input[aria-label="Scope domain or pattern"]')?.focus();
      return true;
    }
    if (action === "scope.submit") {
      if (editingId !== null) void saveEdit();
      else if (pattern.trim()) void add(true);
      else return false;
      return true;
    }
    if (action === "scope.edit") {
      const target = scope.entries.find((entry) => entry.id === selectedId) ?? scope.entries[0];
      if (!target) return false;
      startEdit(target);
      return true;
    }
    if (action === "scope.delete") {
      const target = scope.entries.find((entry) => entry.id === selectedId) ?? scope.entries[0];
      if (!target) return false;
      void remove(target.id);
      return true;
    }
    if (action === "scope.selectPrevious" || action === "scope.selectNext") {
      if (!scope.entries.length) return false;
      const currentIndex = scope.entries.findIndex((entry) => entry.id === selectedId);
      const offset = action.endsWith("Previous") ? -1 : 1;
      const next = currentIndex < 0
        ? 0
        : Math.max(0, Math.min(scope.entries.length - 1, currentIndex + offset));
      selectedId = scope.entries[next].id;
      return true;
    }
    return false;
  }

  function exportList() {
    const link = document.createElement("a");
    const content = scope.entries
      .map((entry) =>
        [
          entry.isInScope ? "in" : "out",
          entry.isRegex ? "regex" : "domain",
          entry.includeSubdomains ? "subdomains" : "exact",
          entry.pattern,
        ].join("\t"),
      )
      .join("\n");
    const url = URL.createObjectURL(
      new Blob([content], { type: "text/plain" }),
    );
    link.href = url;
    link.download = "witness-scope.txt";
    link.click();
    window.setTimeout(() => URL.revokeObjectURL(url), 4000);
  }

  async function importList(event: Event) {
    const file = (event.currentTarget as HTMLInputElement).files?.[0];
    if (!file) return;
    try {
      for (const line of (await file.text()).split(/\r?\n/)) {
        if (!line.trim()) continue;
        const [kind, type, subdomains, ...value] = line.split("\t");
        const isExportedRule =
          (kind === "in" || kind === "out") &&
          (type === "domain" || type === "regex") &&
          (subdomains === "subdomains" || subdomains === "exact");
        const entry = isExportedRule
          ? await commands.addScopeEntry(
              value.join("\t").trim(),
              type === "regex",
              subdomains === "subdomains",
              kind === "in",
            )
          : await commands.addScopeEntry(line.trim(), false, true, true);
        if (
          entry.pattern &&
          !scope.entries.some((item) => item.id === entry.id)
        )
          scope.entries = [...scope.entries, entry];
      }
    } catch (reason) {
      onError(reason);
    }
  }
</script>

<section class="scope-tool">
  {#if !projectOpen}
    <div class="empty">Open a project to persist scope rules.</div>
  {:else}
    <form
      class="scope-add"
      onsubmit={(event) => {
        event.preventDefault();
        void add(true);
      }}
    >
      <input
        aria-label="Scope domain or pattern"
        bind:value={pattern}
        placeholder={regex ? "^api\\d+\\.example\\.com$" : "example.com"}
      />
      <label class="scope-toggle">
        <Toggle bind:checked={regex} ariaLabel="Regular expression" />
        <span>Regular expression</span>
      </label>
      <label class="scope-toggle">
        <Toggle bind:checked={includeSubdomains} disabled={regex} ariaLabel="Include subdomains" />
        <span>Include subdomains</span>
      </label>
      <button class="text-button in-action" type="submit">+ In Scope</button>
      <button class="text-button danger out-action" type="button" onclick={() => void add(false)}>+ Out of Scope</button>
    </form>
    <div class="scope-list">
      {#if scope.entries.length}
        <div class="rule-columns">
          <section class="rule-group">
            <h2>In scope</h2>
            {#each scope.entries.filter((entry) => entry.isInScope) as entry (entry.id)}<div
                class:selected={selectedId === entry.id}
                class="rule"
              >
                {#if editingId === entry.id}
                  <span class="type">{entry.isRegex ? "REGEX" : "DOMAIN"}</span>
                  <form class="inline-editor" aria-label={`Edit ${entry.pattern}`} onsubmit={(event) => { event.preventDefault(); void saveEdit(); }}>
                    <div class="pattern-field">
                      <input bind:value={editPattern} aria-label={`Scope pattern for ${entry.pattern}`} />
                    </div>
                    <label class="scope-toggle inline-option"><Toggle bind:checked={editRegex} ariaLabel="Regex" /><span>Regex</span></label>
                    <label class="scope-toggle inline-option"><Toggle bind:checked={editIncludeSubdomains} disabled={editRegex} ariaLabel="Subdomains" /><span>Subdomains</span></label>
                    <select class="inline-kind" aria-label="Scope rule type" value={editingIsInScope ? "in" : "out"} onchange={(event) => (editingIsInScope = event.currentTarget.value === "in")}><option value="in">In scope</option><option value="out">Out of scope</option></select>
                    <button class="inline-icon save-inline" type="submit" data-tooltip="Save" aria-label="Save changes">
                      <svg viewBox="0 0 20 20" aria-hidden="true" focusable="false"><path d="m4 10 4 4 8-8" /></svg>
                    </button>
                    <button class="inline-icon cancel-inline" type="button" data-tooltip="Cancel" aria-label="Cancel edit" onclick={cancelEdit}>
                      <svg viewBox="0 0 20 20" aria-hidden="true" focusable="false"><path d="m5 5 10 10M15 5 5 15" /></svg>
                    </button>
                  </form>
                {:else}
                  <span class="type">{entry.isRegex ? "REGEX" : "DOMAIN"}</span
                  ><code>{entry.pattern}</code
                  ><span class="subdomains">{!entry.isRegex && entry.includeSubdomains ? "SUBS" : ""}</span
                  ><div class="rule-actions">
                    <button class="icon-button edit-action" type="button" data-tooltip="Edit" aria-label={`Edit ${entry.pattern}`} onclick={() => startEdit(entry)}>
                      <svg viewBox="0 0 20 20" aria-hidden="true" focusable="false"><path d="m13.8 3.2 3 3M4 16l.6-3.2L13.8 3.6a2.1 2.1 0 0 1 3 3L7.6 15.8 4 16Z" /></svg>
                    </button>
                    <button class="icon-button delete-action" type="button" data-tooltip="Delete" aria-label={`Delete ${entry.pattern}`} onclick={() => { selectedId = entry.id; void remove(entry.id); }}>
                      <RecycleBinIcon />
                    </button>
                  </div>
                {/if}
              </div>{:else}<p>No in-scope rules.</p>{/each}
          </section>
          <section class="rule-group">
            <h2>Out of scope</h2>
            {#each scope.entries.filter((entry) => !entry.isInScope) as entry (entry.id)}<div
                class:selected={selectedId === entry.id}
                class="rule"
              >
                {#if editingId === entry.id}
                  <span class="type out">{entry.isRegex ? "REGEX" : "DOMAIN"}</span>
                  <form class="inline-editor" aria-label={`Edit ${entry.pattern}`} onsubmit={(event) => { event.preventDefault(); void saveEdit(); }}>
                    <div class="pattern-field">
                      <input bind:value={editPattern} aria-label={`Scope pattern for ${entry.pattern}`} />
                    </div>
                    <label class="scope-toggle inline-option"><Toggle bind:checked={editRegex} ariaLabel="Regex" /><span>Regex</span></label>
                    <label class="scope-toggle inline-option"><Toggle bind:checked={editIncludeSubdomains} disabled={editRegex} ariaLabel="Subdomains" /><span>Subdomains</span></label>
                    <select class="inline-kind" aria-label="Scope rule type" value={editingIsInScope ? "in" : "out"} onchange={(event) => (editingIsInScope = event.currentTarget.value === "in")}><option value="in">In scope</option><option value="out">Out of scope</option></select>
                    <button class="inline-icon save-inline" type="submit" data-tooltip="Save" aria-label="Save changes">
                      <svg viewBox="0 0 20 20" aria-hidden="true" focusable="false"><path d="m4 10 4 4 8-8" /></svg>
                    </button>
                    <button class="inline-icon cancel-inline" type="button" data-tooltip="Cancel" aria-label="Cancel edit" onclick={cancelEdit}>
                      <svg viewBox="0 0 20 20" aria-hidden="true" focusable="false"><path d="m5 5 10 10M15 5 5 15" /></svg>
                    </button>
                  </form>
                {:else}
                  <span class="type out"
                    >{entry.isRegex ? "REGEX" : "DOMAIN"}</span
                  ><code>{entry.pattern}</code
                  ><span class="subdomains">{!entry.isRegex && entry.includeSubdomains ? "SUBS" : ""}</span
                  ><div class="rule-actions">
                    <button class="icon-button edit-action" type="button" data-tooltip="Edit" aria-label={`Edit ${entry.pattern}`} onclick={() => startEdit(entry)}>
                      <svg viewBox="0 0 20 20" aria-hidden="true" focusable="false"><path d="m13.8 3.2 3 3M4 16l.6-3.2L13.8 3.6a2.1 2.1 0 0 1 3 3L7.6 15.8 4 16Z" /></svg>
                    </button>
                    <button class="icon-button delete-action" type="button" data-tooltip="Delete" aria-label={`Delete ${entry.pattern}`} onclick={() => { selectedId = entry.id; void remove(entry.id); }}>
                      <RecycleBinIcon />
                    </button>
                  </div>
                {/if}
              </div>{:else}<p>No out-of-scope rules.</p>{/each}
          </section>
        </div>
      {:else}<div class="empty">
          Allow all is active. Add in-scope rules to create an allow-list, or add out-of-scope rules to exclude hosts.
        </div>{/if}
    </div>
    <footer>
      <label class="import"
        >Import list<input
          type="file"
          accept="text/plain"
          onchange={importList}
        /></label
      ><button class="text-button" onclick={exportList} disabled={!scope.entries.length}
        >Export list</button
      ><span>{scope.entries.length} rules</span>
    </footer>
  {/if}
  <ConfirmDialog
    open={deleteScopeId !== null}
    title="Delete scope entry?"
    message={`Delete scope entry “${scope.entries.find((entry) => entry.id === deleteScopeId)?.pattern ?? ""}”?`}
    confirmLabel="Delete entry"
    onConfirm={() => void confirmRemove()}
    onCancel={() => (deleteScopeId = null)}
  />
</section>

<style>
  .scope-tool {
    display: grid;
    grid-template-rows: 52px minmax(0, 1fr) 36px;
    gap: 8px;
    height: 100%;
    max-width: 980px;
    margin: auto;
    padding: 0 12px 12px;
    overflow: hidden;
    color: #dce1e8;
  }
  .scope-add {
    display: grid;
    grid-template-columns: minmax(220px, 1fr) auto auto auto auto;
    align-items: center;
    gap: 8px;
    padding: 8px;
    border: 1px solid var(--border, #29303a);
    border-radius: 5px;
    background: var(--surface, #12161b);
  }
  .scope-add > input {
    align-self: center;
    margin: 0;
  }
  .scope-add > input:hover,
  .scope-add > input:focus,
  .scope-add > input:focus-visible {
    border-color: #343c47 !important;
    box-shadow: none !important;
    outline: none !important;
  }
  input,
  select,
  button,
  .import {
    height: 32px;
    padding: 0 10px;
    border: 1px solid #343c47;
    border-radius: 4px;
    color: #ccd3dc;
    background: #171c22;
  }
  .scope-add > label {
    display: flex;
    align-items: center;
    color: #909aa7;
    font-size: var(--font-size-body);
    white-space: nowrap;
  }
  .scope-toggle {
    position: relative;
    display: inline-flex !important;
    align-items: center;
    gap: 6px;
    color: #cbd3dc !important;
    cursor: pointer;
    white-space: nowrap;
  }
  button,
  .import {
    display: inline-flex;
    align-items: center;
    cursor: pointer;
  }
  .scope-add button {
    color: #171106;
    background: #f59e0b;
    border-color: #ca7908;
    font-weight: 700;
  }
  .scope-add .in-action,
  .scope-add .out-action {
    box-sizing: border-box;
    width: 120px;
    min-width: 120px;
    max-width: 120px;
    height: 28px !important;
    min-height: 28px !important;
    max-height: 28px;
    padding: 0 8px;
    align-self: center;
    justify-content: center;
    line-height: 1;
    white-space: nowrap;
    appearance: none;
    -webkit-appearance: none;
  }
  .scope-add .in-action {
    color: #ffffff !important;
    background: var(--success) !important;
    border: none !important;
  }
  .scope-add .out-action {
    color: #ffffff !important;
    background: #8f3038 !important;
    border: none !important;
  }
  .scope-list {
    min-height: 0;
    overflow: auto;
  }
  .rule-columns {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 8px;
    min-height: 100%;
  }
  .rule-group {
    min-width: 0;
    overflow: hidden;
    border: 1px solid var(--border, #29303a);
    border-radius: 5px;
    background: var(--surface, #12161b);
  }
  .rule-group h2 {
    margin: 0;
    padding: 8px 10px;
    color: #a8b0bb;
    border-bottom: 1px solid var(--border, #29303a);
    background: var(--surface-2, #171c22);
    font-size: var(--font-size-compact);
    text-transform: uppercase;
    letter-spacing: 0.08em;
  }
  .rule-group p {
    margin: 0;
    padding: 12px 10px;
    color: #747f8d;
    font-size: var(--font-size-body);
  }
  .rule {
    display: grid;
    grid-template-columns: max-content minmax(0, 1fr) 160px auto;
    gap: 7px;
    align-items: center;
    min-height: 38px;
    padding: 0 10px;
    border-bottom: 1px solid
      color-mix(in srgb, var(--border, #29303a) 70%, transparent);
  }
  .rule.selected {
    background: color-mix(in srgb, var(--accent, #f59e0b) 10%, transparent);
  }
  .rule:last-child {
    border-bottom: 0;
  }
  .inline-editor {
    display: grid;
    grid-column: 2 / -1;
    grid-template-columns: minmax(150px, 1fr) auto auto auto auto auto;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .pattern-field {
    min-width: 0;
  }
  .pattern-field input {
    width: 100%;
    height: 28px;
    padding: 0 8px;
  }
  .inline-option {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: #909aa7;
    font-size: var(--font-size-compact);
    white-space: nowrap;
  }
  .inline-kind {
    width: 92px;
    height: 28px;
    padding: 0 5px;
    font-size: var(--font-size-compact);
  }
  .inline-icon,
  .rule .icon-button {
    display: grid;
    place-items: center;
    width: 27px;
    min-width: 27px;
    height: 27px;
    min-height: 27px;
    padding: 0 !important;
    border: 0 !important;
    border-radius: 0 !important;
    color: #ffffff !important;
    background: transparent !important;
    box-shadow: none !important;
    transition: none !important;
  }
  .inline-icon:hover,
  .rule .icon-button:hover {
    color: #ffffff !important;
    border: 0 !important;
    background: transparent !important;
    box-shadow: none !important;
  }
  .inline-icon svg {
    width: 15px;
    height: 15px;
    fill: none;
    stroke: currentColor;
    stroke-linecap: round;
    stroke-linejoin: round;
    stroke-width: 1.6;
  }
  code {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .type {
    color: #fbbf24;
    font-size: var(--font-size-compact);
    font-weight: 800;
  }
  .type.out {
    color: #f87171;
  }
  .subdomains {
    justify-self: end;
    color: #7e8997;
    font-size: var(--font-size-compact);
    font-weight: 800;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    white-space: nowrap;
  }
  code {
    color: #c8d1dc;
  }
  .rule button {
    height: 27px;
    font-size: var(--font-size-compact);
  }
  .rule-actions {
    display: flex;
    align-items: center;
    gap: 3px;
  }
  .rule .icon-button svg {
    width: 15px;
    height: 15px;
    fill: none;
    stroke: currentColor;
    stroke-linecap: round;
    stroke-linejoin: round;
    stroke-width: var(--svgbuttonstrokewidth, 1.5);
  }
  .rule .delete-action,
  .rule .delete-action:hover {
    color: #ef4444 !important;
  }
  .empty {
    display: grid;
    place-items: center;
    min-height: 150px;
    color: #747f8d;
    border: 1px dashed var(--border, #29303a);
    border-radius: 5px;
    background: var(--surface, #12161b);
  }
  .scope-list > .empty {
    display: grid;
    place-items: center;
    height: 100%;
    min-height: 0;
    padding: 16px;
    border: 0;
    border-radius: 0;
    background: transparent;
    text-align: center;
  }
  footer {
    display: flex;
    align-items: center;
    gap: 7px;
  }
  footer span {
    margin-left: auto;
    color: #7e8997;
    font-size: var(--font-size-body);
  }
  .import {
    font-size: var(--font-size-body);
  }
  .import input {
    display: none;
  }
  @media (max-width: 800px) {
    .scope-add {
      grid-template-columns: 1fr 1fr;
    }
    .scope-add > input:first-child {
      grid-column: 1 / -1;
    }
    .rule-columns {
      grid-template-columns: 1fr;
    }
    .inline-editor {
      grid-template-columns: minmax(0, 1fr) auto auto;
    }
    .inline-option {
      justify-self: start;
    }
    .inline-kind {
      grid-column: span 1;
    }
  }
</style>
