<script lang="ts">
  import { commands } from "$lib/api";
  import type {
    OrganizerBundle,
    OrganizerFolder,
    OrganizerItem,
    OrganizerItemInput,
    OrganizerStage,
    OrganizerTagDefinition,
    OrganizerWorkspaceState,
  } from "$lib/types";
  import { decodeHttpText, encodeHttpText, synchronizeHttpContentLength } from "$lib/http-message";
  import { formatDate } from "$lib/format";
  import MessageViewer from "./MessageViewer.svelte";
  import RecycleBinIcon from "./RecycleBinIcon.svelte";

  let {
    revision = 0,
    workspace,
    onWorkspaceChange = (_state: OrganizerWorkspaceState) => {},
    onSendReplay,
    onSendFuzz,
    onSendDecoder,
    onStatus = (_message: string) => {},
    onError = (_reason: unknown) => {},
  }: {
    revision?: number;
    workspace?: OrganizerWorkspaceState;
    onWorkspaceChange?: (state: OrganizerWorkspaceState) => void;
    onSendReplay: (raw: Uint8Array, tls: boolean) => void;
    onSendFuzz: (raw: Uint8Array, tls: boolean) => void;
    onSendDecoder?: (value: string) => void;
    onStatus?: (message: string) => void;
    onError?: (reason: unknown) => void;
  } = $props();

  let bundle = $state<OrganizerBundle>({ version: 1, folders: [], items: [] });
  let loading = $state(true);
  let selectedFolderId = $state<string | "all" | "unfiled">("all");
  let selectedItemId = $state<string | null>(null);
  let query = $state("");
  let selectedTag = $state("");
  let sort = $state<"updated" | "created" | "title" | "host">("updated");
  let lastRevision = -1;
  let draft = $state<OrganizerItem | null>(null);
  let requestText = $state("");
  let responseText = $state("");
  let tagText = $state("");
  let saving = $state(false);
  let saveQueued = $state(false);
  let saveTimer: ReturnType<typeof setTimeout> | undefined;
  let tagDefinitions = $state<OrganizerTagDefinition[]>([]);
  let stages = $state<OrganizerStage[]>([]);
  let folderDialog = $state<{ id: string | null; parentId: string | null; name: string } | null>(null);
  let tagDialog = $state<{ name: string; color: string } | null>(null);
  let stageDialog = $state<{ id: string | null; name: string; color: string } | null>(null);
  let deleteDialog = $state<{ kind: "item" | "folder" | "stage"; id: string; name: string } | null>(null);
  let workspaceInitialized = $state(false);

  $effect(() => {
    if (workspaceInitialized) return;
    if (workspace) {
      selectedFolderId = workspace.selectedFolderId;
      selectedItemId = workspace.selectedItemId;
      query = workspace.query;
      selectedTag = workspace.selectedTag;
      sort = workspace.sort;
      draft = workspace.draft
        ? { ...workspace.draft, request: [...workspace.draft.request], response: [...workspace.draft.response], tags: [...workspace.draft.tags] }
        : null;
      requestText = workspace.requestText;
      responseText = workspace.responseText;
      tagText = workspace.tagText;
      tagDefinitions = workspace.tagDefinitions.map((tag) => ({ ...tag }));
      stages = workspace.stages.map((stage) => ({ ...stage }));
    }
    workspaceInitialized = true;
  });

  $effect(() => {
    if (!workspaceInitialized) return;
    onWorkspaceChange({
      selectedFolderId,
      selectedItemId,
      query,
      selectedTag,
      sort,
      draft: draft ? { ...draft, request: [...draft.request], response: [...draft.response], tags: [...draft.tags] } : null,
      requestText,
      responseText,
      tagText,
      tagDefinitions: tagDefinitions.map((tag) => ({ ...tag })),
      stages: stages.map((stage) => ({ ...stage })),
    });
  });

  const flatFolders = $derived(flattenFolders(bundle.folders));
  const allTags = $derived([...new Set([...tagDefinitions.map((tag) => tag.name), ...bundle.items.flatMap((item) => item.tags)])].sort((a, b) => a.localeCompare(b)));
  const visibleItems = $derived.by(() => {
    const needle = query.trim().toLocaleLowerCase();
    const descendants = selectedFolderId !== "all" && selectedFolderId !== "unfiled"
      ? new Set([selectedFolderId, ...folderDescendants(selectedFolderId, bundle.folders)])
      : null;
    return bundle.items
      .filter((item) => {
        if (selectedFolderId === "unfiled" && item.folderId) return false;
        if (descendants && (!item.folderId || !descendants.has(item.folderId))) return false;
        if (selectedTag && !item.tags.some((tag) => tag.toLocaleLowerCase() === selectedTag.toLocaleLowerCase())) return false;
        if (!needle) return true;
        return [
          item.title, item.method, item.host, item.path, item.notes, item.source, ...item.tags,
          new TextDecoder().decode(new Uint8Array(item.request)),
          new TextDecoder().decode(new Uint8Array(item.response)),
        ].some((value) => value.toLocaleLowerCase().includes(needle));
      })
      .sort((left, right) => {
        if (sort === "title") return left.title.localeCompare(right.title);
        if (sort === "host") return left.host.localeCompare(right.host) || left.path.localeCompare(right.path);
        const field = sort === "created" ? "createdAt" : "updatedAt";
        return right[field].localeCompare(left[field]);
      });
  });

  $effect(() => {
    if (!workspaceInitialized) return;
    if (revision === lastRevision) return;
    lastRevision = revision;
    void load();
  });

  async function load(preferredId?: string) {
    loading = true;
    try {
      bundle = await commands.getOrganizer();
      if (workspace) {
        selectedFolderId = workspace.selectedFolderId;
        query = workspace.query;
        selectedTag = workspace.selectedTag;
        sort = workspace.sort;
        tagDefinitions = workspace.tagDefinitions.map((tag) => ({ ...tag }));
        stages = workspace.stages.map((stage) => ({ ...stage }));
      }
      ensureTagDefinitions(bundle.items.flatMap((item) => item.tags));
      if (selectedFolderId !== "all" && selectedFolderId !== "unfiled"
        && !bundle.folders.some((folder) => folder.id === selectedFolderId)) {
        selectedFolderId = "all";
      }
      const nextId = preferredId ?? selectedItemId ?? draft?.id;
      if (nextId && bundle.items.some((item) => item.id === nextId)) {
        const nextItem = bundle.items.find((item) => item.id === nextId);
        if (!draft || draft.id !== nextId || draft.updatedAt !== nextItem?.updatedAt) selectItem(nextId);
      } else if (visibleItems[0]) selectItem(visibleItems[0].id);
      else {
        selectedItemId = null;
        draft = null;
        requestText = "";
        responseText = "";
        tagText = "";
      }
    } catch (reason) {
      onError(reason);
    } finally {
      loading = false;
    }
  }

  function flattenFolders(folders: OrganizerFolder[]) {
    const result: Array<OrganizerFolder & { depth: number }> = [];
    const walk = (parentId: string | null, depth: number) => {
      folders
        .filter((folder) => folder.parentId === parentId)
        .sort((a, b) => a.name.localeCompare(b.name))
        .forEach((folder) => {
          result.push({ ...folder, depth });
          walk(folder.id, depth + 1);
        });
    };
    walk(null, 0);
    return result;
  }

  function folderDescendants(id: string, folders: OrganizerFolder[]): string[] {
    return folders
      .filter((folder) => folder.parentId === id)
      .flatMap((folder) => [folder.id, ...folderDescendants(folder.id, folders)]);
  }

  function folderCount(id: string) {
    const included = new Set([id, ...folderDescendants(id, bundle.folders)]);
    return bundle.items.filter((item) => item.folderId && included.has(item.folderId)).length;
  }

  function folderName(id: string | null) {
    return id ? bundle.folders.find((folder) => folder.id === id)?.name ?? "Unknown folder" : "Unfiled";
  }

  function selectItem(id: string) {
    const item = bundle.items.find((candidate) => candidate.id === id);
    if (!item) return;
    selectedItemId = id;
    draft = {
      ...item,
      request: [...item.request],
      response: [...item.response],
      tags: [...item.tags],
    };
    requestText = decodeHttpText(new Uint8Array(item.request));
    responseText = decodeHttpText(new Uint8Array(item.response));
    tagText = "";
  }

  const tagPalette = ["#5794ef", "#23b889", "#f59e0b", "#e66a8a", "#a78bfa", "#14b8a6", "#f97316"];
  const stagePalette = ["#5794ef", "#23b889", "#f59e0b", "#e66a8a", "#a78bfa", "#14b8a6", "#f97316"];

  function defaultColor(name: string, palette: string[]) {
    let hash = 0;
    for (const character of name) hash = (hash * 31 + character.charCodeAt(0)) >>> 0;
    return palette[hash % palette.length];
  }

  function tagDefinition(name: string) {
    return tagDefinitions.find((tag) => tag.name.toLocaleLowerCase() === name.toLocaleLowerCase());
  }

  function tagColor(name: string) {
    return tagDefinition(name)?.color || defaultColor(name, tagPalette);
  }

  function ensureTagDefinitions(names: string[]) {
    const additions: OrganizerTagDefinition[] = [];
    for (const rawName of names) {
      const name = rawName.trim();
      if (!name || tagDefinitions.some((tag) => tag.name.toLocaleLowerCase() === name.toLocaleLowerCase()) || additions.some((tag) => tag.name.toLocaleLowerCase() === name.toLocaleLowerCase())) continue;
      additions.push({ name, color: defaultColor(name, tagPalette) });
    }
    if (additions.length) tagDefinitions = [...tagDefinitions, ...additions];
  }

  function updateTagColor(name: string, color: string) {
    const existing = tagDefinition(name);
    if (existing) tagDefinitions = tagDefinitions.map((tag) => tag.name === existing.name ? { ...tag, color } : tag);
    else tagDefinitions = [...tagDefinitions, { name, color }];
  }

  function createTag() {
    tagDialog = { name: "", color: defaultColor(String(tagDefinitions.length + 1), tagPalette) };
  }

  function submitTag() {
    if (!tagDialog?.name.trim()) return;
    const name = tagDialog.name.trim();
    const existing = tagDefinition(name);
    if (existing) updateTagColor(existing.name, tagDialog.color);
    else tagDefinitions = [...tagDefinitions, { name, color: tagDialog.color }];
    tagDialog = null;
  }

  function addCommittedTags(values: string[], shouldSchedule = true) {
    if (!draft) return false;
    const additions = values.map((tag) => tag.trim()).filter(Boolean);
    if (!additions.length) return false;
    const next = [...draft.tags];
    for (const value of additions) {
      if (!next.some((tag) => tag.toLocaleLowerCase() === value.toLocaleLowerCase())) next.push(value);
    }
    const changed = next.length !== draft.tags.length;
    draft = { ...draft, tags: next };
    ensureTagDefinitions(additions);
    if (changed && shouldSchedule) scheduleSaveDraft();
    return changed;
  }

  function handleTagInput(commitRemainder = false, shouldSchedule = true) {
    const parts = tagText.split(",");
    if (parts.length > 1) {
      addCommittedTags(parts.slice(0, -1), shouldSchedule);
      tagText = parts.at(-1)?.trim() ?? "";
    } else if (commitRemainder && tagText.trim()) {
      addCommittedTags([tagText], shouldSchedule);
      tagText = "";
    }
  }

  function removeTag(tagToRemove: string) {
    if (!draft) return;
    draft = { ...draft, tags: draft.tags.filter((tag) => tag !== tagToRemove) };
    scheduleSaveDraft();
  }

  function stageFor(id: string | null) {
    return id ? stages.find((stage) => stage.id === id) : undefined;
  }

  function createStage() {
    stageDialog = { id: null, name: "", color: defaultColor(String(stages.length + 1), stagePalette) };
  }

  function editStage(stage: OrganizerStage) {
    stageDialog = { ...stage };
  }

  function requestDeleteStage(stage: OrganizerStage) {
    deleteDialog = { kind: "stage", id: stage.id, name: stage.name };
  }

  function submitStage() {
    if (!stageDialog?.name.trim()) return;
    const value = { ...stageDialog, name: stageDialog.name.trim() };
    stages = value.id
      ? stages.map((stage) => stage.id === value.id ? { id: value.id!, name: value.name, color: value.color } : stage)
      : [...stages, { id: `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`, name: value.name, color: value.color }];
    stageDialog = null;
  }

  function beginStageDrag(event: DragEvent, id: string) {
    event.dataTransfer?.setData("text/stage-id", id);
    if (event.dataTransfer) event.dataTransfer.effectAllowed = "move";
  }

  function reorderStage(event: DragEvent, targetId: string) {
    event.preventDefault();
    const sourceId = event.dataTransfer?.getData("text/stage-id");
    if (!sourceId || sourceId === targetId) return;
    const sourceIndex = stages.findIndex((stage) => stage.id === sourceId);
    const targetIndex = stages.findIndex((stage) => stage.id === targetId);
    if (sourceIndex < 0 || targetIndex < 0) return;
    const next = [...stages];
    const [moved] = next.splice(sourceIndex, 1);
    next.splice(targetIndex, 0, moved);
    stages = next;
  }

  function inputFor(item: OrganizerItem): OrganizerItemInput {
    const tags = draft?.id === item.id ? draft.tags : item.tags;
    return {
      title: item.title,
      folderId: item.folderId,
      stageId: item.stageId,
      request: Array.from(encodeHttpText(synchronizeHttpContentLength(requestText))),
      response: Array.from(encodeHttpText(responseText)),
      tls: item.tls,
      source: item.source,
      notes: item.notes,
      tags: [...tags],
    };
  }

  function inputFromItem(item: OrganizerItem, folderId = item.folderId): OrganizerItemInput {
    return {
      title: item.title,
      folderId,
      stageId: item.stageId,
      request: [...item.request],
      response: [...item.response],
      tls: item.tls,
      source: item.source,
      notes: item.notes,
      tags: [...item.tags],
    };
  }

  function scheduleSaveDraft() {
    if (!workspaceInitialized || !draft) return;
    if (saving) {
      saveQueued = true;
      return;
    }
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => { void saveDraft(); }, 450);
  }

  function applyUpdatedItem(updated: OrganizerItem) {
    bundle.items = bundle.items.map((item) => item.id === updated.id ? updated : item);
    if (draft?.id === updated.id) {
      draft = {
        ...draft,
        ...updated,
        request: [...updated.request],
        response: [...updated.response],
        tags: [...updated.tags],
      };
      requestText = decodeHttpText(new Uint8Array(updated.request));
      responseText = decodeHttpText(new Uint8Array(updated.response));
    }
  }

  async function saveDraft() {
    if (!draft || saving) return;
    clearTimeout(saveTimer);
    handleTagInput(true, false);
    saving = true;
    try {
      const updated = await commands.updateOrganizerItem(draft.id, inputFor(draft));
      applyUpdatedItem(updated);
      onStatus("Organizer entry saved");
    } catch (reason) {
      onError(reason);
    } finally {
      saving = false;
      if (saveQueued) {
        saveQueued = false;
        scheduleSaveDraft();
      }
    }
  }

  async function removeItem() {
    if (!draft) return;
    deleteDialog = { kind: "item", id: draft.id, name: draft.title || `${draft.method} ${draft.path}` };
  }

  async function confirmDelete() {
    const target = deleteDialog;
    if (!target) return;
    deleteDialog = null;
    if (target.kind === "folder") {
      const folder = bundle.folders.find((item) => item.id === target.id);
      if (folder) await deleteFolder(folder, true);
      return;
    }
    if (target.kind === "stage") {
      stages = stages.filter((stage) => stage.id !== target.id);
      const affected = bundle.items.filter((item) => item.stageId === target.id);
      try {
        const updatedItems = await Promise.all(affected.map((item) => commands.updateOrganizerItem(item.id, { ...inputFromItem(item), stageId: null })));
        updatedItems.forEach(applyUpdatedItem);
        onStatus("Stage deleted; entries were cleared");
      } catch (reason) {
        onError(reason);
      }
      return;
    }
    try {
      const index = visibleItems.findIndex((item) => item.id === target.id);
      const remaining = visibleItems.filter((item) => item.id !== target.id);
      await commands.deleteOrganizerItem(target.id);
      bundle.items = bundle.items.filter((item) => item.id !== target.id);
      const next = remaining[Math.min(index, remaining.length - 1)];
      if (next) selectItem(next.id);
      else {
        selectedItemId = null;
        draft = null;
        requestText = "";
        responseText = "";
        tagText = "";
      }
      onStatus("Organizer entry deleted");
    } catch (reason) {
      onError(reason);
    }
  }

  function createFolder(parentId: string | null = null) {
    folderDialog = { id: null, parentId, name: "" };
  }

  function renameFolder(folder: OrganizerFolder) {
    folderDialog = { id: folder.id, parentId: folder.parentId, name: folder.name };
  }

  async function submitFolder() {
    if (!folderDialog?.name.trim()) return;
    const editing = Boolean(folderDialog.id);
    try {
      const folder = folderDialog.id
        ? await commands.updateOrganizerFolder(
            folderDialog.id,
            folderDialog.name.trim(),
            folderDialog.parentId,
          )
        : await commands.createOrganizerFolder(
            folderDialog.name.trim(),
            folderDialog.parentId,
          );
      bundle.folders = editing
        ? bundle.folders.map((item) => item.id === folder.id ? folder : item)
        : [...bundle.folders, folder];
      selectedFolderId = folder.id;
      folderDialog = null;
      onStatus(editing ? "Folder renamed" : "Folder created");
    } catch (reason) {
      onError(reason);
    }
  }

  function requestDeleteFolder(folder: OrganizerFolder) {
    deleteDialog = { kind: "folder", id: folder.id, name: folder.name };
  }

  async function deleteFolder(folder: OrganizerFolder, confirmed = false) {
    if (!confirmed) {
      requestDeleteFolder(folder);
      return;
    }
    try {
      const removed = new Set([folder.id, ...folderDescendants(folder.id, bundle.folders)]);
      await commands.deleteOrganizerFolder(folder.id);
      bundle.folders = bundle.folders.filter((item) => !removed.has(item.id));
      bundle.items = bundle.items.map((item) => item.folderId && removed.has(item.folderId)
        ? { ...item, folderId: null }
        : item);
      if (removed.has(String(selectedFolderId))) selectedFolderId = "all";
      if (draft?.folderId && removed.has(draft.folderId)) draft.folderId = null;
      onStatus("Folder deleted; entries moved to Unfiled");
    } catch (reason) {
      onError(reason);
    }
  }

  async function moveItem(itemId: string, folderId: string | null) {
    const item = bundle.items.find((candidate) => candidate.id === itemId);
    if (!item || item.folderId === folderId) return;
    try {
      const updated = await commands.updateOrganizerItem(item.id, {
        ...inputFromItem(item, folderId),
      });
      applyUpdatedItem(updated);
      if (draft?.id === updated.id) selectItem(updated.id);
      onStatus(`Moved to ${folderName(folderId)}`);
    } catch (reason) {
      onError(reason);
    }
  }

  function beginDrag(event: DragEvent, id: string) {
    event.dataTransfer?.setData("text/plain", id);
    if (event.dataTransfer) event.dataTransfer.effectAllowed = "move";
  }

  function acceptDrop(event: DragEvent, folderId: string | null) {
    event.preventDefault();
    const id = event.dataTransfer?.getData("text/plain");
    if (id) void moveItem(id, folderId);
  }

  async function importJson() {
    try {
      const count = await commands.importOrganizerJson();
      if (count === null) return;
      await load();
      onStatus(`Imported ${count} organizer ${count === 1 ? "entry" : "entries"}`);
    } catch (reason) {
      onError(reason);
    }
  }

  async function exportJson() {
    try {
      const path = await commands.exportOrganizerJson();
      if (path) onStatus(`Organizer JSON exported to ${path}`);
    } catch (reason) {
      onError(reason);
    }
  }

  export function handleShortcut(action: string): boolean {
    if (action === "transient.close") {
      if (deleteDialog) {
        deleteDialog = null;
        return true;
      }
      if (stageDialog) {
        stageDialog = null;
        return true;
      }
      if (tagDialog) {
        tagDialog = null;
        return true;
      }
      if (folderDialog) {
        folderDialog = null;
        return true;
      }
      return false;
    }
    if (action === "organizer.selectPrevious" || action === "organizer.selectNext") {
      if (!visibleItems.length) return false;
      const currentIndex = visibleItems.findIndex((item) => item.id === selectedItemId);
      const offset = action.endsWith("Previous") ? -1 : 1;
      const nextIndex = currentIndex < 0
        ? 0
        : Math.max(0, Math.min(visibleItems.length - 1, currentIndex + offset));
      selectItem(visibleItems[nextIndex].id);
      return true;
    }
    if (action === "organizer.openSelected") return Boolean(draft);
    if (action === "organizer.deleteSelected") {
      if (!draft) return false;
      void removeItem();
      return true;
    }
    if (action === "organizer.createFolder") {
      createFolder(null);
      return true;
    }
    if (action === "organizer.export") {
      if (!bundle.items.length) return false;
      void exportJson();
      return true;
    }
    if (action === "organizer.import") {
      void importJson();
      return true;
    }
    if (!draft) return false;
    const request = encodeHttpText(synchronizeHttpContentLength(requestText));
    if (action === "transfer.organizer.replay") {
      onSendReplay(request, draft.tls);
      return true;
    }
    if (action === "transfer.organizer.fuzz") {
      onSendFuzz(request, draft.tls);
      return true;
    }
    if (action === "transfer.organizer.decoder") {
      if (!onSendDecoder) return false;
      onSendDecoder(decodeHttpText(request));
      return true;
    }
    if (action === "transfer.organizer.organizer") {
      void commands.createOrganizerItem(inputFromItem(draft)).then(() => load()).catch(onError);
      return true;
    }
    return false;
  }
