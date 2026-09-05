<script lang="ts">
  import { onDestroy } from "svelte";
  import { commands } from "$lib/api";
  import ConfirmDialog from "./ConfirmDialog.svelte";
  import type {
    Identity,
    IdentityBundle,
    IdentityGroup,
    IdentityGroupInput,
    IdentityInjectionType,
    IdentityInput,
    IdentityWorkspaceState,
  } from "$lib/types";

  type GroupModel = IdentityGroup & { identities: Identity[] };
  type GroupDraft = IdentityGroupInput;
  type IdentityCommands = {
    getIdentityGroups: () => Promise<IdentityBundle>;
    createIdentityGroup: (input: IdentityGroupInput) => Promise<IdentityGroup>;
    updateIdentityGroup: (id: string, input: IdentityGroupInput) => Promise<IdentityGroup>;
    deleteIdentityGroup: (id: string) => Promise<boolean>;
    createIdentity: (input: IdentityInput) => Promise<Identity>;
    updateIdentity: (id: string, input: IdentityInput) => Promise<Identity>;
    deleteIdentity: (id: string) => Promise<boolean>;
    exportIdentitiesJson: () => Promise<string | null>;
    importIdentitiesJson: () => Promise<number | null>;
  };

  const identityCommands = commands as unknown as IdentityCommands;
  const emptyGroupDraft = (): GroupDraft => ({ name: "", description: "", injectionType: "cookie", injectionKey: "" });
  const randomIdentityColor = () => {
    const bytes = new Uint32Array(1);
    if (typeof crypto !== "undefined" && typeof crypto.getRandomValues === "function") crypto.getRandomValues(bytes);
    else bytes[0] = Math.floor(Math.random() * 0xffffffff);
    const hue = (bytes[0] / 0xffffffff) * 360;
    const saturation = 0.72;
    const lightness = 0.58;
    const chroma = (1 - Math.abs(2 * lightness - 1)) * saturation;
    const match = (hue / 60) % 2 - 1;
    const second = chroma * (1 - Math.abs(match));
    const [red, green, blue] = hue < 60 ? [chroma, second, 0]
      : hue < 120 ? [second, chroma, 0]
        : hue < 180 ? [0, chroma, second]
          : hue < 240 ? [0, second, chroma]
            : hue < 300 ? [second, 0, chroma]
              : [chroma, 0, second];
    const offset = lightness - chroma / 2;
    const channel = (value: number) => Math.round((value + offset) * 255).toString(16).padStart(2, "0");
    return `#${channel(red)}${channel(green)}${channel(blue)}`;
  };
  const emptyIdentityDraft = (groupId = ""): IdentityInput => ({ groupId, name: "", color: randomIdentityColor(), notes: "", authValue: "" });

  let {
    workspace,
    onWorkspaceChange = (_state: IdentityWorkspaceState) => {},
    onStatus = (_message: string) => {},
    onError = (_reason: unknown) => {},
  }: {
    workspace?: IdentityWorkspaceState;
    onWorkspaceChange?: (state: IdentityWorkspaceState) => void;
    onStatus?: (message: string) => void;
    onError?: (reason: unknown) => void;
  } = $props();

  let groups = $state<GroupModel[]>([]);
  let loading = $state(true);
  let saving = $state(false);
  let selectedGroupId = $state<string | null>(null);
  let selectedIdentityId = $state<string | null>(null);
  let groupDraft = $state<GroupDraft | null>(null);
  let identityDraft = $state<Identity | null>(null);
  let groupDialog = $state<GroupDraft | null>(null);
  let identityDialog = $state<IdentityInput | null>(null);
  let deletion = $state<{ kind: "group"; group: GroupModel } | { kind: "identity"; identity: Identity } | null>(null);
  let focusedList = $state<"groups" | "identities">("groups");
  let groupSaveTimer: ReturnType<typeof setTimeout> | undefined;
  let identitySaveTimer: ReturnType<typeof setTimeout> | undefined;
  let groupSaveQueued = false;
  let identitySaveQueued = false;
  let initialLoadStarted = $state(false);
  let workspaceInitialized = $state(false);

  $effect(() => {
    if (workspaceInitialized) return;
    if (workspace) {
      selectedGroupId = workspace.selectedGroupId;
      selectedIdentityId = workspace.selectedIdentityId;
      groupDraft = workspace.groupDraft ? { ...workspace.groupDraft } : null;
      identityDraft = workspace.identityDraft ? { ...workspace.identityDraft } : null;
    }
    workspaceInitialized = true;
  });

  $effect(() => {
    if (!workspaceInitialized) return;
    onWorkspaceChange({
      selectedGroupId,
      selectedIdentityId,
      groupDraft: groupDraft ? { ...groupDraft } : null,
      identityDraft: identityDraft ? { ...identityDraft } : null,
    });
  });

  const selectedGroup = $derived(groups.find((group) => group.id === selectedGroupId) ?? null);
  const identities = $derived(selectedGroup?.identities ?? []);

  $effect(() => {
    if (!workspaceInitialized || initialLoadStarted) return;
    initialLoadStarted = true;
    void load();
  });
  onDestroy(() => {
    if (groupSaveTimer) clearTimeout(groupSaveTimer);
    if (identitySaveTimer) clearTimeout(identitySaveTimer);
  });

  function injectionTypeLabel(type: IdentityInjectionType) {
    return type === "queryParameter" ? "Query Parameter" : `${type[0].toUpperCase()}${type.slice(1)}`;
  }

  function copyGroupDraft(group: GroupModel): GroupDraft {
    return {
      name: group.name,
      description: group.description ?? "",
      injectionType: group.injectionType,
      injectionKey: group.injectionKey,
    };
  }

  function copyIdentity(identity: Identity): Identity {
    return { ...identity };
  }

  function groupInput(draft: GroupDraft): IdentityGroupInput {
    return {
      name: draft.name.trim(),
      description: draft.description?.trim() ?? "",
      injectionType: draft.injectionType,
      injectionKey: draft.injectionKey.trim(),
    };
  }

  function identityInput(draft: IdentityInput | Identity): IdentityInput {
    return {
      groupId: draft.groupId,
      name: draft.name,
      color: draft.color,
      notes: draft.notes,
      authValue: draft.authValue,
    };
  }

  function groupWithIdentities(group: IdentityGroup, identities: Identity[]): GroupModel {
    return { ...group, identities: identities.filter((identity) => identity.groupId === group.id) };
  }

  function selectGroup(id: string | null) {
    const group = groups.find((candidate) => candidate.id === id) ?? null;
    selectedGroupId = group?.id ?? null;
    groupDraft = group ? copyGroupDraft(group) : null;
    const firstIdentity = group?.identities[0] ?? null;
    selectIdentity(firstIdentity?.id ?? null, group);
  }

  function selectIdentity(id: string | null, group = selectedGroup) {
    const identity = group?.identities.find((candidate) => candidate.id === id) ?? null;
    selectedIdentityId = identity?.id ?? null;
    identityDraft = identity ? copyIdentity(identity) : null;
  }

  async function load(preferredGroupId?: string, preferredIdentityId?: string) {
    loading = true;
    try {
      const bundle: IdentityBundle = await identityCommands.getIdentityGroups();
      groups = bundle.groups.map((group) => groupWithIdentities(group, bundle.identities));
      const savedGroupDraft = groupDraft;
      const savedIdentityDraft = identityDraft;
      const requestedGroupId = preferredGroupId ?? selectedGroupId;
      const group = groups.find((candidate) => candidate.id === requestedGroupId) ?? groups[0] ?? null;
      selectedGroupId = group?.id ?? null;
      groupDraft = group && savedGroupDraft && group.id === requestedGroupId
        ? { ...savedGroupDraft }
        : group ? copyGroupDraft(group) : null;
      const requestedIdentityId = preferredIdentityId ?? selectedIdentityId;
      const identity = group?.identities.find((candidate) => candidate.id === requestedIdentityId)
        ?? group?.identities[0]
        ?? null;
      selectedIdentityId = identity?.id ?? null;
      identityDraft = identity && savedIdentityDraft && identity.id === requestedIdentityId
        ? { ...savedIdentityDraft }
        : identity ? copyIdentity(identity) : null;
    } catch (reason) {
      onError(reason);
    } finally {
      loading = false;
    }
  }

  function replaceGroup(updated: IdentityGroup, fallbackIdentities: Identity[] = []) {
    const next = groupWithIdentities(updated, fallbackIdentities);
    groups = groups.map((group) => group.id === next.id ? next : group);
    return next;
  }

  async function createGroup() {
    if (!groupDialog?.name.trim() || !groupDialog.injectionKey.trim() || saving) return;
    saving = true;
    try {
      const group = groupWithIdentities(await identityCommands.createIdentityGroup(groupInput(groupDialog)), []);
      groups = [...groups, group];
      groupDialog = null;
      selectGroup(group.id);
      onStatus("Identity group created");
    } catch (reason) {
      onError(reason);
    } finally {
      saving = false;
    }
  }

  function scheduleGroupSave() {
    if (groupSaveTimer) clearTimeout(groupSaveTimer);
    groupSaveTimer = setTimeout(() => void saveGroup(), 350);
  }

  function scheduleIdentitySave() {
    if (identitySaveTimer) clearTimeout(identitySaveTimer);
    identitySaveTimer = setTimeout(() => void saveIdentity(), 350);
  }

  async function saveGroup() {
    if (!selectedGroup || !groupDraft?.name.trim() || !groupDraft.injectionKey.trim()) return;
    if (saving) {
      groupSaveQueued = true;
      return;
    }
    const groupId = selectedGroup.id;
    const fallbackIdentities = selectedGroup.identities;
    const draft = { ...groupDraft };
    saving = true;
    try {
      const updated = replaceGroup(
        await identityCommands.updateIdentityGroup(groupId, groupInput(draft)),
        fallbackIdentities,
      );
      const current = groupDraft;
      if (selectedGroupId === groupId && current
        && current.name === draft.name
        && current.description === draft.description
        && current.injectionType === draft.injectionType
        && current.injectionKey === draft.injectionKey) {
        groupDraft = copyGroupDraft(updated);
      }
    } catch (reason) {
      onError(reason);
    } finally {
      saving = false;
      if (groupSaveQueued) {
        groupSaveQueued = false;
        scheduleGroupSave();
      }
      if (identitySaveQueued) {
        identitySaveQueued = false;
        scheduleIdentitySave();
      }
    }
  }

  async function createIdentity() {
    if (!selectedGroup || !identityDialog || saving) return;
    saving = true;
    try {
      const identity = await identityCommands.createIdentity(identityInput({ ...identityDialog, groupId: selectedGroup.id }));
      const next = { ...selectedGroup, identities: [...selectedGroup.identities, identity] };
      groups = groups.map((group) => group.id === next.id ? next : group);
      identityDialog = null;
      selectedGroupId = next.id;
      groupDraft = copyGroupDraft(next);
      selectIdentity(identity.id, next);
      onStatus("Identity created");
    } catch (reason) {
      onError(reason);
    } finally {
      saving = false;
    }
  }

  async function saveIdentity() {
    if (!selectedGroup || !identityDraft?.name.trim()) return;
    if (saving) {
      identitySaveQueued = true;
      return;
    }
    const groupId = selectedGroup.id;
    const identityId = identityDraft.id;
    const draft = { ...identityDraft };
    saving = true;
    try {
      const updated = await identityCommands.updateIdentity(identityId, identityInput(draft));
      const currentGroup = groups.find((group) => group.id === groupId);
      if (!currentGroup) return;
      const next = {
        ...currentGroup,
        identities: currentGroup.identities.map((identity) => identity.id === updated.id ? updated : identity),
      };
      groups = groups.map((group) => group.id === next.id ? next : group);
      if (selectedGroupId === groupId && selectedIdentityId === identityId && identityDraft
        && identityDraft.name === draft.name
        && identityDraft.color === draft.color
        && identityDraft.notes === draft.notes
        && identityDraft.authValue === draft.authValue) {
        groupDraft = copyGroupDraft(next);
        selectIdentity(updated.id, next);
      }
    } catch (reason) {
      onError(reason);
    } finally {
      saving = false;
      if (groupSaveQueued) {
        groupSaveQueued = false;
        scheduleGroupSave();
      }
      if (identitySaveQueued) {
        identitySaveQueued = false;
        scheduleIdentitySave();
      }
    }
  }

  function requestDeleteGroup(group: GroupModel) {
    deletion = { kind: "group", group };
  }

  function requestDeleteIdentity(identity: Identity) {
    deletion = { kind: "identity", identity };
  }

  async function confirmDeletion() {
    if (!deletion || saving) return;
    const target = deletion;
    saving = true;
    try {
      if (target.kind === "group") {
        await identityCommands.deleteIdentityGroup(target.group.id);
        const index = groups.findIndex((group) => group.id === target.group.id);
        const remaining = groups.filter((group) => group.id !== target.group.id);
        groups = remaining;
        selectGroup(remaining[Math.min(index, remaining.length - 1)]?.id ?? null);
        onStatus("Identity group deleted");
      } else if (selectedGroup) {
        await identityCommands.deleteIdentity(target.identity.id);
        const index = selectedGroup.identities.findIndex((identity) => identity.id === target.identity.id);
        const next = {
          ...selectedGroup,
          identities: selectedGroup.identities.filter((identity) => identity.id !== target.identity.id),
        };
        groups = groups.map((group) => group.id === next.id ? next : group);
        selectedGroupId = next.id;
        groupDraft = copyGroupDraft(next);
        selectIdentity(next.identities[Math.min(index, next.identities.length - 1)]?.id ?? null, next);
        onStatus("Identity deleted");
      }
      deletion = null;
    } catch (reason) {
      onError(reason);
    } finally {
      saving = false;
    }
  }

  async function exportJson() {
    try {
      const path = await identityCommands.exportIdentitiesJson();
      if (path) onStatus(`Identity JSON exported to ${path}`);
    } catch (reason) {
      onError(reason);
    }
  }

  async function importJson() {
    try {
      const count = await identityCommands.importIdentitiesJson();
      if (count === null) return;
      await load();
      onStatus(`Imported ${count} ${count === 1 ? "identity" : "identities"}`);
    } catch (reason) {
      onError(reason);
    }
  }

  export function handleShortcut(action: string): boolean {
    if (action === "transient.close") {
      if (deletion) {
        if (saving) return true;
        deletion = null;
        return true;
      }
      if (identityDialog) {
        if (saving) return true;
        identityDialog = null;
        return true;
      }
      if (groupDialog) {
        if (saving) return true;
        groupDialog = null;
        return true;
      }
      return false;
    }
    if (action === "identity.createGroup") {
      groupDialog = emptyGroupDraft();
      return true;
    }
    if (action === "identity.createIdentity") {
      if (!selectedGroup) return false;
      identityDialog = emptyIdentityDraft(selectedGroup.id);
      return true;
    }
    if (action === "identity.selectPrevious" || action === "identity.selectNext") {
      const inIdentityList = focusedList === "identities" && Boolean(selectedGroup);
      const list = inIdentityList ? identities : groups;
      if (!list.length) return false;
      const currentId = inIdentityList ? selectedIdentityId : selectedGroupId;
      const currentIndex = list.findIndex((item) => item.id === currentId);
      const offset = action.endsWith("Previous") ? -1 : 1;
      const nextIndex = currentIndex < 0
        ? 0
        : Math.max(0, Math.min(list.length - 1, currentIndex + offset));
      const next = list[nextIndex];
      if (inIdentityList) selectIdentity(next.id);
      else selectGroup(next.id);
      return true;
    }
    if (action === "identity.deleteSelected") {
      if (selectedIdentityId && selectedGroup) {
        const identity = selectedGroup.identities.find((candidate) => candidate.id === selectedIdentityId);
        if (!identity) return false;
        requestDeleteIdentity(identity);
      } else if (selectedGroup) requestDeleteGroup(selectedGroup);
      else return false;
      return true;
    }
    if (action === "identity.export") {
      if (!groups.length) return false;
      void exportJson();
      return true;
    }
    if (action === "identity.import") {
      void importJson();
      return true;
    }
    return false;
  }
</script>

<section class="identity-plus" aria-label="ID+ identity management workspace">
  <header class="workspace-header">
    <div class="workspace-title">
      <span>{groups.length} {groups.length === 1 ? "group" : "groups"}</span>
    </div>
    <div class="workspace-actions">
      <button class="text-button" onclick={() => (groupDialog = emptyGroupDraft())}>+ Group</button>
      <button class="text-button" onclick={() => void exportJson()} disabled={!groups.length}>Export JSON</button>
      <button class="text-button" onclick={() => void importJson()}>Import JSON</button>
    </div>
  </header>

  <div class="workspace-body">
    <aside class="group-pane" aria-label="Identity groups">
      <div class="pane-label"><span>Identity groups</span><button data-tooltip="Create identity group" aria-label="Create identity group" onclick={() => (groupDialog = emptyGroupDraft())}>+</button></div>
      <div class="group-list" aria-live="polite">
        {#each groups as group (group.id)}
          <button class:active={selectedGroupId === group.id} onfocus={() => (focusedList = "groups")} onclick={() => selectGroup(group.id)}>
            <span class="group-mark" aria-hidden="true">◈</span>
            <span class="group-main"><strong data-tooltip={group.name}>{group.name || "Untitled group"}</strong><small>{injectionTypeLabel(group.injectionType)} · {group.injectionKey || "No key"}</small></span>
            <small>{group.identities.length}</small>
          </button>
        {:else}
          <div class="empty-list">
            <span aria-hidden="true">◈</span>
            <strong>{loading ? "Loading identity groups…" : "No identity groups"}</strong>
            <small>Create a group to store identities and choose how to inject them.</small>
          </div>
        {/each}
      </div>
    </aside>

    <section class="identity-pane" aria-label="Identities">
      <div class="pane-label"><span>{selectedGroup?.name || "Identities"}</span><button data-tooltip="Create identity" aria-label="Create identity" disabled={!selectedGroup} onclick={() => selectedGroup && (identityDialog = emptyIdentityDraft(selectedGroup.id))}>+</button></div>
      <div class="identity-list">
        {#if selectedGroup}
          {#each identities as identity (identity.id)}
            <button class:active={selectedIdentityId === identity.id} onfocus={() => (focusedList = "identities")} onclick={() => selectIdentity(identity.id)}>
              <span class="identity-color" style={`--identity-color:${identity.color || "#5794ef"}`} aria-hidden="true"></span>
              <span><strong data-tooltip={identity.name}>{identity.name || "Untitled identity"}</strong><small>{identity.notes || "No notes"}</small></span>
            </button>
          {:else}
            <div class="empty-list"><span aria-hidden="true">●</span><strong>No identities yet</strong><small>Create identities for the selected group.</small></div>
          {/each}
        {:else}
          <div class="empty-list"><span aria-hidden="true">●</span><strong>Select a group</strong><small>Its identities will appear here.</small></div>
        {/if}
      </div>
    </section>

    <section class:empty={!selectedGroup || !groupDraft} class="detail-pane" aria-label="Identity group details">
      {#if selectedGroup && groupDraft}
        <div class="detail-header">
          <div><p>GROUP SETTINGS</p><strong>{selectedGroup.name || "Untitled group"}</strong></div>
          <div class="detail-actions"><button class="text-button danger delete-button" onclick={() => requestDeleteGroup(selectedGroup)}>Delete group</button></div>
        </div>
        <div class="group-fields">
          <label><span>Group name</span><input bind:value={groupDraft.name} oninput={scheduleGroupSave} maxlength="120" placeholder="e.g. Admin accounts" /></label>
          <label><span>Injection type</span><select bind:value={groupDraft.injectionType} onchange={scheduleGroupSave}><option value="cookie">Cookie</option><option value="header">Header</option><option value="queryParameter">Query Parameter</option></select></label>
          <label><span>Injection key</span><input bind:value={groupDraft.injectionKey} oninput={scheduleGroupSave} maxlength="160" placeholder={groupDraft.injectionType === "cookie" ? "session" : groupDraft.injectionType === "header" ? "Authorization" : "api_key"} /></label>
          <label class="group-description"><span>Description <em>(optional)</em></span><textarea bind:value={groupDraft.description} oninput={scheduleGroupSave} maxlength="500" placeholder="Purpose or context for this group"></textarea></label>
        </div>

        <div class="identity-editor">
          {#if identityDraft}
            <div class="editor-heading"><div><p>IDENTITY</p><strong>{identityDraft.name || "Untitled identity"}</strong></div><div><button class="text-button danger delete-button" onclick={() => requestDeleteIdentity(identityDraft!)}>Delete identity</button></div></div>
            <div class="identity-fields">
              <label><span>Name</span><input bind:value={identityDraft.name} oninput={scheduleIdentitySave} maxlength="120" placeholder="e.g. Alice, admin" /></label>
              <label><span>Color</span><input class="color-input" type="color" bind:value={identityDraft.color} onchange={scheduleIdentitySave} aria-label="Identity color" /><input bind:value={identityDraft.color} oninput={scheduleIdentitySave} maxlength="20" aria-label="Identity color value" /></label>
              <label class="auth-value"><span>Authentication value</span><textarea bind:value={identityDraft.authValue} oninput={scheduleIdentitySave} placeholder="Enter the value injected for this identity" spellcheck="false"></textarea></label>
              <label class="notes"><span>Notes</span><textarea bind:value={identityDraft.notes} oninput={scheduleIdentitySave} placeholder="Purpose, account role, expiration, or other non-sensitive context…"></textarea></label>
            </div>
          {:else}
            <div class="empty-detail"><span aria-hidden="true">●</span><strong>Select an identity</strong><small>Authentication values are editable only here and are never shown in the group list.</small><button class="text-button" onclick={() => (identityDialog = emptyIdentityDraft(selectedGroup.id))}>+ Create identity</button></div>
          {/if}
        </div>
      {:else}
        <div class="empty-detail"><span aria-hidden="true">◈</span><strong>{loading ? "Loading ID+…" : "Select an identity group"}</strong><small>Create a group to define an injection method and store identities.</small><button class="text-button" onclick={() => (groupDialog = emptyGroupDraft())} disabled={loading}>+ Create group</button></div>
      {/if}
    </section>
  </div>
</section>

{#if groupDialog}
  <div class="modal-backdrop" role="presentation" onclick={(event) => { if (event.target === event.currentTarget) groupDialog = null; }}>
    <form class="dialog" aria-labelledby="new-group-title" onsubmit={(event) => { event.preventDefault(); void createGroup(); }}>
      <p>NEW IDENTITY GROUP</p><h2 id="new-group-title">Create identity group</h2>
      <label><span>Group name</span><input bind:value={groupDialog.name} maxlength="120" placeholder="e.g. Admin accounts" /></label>
      <label><span>Description <em>(optional)</em></span><textarea bind:value={groupDialog.description} maxlength="500" placeholder="Purpose or context for this group"></textarea></label>
      <label><span>Injection type</span><select bind:value={groupDialog.injectionType}><option value="cookie">Cookie</option><option value="header">Header</option><option value="queryParameter">Query Parameter</option></select></label>
      <label><span>Injection key</span><input bind:value={groupDialog.injectionKey} maxlength="160" placeholder="e.g. Authorization or session" /></label>
      <div class="dialog-actions"><button class="text-button" type="button" onclick={() => (groupDialog = null)}>Cancel</button><button class="text-button save-button" type="submit" disabled={saving || !groupDialog.name.trim() || !groupDialog.injectionKey.trim()}>{saving ? "Creating…" : "Create group"}</button></div>
    </form>
  </div>
{/if}

{#if identityDialog}
  <div class="modal-backdrop" role="presentation" onclick={(event) => { if (event.target === event.currentTarget) identityDialog = null; }}>
    <form class="dialog identity-dialog" aria-labelledby="new-identity-title" onsubmit={(event) => { event.preventDefault(); void createIdentity(); }}>
      <p>NEW IDENTITY</p><h2 id="new-identity-title">Add to {selectedGroup?.name || "group"}</h2>
      <label><span>Name</span><input bind:value={identityDialog.name} maxlength="120" placeholder="e.g. Alice, admin" /></label>
      <label><span>Color</span><input class="color-input" type="color" bind:value={identityDialog.color} aria-label="Identity color" /></label>
      <label><span>Authentication value</span><textarea bind:value={identityDialog.authValue} placeholder="Enter the value injected for this identity" spellcheck="false"></textarea></label>
      <label><span>Notes</span><textarea bind:value={identityDialog.notes} placeholder="Optional non-sensitive context"></textarea></label>
      <div class="dialog-actions"><button class="text-button" type="button" onclick={() => (identityDialog = null)}>Cancel</button><button class="text-button save-button" type="submit" disabled={saving}>{saving ? "Creating…" : "Create identity"}</button></div>
    </form>
  </div>
{/if}

{#if deletion}
  <ConfirmDialog
    open={true}
    title={`Delete ${deletion.kind === "group" ? "identity group" : "identity"}?`}
    message={deletion.kind === "group"
      ? `This permanently deletes “${deletion.group.name || "Untitled group"}” and its ${deletion.group.identities.length} ${deletion.group.identities.length === 1 ? "identity" : "identities"}.`
      : `This permanently deletes “${deletion.identity.name || "Untitled identity"}”.`}
    confirmLabel="Delete permanently"
    busy={saving}
    onConfirm={() => void confirmDeletion()}
    onCancel={() => { if (!saving) deletion = null; }}
  />
{/if}

<style>
  .identity-plus { display: grid; grid-template-rows: 48px minmax(0, 1fr); height: 100%; min-height: 0; padding: 4px; color: var(--text); overflow: hidden; }
  .workspace-header { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 0 8px 0 11px; border: 1px solid var(--border); border-radius: 3px 3px 0 0; background: var(--surface); }
  .workspace-title, .workspace-actions, .detail-actions, .editor-heading > div:last-child, .dialog-actions { display: flex; align-items: center; gap: 5px; min-width: 0; }
  .workspace-title { align-items: baseline; gap: 9px; }.detail-header p, .editor-heading p, .dialog > p { margin: 0; color: var(--accent); font-size: var(--font-size-compact); font-weight: 800; letter-spacing: .13em; }.workspace-title span { color: var(--muted); font-size: var(--font-size-compact); }
  button, input, select, textarea { font: inherit; } button, select { min-height: 27px; border: 1px solid var(--border-strong); border-radius: 3px; color: var(--text); background: var(--surface-2); cursor: pointer; } button { padding: 0 8px; } button:disabled { opacity: .45; cursor: default; } button:hover:not(:disabled) { border-color: var(--accent); background: var(--surface-3); color: var(--text); } .group-list > button:hover, .group-list > button.active, .identity-list > button:hover, .identity-list > button.active { background: var(--accent-soft); } .pane-label button:hover:not(:disabled), .workspace-actions button:hover:not(:disabled), .empty-detail button:hover:not(:disabled), .dialog-actions button:hover:not(:disabled) { background: var(--surface-3); } .save-button:hover:not(:disabled) { border-color: var(--accent); background: color-mix(in srgb, var(--accent) 82%, var(--surface)); } .delete-button:hover:not(:disabled) { border-color: var(--danger); background: color-mix(in srgb, var(--danger) 16%, var(--surface-2)); } input, select, textarea { min-width: 0; color: var(--text); border: 1px solid var(--border-strong); border-radius: 3px; background: var(--input); }
  .workspace-body { display: grid; grid-template-columns: 230px 285px minmax(420px, 1fr); min-height: 0; border: 1px solid var(--border); border-top: 0; background: var(--bg); overflow: hidden; }.group-pane, .identity-pane, .detail-pane { min-width: 0; min-height: 0; background: var(--surface); }.group-pane, .identity-pane { border-right: 1px solid var(--border); display: grid; grid-template-rows: 31px minmax(0, 1fr); }.pane-label { display: flex; align-items: center; justify-content: space-between; padding: 0 7px 0 9px; color: var(--muted); border-bottom: 1px solid var(--border); background: var(--surface-2); font-size: var(--font-size-compact); font-weight: 750; letter-spacing: .08em; text-transform: uppercase; }.pane-label button { width: 23px; min-height: 22px; padding: 0; }
  .group-list, .identity-list { min-height: 0; overflow: auto; }.group-list > button { display: grid; grid-template-columns: 19px minmax(0, 1fr) auto; gap: 5px; align-items: center; width: 100%; min-height: 58px; padding: 7px; border: 0; border-bottom: 1px solid var(--border); border-radius: 0; text-align: left; background: transparent; }.identity-list > button { display: grid; grid-template-columns: 12px minmax(0, 1fr); gap: 8px; align-items: center; width: 100%; min-height: 52px; padding: 7px 9px; border: 0; border-bottom: 1px solid var(--border); border-radius: 0; text-align: left; background: transparent; }.group-list > button:hover, .group-list > button.active, .identity-list > button:hover, .identity-list > button.active { background: var(--accent-soft); }.group-list > button.active, .identity-list > button.active { box-shadow: inset 3px 0 var(--accent); }.group-mark { color: var(--accent); }.group-main, .identity-list > button > span:last-child { display: grid; min-width: 0; gap: 3px; }.group-main strong, .group-main small, .identity-list strong, .identity-list small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }.group-main strong, .identity-list strong { font-size: var(--font-size-compact); }.group-main small, .group-list > button > small, .identity-list small { color: var(--muted); font-size: var(--font-size-compact); }.identity-color { width: 10px; height: 10px; border-radius: 50%; background: var(--identity-color); box-shadow: 0 0 0 2px color-mix(in srgb, var(--identity-color) 20%, transparent); }
  .detail-pane { display: grid; grid-template-rows: auto auto minmax(0, 1fr); overflow: hidden; }.detail-pane.empty { grid-template-rows: minmax(0, 1fr); }.detail-header, .editor-heading { display: flex; align-items: center; justify-content: space-between; gap: 9px; padding: 6px 8px; border-bottom: 1px solid var(--border); background: var(--surface-2); }.detail-header strong, .editor-heading strong { display: block; font-size: var(--font-size-body); }.detail-actions button, .editor-heading button { white-space: nowrap; font-size: var(--font-size-compact); }.save-button { color: var(--accent-contrast); border-color: var(--accent); background: var(--accent); font-weight: 700; }.delete-button { color: var(--danger); }.group-fields { display: grid; grid-template-columns: minmax(150px, 1fr) minmax(115px, .6fr) minmax(150px, 1fr); gap: 5px; padding: 5px; border-bottom: 1px solid var(--border); }.group-fields label, .identity-fields label, .dialog label { display: grid; gap: 4px; color: var(--muted); font-size: var(--font-size-compact); }.group-fields input, .group-fields select { height: 29px; padding: 0 7px; }.group-fields .group-description { grid-column: 1 / -1; }.group-fields textarea { min-height: 58px; padding: 7px; resize: vertical; }.identity-editor { min-height: 0; overflow: auto; background: var(--bg); }.editor-heading { position: sticky; top: 0; z-index: 1; }.identity-fields { display: grid; grid-template-columns: minmax(180px, 1fr) minmax(140px, .7fr); gap: 6px; padding: 8px; }.identity-fields input { height: 30px; padding: 0 7px; }.identity-fields .auth-value, .identity-fields .notes { grid-column: 1 / -1; }.identity-fields textarea, .dialog textarea { min-height: 84px; padding: 7px; resize: vertical; }.identity-fields .notes textarea { min-height: 58px; }.identity-fields label:has(.color-input) { grid-template-columns: 36px minmax(0, 1fr); align-content: end; }.identity-fields label:has(.color-input) > span { grid-column: 1 / -1; }.color-input { width: 34px; padding: 2px !important; cursor: pointer; }
  .empty-list, .empty-detail { display: grid; place-content: center; justify-items: center; gap: 6px; height: 100%; padding: 24px; color: var(--muted); text-align: center; font-size: var(--font-size-compact); }.empty-detail { gap: 12px; }.empty-list > span, .empty-detail > span { color: var(--accent); font-size: var(--font-size-title); }.empty-list strong, .empty-detail strong { color: var(--text); }.empty-detail button { margin-top: 5px; }
  .modal-backdrop { position: fixed; z-index: 60; inset: 0; display: grid; place-items: center; background: color-mix(in srgb, var(--bg) 70%, transparent); backdrop-filter: blur(3px); }.dialog { display: grid; gap: 10px; width: min(420px, calc(100vw - 32px)); max-height: calc(100vh - 32px); overflow: auto; padding: 20px; color: var(--text); border: 1px solid var(--border-strong); border-radius: 8px; background: var(--surface); box-shadow: 0 20px 55px #0008; }.dialog h2 { margin: -3px 0 0; font-size: var(--font-size-heading); }.dialog input, .dialog select { height: 34px; padding: 0 9px; }.identity-dialog { width: min(490px, calc(100vw - 32px)); }.dialog-actions { justify-content: flex-end; margin-top: 4px; }
  @media (max-width: 1050px) { .workspace-body { grid-template-columns: 190px 250px minmax(330px, 1fr); }.group-fields { grid-template-columns: 1fr 1fr; }.group-fields label:first-child { grid-column: 1 / -1; } }
</style>