</script>

<section class="organizer" aria-label="Organizer">
  <header class="organizer-header">
    <div>
      <span>{bundle.items.length} saved {bundle.items.length === 1 ? "entry" : "entries"}</span>
    </div>
    <div class="organizer-actions">
      <label class="search"><span aria-hidden="true">⌕</span><input bind:value={query} placeholder="Search requests, responses, notes, tags…" aria-label="Search organizer" /></label>
      <select bind:value={sort} aria-label="Sort organizer">
        <option value="updated">Recently updated</option>
        <option value="created">Recently added</option>
        <option value="title">Title</option>
        <option value="host">Host</option>
      </select>
      <button class="text-button" onclick={() => createFolder(null)}>+ Folder</button>
      <button class="text-button" onclick={() => void exportJson()} disabled={!bundle.items.length}>Export JSON</button>
      <button class="text-button" onclick={() => void importJson()}>Import JSON</button>
    </div>
  </header>

  <div class="organizer-body">
    <aside class="folder-pane">
      <div class="pane-label"><span>Folders</span><button data-tooltip="New top-level folder" aria-label="New top-level folder" onclick={() => createFolder(null)}>+</button></div>
      <div class="folder-list">
        <button class:active={selectedFolderId === "all"} onclick={() => (selectedFolderId = "all")}>
          <span class="folder-icon">▤</span><span>All entries</span><small>{bundle.items.length}</small>
        </button>
        <button
          class:active={selectedFolderId === "unfiled"}
          ondragover={(event) => event.preventDefault()}
          ondrop={(event) => acceptDrop(event, null)}
          onclick={() => (selectedFolderId = "unfiled")}
        >
          <span class="folder-icon">◇</span><span>Unfiled</span><small>{bundle.items.filter((item) => !item.folderId).length}</small>
        </button>
        {#each flatFolders as folder (folder.id)}
          <div class="folder-row" style={`--depth:${folder.depth}`}>
            <button
              class:active={selectedFolderId === folder.id}
              ondragover={(event) => event.preventDefault()}
              ondrop={(event) => acceptDrop(event, folder.id)}
              onclick={() => (selectedFolderId = folder.id)}
            >
              <span class="folder-icon">▱</span><span data-tooltip={folder.name}>{folder.name}</span><small>{folderCount(folder.id)}</small>
            </button>
            <div class="folder-buttons">
              {#if folder.depth < 3}<button data-tooltip="New nested folder" aria-label="New nested folder" onclick={() => createFolder(folder.id)}>+</button>{/if}
              <button data-tooltip="Rename folder" aria-label="Rename folder" onclick={() => renameFolder(folder)}>✎</button>
              <button data-tooltip="Delete folder" aria-label="Delete folder" onclick={() => requestDeleteFolder(folder)}>×</button>
            </div>
          </div>
        {/each}
      </div>
      <div class="pane-label tags-label"><span>Tags</span><button data-tooltip="Define project tag" aria-label="Define project tag" onclick={createTag}>+</button></div>
      <div class="tag-list">
        {#each allTags as tag}
          <div class:active={selectedTag === tag} class="tag-list-row">
            <button class="tag-filter" type="button" onclick={() => (selectedTag = selectedTag === tag ? "" : tag)}><span>{tag}</span><small>{bundle.items.filter((item) => item.tags.some((itemTag) => itemTag.toLocaleLowerCase() === tag.toLocaleLowerCase())).length}</small></button>
            <input class="tag-color" type="color" value={tagColor(tag)} aria-label={`Change ${tag} color`} onchange={(event) => updateTagColor(tag, event.currentTarget.value)} />
          </div>
        {:else}
          <p>Tags added to entries appear here.</p>
        {/each}
      </div>
      <div class="pane-label stages-label"><span>Stages</span><button data-tooltip="New stage" aria-label="New stage" onclick={createStage}>+</button></div>
      <div class="stage-list">
        {#each stages as stage (stage.id)}
          <div class="stage-row" role="listitem" draggable="true" ondragstart={(event) => beginStageDrag(event, stage.id)} ondragover={(event) => event.preventDefault()} ondrop={(event) => reorderStage(event, stage.id)}>
            <span class="stage-drag" aria-hidden="true">⋮⋮</span>
            <span class="stage-dot" style={`background:${stage.color}`} aria-hidden="true"></span>
            <span class="stage-name" data-tooltip={stage.name}>{stage.name}</span>
            <small>{bundle.items.filter((item) => item.stageId === stage.id).length}</small>
            <button class="stage-edit" type="button" data-tooltip={`Edit ${stage.name}`} aria-label={`Edit ${stage.name}`} onclick={() => editStage(stage)}>✎</button>
            <button class="stage-edit stage-delete" type="button" data-tooltip={`Delete ${stage.name}`} aria-label={`Delete ${stage.name}`} onclick={() => requestDeleteStage(stage)}>×</button>
          </div>
        {:else}
          <p>Stages can organize progress.</p>
        {/each}
      </div>
    </aside>

    <section class="item-pane" aria-label="Saved entries">
      <div class="pane-label">
        <span>{selectedTag ? `#${selectedTag}` : selectedFolderId === "all" ? "All entries" : selectedFolderId === "unfiled" ? "Unfiled" : folderName(selectedFolderId)}</span>
        <small>{visibleItems.length}</small>
      </div>
      <div class="item-list">
        {#each visibleItems as item (item.id)}
          <button
            class:active={selectedItemId === item.id}
            draggable="true"
            ondragstart={(event) => beginDrag(event, item.id)}
            onfocus={() => selectItem(item.id)}
            onclick={() => selectItem(item.id)}
          >
            <span class={`method method-${item.method.toLocaleLowerCase()}`}>{item.method}</span>
            <span class="item-main">
              <strong data-tooltip={item.title}>{item.title || "Untitled request"}</strong>
              <small data-tooltip={`${item.host}${item.path}`}>{item.host || "Unknown host"}{item.path}</small>
              <span class="item-labels">
                {#if item.stageId && stageFor(item.stageId)}<i class="item-stage" style={`--stage-color:${stageFor(item.stageId)?.color}`}>{stageFor(item.stageId)?.name}</i>{/if}
                {#if item.tags.length}<span class="item-tags">{#each item.tags.slice(0, 3) as tag}<i style={`--tag-color:${tagColor(tag)}`}>{tag}</i>{/each}</span>{/if}
              </span>
            </span>
            <span class="item-meta">
              {#if item.status}<strong class:error={item.status >= 400}>{item.status}</strong>{/if}
              <small>{formatDate(item.updatedAt)}</small>
            </span>
          </button>
        {:else}
          <div class="empty-list">
            <span aria-hidden="true">▤</span>
            <strong>{loading ? "Loading organizer…" : "No saved entries here"}</strong>
            <small>Use the disk button beside a request to save a snapshot.</small>
          </div>
        {/each}
      </div>
    </section>

    <section class:empty={!draft} class="detail-pane" aria-label="Organizer entry editor">
      {#if draft}
        <div class="detail-toolbar">
          <div class="entry-title-row">
            <span class={`method method-${draft.method.toLocaleLowerCase()}`}>{draft.method || "REQUEST"}</span>
            <input class="title-input" bind:value={draft.title} oninput={scheduleSaveDraft} aria-label="Entry title" placeholder="Optional title" />
            {#if draft.stageId && stageFor(draft.stageId)}<span class="detail-stage" style={`--stage-color:${stageFor(draft.stageId)?.color}`}>{stageFor(draft.stageId)?.name}</span>{/if}
          </div>
          <label><span>Folder</span>
            <select bind:value={draft.folderId} onchange={scheduleSaveDraft}>
              <option value={null}>Unfiled</option>
              {#each flatFolders as folder}<option value={folder.id}>{"\u00a0\u00a0".repeat(folder.depth)}{folder.name}</option>{/each}
            </select>
          </label>
          <label><span>Target</span>
            <select bind:value={draft.tls} onchange={scheduleSaveDraft}>
              <option value={true}>HTTPS</option><option value={false}>HTTP</option>
            </select>
          </label>
          <select class="stage-select" bind:value={draft.stageId} onchange={scheduleSaveDraft} aria-label="Entry stage">
            <option value={null}>No stage</option>
            {#each stages as stage}<option value={stage.id}>{stage.name}</option>{/each}
          </select>
          <button class="organizer-icon-action" type="button" aria-label="Send to Replay" data-tooltip="To Replay" onclick={() => onSendReplay(encodeHttpText(synchronizeHttpContentLength(requestText)), draft?.tls ?? true)}>
            <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 5h11v14H4zM8 9h10M14 5l4 4-4 4M7 13h5"/></svg>
          </button>
          <button class="organizer-icon-action" type="button" aria-label="Send to Fuzz" data-tooltip="To Fuzz" onclick={() => onSendFuzz(encodeHttpText(synchronizeHttpContentLength(requestText)), draft?.tls ?? true)}>
            <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="6"/><path d="M12 3v3M12 18v3M3 12h3M18 12h3M12 10v4M10 12h4"/></svg>
          </button>
          <button class="organizer-icon-action delete-button" type="button" aria-label="Delete entry" data-tooltip="Delete entry" onclick={removeItem}>
            <RecycleBinIcon size={16} />
          </button>
        </div>
        <div class="detail-meta">
          <label class="tag-editor"><span>Tags</span>
            <div class="tag-input-wrap">
              <div class="tag-pills">
                {#each draft.tags as tag (tag.toLocaleLowerCase())}
                  <button class="tag-pill" type="button" style={`--tag-color:${tagColor(tag)}`} onclick={() => removeTag(tag)} data-tooltip={`Remove ${tag}`}><span>{tag}</span><b aria-hidden="true">×</b></button>
                {/each}
                <input list="organizer-tag-options" bind:value={tagText} oninput={() => handleTagInput()} onblur={() => handleTagInput(true)} placeholder={draft.tags.length ? "Add tag…" : "auth, checkout, regression"} aria-label="Add tags" />
              </div>
              {#if allTags.length}<select class="tag-option-select" aria-label="Choose project tag" onchange={(event) => { const value = event.currentTarget.value; if (value) { addCommittedTags([value]); event.currentTarget.value = ""; } }}><option value="">Choose existing tag…</option>{#each allTags as tag}<option value={tag}>{tag}</option>{/each}</select>{/if}
              <datalist id="organizer-tag-options">{#each allTags as tag}<option value={`${tag},`}>{tag}</option>{/each}</datalist>
            </div>
          </label>
          <label class="notes"><span>Notes</span><textarea bind:value={draft.notes} oninput={scheduleSaveDraft} placeholder="Why this request matters, observations, follow-up ideas…"></textarea></label>
        </div>
        <div class="message-grid">
          <MessageViewer
            title="Request"
            kind="request"
            raw={new Uint8Array(draft.request)}
            metadata={draft.host}
            editable
            onTextChange={(value) => { requestText = value; draft = draft ? { ...draft, request: Array.from(encodeHttpText(value)) } : draft; scheduleSaveDraft(); }}
            onSendReplay={(raw) => onSendReplay(raw, draft?.tls ?? true)}
            onSendFuzz={(raw) => onSendFuzz(raw, draft?.tls ?? true)}
            onSendDecoder={onSendDecoder}
          />
          <MessageViewer
            title="Response"
            kind="response"
            raw={new Uint8Array(draft.response)}
            metadata={draft.status ? `HTTP ${draft.status}` : "No response saved"}
            editable
            onTextChange={(value) => { responseText = value; draft = draft ? { ...draft, response: Array.from(encodeHttpText(value)) } : draft; scheduleSaveDraft(); }}
            onSendDecoder={onSendDecoder}
          />
        </div>
      {:else}
        <div class="empty-detail">
          <span aria-hidden="true">▤</span>
          <strong>Select a saved entry</strong>
          <small>Edit the request, response, notes, tags, title, stage, and folder here.</small>
        </div>
      {/if}
    </section>
  </div>
</section>

{#if folderDialog}
  <div class="organizer-modal-backdrop" role="presentation" onclick={(event) => { if (event.target === event.currentTarget) folderDialog = null; }}>
    <form class="folder-dialog" onsubmit={(event) => { event.preventDefault(); void submitFolder(); }}>
      <p>{folderDialog.id ? "EDIT FOLDER" : "NEW FOLDER"}</p>
      <h2>{folderDialog.id ? "Rename folder" : folderDialog.parentId ? "Create nested folder" : "Create folder"}</h2>
      {#if folderDialog.parentId}<span>Inside {folderName(folderDialog.parentId)}</span>{/if}
      <label>
        <span>Folder name</span>
        <input bind:value={folderDialog.name} maxlength="120" placeholder="e.g. Authentication" />
      </label>
      <div>
        <button class="text-button" type="button" onclick={() => (folderDialog = null)}>Cancel</button>
        <button class="text-button confirm-folder" type="submit" disabled={!folderDialog.name.trim()}>{folderDialog.id ? "Save" : "Create folder"}</button>
      </div>
    </form>
  </div>
{/if}

{#if tagDialog}
  <div class="organizer-modal-backdrop" role="presentation" onclick={(event) => { if (event.target === event.currentTarget) tagDialog = null; }}>
    <form class="folder-dialog stage-dialog" onsubmit={(event) => { event.preventDefault(); submitTag(); }}>
      <p>PROJECT TAG</p>
      <h2>Define tag</h2>
      <label><span>Tag name</span><input bind:value={tagDialog.name} maxlength="60" placeholder="e.g. authentication" /></label>
      <label><span>Tag color</span><input class="stage-color-input" type="color" bind:value={tagDialog.color} /></label>
      <div><button class="text-button" type="button" onclick={() => (tagDialog = null)}>Cancel</button><button class="text-button confirm-folder" type="submit" disabled={!tagDialog.name.trim()}>Add tag</button></div>
    </form>
  </div>
{/if}

{#if stageDialog}
  <div class="organizer-modal-backdrop" role="presentation" onclick={(event) => { if (event.target === event.currentTarget) stageDialog = null; }}>
    <form class="folder-dialog stage-dialog" onsubmit={(event) => { event.preventDefault(); submitStage(); }}>
      <p>{stageDialog.id ? "EDIT STAGE" : "NEW STAGE"}</p>
      <h2>{stageDialog.id ? "Edit stage" : "Create stage"}</h2>
      <label><span>Stage name</span><input bind:value={stageDialog.name} maxlength="80" placeholder="e.g. Needs review" /></label>
      <label><span>Stage color</span><input class="stage-color-input" type="color" bind:value={stageDialog.color} /></label>
      <div><button class="text-button" type="button" onclick={() => (stageDialog = null)}>Cancel</button><button class="text-button confirm-folder" type="submit" disabled={!stageDialog.name.trim()}>{stageDialog.id ? "Save" : "Create stage"}</button></div>
    </form>
  </div>
{/if}

{#if deleteDialog}
  <div class="organizer-modal-backdrop" role="presentation" onclick={(event) => { if (event.target === event.currentTarget) deleteDialog = null; }}>
    <div class="folder-dialog delete-dialog" role="dialog" aria-modal="true" aria-labelledby="organizer-delete-title">
      <p>CONFIRM DELETE</p>
      <h2 id="organizer-delete-title">Delete {deleteDialog.kind === "folder" ? "folder" : deleteDialog.kind === "stage" ? "stage" : "entry"}?</h2>
      <span>{deleteDialog.kind === "folder" ? `“${deleteDialog.name}” and its nested folders will be removed. Saved entries will move to Unfiled.` : deleteDialog.kind === "stage" ? `“${deleteDialog.name}” will be removed and cleared from saved entries.` : `“${deleteDialog.name}” will be permanently removed from this project.`}</span>
      <div><button class="text-button" type="button" onclick={() => (deleteDialog = null)}>Cancel</button><button class="text-button delete-confirm" type="button" onclick={() => void confirmDelete()}>Delete</button></div>
    </div>
  </div>
{/if}

<style>
  .organizer { display: grid; grid-template-rows: 48px minmax(0, 1fr); height: 100%; min-height: 0; padding: 4px; color: var(--text); overflow: hidden; }
  .organizer-header { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 0 8px 0 11px; border: 1px solid var(--border); border-radius: 3px 3px 0 0; background: var(--surface); }
  .organizer-header > div:first-child { display: flex; align-items: baseline; gap: 9px; min-width: 0; }
  .organizer-header span { color: var(--muted); font-size: var(--font-size-compact); }
  .organizer-actions { display: flex; align-items: center; gap: 4px; min-width: 0; }
  .organizer button, .organizer select, .organizer input, .organizer textarea { font: inherit; }
  .organizer button, .organizer select { min-height: 27px; border: 1px solid var(--border-strong); border-radius: 3px; color: var(--text); background: var(--surface-2); cursor: pointer; }
  .organizer button { padding: 0 8px; }
  .organizer button:disabled { opacity: .45; cursor: default; }
  .organizer button:hover:not(:disabled) { border-color: var(--accent); background: var(--surface-3); }
  .organizer input, .organizer textarea { min-width: 0; color: var(--text); border: 1px solid var(--border-strong); border-radius: 3px; background: var(--input); }
  .search { position: relative; width: min(330px, 25vw); }
  .search span { position: absolute; left: 8px; top: 6px; font-size: var(--font-size-body); }
  .search input { width: 100%; height: 29px; padding: 0 8px 0 26px; font-size: var(--font-size-compact); }
  .organizer-actions select { height: 29px; padding: 0 6px; font-size: var(--font-size-compact); }
  .organizer-body { display: grid; grid-template-columns: 215px 330px minmax(420px, 1fr); min-height: 0; border: 1px solid var(--border); border-top: 0; background: var(--bg); overflow: hidden; }
  .folder-pane, .item-pane, .detail-pane { min-width: 0; min-height: 0; background: var(--surface); }
  .folder-pane, .item-pane { border-right: 1px solid var(--border); }
  .folder-pane { display: grid; grid-template-rows: 31px minmax(80px, 1fr) 31px minmax(70px, .45fr) 31px minmax(70px, .45fr); }
  .item-pane { display: grid; grid-template-rows: 31px minmax(0, 1fr); }
  .pane-label { display: flex; align-items: center; justify-content: space-between; min-width: 0; padding: 0 7px 0 9px; color: var(--muted); border-bottom: 1px solid var(--border); background: var(--surface-2); font-size: var(--font-size-compact); font-weight: 750; text-transform: uppercase; letter-spacing: .08em; }
  .pane-label button { width: 23px; min-height: 22px; padding: 0; }
  .pane-label small { color: var(--muted); }
  .folder-list, .tag-list, .item-list { min-height: 0; overflow: auto; }
  .folder-list > button, .folder-row > button, .tag-list button { display: grid; align-items: center; width: 100%; height: 29px; min-height: 29px; padding: 0 7px; border: 0; border-radius: 0; color: var(--muted); background: transparent; text-align: left; }
  .folder-list > button, .folder-row > button { grid-template-columns: 18px minmax(0, 1fr) auto; }
  .folder-list button.active { color: var(--text); background: var(--accent-soft); box-shadow: inset 2px 0 var(--accent); }
  .folder-list button span:nth-child(2), .folder-row button span:nth-child(2) { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .folder-list small, .tag-list small { color: var(--muted); font-size: var(--font-size-compact); }
  .folder-icon { color: var(--accent); }
  .folder-row { position: relative; padding-left: calc(var(--depth) * 13px); }
  .folder-row:hover .folder-buttons { display: flex; }
  .folder-buttons { position: absolute; top: 3px; right: 22px; display: none; gap: 1px; background: var(--surface-2); }
  .folder-buttons button { width: 22px; min-height: 22px; padding: 0; border: 0; }
  .tags-label { border-top: 1px solid var(--border); }
  .tag-list-row { display: grid; grid-template-columns: minmax(0, 1fr) 25px; align-items: center; min-height: 27px; }
  .tag-list-row.active { background: var(--accent-soft); box-shadow: inset 2px 0 var(--accent); }
  .tag-filter { display: grid !important; grid-template-columns: minmax(0, 1fr) auto; align-items: center; width: 100%; height: 27px; min-height: 27px; padding: 0 7px !important; border: 0 !important; border-radius: 0 !important; color: var(--muted) !important; background: transparent !important; text-align: left; }
  .tag-list-row.active .tag-filter { color: var(--text) !important; }
  .tag-filter span { overflow: hidden; color: inherit; text-overflow: ellipsis; white-space: nowrap; }
  .tag-color { width: 12px; height: 12px; padding: 0; border: 0 !important; border-radius: 50% !important; background: transparent !important; cursor: pointer; }
  .tag-list p { margin: 12px; color: var(--muted); font-size: var(--font-size-compact); }
  .item-list > button { display: grid; grid-template-columns: 48px minmax(0, 1fr) 54px; gap: 7px; width: 100%; min-height: 70px; padding: 7px 8px; border: 0; border-bottom: 1px solid var(--border); border-radius: 0; text-align: left; background: transparent; }
  .item-list > button:hover, .item-list > button.active { background: var(--accent-soft); }
  .item-list > button.active { box-shadow: inset 3px 0 var(--accent); }
  .method { align-self: start; width: fit-content; padding: 2px 5px; color: var(--muted); border: 1px solid var(--border); border-radius: 3px; font-size: var(--font-size-compact); font-weight: 800; }
  .method-get { color: #23b889; } .method-post { color: #5794ef; } .method-delete { color: var(--danger); }
  .item-main, .item-meta { display: grid; align-content: start; min-width: 0; gap: 3px; }
  .item-main strong, .item-main small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .item-main strong { font-size: var(--font-size-compact); }
  .item-main small, .item-meta small { color: var(--muted); font-size: var(--font-size-compact); }
  .item-labels { display: flex; align-items: center; gap: 4px; min-width: 0; overflow: hidden; }
  .item-tags { display: flex; gap: 3px; overflow: hidden; }
  .item-tags i, .item-stage { padding: 1px 5px; color: var(--tag-color, var(--accent)); border-radius: 8px; background: color-mix(in srgb, var(--tag-color, var(--accent)) 15%, transparent); font-size: var(--font-size-compact); font-style: normal; white-space: nowrap; }
  .item-stage { color: var(--stage-color, var(--accent)); background: color-mix(in srgb, var(--stage-color, var(--accent)) 15%, transparent); }
  .item-meta { justify-items: end; }
  .item-meta strong { color: var(--success); font-size: var(--font-size-compact); } .item-meta strong.error { color: var(--danger); }
  .empty-list, .empty-detail { display: grid; place-content: center; justify-items: center; gap: 5px; height: 100%; padding: 20px; color: var(--muted); text-align: center; font-size: var(--font-size-compact); }
  .empty-list > span, .empty-detail > span { color: var(--accent); font-size: var(--font-size-title); }
  .empty-list strong, .empty-detail strong { color: var(--text); font-size: var(--font-size-compact); }
  .detail-pane { display: grid; grid-template-rows: auto auto minmax(0, 1fr); overflow: hidden; }
  .detail-pane.empty { grid-template-rows: minmax(0, 1fr); }
  .detail-toolbar { display: flex; align-items: center; gap: 4px; min-height: 37px; padding: 4px; border-bottom: 1px solid var(--border); background: var(--surface-2); }
  .entry-title-row { display: flex; align-items: center; flex: 1; min-width: 0; gap: 5px; }
  .title-input { flex: 1; min-width: 0; height: 29px; padding: 0 4px; border: 0 !important; border-bottom: 1px solid var(--border-strong) !important; border-radius: 0 !important; background: transparent !important; font-weight: 650; }
  .detail-toolbar label { display: flex; align-items: center; gap: 4px; color: var(--muted); font-size: var(--font-size-compact); }
  .detail-toolbar select { max-width: 125px; height: 29px; padding: 0 5px; font-size: var(--font-size-compact); }
  .detail-toolbar button { white-space: nowrap; font-size: var(--font-size-compact); }
  .stage-select { max-width: 125px; height: 29px; padding: 0 5px; font-size: var(--font-size-compact); }
  .detail-stage { padding: 2px 6px; color: var(--stage-color, var(--accent)); border-radius: 8px; background: color-mix(in srgb, var(--stage-color, var(--accent)) 15%, transparent); font-size: var(--font-size-compact); white-space: nowrap; }
  .organizer-icon-action { position: relative; display: grid; width: 30px; min-height: 29px; place-items: center; padding: 0 !important; }
  .organizer-icon-action svg { width: 16px; height: 16px; fill: none; stroke: currentColor; stroke-linecap: round; stroke-linejoin: round; stroke-width: var(--svgbuttonstrokewidth, 1.5); }
  .delete-button { color: var(--danger) !important; }
  .detail-meta { display: grid; grid-template-columns: minmax(180px, .7fr) minmax(260px, 1.3fr); gap: 4px; padding: 4px; border-bottom: 1px solid var(--border); }
  .detail-meta label { display: grid; grid-template-columns: 38px minmax(0, 1fr); align-items: center; gap: 5px; color: var(--muted); font-size: var(--font-size-compact); }
  .tag-editor { align-items: start !important; }
  .tag-input-wrap { min-width: 0; min-height: 29px; border: 1px solid var(--border-strong); border-radius: 3px; background: var(--input); }
  .tag-pills { display: flex; align-items: center; flex-wrap: wrap; gap: 3px; min-height: 27px; padding: 2px 5px; }
  .tag-pills > input { flex: 1; min-width: 80px; height: 23px; padding: 0 2px; border: 0 !important; background: transparent !important; }
  .tag-option-select { width: 100%; height: 24px; min-height: 24px !important; padding: 0 5px !important; border: 0 !important; border-top: 1px solid var(--border) !important; border-radius: 0 !important; background: transparent !important; color: var(--muted) !important; font-size: var(--font-size-compact); }
  .tag-pill { display: inline-flex !important; align-items: center; gap: 4px; min-height: 21px !important; padding: 1px 5px !important; color: var(--tag-color, var(--accent)) !important; border: 0 !important; border-radius: 999px !important; background: color-mix(in srgb, var(--tag-color, var(--accent)) 15%, transparent) !important; font-size: var(--font-size-compact); }
  .tag-pill b { font-size: 13px; font-weight: 500; line-height: 1; }
  .stages-label { border-top: 1px solid var(--border); }
  .stage-list { min-height: 0; overflow: auto; }
  .stage-list > p { margin: 12px; color: var(--muted); font-size: var(--font-size-compact); }
  .stage-row { display: grid; grid-template-columns: 13px 10px minmax(0, 1fr) auto 22px 22px; align-items: center; gap: 5px; min-height: 28px; padding: 0 7px; color: var(--muted); font-size: var(--font-size-compact); }
  .stage-row:hover { background: var(--surface-2); }
  .stage-drag { color: var(--border-strong); cursor: grab; letter-spacing: -3px; }
  .stage-dot { width: 8px; height: 8px; border-radius: 50%; }
  .stage-name { overflow: hidden; color: var(--text); text-overflow: ellipsis; white-space: nowrap; }
  .stage-row small { color: var(--muted); }
  .stage-edit { width: 22px !important; min-height: 22px !important; padding: 0 !important; border: 0 !important; background: transparent !important; }
  .stage-delete { color: var(--danger) !important; }
  .detail-meta input { height: 29px; padding: 0 7px; font-size: var(--font-size-compact); }
  .detail-meta textarea { height: 42px; padding: 5px 7px; resize: vertical; font-size: var(--font-size-compact); }
  .message-grid { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); gap: 4px; min-height: 0; padding: 4px; background: var(--bg); overflow: hidden; }
  .message-grid :global(.message-viewer) { min-width: 0; min-height: 0; }
  .organizer-modal-backdrop { position: fixed; z-index: 60; inset: 0; display: grid; place-items: center; background: color-mix(in srgb, var(--bg) 70%, transparent); backdrop-filter: blur(3px); }
  .folder-dialog { display: grid; gap: 10px; width: min(380px, calc(100vw - 32px)); padding: 20px; color: var(--text); border: 1px solid var(--border-strong); border-radius: 8px; background: var(--surface); box-shadow: 0 20px 55px #0008; }
  .folder-dialog p { margin: 0; color: var(--accent); font-size: var(--font-size-compact); font-weight: 800; letter-spacing: .13em; }
  .folder-dialog h2 { margin: -3px 0 0; font-size: var(--font-size-heading); }
  .folder-dialog > span { margin-top: -5px; color: var(--muted); font-size: var(--font-size-compact); }
  .folder-dialog label { display: grid; gap: 5px; color: var(--muted); font-size: var(--font-size-compact); }
  .folder-dialog input { width: 100%; height: 34px; padding: 0 9px; border: 1px solid var(--border-strong) !important; border-radius: 3px !important; box-shadow: none !important; background: var(--input) !important; }
  .folder-dialog input:focus { border-color: var(--accent) !important; box-shadow: none !important; }
  .folder-dialog > div { display: flex; justify-content: flex-end; gap: 5px; margin-top: 4px; }
  .folder-dialog .confirm-folder { color: var(--accent-contrast); border-color: var(--accent); background: var(--accent); font-weight: 700; }
  .stage-color-input { width: 38px; height: 29px; padding: 2px; }
  .delete-dialog > span { color: var(--muted); line-height: 1.45; }
  .delete-confirm { color: #fff !important; border-color: var(--danger) !important; background: var(--danger) !important; }
  @media (max-width: 1100px) {
    .organizer-body { grid-template-columns: 185px 280px minmax(360px, 1fr); }
    .detail-toolbar label span { display: none; }
    .detail-toolbar button { padding: 0 5px; }
  }
</style>
