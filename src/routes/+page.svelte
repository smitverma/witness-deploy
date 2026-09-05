<script lang="ts">
  import "$lib/tooltip.css";
  import { onMount, untrack } from "svelte";
  import { currentMonitor, getCurrentWindow, LogicalSize, PhysicalPosition } from "@tauri-apps/api/window";
  import { commands, isTauri, onWitnessEvent } from "$lib/api";
  import FilterBar from "$lib/components/FilterBar.svelte";
  import HistoryTable from "$lib/components/HistoryTable.svelte";
  import InterceptTable from "$lib/components/InterceptTable.svelte";
  import Fuzz from "$lib/components/Intruder.svelte";
  import ProjectLauncher from "$lib/components/ProjectLauncher.svelte";
  import MessageViewer from "$lib/components/MessageViewer.svelte";
  import Decoder from "$lib/components/Decoder.svelte";
  import DuplicateButton from "$lib/components/DuplicateButton.svelte";
  import Comparer from "$lib/components/Comparer.svelte";
  import ScopeManager from "$lib/components/ScopeManager.svelte";
  import SiteMap from "$lib/components/SiteMap.svelte";
  import SettingsPanel from "$lib/components/SettingsPanel.svelte";
  import AiSettings from "$lib/components/AiSettings.svelte";
  import ErrorToast from "$lib/components/ErrorToast.svelte";
  import InfoToast from "$lib/components/InfoToast.svelte";
  import ContextMenu, { type ContextMenuItem } from "$lib/components/ContextMenu.svelte";
  import LogViewer from "$lib/components/LogViewer.svelte";
  import Organizer from "$lib/components/Organizer.svelte";
  import IdentityPlus from "$lib/components/IdentityPlus.svelte";
  import AiController from "$lib/components/AiController.svelte";
  import ConfirmDialog from "$lib/components/ConfirmDialog.svelte";
  import Toggle from "$lib/components/Toggle.svelte";
  import { showErrorToast } from "$lib/errorToast";
  import { showInfoToast } from "$lib/infoToast";
  import { curlToHttpRequest } from "$lib/curl-import";
  import { buildSiteMap, flattenSiteMap, isHostInScope } from "$lib/siteMap";
  import { clonePayloadWarehouse, createIntruderTab, createPayloadWarehouse, findTestPositions, generatePayloads, planPayloadRows, processPayloads } from "$lib/intruder";
  import { forgeTools } from "$lib/forge-tools";
  import {
    SHORTCUTS,
    detectShortcutPlatform,
    formatShortcut,
    formatShortcutParts,
    isEditableTarget,
    normalizeShortcutModifier,
    resolveShortcut,
    type ShortcutDefinition,
    type ShortcutPlatform,
  } from "$lib/keyboard-shortcuts";
  import { decodeHttpText, encodeHttpText, finalizeHttpRequestBytes, HTTP_METADATA_LIMIT, normalizeHttpLineEndingBytes, requestHeaderPrefix, splitHttpMessage, synchronizeHttpContentLength } from "$lib/http-message";
  import { formatClock as formatClockLib } from "$lib/format";
  import { highlightSearchText, type SearchTextPart } from "$lib/text-search";
  import { buildTabBarEntries as buildTabBarEntriesLib, type GroupableTab, type TabBarEntry } from "$lib/tab-groups";
  import {
    parseHostname as parseHostnameLib,
    parseRequestMetadata as parseRequestMetadataLib,
    parseRequestMetadataText as parseRequestMetadataTextLib,
    requestHost as requestHostLib,
  } from "$lib/http-url";
  import { startTutorial } from "$lib/tutorial";
  import {
    checkForUpdate,
    closeUpdate,
    getUpdateMode,
    installUpdateAndRelaunch,
  } from "$lib/updater";
  import type { Update } from "@tauri-apps/plugin-updater";
  import type { WitnessEvent, AppSnapshot, ComparerWorkspaceState, DecoderWorkspaceState, ForgeChatSnapshot, ForgeMessageSnapshot, ForgeWorkspaceState, HistoryDetail, HistoryEntry, HistoryFilter, Identity, IdentityBundle, IdentityGroup, IdentityGroupInput, IdentityInput, IdentityWorkspaceState, InterceptEntry, InterceptionRule, InterceptionRuleMatchType, InterceptionRuleRelationship, IntruderMode, IntruderResult, IntruderScan, IntruderState, IntruderStateSnapshot, IntruderTab, IntruderTabSnapshot, IntruderWorkspaceTab, IntruderWorkspaceTabSnapshot, MatchReplaceRule, MatchReplaceRuleType, OrganizerBundle, OrganizerItem, OrganizerItemInput, OrganizerWorkspaceState, PayloadProcessingRule, PayloadProcessingRuleType, PayloadWarehouse, RecentProject, ReplayTabSnapshot, ScopeEntry, SettingsPatch, SettingsSection, SiteMapWorkspaceState, TabGroup, WorkspaceSnapshot } from "$lib/types";

  type ConfirmationRequest = {
    title: string;
    message: string;
    confirmLabel?: string;
    danger?: boolean;
    resolve: (accepted: boolean) => void;
  };
  type Tab = "Proxy" | "History" | "Site Map" | "Replay" | "Fuzz" | "Organizer" | "ID+" | "Decoder" | "Comparer" | "Scope" | "AI" | "Logs" | "Settings";
  type ShortcutController = { handleShortcut: (action: string) => boolean | Promise<boolean> };
  type TabWorkspace = "Replay" | "Fuzz";
  type TabContextMenuState = { x: number; y: number; workspace: TabWorkspace; tabId: number };
  type TabGroupContextMenuState = { x: number; y: number; workspace: TabWorkspace; groupId: string };
  type TabGroupDialogState =
    | { mode: "create"; workspace: TabWorkspace; tabId: number }
    | { mode: "edit"; workspace: TabWorkspace; groupId: string };
  type TabRenameDialogState = { workspace: TabWorkspace; tabId: number };
  type ClosedTabEntry =
    | { id: number; workspace: "Replay"; tab: ReplayTabSnapshot }
    | { id: number; workspace: "Fuzz"; tab: IntruderTabSnapshot };
  // GroupableTab / TabBarEntry now live in $lib/tab-groups (imported above for use below).

  const primaryTabs: Tab[] = ["Proxy", "History", "Site Map", "Replay", "Fuzz", "Organizer", "ID+", "Decoder", "Comparer", "Scope", "AI"];
  const tabLabel = (tab: Tab) => tab === "AI" ? "Forge" : tab;
  const shortcutPlatform: ShortcutPlatform = detectShortcutPlatform();
  const tabsWithLogs: Tab[] = [...primaryTabs, "Logs"];
  let activeTab = $state<Tab>("Proxy");
  let decoderInput = $state("");
  let snapshot = $state<AppSnapshot | null>(null);
  const tabs: Tab[] = $derived(snapshot?.settings.showLogsTab ? tabsWithLogs : primaryTabs);
  let previewTheme = $state<"dark" | "light">("dark");
  const currentTheme = $derived<"dark" | "light">("dark");
  const activeShortcutModifier = $derived(normalizeShortcutModifier(snapshot?.settings.shortcutModifier, shortcutPlatform));
  const globalShortcutDefinitions = SHORTCUTS.filter((definition) => definition.scope === "global");
  const activeTabShortcutDefinitions = $derived.by(() =>
    SHORTCUTS.filter((definition) => definition.scope === activeTab),
  );
  const shortcutColumnCount = $derived.by(() => {
    const count = activeTabShortcutDefinitions.length;
    return count > 8 ? 3 : count > 4 ? 2 : 1;
  });
  const windowTitle = $derived(
    snapshot?.project.currentProjectPath
      ? `Witness - ${snapshot.project.temporary ? "Temporary Session" : snapshot.project.name ?? snapshot.project.archivePath ?? snapshot.project.currentProjectPath}`
      : "Witness - Web Security Testing",
  );
  $effect(() => {
    if (isTauri()) void getCurrentWindow().setTitle(windowTitle).catch(showError);
  });
  $effect(() => {
    const interfaceSize = snapshot?.settings.fontSize ?? 14;
    const editorSize = snapshot?.settings.messageEditorFontSize ?? 12;
    const root = document.documentElement;
    root.style.setProperty("--font-size-compact", `${Math.max(8, interfaceSize - 4)}px`);
    root.style.setProperty("--font-size-body", `${Math.max(10, interfaceSize - 2)}px`);
    root.style.setProperty("--font-size-heading", `${interfaceSize}px`);
    root.style.setProperty("--font-size-title", `${interfaceSize + 4}px`);
    root.style.setProperty("--font-size-editor", `${editorSize}px`);
  });
  $effect(() => {
    if (snapshot?.settings.theme === "light") {
      snapshot.settings.theme = "dark";
      previewTheme = "dark";
      void commands.updateSettings({ theme: "dark" }).catch(() => {});
    }
  });
  let busy = $state(false);
  let statusMessage = $state("Desktop bridge is starting…");
  let eventCount = $state(0);
  let startupEvents: WitnessEvent[] = [];
  let history = $state<HistoryEntry[]>([]);
  let historyDetail = $state<HistoryDetail | null>(null);
  let historyInspectorsVisible = $state(true);
  let historyLoading = $state(false);
  let historyHasMore = $state(true);
  let historyFilter = $state<HistoryFilter>({ sortBy: "timestamp", sortDescending: true, inScopeOnly: false });
  let historyStale = $state(true);
  let historyCacheKey = $state("");
  let filterTimer: ReturnType<typeof setTimeout> | undefined;

  function currentHistoryCacheKey(): string {
    return `${snapshot?.project.currentProjectPath ?? ""}|${JSON.stringify(historyFilter)}`;
  }
  let contextMenu = $state<{ x: number; y: number; entry: HistoryEntry } | null>(null);
  let tabContextMenu = $state<TabContextMenuState | null>(null);
  let tabGroupContextMenu = $state<TabGroupContextMenuState | null>(null);
  let tabRenameDialog = $state<TabRenameDialogState | null>(null);
  let tabRenameValue = $state("");
  let tabGroupDialog = $state<TabGroupDialogState | null>(null);
  let tabGroupName = $state("");
  let tabGroupColor = $state("#ffa500");
  let tabGroupError = $state("");
  let tabGroupSelectedTabIds = $state<number[]>([]);
  const MAX_CLOSED_TABS = 5;
  let closedTabs = $state<ClosedTabEntry[]>([]);
  let nextClosedTabEntryId = 1;
  const tabGroupColors = [
    "#665cff", "#e83e68", "#d89b08", "#3b9bd6", "#2b879d", "#35a9b3", "#b95b9d",
    "#8066c5", "#4856a6", "#b84a4a", "#4b8d42", "#177f9b", "#597b91", "#9c4b83",
  ];
  let tabGroups = $state<TabGroup[]>([]);
  const tabContextMenuItems = $derived.by((): ContextMenuItem[] => [
    { id: "rename-tab", label: "Rename tab" },
    { id: "tab-menu-divider-rename", separator: true },
    { id: "close-tab", label: "Close tab" },
    {
      id: "close-tabs-to",
      label: "Close tabs to",
      submenu: [
        { id: "close-tabs-left", label: "Close tabs to the left" },
        { id: "close-tabs-right", label: "Close tabs to the right" },
      ],
    },
    {
      id: "restore-closed-tabs",
      label: "Restore closed tab",
      disabled: !closedTabs.length,
      submenu: closedTabs.map((entry) => ({
        id: `restore-closed-tab:${entry.id}`,
        label: entry.tab.title,
      })),
    },
    { id: "tab-menu-divider-close", separator: true },
    {
      id: "add-tab-to-group",
      label: "Add tab to group",
      submenu: [
        { id: "remove-tab-from-group", label: "No group" },
        ...tabGroups.map((group) => ({ id: "tab-group:" + group.id, label: group.name, markerColor: group.color })),
        { id: "tab-group-separator", separator: true },
        { id: "create-tab-group", label: "Create new tab group…" },
      ],
    },
  ]);
  const historyContextMenuItems: ContextMenuItem[] = [
    { id: "send-replay", label: "Send to Replay" },
    { id: "send-fuzz", label: "Send to Fuzz" },
    { id: "save-organizer", label: "Save to Organizer" },
    { id: "compare-request", label: "Compare request" },
    { id: "compare-response", label: "Compare response" },
    { id: "copy-url", label: "Copy URL" },
    { id: "copy-curl", label: "Copy as cURL" },
    { id: "delete-entry", label: "Delete entry", danger: true },
  ];
  const tabGroupContextMenuItems: ContextMenuItem[] = [
    { id: "edit-tab-group", label: "Edit group" },
    { id: "tab-group-menu-divider", separator: true },
    { id: "ungroup-tabs", label: "Ungroup tabs" },
    { id: "close-group-tabs", label: "Close all tabs in group" },
  ];

  // buildTabBarEntries now imported from $lib/tab-groups; thin compat wrapper
  // passes the page-level tabGroups explicitly.
  function buildTabBarEntries<T extends GroupableTab>(tabs: T[]): TabBarEntry<T>[] {
    return buildTabBarEntriesLib(tabs, tabGroups);
  }
  let pendingIntercepts = $state<InterceptEntry[]>([]);
  let selectedInterceptId = $state<string | null>(null);
  let interceptDraft = $state<{ entryId: string; value: string } | null>(null);
  let interceptDraftChanged = $state<{ entryId: string; changed: boolean } | null>(null);
  let interceptMetadataCache = $state<RequestMetadataCache | null>(null);
  let bulkResolving = $state(false);
  const resolvingInterceptIds = new Set<string>();
  const selectedIntercept = $derived(pendingIntercepts.find((entry) => entry.id === selectedInterceptId) ?? null);
  let temporarySaveDialog = $state(false);
  let closeAfterTemporarySave = $state(false);
  let closeWindowAfterTemporaryAction = $state(false);
  let updateAvailableDialog = $state(false);
  let updateAvailableVersion = $state<string | null>(null);
  let pendingBgUpdate = $state<Update | null>(null);
  let bgUpdateBusy = $state(false);
  let bgUpdateProgress = $state<number | null>(null);
  let updateGuardDialog = $state(false);
  let updateGuardVersion = $state<string | null>(null);
  let updateGuardChoice: ((choice: "save" | "nosave" | "cancel") => void) | null = null;
  let updateSaveWaiter: ((saved: boolean) => void) | null = null;
  let closeAfterProjectSave = $state(false);
  let closeTemporaryDialog = $state(false);
  let confirmationRequest = $state<ConfirmationRequest | null>(null);
  let temporaryProjectName = $state("Witness Project");
  let temporaryProjectPath = $state("");
  let recentProjects = $state<RecentProject[]>([]);
  let exportDialog = $state(false);
  let exportPath = $state("");
  let exportBusy = $state(false);
  let importDialog = $state<"options" | "curl" | null>(null);
  let importTarget = $state<"Replay" | "Fuzz" | null>(null);
  let curlImportCommand = $state("");
  let curlImportError = $state("");
  type IdentityConfig = { groupId: string; groupName: string; identityIds: string[] };
  type IdentityResponse = {
    executionId: string;
    identityId: string;
    name: string;
    color: string;
    raw: Uint8Array;
    status: number | null;
    durationMs: number | null;
    size: number | null;
    error: string | null;
    sending: boolean;
  };
  type IdentityChoice = Pick<Identity, "id" | "groupId" | "name" | "color">;
  type IdentityGroupChoice = Pick<IdentityGroup, "id" | "name"> & { identities: IdentityChoice[] };
  type RequestMetadata = { method: string; url: string; host: string };
  type RequestMetadataCache = {
    ownerId: number | string;
    headerPrefix: string | null;
    metadata: RequestMetadata;
  };
  type ReplayTab = {
    id: number;
    title: string;
    groupId: string | null;
    request: Uint8Array;
    response: Uint8Array;
    sending: boolean;
    tls: boolean;
    history: Uint8Array[];
    historyIndex: number;
    identityConfig: IdentityConfig | null;
    identityResponses: Record<string, IdentityResponse>;
    activeIdentityResponseId: string | null;
    pendingRequestIds: string[];
  };
  type ReplaySearchResult = { tab: ReplayTab; snippet: string };
  // highlightSearchText + SearchTextPart imported from $lib/text-search.
  const createReplayTab = (id: number, request = new Uint8Array(), tls = true): ReplayTab => ({
    id,
    title: `${id}`,
    groupId: null,
    request,
    response: new Uint8Array(),
    sending: false,
    tls,
    history: [],
    historyIndex: -1,
    identityConfig: null,
    identityResponses: {},
    activeIdentityResponseId: null,
    pendingRequestIds: [],
  });
  let nextReplayId = 2;
  const replayOperations = new Map<number, Promise<unknown>>();
  const replayCloseRequests = new Set<number>();
  let replayTabs = $state<ReplayTab[]>([createReplayTab(1)]);
  let activeReplayId = $state(1);
  let replayDraft = $state<{ tabId: number; value: string } | null>(null);
  let replayMetadataCache = $state<RequestMetadataCache | null>(null);
  const activeReplay = $derived(replayTabs.find((tab) => tab.id === activeReplayId) ?? replayTabs[0]);
  const activeReplayRequestText = $derived(replayText(activeReplay));
  const replayTabBarEntries = $derived.by(() => buildTabBarEntries(replayTabs));
  // Searching joins every tab's full request/response/history bodies, so
  // debounce the query: typing stays instant, results fill in after pause.
  let debouncedReplaySearchQuery = $state("");
  $effect(() => {
    const next = replaySearchQuery;
    const timer = setTimeout(() => {
      debouncedReplaySearchQuery = next;
    }, 150);
    return () => clearTimeout(timer);
  });
  const replaySearchResults = $derived.by((): ReplaySearchResult[] => {
    const query = debouncedReplaySearchQuery.trim().toLocaleLowerCase();
    if (!query) return [];
    return replayTabs.flatMap((tab) => {
      const searchable = [
        tab.title,
        tab.identityConfig?.groupName ?? "",
        replayText(tab),
        decodeHttpText(tab.response),
        ...tab.history.map((entry) => decodeHttpText(entry)),
        ...Object.values(tab.identityResponses).flatMap((response) => [response.name, decodeHttpText(response.raw), response.error ?? ""]),
      ].join("\n");
      const matchIndex = searchable.toLocaleLowerCase().indexOf(query);
      if (matchIndex < 0) return [];
      const start = Math.max(0, matchIndex - 48);
      const end = Math.min(searchable.length, matchIndex + query.length + 92);
      const snippet = searchable.slice(start, end).replace(/\s+/g, " ").trim();
      return [{ tab, snippet: `${start > 0 ? "…" : ""}${snippet}${end < searchable.length ? "…" : ""}` }];
    });
  });
  const activeReplayMetadata = $derived(
    activeReplay && replayMetadataCache?.ownerId === activeReplay.id
      ? replayMetadataCache.metadata.host
      : "",
  );
  const selectedInterceptMetadata = $derived(
    selectedIntercept && interceptMetadataCache?.ownerId === selectedIntercept.id
      ? interceptMetadataCache.metadata.host || selectedIntercept.host
      : selectedIntercept?.host ?? "",
  );
  const activeIdentityResponse = $derived(
    activeReplay?.identityConfig && activeReplay.activeIdentityResponseId
      ? activeReplay.identityResponses[activeReplay.activeIdentityResponseId] ?? null
      : null,
  );
  let identityConfigDialog = $state(false);
  let identityGroups = $state<IdentityGroupChoice[]>([]);
  let identityDialogGroupId = $state<string | null>(null);
  let identityDialogIds = $state<string[]>([]);
  let identityDialogLoading = $state(false);
  let identityDialogError = $state("");
  const identityDialogGroup = $derived(identityGroups.find((group) => group.id === identityDialogGroupId) ?? null);
  const identityDialogAllSelected = $derived(
    Boolean(identityDialogGroup?.identities.length) && identityDialogIds.length === identityDialogGroup?.identities.length,
  );
  let fuzz = $state<IntruderState>({
    tabs: [createIntruderTab(1)],
    activeTabId: 1,
    nextTabId: 2,
    editorDraft: null,
  });
  const activeFuzzEntry = $derived(
    fuzz.tabs.find((tab) => tab.id === fuzz.activeTabId) ?? fuzz.tabs.find((tab) => tab.kind === "setup"),
  );
  const activeFuzz = $derived.by(() => {
    if (activeFuzzEntry?.kind === "setup") return activeFuzzEntry;
    if (activeFuzzEntry?.kind === "result") {
      return fuzz.tabs.find((tab): tab is IntruderTab => tab.kind === "setup" && tab.id === activeFuzzEntry.sourceTabId);
    }
    return fuzz.tabs.find((tab): tab is IntruderTab => tab.kind === "setup");
  });
  const activeFuzzScan = $derived.by(() => {
    if (activeFuzzEntry?.kind === "result") {
      return activeFuzz?.scans.find((scan) => scan.session.id === activeFuzzEntry.scanId) ?? null;
    }
    return activeFuzz?.scans.find((scan) => scan.session.id === activeFuzz.activeScanId) ?? null;
  });
  const tabGroupDialogTabs = $derived.by(() => {
    const source = tabGroupDialog?.workspace === "Fuzz" ? fuzz.tabs : replayTabs;
    return source.map((tab) => ({ id: tab.id, title: tab.title }));
  });
  const tabGroupAllTabsSelected = $derived(
    tabGroupDialogTabs.length > 0 && tabGroupSelectedTabIds.length === tabGroupDialogTabs.length,
  );
  let aiFuzzLaunch = $state<{ id: string; tabId: number; action: "start" | "resume"; scanId?: string } | null>(null);
  let decoderWorkspace = $state<DecoderWorkspaceState>({
    input: "",
    recipe: [],
    stageOutputs: [],
    detected: "Plain text",
    padding: true,
    filter: "",
    notice: "",
    nextStepId: 1,
  });
  let comparerWorkspace = $state<ComparerWorkspaceState>({
    left: "",
    right: "",
    granularity: "character",
    layout: "side",
  });
  let comparerLeft = $state("");
  let comparerRight = $state("");
  let siteMapWorkspace = $state<SiteMapWorkspaceState>({ search: "", inScopeOnly: false, collapsed: [], selectedEntryId: null, selectedRowKey: null });
  let organizerWorkspace = $state<OrganizerWorkspaceState>({ selectedFolderId: "all", selectedItemId: null, query: "", selectedTag: "", sort: "updated", draft: null, requestText: "", responseText: "", tagText: "", tagDefinitions: [], stages: [] });
  let identityWorkspace = $state<IdentityWorkspaceState>({ selectedGroupId: null, selectedIdentityId: null, groupDraft: null, identityDraft: null });
  let forgeWorkspace = $state<ForgeWorkspaceState>({ chats: [], activeChatId: "", draft: "" });
  // Session-only Forge permission; deliberately excluded from WorkspaceSnapshot persistence.
  let forgeTrustTools = $state(false);
  let showShortcuts = $state(false);
  let shortcutDialogElement = $state<HTMLElement | null>(null);
  let shortcutOpener = $state<HTMLElement | null>(null);
  let replaySearchOpen = $state(false);
  let replaySearchDialogElement = $state<HTMLElement | null>(null);
  let replaySearchOpener = $state<HTMLElement | null>(null);
  let replaySearchQuery = $state("");
  let replayTabScrollElement = $state<HTMLElement | null>(null);
  let replayTabsCanScrollRight = $state(false);
  let forgeController = $state<ShortcutController | null>(null);
  let fuzzController = $state<ShortcutController | null>(null);
  let decoderController = $state<ShortcutController | null>(null);
  let comparerController = $state<ShortcutController | null>(null);
  let scopeController = $state<ShortcutController | null>(null);
  let organizerController = $state<ShortcutController | null>(null);
  let identityController = $state<ShortcutController | null>(null);
  let siteMapController = $state<ShortcutController | null>(null);
  let logController = $state<ShortcutController | null>(null);
  let historyWorkspace = $state<HTMLElement>();
  let organizerRevision = $state(0);
  let settingsSection = $state<SettingsSection>("display");
  let statusClock = $state(new Date());
  let use24HourClock = $state(false);
  let workspaceReady = $state(false);
  let projectLifecycleToken = $state(0);
  let projectTransitioning = $state(false);
  let snapshotRequestToken = 0;
  let workspaceSaveTimer: ReturnType<typeof setTimeout> | undefined;
  let restoredHistoryDetailId = $state<string | null>(null);

  function beginProjectTransition() {
    if (projectTransitioning) throw new Error("another project transition is already in progress");
    projectTransitioning = true;
    snapshotRequestToken += 1;
  }

  function endProjectTransition() {
    projectLifecycleToken += 1;
    projectTransitioning = false;
    snapshotRequestToken += 1;
  }

  async function refreshSnapshot() {
    const requestToken = ++snapshotRequestToken;
    const value = await commands.snapshot();
    if (requestToken === snapshotRequestToken && !projectTransitioning) snapshot = value;
    return value;
  }
  const memoryUsage = $derived(formatMemoryUsage(snapshot?.memoryUsageBytes));
  const clockTime = $derived(formatClock(statusClock, use24HourClock));

  $effect(() => {
    if (activeTab === "Logs" && !snapshot?.settings.showLogsTab) activeTab = "Proxy";
  });

  $effect(() => {
    const tab = activeReplay;
    if (!tab) return;
    const raw = tab.request;
    untrack(() => updateReplayMetadata(tab.id, decodeHttpText(raw.slice(0, HTTP_METADATA_LIMIT))));
  });

  $effect(() => {
    const entry = selectedIntercept;
    if (!entry) return;
    const raw = entry.kind === "response"
      ? entry.requestRaw ?? new Uint8Array()
      : entry.raw;
    untrack(() => updateInterceptMetadata(
      entry.id,
      decodeHttpText(raw.slice(0, HTTP_METADATA_LIMIT)),
      entry.host,
    ));
  });

  function formatMemoryUsage(bytes: number | null | undefined) {
    if (bytes == null) return "—";
    const units = ["B", "KB", "MB", "GB"];
    let value = bytes;
    let unitIndex = 0;
    while (value >= 1024 && unitIndex < units.length - 1) {
      value /= 1024;
      unitIndex += 1;
    }
    return `${value >= 10 || unitIndex === 0 ? Math.round(value) : value.toFixed(1)} ${units[unitIndex]}`;
  }

  function formatClock(date: Date, use24Hour: boolean) {
    return formatClockLib(date, use24Hour);
  }

  function encodeBytes(value: Uint8Array | number[]) {
    const bytes = value instanceof Uint8Array ? value : new Uint8Array(value);
    let binary = "";
    for (let offset = 0; offset < bytes.length; offset += 0x8000) {
      binary += String.fromCharCode(...bytes.subarray(offset, offset + 0x8000));
    }
    return btoa(binary);
  }

  function decodeBytes(value: unknown) {
    if (typeof value !== "string") throw new Error("workspace contains invalid binary data");
    const binary = atob(value);
    const bytes = new Uint8Array(binary.length);
    for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index);
    return bytes;
  }

  function record(value: unknown): Record<string, unknown> {
    if (!value || typeof value !== "object" || Array.isArray(value)) {
      throw new Error("workspace contains an invalid object");
    }
    return value as Record<string, unknown>;
  }

  function textValue(value: unknown, fallback = "") {
    return typeof value === "string" ? value : fallback;
  }

  function numberValue(value: unknown, fallback: number) {
    return typeof value === "number" && Number.isFinite(value) ? value : fallback;
  }

  function booleanValue(value: unknown, fallback: boolean) {
    return typeof value === "boolean" ? value : fallback;
  }

  function tabGroupColorValue(value: unknown, fallback = "#ffa500") {
    return typeof value === "string" && /^#[0-9a-f]{6}$/i.test(value) ? value : fallback;
  }

  function restoreTabGroups(value: unknown): TabGroup[] {
    if (!Array.isArray(value)) return [];
    const seen = new Set<string>();
    return value.flatMap((rawGroup) => {
      if (!rawGroup || typeof rawGroup !== "object" || Array.isArray(rawGroup)) return [];
      const source = rawGroup as Record<string, unknown>;
      const id = textValue(source.id).trim();
      const name = textValue(source.name).trim();
      if (!id || !name || seen.has(id)) return [];
      seen.add(id);
      return [{
        id,
        name,
        color: tabGroupColorValue(source.color),
        collapsed: booleanValue(source.collapsed, false),
      }];
    });
  }

  function byteArray(value: unknown) {
    return Array.isArray(value)
      ? value.filter((item): item is number => typeof item === "number" && Number.isInteger(item) && item >= 0 && item <= 255)
      : [];
  }

  function restoreOrganizerDraft(value: unknown) {
    if (!value || typeof value !== "object" || Array.isArray(value)) return null;
    const source = value as Record<string, unknown>;
    const id = textValue(source.id);
    if (!id) return null;
    return {
      id,
      title: textValue(source.title),
      folderId: typeof source.folderId === "string" ? source.folderId : null,
      request: byteArray(source.request),
      response: byteArray(source.response),
      tls: booleanValue(source.tls, true),
      source: textValue(source.source),
      method: textValue(source.method),
      host: textValue(source.host),
      path: textValue(source.path),
      status: typeof source.status === "number" && Number.isFinite(source.status) ? source.status : null,
      notes: textValue(source.notes),
      tags: Array.isArray(source.tags) ? source.tags.filter((tag): tag is string => typeof tag === "string") : [],
      stageId: typeof source.stageId === "string" ? source.stageId : null,
      createdAt: textValue(source.createdAt),
      updatedAt: textValue(source.updatedAt),
    };
  }

  function restoreIdentityDraft(value: unknown) {
    if (!value || typeof value !== "object" || Array.isArray(value)) return null;
    const source = value as Record<string, unknown>;
    const id = textValue(source.id);
    const groupId = textValue(source.groupId);
    if (!id || !groupId) return null;
    return {
      id,
      groupId,
      name: textValue(source.name),
      color: textValue(source.color, "#5794ef"),
      notes: textValue(source.notes),
      authValue: textValue(source.authValue),
    };
  }

  function cloneJson<T>(value: T): T {
    return JSON.parse(JSON.stringify(value)) as T;
  }

  function restoreForgeWorkspace(value: unknown): ForgeWorkspaceState {
    const empty = (draft = ""): ForgeWorkspaceState => ({ chats: [], activeChatId: "", draft });
    if (!value || typeof value !== "object" || Array.isArray(value)) return empty();
    const source = value as Record<string, unknown>;
    const draft = textValue(source.draft);
    if (!Array.isArray(source.chats)) return empty(draft);
    const chats: ForgeChatSnapshot[] = [];
    for (const rawChat of source.chats) {
      if (!rawChat || typeof rawChat !== "object" || Array.isArray(rawChat)) continue;
      const chatSource = rawChat as Record<string, unknown>;
      const id = textValue(chatSource.id);
      if (!id) continue;
      const messages: ForgeMessageSnapshot[] = [];
      if (Array.isArray(chatSource.messages)) {
        for (const rawMessage of chatSource.messages) {
          if (!rawMessage || typeof rawMessage !== "object" || Array.isArray(rawMessage)) continue;
          const messageSource = rawMessage as Record<string, unknown>;
          const role = textValue(messageSource.role);
          if (!["system", "user", "assistant", "tool"].includes(role)) continue;
          const message: ForgeMessageSnapshot = {
            role: role as ForgeMessageSnapshot["role"],
            uiTimestamp: numberValue(messageSource.uiTimestamp, Date.now()),
          };
          if (typeof messageSource.content === "string" || messageSource.content === null) message.content = messageSource.content;
          if (typeof messageSource.toolCallId === "string") message.toolCallId = messageSource.toolCallId;
          if (role === "tool" && ["approved", "trusted", "session-trusted", "rejected"].includes(textValue(messageSource.uiEvent))) {
            message.uiEvent = messageSource.uiEvent as ForgeMessageSnapshot["uiEvent"];
            const uiToolName = textValue(messageSource.uiToolName);
            if (uiToolName) message.uiToolName = uiToolName;
          }
          if (Array.isArray(messageSource.toolCalls)) {
            const toolCalls = messageSource.toolCalls.flatMap((rawCall) => {
              if (!rawCall || typeof rawCall !== "object" || Array.isArray(rawCall)) return [];
              const call = rawCall as Record<string, unknown>;
              if (!call.function || typeof call.function !== "object" || Array.isArray(call.function)) return [];
              const functionValue = call.function as Record<string, unknown>;
              const id = textValue(call.id);
              const name = textValue(functionValue.name);
              const args = functionValue.arguments;
              if (!id || !name || typeof args !== "string") return [];
              return [{ id, type: textValue(call.type, "function"), function: { name, arguments: args } }];
            });
            if (toolCalls.length) message.toolCalls = toolCalls;
          }
          messages.push(message);
        }
      }
      chats.push({ id, title: textValue(chatSource.title, "New chat"), messages });
    }
    const uniqueChats = chats.filter((chat, index) => chats.findIndex((candidate) => candidate.id === chat.id) === index);
    if (!uniqueChats.length) return empty(draft);
    const requestedActive = textValue(source.activeChatId);
    return {
      chats: uniqueChats,
      activeChatId: uniqueChats.some((chat) => chat.id === requestedActive) ? requestedActive : uniqueChats[0].id,
      draft,
    };
  }

  function serializeReplayTab(tab: ReplayTab): ReplayTabSnapshot {
    return {
      id: tab.id,
      title: tab.title,
      groupId: tab.groupId,
      request: encodeBytes(tab.request),
      response: encodeBytes(tab.response),
      tls: tab.tls,
      history: tab.history.map(encodeBytes),
      historyIndex: tab.historyIndex,
      identityConfig: tab.identityConfig ? { ...tab.identityConfig, identityIds: [...tab.identityConfig.identityIds] } : null,
      identityResponses: Object.fromEntries(
        Object.entries(tab.identityResponses).map(([id, response]) => [id, {
          executionId: response.executionId,
          identityId: response.identityId,
          name: response.name,
          color: response.color,
          raw: encodeBytes(response.raw),
          status: response.status,
          durationMs: response.durationMs,
          size: response.size,
          error: response.error,
        }]),
      ),
      activeIdentityResponseId: tab.activeIdentityResponseId,
    };
  }

  function serializeFuzzTab(tab: IntruderTab): IntruderTabSnapshot {
    return {
      ...tab,
      request: encodeBytes(tab.request),
      warehouse: cloneJson(tab.warehouse),
      positionWarehouses: cloneJson(tab.positionWarehouses),
      scans: tab.scans.map((scan) => {
        const interrupted = scan.running || scan.stopRequested || Boolean(scan.currentRequestId);
        return {
          ...scan,
          session: {
            ...scan.session,
            template: encodeBytes(scan.session.template),
            payloadRows: scan.session.payloadRows.map((row) => [...row]),
          },
          running: false,
          stopped: scan.stopped || interrupted,
          stopRequested: false,
          currentRequestId: null,
          completedAt: scan.completedAt ?? (interrupted ? new Date().toISOString() : null),
          error: interrupted && !scan.error
            ? "Scan interrupted while the project was being saved. Completed results were preserved."
            : scan.error,
          results: scan.results.map((result) => ({
            ...result,
            request: encodeBytes(result.request),
            response: encodeBytes(result.response),
            payloads: [...result.payloads],
            modifiedRanges: result.modifiedRanges.map((range) => ({ ...range })),
          })),
        };
      }),
    };
  }

  function serializeFuzzWorkspaceTab(tab: IntruderWorkspaceTab): IntruderWorkspaceTabSnapshot {
    return tab.kind === "result" ? { ...tab } : serializeFuzzTab(tab);
  }

  function serializeFuzzState(value: IntruderState): IntruderStateSnapshot {
    return {
      activeTabId: value.activeTabId,
      nextTabId: value.nextTabId,
      editorDraft: value.editorDraft ? { ...value.editorDraft } : null,
      tabs: value.tabs.map(serializeFuzzWorkspaceTab),
    };
  }

  function restoreReplayTab(value: unknown): ReplayTab {
    const source = record(value);
    const id = numberValue(source.id, 0);
    if (!Number.isInteger(id) || id < 1) throw new Error("workspace contains an invalid Replay tab");
    const tab = createReplayTab(id, decodeBytes(source.request), booleanValue(source.tls, true));
    tab.title = textValue(source.title, `${id}`);
    tab.groupId = typeof source.groupId === "string" ? source.groupId : null;
    tab.response = decodeBytes(source.response);
    tab.history = Array.isArray(source.history) ? source.history.map(decodeBytes) : [];
    tab.historyIndex = Math.max(-1, Math.min(tab.history.length - 1, numberValue(source.historyIndex, -1)));
    const identityConfig = source.identityConfig;
    if (identityConfig) {
      const config = record(identityConfig);
      const groupId = textValue(config.groupId);
      const groupName = textValue(config.groupName);
      const identityIds = Array.isArray(config.identityIds) ? config.identityIds.filter((id): id is string => typeof id === "string") : [];
      if (groupId && groupName && identityIds.length) tab.identityConfig = { groupId, groupName, identityIds };
    }
    const identityResponses = source.identityResponses;
    if (identityResponses && typeof identityResponses === "object" && !Array.isArray(identityResponses)) {
      for (const [idKey, rawResponse] of Object.entries(identityResponses as Record<string, unknown>)) {
        try {
          const response = record(rawResponse);
          const executionId = textValue(response.executionId, idKey);
          tab.identityResponses[idKey] = {
            executionId,
            identityId: textValue(response.identityId),
            name: textValue(response.name, "Identity"),
            color: textValue(response.color, "#9ca3af"),
            raw: decodeBytes(response.raw),
            status: typeof response.status === "number" ? response.status : null,
            durationMs: typeof response.durationMs === "number" ? response.durationMs : null,
            size: typeof response.size === "number" ? response.size : null,
            error: typeof response.error === "string" ? response.error : null,
            sending: false,
          };
        } catch {
          // A damaged individual identity response should not discard its tab.
        }
      }
    }
    tab.activeIdentityResponseId = typeof source.activeIdentityResponseId === "string"
      && tab.identityResponses[source.activeIdentityResponseId]
      ? source.activeIdentityResponseId
      : null;
    return tab;
  }

  function restoreWarehouse(value: unknown, fallback: PayloadWarehouse): PayloadWarehouse {
    if (!value || typeof value !== "object" || Array.isArray(value)) return fallback;
    const source = value as Record<string, unknown>;
    const objectPart = (key: string) => source[key] && typeof source[key] === "object" && !Array.isArray(source[key])
      ? source[key] as Record<string, unknown>
      : {};
    const list = objectPart("list");
    const numbers = objectPart("numbers");
    const nullPayload = objectPart("nullPayload");
    const bruteForce = objectPart("bruteForce");
    const dates = objectPart("dates");
    const characterSubstitution = objectPart("characterSubstitution");
    const mappings = Array.isArray(characterSubstitution.mappings)
      ? characterSubstitution.mappings.filter((item): item is Record<string, unknown> => Boolean(item && typeof item === "object" && !Array.isArray(item))).map((item) => ({
        from: textValue(item.from),
        to: textValue(item.to),
      }))
      : fallback.characterSubstitution.mappings;
    return {
      ...fallback,
      type: ["list", "numbers", "null", "bruteForce", "dates", "characterSubstitution"].includes(textValue(source.type))
        ? source.type as PayloadWarehouse["type"]
        : fallback.type,
      list: { ...fallback.list, text: textValue(list.text), builtin: textValue(list.builtin), url: textValue(list.url) },
      numbers: {
        ...fallback.numbers,
        mode: numbers.mode === "random" ? "random" : "sequential",
        from: textValue(numbers.from, fallback.numbers.from),
        to: textValue(numbers.to, fallback.numbers.to),
        step: textValue(numbers.step, fallback.numbers.step),
        count: textValue(numbers.count, fallback.numbers.count),
      },
      nullPayload: {
        ...fallback.nullPayload,
        mode: nullPayload.mode === "infinite" ? "infinite" : "count",
        count: textValue(nullPayload.count, fallback.nullPayload.count),
      },
      bruteForce: {
        ...fallback.bruteForce,
        characterSet: textValue(bruteForce.characterSet, fallback.bruteForce.characterSet),
        minLength: textValue(bruteForce.minLength, fallback.bruteForce.minLength),
        maxLength: textValue(bruteForce.maxLength, fallback.bruteForce.maxLength),
      },
      dates: {
        ...fallback.dates,
        from: textValue(dates.from, fallback.dates.from),
        to: textValue(dates.to, fallback.dates.to),
        step: textValue(dates.step, fallback.dates.step),
        unit: ["days", "weeks", "months", "years"].includes(textValue(dates.unit)) ? dates.unit as typeof fallback.dates.unit : fallback.dates.unit,
        formatMode: dates.formatMode === "custom" ? "custom" : "preset",
        format: textValue(dates.format, fallback.dates.format),
        customFormat: textValue(dates.customFormat, fallback.dates.customFormat),
      },
      characterSubstitution: {
        ...fallback.characterSubstitution,
        mappings,
        caseSensitive: booleanValue(characterSubstitution.caseSensitive, fallback.characterSubstitution.caseSensitive),
        itemsText: textValue(characterSubstitution.itemsText),
        newItem: textValue(characterSubstitution.newItem),
        builtin: textValue(characterSubstitution.builtin),
      },
      processing: Array.isArray(source.processing)
        ? source.processing.filter((item): item is typeof fallback.processing[number] => Boolean(item && typeof item === "object" && !Array.isArray(item))).map((item) => ({
          ...fallback.processing[0],
          ...item,
          id: textValue(item.id, `${Date.now()}-${Math.random()}`),
          enabled: booleanValue(item.enabled, true),
        }))
        : [],
    };
  }

  function restoreFuzzState(value: unknown): IntruderState {
    const source = record(value);
    const tabs: IntruderState["tabs"] = [];
    if (Array.isArray(source.tabs)) {
      for (const rawTab of source.tabs) {
        try {
          const item = record(rawTab);
          const id = numberValue(item.id, 0);
          if (!Number.isInteger(id) || id < 1) continue;
          if (item.kind === "result") {
            const sourceTabId = numberValue(item.sourceTabId, 0);
            const scanId = textValue(item.scanId);
            const title = textValue(item.title);
            if (!Number.isInteger(sourceTabId) || sourceTabId < 1 || !scanId || !title) continue;
            tabs.push({
              kind: "result",
              id,
              title,
              groupId: typeof item.groupId === "string" ? item.groupId : null,
              sourceTabId,
              scanId,
            });
            continue;
          }
          const tab = createIntruderTab(id, decodeBytes(item.request), booleanValue(item.tls, true));
          tab.title = textValue(item.title, `${id}`);
          tab.groupId = typeof item.groupId === "string" ? item.groupId : null;
          tab.mode = ["single", "spread", "map", "combine"].includes(textValue(item.mode))
            ? item.mode as typeof tab.mode
            : tab.mode;
          tab.scanName = textValue(item.scanName);
          tab.warehouse = restoreWarehouse(item.warehouse, tab.warehouse);
          tab.positionWarehouses = Array.isArray(item.positionWarehouses)
            ? item.positionWarehouses.map((warehouse) => restoreWarehouse(warehouse, createIntruderTab(id).warehouse))
            : [];
          tab.selectedPayloadPosition = Math.max(0, numberValue(item.selectedPayloadPosition, 0));
          tab.error = textValue(item.error);
          if (Array.isArray(item.scans)) {
            tab.scans = item.scans.flatMap((rawScan) => {
              try {
                const scanSource = record(rawScan);
                const sessionSource = record(scanSource.session);
                const session = {
                  id: textValue(sessionSource.id, `${Date.now()}-${id}`),
                  template: Array.from(decodeBytes(sessionSource.template)),
                  tls: booleanValue(sessionSource.tls, tab.tls),
                  mode: ["single", "spread", "map", "combine"].includes(textValue(sessionSource.mode))
                    ? sessionSource.mode as typeof tab.mode
                    : tab.mode,
                  payloadRows: Array.isArray(sessionSource.payloadRows)
                    ? sessionSource.payloadRows.map((row) => Array.isArray(row) ? row.filter((item): item is string => typeof item === "string") : [])
                    : [],
                  totalRequests: typeof sessionSource.totalRequests === "number" ? sessionSource.totalRequests : null,
                  repeatIndefinitely: booleanValue(sessionSource.repeatIndefinitely, false),
                  theme: sessionSource.theme === "light" ? "light" as const : "dark" as const,
                };
                const name = textValue(scanSource.name);
                if (!name) return [];
                const results = Array.isArray(scanSource.results) ? scanSource.results.flatMap((rawResult) => {
                  try {
                    const resultSource = record(rawResult);
                    return [{
                      id: textValue(resultSource.id, `${session.id}-${numberValue(resultSource.sequence, 0)}`),
                      sequence: numberValue(resultSource.sequence, 0),
                      position: typeof resultSource.position === "number" ? resultSource.position : null,
                      payload: textValue(resultSource.payload),
                      payloads: Array.isArray(resultSource.payloads) ? resultSource.payloads.filter((item): item is string => typeof item === "string") : [],
                      modifiedRanges: Array.isArray(resultSource.modifiedRanges)
                        ? resultSource.modifiedRanges.flatMap((rawRange) => {
                          if (!rawRange || typeof rawRange !== "object" || Array.isArray(rawRange)) return [];
                          const range = rawRange as Record<string, unknown>;
                          const from = numberValue(range.from, -1);
                          const to = numberValue(range.to, -1);
                          return Number.isInteger(from) && Number.isInteger(to) && from >= 0 && to > from
                            ? [{ from, to }]
                            : [];
                        })
                        : [],
                      status: typeof resultSource.status === "number" ? resultSource.status : null,
                      length: numberValue(resultSource.length, 0),
                      durationMs: numberValue(resultSource.durationMs, 0),
                      error: textValue(resultSource.error),
                      request: decodeBytes(resultSource.request),
                      response: decodeBytes(resultSource.response),
                    }];
                  } catch {
                    return [];
                  }
                }) : [];
                const selectedResultId = typeof scanSource.selectedResultId === "string" && results.some((result) => result.id === scanSource.selectedResultId)
                  ? scanSource.selectedResultId
                  : results[0]?.id ?? null;
                return [{
                  name,
                  session,
                  startedAt: textValue(scanSource.startedAt, new Date().toISOString()),
                  completedAt: typeof scanSource.completedAt === "string" ? scanSource.completedAt : null,
                  running: false,
                  stopped: booleanValue(scanSource.stopped, false),
                  stopRequested: false,
                  currentRequestId: null,
                  nextPayloadIndex: Math.max(0, numberValue(scanSource.nextPayloadIndex, results.length)),
                  results,
                  selectedResultId,
                  error: textValue(scanSource.error),
                  persistenceError: textValue(scanSource.persistenceError),
                }];
              } catch {
                return [];
              }
            });
          }
          const activeScanId = typeof item.activeScanId === "string" && tab.scans.some((scan) => scan.session.id === item.activeScanId)
            ? item.activeScanId
            : tab.scans[0]?.session.id ?? null;
          tab.activeScanId = activeScanId;
          tabs.push(tab);
        } catch {
          // Ignore one malformed tab and recover the remaining workspace.
        }
      }
    }
    const setupTabs = tabs.filter((tab): tab is IntruderTab => tab.kind === "setup");
    const restoredTabs = tabs.filter((tab) => tab.kind === "setup" || (
      setupTabs.some((sourceTab) => sourceTab.id === tab.sourceTabId && sourceTab.scans.some((scan) => scan.session.id === tab.scanId))
    ));
    if (!setupTabs.length) return {
      tabs: [createIntruderTab(1)],
      activeTabId: 1,
      nextTabId: 2,
      editorDraft: null,
    };
    const maxId = Math.max(...restoredTabs.map((tab) => tab.id));
    const activeTabId = restoredTabs.some((tab) => tab.id === source.activeTabId) ? source.activeTabId as number : setupTabs[0].id;
    return {
      tabs: restoredTabs,
      activeTabId,
      nextTabId: Math.max(maxId + 1, numberValue(source.nextTabId, maxId + 1)),
      editorDraft: source.editorDraft && typeof source.editorDraft === "object" && !Array.isArray(source.editorDraft)
        ? { tabId: numberValue((source.editorDraft as Record<string, unknown>).tabId, activeTabId), value: textValue((source.editorDraft as Record<string, unknown>).value) }
        : null,
    };
  }

  function recordClosedTab(workspace: "Replay", tab: ReplayTab): void;
  function recordClosedTab(workspace: "Fuzz", tab: IntruderTab): void;
  function recordClosedTab(workspace: TabWorkspace, tab: ReplayTab | IntruderTab) {
    const entry: ClosedTabEntry = workspace === "Replay"
      ? { id: nextClosedTabEntryId++, workspace, tab: serializeReplayTab(tab as ReplayTab) }
      : { id: nextClosedTabEntryId++, workspace, tab: serializeFuzzTab(tab as IntruderTab) };
    closedTabs = [entry, ...closedTabs].slice(0, MAX_CLOSED_TABS);
  }

  function restoreClosedTab(entryId: number) {
    const index = closedTabs.findIndex((entry) => entry.id === entryId);
    if (index < 0) return;
    const entry = closedTabs[index];
    let restoredReplay: ReplayTab | null = null;
    let restoredFuzz: IntruderTab | null = null;
    try {
      if (entry.workspace === "Replay") {
        restoredReplay = restoreReplayTab(entry.tab);
        if (replayTabs.some((tab) => tab.id === restoredReplay?.id)) restoredReplay.id = nextReplayId++;
        replayTabs.push(restoredReplay);
        activeReplayId = restoredReplay.id;
        nextReplayId = Math.max(nextReplayId, restoredReplay.id + 1);
      } else {
        const restoredState = restoreFuzzState({
          tabs: [entry.tab],
          activeTabId: entry.tab.id,
          nextTabId: entry.tab.id + 1,
          editorDraft: null,
        });
        restoredFuzz = restoredState.tabs.find((tab): tab is IntruderTab => tab.kind === "setup" && tab.id === entry.tab.id) ?? restoredState.tabs.find((tab): tab is IntruderTab => tab.kind === "setup") ?? null;
        if (!restoredFuzz) return;
        if (fuzz.tabs.some((tab) => tab.id === restoredFuzz?.id)) restoredFuzz.id = fuzz.nextTabId++;
        fuzz.tabs.push(restoredFuzz);
        fuzz.activeTabId = restoredFuzz.id;
        fuzz.nextTabId = Math.max(fuzz.nextTabId, restoredFuzz.id + 1);
      }
    } catch {
      return;
    }
    closedTabs = closedTabs.filter((candidate) => candidate.id !== entry.id);
    markWorkspaceDirty();
    changeTab(entry.workspace);
  }

  function restoreLastClosedTab(workspace: TabWorkspace): boolean {
    const entry = closedTabs.find((candidate) => candidate.workspace === workspace);
    if (!entry) return false;
    restoreClosedTab(entry.id);
    return true;
  }

  function buildWorkspaceSnapshot(): WorkspaceSnapshot {
    return {
      version: 1,
      savedAt: new Date().toISOString(),
      activeTab,
      use24HourClock,
      decoderInput,
      decoder: cloneJson({ ...decoderWorkspace, input: decoderInput }),
      comparer: cloneJson({ ...comparerWorkspace, left: comparerLeft, right: comparerRight }),
      siteMap: cloneJson(siteMapWorkspace),
      organizer: cloneJson(organizerWorkspace),
      identity: cloneJson(identityWorkspace),
      historyFilter: cloneJson(historyFilter),
      historyDetailId: historyDetail?.entry.id ?? restoredHistoryDetailId,
      historyInspectorsVisible,
      activeReplayId,
      nextReplayId,
      replayTabs: replayTabs.map(serializeReplayTab),
      replayDraft: replayDraft ? { ...replayDraft } : null,
      fuzz: serializeFuzzState(fuzz),
      tabGroups: tabGroups.map((group) => ({ ...group })),
      settingsSection,
      forge: cloneJson(forgeWorkspace),
    };
  }

  function resetWorkspaceState() {
    activeTab = "Proxy";
    use24HourClock = false;
    decoderInput = "";
    decoderWorkspace = {
      input: "",
      recipe: [], stageOutputs: [], detected: "Plain text",
      padding: true, filter: "", notice: "", nextStepId: 1,
    };
    comparerWorkspace = { left: "", right: "", granularity: "character", layout: "side" };
    comparerLeft = "";
    comparerRight = "";
    siteMapWorkspace = { search: "", inScopeOnly: false, collapsed: [], selectedEntryId: null, selectedRowKey: null };
    organizerWorkspace = { selectedFolderId: "all", selectedItemId: null, query: "", selectedTag: "", sort: "updated", draft: null, requestText: "", responseText: "", tagText: "", tagDefinitions: [], stages: [] };
    identityWorkspace = { selectedGroupId: null, selectedIdentityId: null, groupDraft: null, identityDraft: null };
    forgeWorkspace = { chats: [], activeChatId: "", draft: "" };
    historyFilter = { sortBy: "timestamp", sortDescending: true, inScopeOnly: false };
    historyInspectorsVisible = true;
    restoredHistoryDetailId = null;
    replayTabs = [createReplayTab(1)];
    activeReplayId = 1;
    nextReplayId = 2;
    replayDraft = null;
    tabGroups = [];
    closedTabs = [];
    nextClosedTabEntryId = 1;
    resetFuzz();
    settingsSection = "display";
  }

  function resetTransientProjectState() {
    clearPendingIntercepts();
    replayMetadataCache = null;
    contextMenu = null;
    tabContextMenu = null;
    tabGroupContextMenu = null;
    tabRenameDialog = null;
    tabGroupDialog = null;
  }

  function restoreWorkspaceSnapshot(serialized: string) {
    const source = record(JSON.parse(serialized));
    if (source.version !== 1) throw new Error(`unsupported workspace version: ${String(source.version)}`);
    resetWorkspaceState();
    const allowedTabs: Tab[] = ["Proxy", "History", "Site Map", "Replay", "Fuzz", "Organizer", "ID+", "Decoder", "Comparer", "Scope", "AI", "Logs", "Settings"];
    if (allowedTabs.includes(source.activeTab as Tab)) activeTab = source.activeTab as Tab;
    if (activeTab === "Logs" && !snapshot?.settings.showLogsTab) activeTab = "Proxy";
    use24HourClock = booleanValue(source.use24HourClock, false);
    decoderInput = textValue(source.decoderInput);
    const decoder = source.decoder && typeof source.decoder === "object" && !Array.isArray(source.decoder) ? source.decoder as Record<string, unknown> : {};
    decoderWorkspace = {
      input: textValue(decoder.input, decoderInput),
      recipe: Array.isArray(decoder.recipe) ? decoder.recipe.filter((item): item is { id: number; operation: string } => Boolean(item && typeof item === "object" && typeof (item as Record<string, unknown>).operation === "string")).map((item) => ({ id: numberValue(item.id, 1), operation: item.operation })) : [],
      stageOutputs: Array.isArray(decoder.stageOutputs) ? decoder.stageOutputs.filter((item): item is string => typeof item === "string") : [],
      detected: textValue(decoder.detected, "Plain text"),
      padding: booleanValue(decoder.padding, true),
      filter: textValue(decoder.filter),
      notice: textValue(decoder.notice),
      nextStepId: Math.max(1, numberValue(decoder.nextStepId, 1)),
    };
    decoderInput = decoderWorkspace.input;
    const comparer = source.comparer && typeof source.comparer === "object" && !Array.isArray(source.comparer) ? source.comparer as Record<string, unknown> : {};
    comparerWorkspace = {
      left: textValue(comparer.left),
      right: textValue(comparer.right),
      granularity: ["character", "line", "word"].includes(textValue(comparer.granularity)) ? comparer.granularity as ComparerWorkspaceState["granularity"] : "character",
      layout: comparer.layout === "stacked" ? "stacked" : "side",
    };
    comparerLeft = comparerWorkspace.left;
    comparerRight = comparerWorkspace.right;
    const siteMap = source.siteMap && typeof source.siteMap === "object" && !Array.isArray(source.siteMap)
      ? source.siteMap as Record<string, unknown>
      : {};
    siteMapWorkspace = {
      search: textValue(siteMap.search),
      inScopeOnly: booleanValue(siteMap.inScopeOnly, false),
      collapsed: Array.isArray(siteMap.collapsed) ? siteMap.collapsed.filter((key): key is string => typeof key === "string") : [],
      selectedEntryId: typeof siteMap.selectedEntryId === "string" ? siteMap.selectedEntryId : null,
      selectedRowKey: typeof siteMap.selectedRowKey === "string" ? siteMap.selectedRowKey : null,
    };
    const organizer = source.organizer && typeof source.organizer === "object" && !Array.isArray(source.organizer)
      ? source.organizer as Record<string, unknown>
      : {};
    organizerWorkspace = {
      selectedFolderId: typeof organizer.selectedFolderId === "string" ? organizer.selectedFolderId as OrganizerWorkspaceState["selectedFolderId"] : "all",
      selectedItemId: typeof organizer.selectedItemId === "string" ? organizer.selectedItemId : null,
      query: textValue(organizer.query),
      selectedTag: textValue(organizer.selectedTag),
      sort: ["updated", "created", "title", "host"].includes(textValue(organizer.sort)) ? organizer.sort as OrganizerWorkspaceState["sort"] : "updated",
      draft: restoreOrganizerDraft(organizer.draft),
      requestText: textValue(organizer.requestText),
      responseText: textValue(organizer.responseText),
      tagText: textValue(organizer.tagText),
      tagDefinitions: Array.isArray(organizer.tagDefinitions)
        ? organizer.tagDefinitions.flatMap((value) => {
          if (!value || typeof value !== "object" || Array.isArray(value)) return [];
          const item = value as Record<string, unknown>;
          const name = textValue(item.name).trim();
          return name ? [{ name, color: textValue(item.color, "#5794ef") }] : [];
        })
        : [],
      stages: Array.isArray(organizer.stages)
        ? organizer.stages.flatMap((value) => {
          if (!value || typeof value !== "object" || Array.isArray(value)) return [];
          const item = value as Record<string, unknown>;
          const id = textValue(item.id);
          const name = textValue(item.name).trim();
          return id && name ? [{ id, name, color: textValue(item.color, "#5794ef") }] : [];
        })
        : [],
    };
    const identity = source.identity && typeof source.identity === "object" && !Array.isArray(source.identity)
      ? source.identity as Record<string, unknown>
      : {};
    identityWorkspace = {
      selectedGroupId: typeof identity.selectedGroupId === "string" ? identity.selectedGroupId : null,
      selectedIdentityId: typeof identity.selectedIdentityId === "string" ? identity.selectedIdentityId : null,
      groupDraft: identity.groupDraft && typeof identity.groupDraft === "object" && !Array.isArray(identity.groupDraft)
        ? {
          name: textValue((identity.groupDraft as Record<string, unknown>).name),
          description: textValue((identity.groupDraft as Record<string, unknown>).description),
          injectionType: ["cookie", "header", "queryParameter"].includes(textValue((identity.groupDraft as Record<string, unknown>).injectionType))
            ? (identity.groupDraft as Record<string, unknown>).injectionType as "cookie" | "header" | "queryParameter"
            : "cookie",
          injectionKey: textValue((identity.groupDraft as Record<string, unknown>).injectionKey),
        }
        : null,
      identityDraft: restoreIdentityDraft(identity.identityDraft),
    };
    if (source.historyFilter && typeof source.historyFilter === "object" && !Array.isArray(source.historyFilter)) {
      const filter = source.historyFilter as Record<string, unknown>;
      historyFilter = {
        method: typeof filter.method === "string" ? filter.method : null,
        host: typeof filter.host === "string" ? filter.host : null,
        statusMin: typeof filter.statusMin === "number" ? filter.statusMin : null,
        statusMax: typeof filter.statusMax === "number" ? filter.statusMax : null,
        mimeType: typeof filter.mimeType === "string" ? filter.mimeType : null,
        search: typeof filter.search === "string" ? filter.search : null,
        inScopeOnly: booleanValue(filter.inScopeOnly, false),
        sortBy: typeof filter.sortBy === "string" ? filter.sortBy : "timestamp",
        sortDescending: booleanValue(filter.sortDescending, true),
      };
    }
    historyInspectorsVisible = booleanValue(source.historyInspectorsVisible, true);
    restoredHistoryDetailId = typeof source.historyDetailId === "string" ? source.historyDetailId : null;
    tabGroups = restoreTabGroups(source.tabGroups);
    const restoredReplay = Array.isArray(source.replayTabs) ? source.replayTabs.flatMap((tab) => {
      try { return [restoreReplayTab(tab)]; } catch { return []; }
    }) : [];
    if (restoredReplay.length) replayTabs = restoredReplay;
    const maxReplayId = Math.max(...replayTabs.map((tab) => tab.id));
    activeReplayId = replayTabs.some((tab) => tab.id === source.activeReplayId) ? source.activeReplayId as number : replayTabs[0].id;
    nextReplayId = Math.max(maxReplayId + 1, numberValue(source.nextReplayId, maxReplayId + 1));
    replayDraft = source.replayDraft && typeof source.replayDraft === "object" && !Array.isArray(source.replayDraft)
      ? { tabId: numberValue((source.replayDraft as Record<string, unknown>).tabId, activeReplayId), value: textValue((source.replayDraft as Record<string, unknown>).value) }
      : null;
    if (source.fuzz) fuzz = restoreFuzzState(source.fuzz);
    forgeWorkspace = restoreForgeWorkspace(source.forge);
    if (["proxy", "display", "storage", "certificates", "ai", "miscellaneous", "keyboard", "updates", "about"].includes(textValue(source.settingsSection))) settingsSection = source.settingsSection as SettingsSection;
  }

  function markWorkspaceDirty() {
    if (!workspaceReady || !isTauri() || !snapshot?.project.currentProjectPath) return;
    clearTimeout(workspaceSaveTimer);
    workspaceSaveTimer = setTimeout(() => { void persistWorkspace().catch(showError); }, 750);
  }

  let workspaceSavePromise: Promise<void> | null = null;
  let workspaceSaveQueued = false;

  async function persistWorkspace() {
    const lifecycleToken = projectLifecycleToken;
    const projectPath = snapshot?.project.currentProjectPath;
    if (!workspaceReady || !isTauri() || !projectPath) return;
    clearTimeout(workspaceSaveTimer);
    while (true) {
      if (lifecycleToken !== projectLifecycleToken || snapshot?.project.currentProjectPath !== projectPath) return;
      if (workspaceSavePromise) {
        workspaceSaveQueued = true;
        await workspaceSavePromise;
        continue;
      }
      const operation = commands.saveWorkspace(JSON.stringify(buildWorkspaceSnapshot()));
      workspaceSavePromise = operation;
      try {
        await operation;
      } finally {
        workspaceSavePromise = null;
      }
      if (lifecycleToken !== projectLifecycleToken || snapshot?.project.currentProjectPath !== projectPath) return;
      if (!workspaceSaveQueued) return;
      workspaceSaveQueued = false;
    }
  }

  onMount(() => {
    document.getElementById("witness-boot-splash")?.remove();
    const clockTimer = window.setInterval(() => (statusClock = new Date()), 1_000);
    if (!isTauri()) {
      previewTheme = "dark";
      statusMessage = "Browser preview — launch with npm run tauri dev for proxy controls";
      return () => window.clearInterval(clockTimer);
    }
    let unlisten: (() => void) | undefined;
    let unlistenClose: (() => void) | undefined;
    let closingWindow = false;
    void (async () => {
      try {
        unlisten = await onWitnessEvent(handleEvent);
        unlistenClose = await getCurrentWindow().onCloseRequested(async (event) => {
          if (closingWindow) return;
          event.preventDefault();
          if (busy) {
            showError(new Error("Finish the current action before closing the window"));
            return;
          }
          closingWindow = true;
          try {
            if (snapshot?.project.temporary) {
              busy = true;
              try {
                await prepareProjectTransition();
                closeWindowAfterTemporaryAction = true;
                closeTemporaryDialog = true;
              } finally {
                busy = false;
              }
              closingWindow = false;
              return;
            }
            const closed = await closeProject(true);
            if (!closed) closingWindow = false;
          } catch (reason) {
            await recoverProjectTransition();
            closingWindow = false;
            showError(reason);
          }
        });
        const value = await refreshSnapshot();
        workspaceReady = false;
        resetWorkspaceState();
        for (const event of startupEvents.splice(0)) applyEvent(event);
        statusMessage = "Ready";
        recentProjects = await commands.getRecentProjects();
        if (value.project.currentProjectPath) {
          let workspaceInvalid = false;
          const workspace = await commands.getWorkspace();
          if (workspace) {
            try { restoreWorkspaceSnapshot(workspace); }
            catch (reason) { workspaceInvalid = true; statusMessage = "Project opened with a fresh workspace; saved workspace data was invalid"; showError(reason); }
          }
          workspaceReady = true;
          if (activeTab === "History" || activeTab === "Site Map") void loadHistory(true);
          if (!workspaceInvalid) void persistWorkspace().catch(showError);
          await setProjectWindowMode(true);
        } else {
          // Launcher geometry already matches tauri.conf.json at boot; a full
          // resize here visibly shrinks the window on Windows after splash.
          await ensureLauncherWindowChrome();
        }
        // Expose for Settings → About → Replay tutorial & console (manual trigger)
        (window as any).__witnessTutorial = { start: startTutorial };
        scheduleBackgroundUpdateCheck();
      } catch (reason) {
        showError(reason);
      }
    })();
    const memoryTimer = window.setInterval(() => {
      void refreshSnapshot().catch(showError);
    }, 5_000);
    window.addEventListener("keydown", handleShortcut);
    return () => {
      unlisten?.();
      unlistenClose?.();
      window.clearInterval(clockTimer);
      window.clearInterval(memoryTimer);
      window.removeEventListener("keydown", handleShortcut);
    };
  });

  function closeReplaySearch() {
    if (!replaySearchOpen) return;
    replaySearchOpen = false;
    replaySearchQuery = "";
    const opener = replaySearchOpener;
    replaySearchOpener = null;
    window.requestAnimationFrame(() => {
      if (opener?.isConnected) opener.focus();
    });
  }

  function openReplaySearch(event?: Event) {
    replaySearchOpener = event?.currentTarget instanceof HTMLElement
      ? event.currentTarget
      : document.activeElement instanceof HTMLElement ? document.activeElement : null;
    replaySearchOpen = true;
  }

  $effect(() => {
    if (!replaySearchOpen) return;
    window.requestAnimationFrame(() => {
      replaySearchDialogElement?.querySelector<HTMLInputElement>("input")?.focus();
    });
  });

  function openReplaySearchResult(tabId: number) {
    switchReplayTab(tabId);
    closeReplaySearch();
  }

  function updateReplayTabScrollState() {
    const element = replayTabScrollElement;
    replayTabsCanScrollRight = Boolean(element && element.scrollLeft + element.clientWidth < element.scrollWidth - 1);
  }

  function scrollReplayTabsRight() {
    replayTabScrollElement?.scrollBy({ left: 120, behavior: "smooth" });
  }

  $effect(() => {
    replayTabBarEntries.length;
    replayTabs.length;
    const element = replayTabScrollElement;
    if (!element) return;
    updateReplayTabScrollState();
    const observer = new ResizeObserver(updateReplayTabScrollState);
    observer.observe(element);
    return () => observer.disconnect();
  });

  function closeShortcuts() {
    if (!showShortcuts) return;
    showShortcuts = false;
    const opener = shortcutOpener;
    shortcutOpener = null;
    window.requestAnimationFrame(() => {
      if (opener?.isConnected) opener.focus();
    });
  }

  function toggleShortcuts(event?: Event) {
    if (showShortcuts) {
      closeShortcuts();
      return;
    }
    shortcutOpener = event?.currentTarget instanceof HTMLElement
      ? event.currentTarget
      : document.activeElement instanceof HTMLElement ? document.activeElement : null;
    showShortcuts = true;
  }

  $effect(() => {
    if (!showShortcuts) return;
    window.requestAnimationFrame(() => {
      const firstControl = shortcutDialogElement?.querySelector<HTMLElement>("button, [href]");
      (firstControl ?? shortcutDialogElement)?.focus();
    });
  });

  function trapShortcutDialog(event: KeyboardEvent) {
    if (event.key !== "Tab" || !shortcutDialogElement) return;
    const controls = [...shortcutDialogElement.querySelectorAll<HTMLElement>("button, [href], [tabindex]:not([tabindex=\"-1\"])")]
      .filter((element) => !element.hasAttribute("disabled") && element.offsetParent !== null);
    if (!controls.length) {
      event.preventDefault();
      shortcutDialogElement.focus();
      return;
    }
    const first = controls[0];
    const last = controls.at(-1) ?? first;
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  }

  function activeShortcutController(): ShortcutController | null {
    switch (activeTab) {
      case "AI": return forgeController;
      case "Fuzz": return fuzzController;
      case "Decoder": return decoderController;
      case "Comparer": return comparerController;
      case "Scope": return scopeController;
      case "Organizer": return organizerController;
      case "ID+": return identityController;
      case "Site Map": return siteMapController;
      case "Logs": return logController;
      default: return null;
    }
  }

  function hasComponentTransientState() {
    if (document.querySelector(".export-backdrop")) return true;
    if (activeTab === "AI" && document.querySelector(".forge-panel-host .approval")) return true;
    if (activeTab === "Site Map") return Boolean(document.querySelector(".node-menu"));
    if (activeTab === "Fuzz") return Boolean(document.querySelector(".intruder-shell .modal-backdrop"));
    if (activeTab === "Organizer") return Boolean(document.querySelector(".organizer-modal-backdrop"));
    if (activeTab === "ID+") return Boolean(document.querySelector(".identity-plus .modal-backdrop"));
    return false;
  }

  function hasBlockingTransientState() {
    return Boolean(
      showShortcuts ||
      replaySearchOpen ||
      contextMenu ||
      tabContextMenu ||
      tabGroupContextMenu ||
      tabRenameDialog ||
      tabGroupDialog ||
      exportDialog ||
      importDialog ||
      identityConfigDialog ||
      temporarySaveDialog ||
      closeTemporaryDialog ||
      hasComponentTransientState(),
    );
  }

  function closeTopmostTransientState(): boolean {
    if (showShortcuts) {
      closeShortcuts();
      return true;
    }
    if (replaySearchOpen) {
      closeReplaySearch();
      return true;
    }
    if (closeTemporaryDialog) {
      cancelTemporaryClose();
      return true;
    }
    if (temporarySaveDialog) {
      if (busy) return true;
      cancelTemporarySaveDialog();
      return true;
    }
    if (identityConfigDialog) {
      if (identityDialogLoading) return true;
      identityConfigDialog = false;
      return true;
    }
    if (importDialog) {
      importDialog = null;
      return true;
    }
    if (exportDialog) {
      if (exportBusy) return true;
      exportDialog = false;
      closeAfterProjectSave = false;
      return true;
    }
    if (tabGroupDialog) {
      tabGroupDialog = null;
      tabGroupError = "";
      return true;
    }
    if (tabRenameDialog) {
      tabRenameDialog = null;
      return true;
    }
    if (tabGroupContextMenu) {
      tabGroupContextMenu = null;
      return true;
    }
    if (tabContextMenu) {
      tabContextMenu = null;
      return true;
    }
    if (contextMenu) {
      contextMenu = null;
      return true;
    }
    if (hasComponentTransientState() && dispatchController(activeShortcutController(), "transient.close")) {
      return true;
    }
    return false;
  }

  function activeHistoryEntry() {
    return historyDetail?.entry ?? null;
  }

  function selectedInterceptRequest(): { raw: Uint8Array; tls: boolean } | null {
    if (!selectedIntercept) return null;
    const raw = selectedIntercept.kind === "response"
      ? selectedIntercept.requestRaw ?? new Uint8Array()
      : selectedIntercept.raw;
    if (!raw.length) return null;
    return { raw, tls: selectedIntercept.url.startsWith("https://") };
  }

  function selectedTextOr(raw: Uint8Array): string {
    const selected = window.getSelection()?.toString() ?? "";
    return selected || decodeHttpText(raw);
  }

  function selectedFuzzTransfer(): IntruderResult | null {
    return activeFuzzScan?.selectedResultId
      ? activeFuzzScan.results.find((result) => result.id === activeFuzzScan.selectedResultId) ?? null
      : null;
  }

  function dispatchController(controller: ShortcutController | null, action: string): boolean {
    if (!controller) return false;
    const result = controller.handleShortcut(action);
    if (result instanceof Promise) {
      void result.catch(showError);
      return true;
    }
    return result;
  }

  function dispatchShortcutAction(definition: ShortcutDefinition, event: KeyboardEvent): boolean {
    switch (definition.action) {
      case "project.save":
        if (!snapshot?.project.currentProjectPath) return false;
        void saveProjectShortcut();
        return true;
      case "settings.open":
        openSettings(settingsSection);
        return true;
      case "shortcuts.toggle":
        toggleShortcuts(event);
        return true;
      case "proxy.forward":
        if (!selectedIntercept) return false;
        void resolveIntercept("forward");
        return true;
      case "proxy.drop":
        if (!selectedIntercept) return false;
        void resolveIntercept("drop");
        return true;
      case "proxy.forwardAll":
        if (!pendingIntercepts.length) return false;
        void resolveAllIntercepts("forward");
        return true;
      case "proxy.dropAll":
        if (!pendingIntercepts.length) return false;
        void resolveAllIntercepts("drop");
        return true;
      case "proxy.selectPrevious":
      case "proxy.selectNext": {
        if (!pendingIntercepts.length) return false;
        const index = pendingIntercepts.findIndex((entry) => entry.id === selectedInterceptId);
        const offset = definition.action.endsWith("Previous") ? -1 : 1;
        const next = index < 0
          ? (offset < 0 ? pendingIntercepts.length - 1 : 0)
          : Math.max(0, Math.min(pendingIntercepts.length - 1, index + offset));
        selectIntercept(pendingIntercepts[next].id);
        return true;
      }
      case "transfer.proxy.replay": {
        const selected = selectedInterceptRequest();
        if (!selected) return false;
        void requestReplayTab(selected.raw, selected.tls);
        return true;
      }
      case "transfer.proxy.fuzz": {
        const selected = selectedInterceptRequest();
        if (!selected) return false;
        sendRawToFuzz(selected.raw, selected.tls);
        return true;
      }
      case "transfer.proxy.decoder": {
        const selected = selectedInterceptRequest();
        if (!selected) return false;
        sendToDecoder(selectedTextOr(selected.raw));
        return true;
      }
      case "transfer.proxy.organizer": {
        const selected = selectedInterceptRequest();
        if (!selected) return false;
        void saveToOrganizer(selected.raw, selectedIntercept?.kind === "response" ? selectedIntercept.raw : new Uint8Array(), selected.tls, "Proxy");
        return true;
      }
      case "history.selectPrevious":
      case "history.selectNext": {
        if (!history.length) return false;
        const index = history.findIndex((entry) => entry.id === historyDetail?.entry.id);
        const offset = definition.action.endsWith("Previous") ? -1 : 1;
        const next = Math.max(0, Math.min(history.length - 1, (index < 0 ? 0 : index) + offset));
        void selectHistory(history[next]);
        return true;
      }
      case "history.copyRequest": {
        const entry = activeHistoryEntry();
        if (!entry || activeTab !== "History" || window.getSelection()?.toString()) return false;
        void navigator.clipboard.writeText(decodeHttpText(new Uint8Array(historyDetail!.request))).then(() => (statusMessage = "Request copied")).catch(showError);
        return true;
      }
      case "history.deleteEntry": {
        const entry = activeHistoryEntry();
        if (!entry) return false;
        void deleteEntry(entry);
        return true;
      }
      case "transfer.history.replay": {
        const entry = activeHistoryEntry();
        if (!entry) return false;
        void sendEntryToReplay(entry);
        return true;
      }
      case "transfer.history.fuzz": {
        const entry = activeHistoryEntry();
        if (!entry) return false;
        void sendEntryToFuzz(entry);
        return true;
      }
      case "transfer.history.decoder": {
        const entry = activeHistoryEntry();
        if (!entry) return false;
        void sendHistoryEntryToDecoder(entry);
        return true;
      }
      case "transfer.history.organizer": {
        const entry = activeHistoryEntry();
        if (!entry) return false;
        void saveHistoryToOrganizer(entry);
        return true;
      }
      case "siteMap.selectPrevious":
      case "siteMap.selectNext":
      case "siteMap.expandAll":
      case "siteMap.collapseAll":
      case "siteMap.openSelected":
        return dispatchController(siteMapController, definition.action);
      case "siteMap.deleteSelected": {
        const entry = history.find((candidate) => candidate.id === siteMapWorkspace.selectedEntryId);
        if (!entry) return false;
        void deleteEntry(entry);
        return true;
      }
      case "transfer.siteMap.replay":
      case "transfer.siteMap.fuzz":
      case "transfer.siteMap.decoder":
      case "transfer.siteMap.organizer": {
        const entry = history.find((candidate) => candidate.id === siteMapWorkspace.selectedEntryId);
        if (!entry) return false;
        if (definition.action.endsWith("replay")) void sendEntryToReplay(entry);
        else if (definition.action.endsWith("fuzz")) void sendEntryToFuzz(entry);
        else if (definition.action.endsWith("decoder")) void sendHistoryEntryToDecoder(entry);
        else void saveHistoryToOrganizer(entry);
        return true;
      }
      case "replay.send":
        if (!activeReplay?.request.length || activeReplay.sending) return false;
        void runReplay();
        return true;
      case "replay.search":
        openReplaySearch(event);
        return true;
      case "replay.newTab":
        sendRawToReplay(new Uint8Array(), true);
        return true;
      case "replay.duplicateTab":
        if (!activeReplay) return false;
        duplicateReplayTab();
        return true;
      case "replay.closeTab":
        if (!activeReplay) return false;
        closeReplayTab(activeReplay.id);
        return true;
      case "replay.reopenTab":
        return restoreLastClosedTab("Replay");
      case "replay.previousRequest":
        if (!activeReplay?.history.length) return false;
        navigateReplayHistory(-1);
        return true;
      case "replay.nextRequest":
        if (!activeReplay?.history.length) return false;
        navigateReplayHistory(1);
        return true;
      case "replay.configureIdentities":
        if (!activeReplay) return false;
        void openIdentityConfig();
        return true;
      case "transfer.replay.replay":
        if (!activeReplay?.request.length) return false;
        flushReplayDraft();
        sendRawToReplay(activeReplay.request, activeReplay.tls);
        return true;
      case "transfer.replay.fuzz":
        if (!activeReplay?.request.length) return false;
        flushReplayDraft();
        sendRawToFuzz(activeReplay.request, activeReplay.tls);
        return true;
      case "transfer.replay.decoder":
        if (!activeReplay?.request.length) return false;
        sendToDecoder(selectedTextOr(encodeHttpText(activeReplayRequestText)));
        return true;
      case "transfer.replay.organizer":
        if (!activeReplay?.request.length) return false;
        flushReplayDraft();
        void saveToOrganizer(activeReplay.request, selectedReplayResponse(activeReplay), activeReplay.tls, "Replay");
        return true;
      case "fuzz.search":
      case "fuzz.launch":
      case "fuzz.stop":
      case "fuzz.results":
      case "fuzz.newTab":
      case "fuzz.duplicateTab":
        return dispatchController(fuzzController, definition.action);
      case "fuzz.closeTab":
        if (!activeFuzz || activeFuzz.scans.some((scan) => scan.running)) return false;
        closeFuzzTab(activeFuzz.id);
        return true;
      case "fuzz.reopenTab":
        return restoreLastClosedTab("Fuzz");
      case "transfer.fuzz.replay":
      case "transfer.fuzz.fuzz":
      case "transfer.fuzz.decoder":
      case "transfer.fuzz.organizer": {
        const result = selectedFuzzTransfer();
        if (!result) return false;
        if (definition.action.endsWith("replay")) sendRawToReplay(result.request, activeFuzz?.tls ?? true);
        else if (definition.action.endsWith("fuzz")) sendRawToFuzz(result.request, activeFuzz?.tls ?? true);
        else if (definition.action.endsWith("decoder")) sendToDecoder(decodeHttpText(result.response.length ? result.response : result.request));
        else void saveToOrganizer(result.request, result.response, activeFuzz?.tls ?? true, "Fuzz");
        return true;
      }
      case "organizer.selectPrevious":
      case "organizer.selectNext":
      case "organizer.openSelected":
      case "organizer.deleteSelected":
      case "organizer.createFolder":
      case "organizer.export":
      case "organizer.import":
        return dispatchController(organizerController, definition.action);
      case "transfer.organizer.replay":
      case "transfer.organizer.fuzz":
      case "transfer.organizer.decoder":
      case "transfer.organizer.organizer":
        return dispatchController(organizerController, definition.action);
      case "identity.createGroup":
      case "identity.createIdentity":
      case "identity.selectPrevious":
      case "identity.selectNext":
      case "identity.deleteSelected":
      case "identity.export":
      case "identity.import":
        return dispatchController(identityController, definition.action);
      case "decoder.focusFilter":
      case "decoder.run":
      case "decoder.clear":
      case "decoder.reverse":
      case "decoder.useOutput":
      case "decoder.copyOutput":
        return dispatchController(decoderController, definition.action);
      case "comparer.focusLeft":
      case "comparer.focusRight":
      case "comparer.recompute":
      case "comparer.clear":
      case "comparer.toggleLayout":
        return dispatchController(comparerController, definition.action);
      case "scope.focusFilter":
      case "scope.create":
      case "scope.selectPrevious":
      case "scope.selectNext":
      case "scope.edit":
      case "scope.delete":
      case "scope.submit":
        return dispatchController(scopeController, definition.action);
      case "forge.focusComposer":
      case "forge.send":
      case "forge.stop":
      case "forge.newChat":
      case "forge.previousChat":
      case "forge.nextChat":
      case "forge.deleteChat":
        return dispatchController(forgeController, definition.action);
      case "logs.focusFilter":
      case "logs.export":
      case "logs.clear":
        return dispatchController(logController, definition.action);
      case "settings.previousSection":
      case "settings.nextSection":
      case "settings.openSection":
        return dispatchSettingsShortcut(definition.action, event);
      default:
        return false;
    }
  }

  function dispatchSettingsShortcut(action: string, event: KeyboardEvent): boolean {
    if (isEditableTarget(event.target) && action !== "settings.openSection") return false;
    const sectionIds: SettingsSection[] = ["proxy", "display", "storage", "keyboard", "certificates", "ai", "miscellaneous", "updates"];
    const current = Math.max(0, sectionIds.indexOf(settingsSection));
    if (action === "settings.openSection") {
      if (!(event.target instanceof HTMLElement) || !event.target.closest(".settings-sidebar")) return false;
      return false;
    }
    const direction = action === "settings.previousSection" ? -1 : 1;
    settingsSection = sectionIds[Math.max(0, Math.min(sectionIds.length - 1, current + direction))];
    markWorkspaceDirty();
    window.requestAnimationFrame(() => {
      document.querySelector<HTMLButtonElement>(`.settings-sidebar button[data-section="${settingsSection}"]`)?.focus();
    });
    return true;
  }

  function handleShortcut(event: KeyboardEvent) {
    if (event.isComposing || event.defaultPrevented) return;
    if (showShortcuts) {
      const overlayShortcut = resolveShortcut(event, ["global"], shortcutPlatform, activeShortcutModifier);
      if (overlayShortcut?.action === "shortcuts.toggle") {
        event.preventDefault();
        closeShortcuts();
        return;
      }
      trapShortcutDialog(event);
      if (event.key === "Escape" && !event.metaKey && !event.ctrlKey && !event.altKey && !event.shiftKey && !event.repeat) {
        event.preventDefault();
        closeShortcuts();
      }
      return;
    }
    if (event.key === "Escape" && !event.metaKey && !event.ctrlKey && !event.altKey && !event.shiftKey && !event.repeat) {
      if (closeTopmostTransientState()) {
        event.preventDefault();
        return;
      }
      if (dispatchController(activeShortcutController(), "transient.close")) {
        event.preventDefault();
        return;
      }
      if (dispatchController(forgeController, "forge.stop")) {
        event.preventDefault();
        return;
      }
      if (activeTab === "History" && historyDetail) {
        historyDetail = null;
        restoredHistoryDetailId = null;
        event.preventDefault();
        return;
      }
      if (activeTab === "Proxy" && selectedInterceptId) {
        selectedInterceptId = null;
        event.preventDefault();
        return;
      }
      return;
    }
    if (hasBlockingTransientState()) return;
    const definition = resolveShortcut(event, [activeTab, "global"], shortcutPlatform, activeShortcutModifier);
    if (!definition) return;
    if (dispatchShortcutAction(definition, event)) event.preventDefault();
  }

  function handleEvent(event: WitnessEvent) {
    eventCount += 1;
    if (!snapshot) {
      startupEvents.push(event);
      return;
    }
    applyEvent(event);
  }

  function applyEvent(event: WitnessEvent) {
    if (!snapshot) return;
    const type = event.payload.type;
    if (event.kind === "proxy") {
      if (type === "started") {
        snapshot.proxy.running = true;
        statusMessage = `Proxy listening on ${String(event.payload.address)}`;
      } else if (type === "stopped") {
        snapshot.proxy.running = false;
        snapshot.proxy.connectionCount = 0;
        statusMessage = "Proxy stopped";
      } else if (type === "connectionCount") {
        snapshot.proxy.connectionCount = Number(event.payload.count);
      } else if (type === "tlsStatus") {
        snapshot.proxy.certificateStatus = String(event.payload.status);
      } else if (type === "error") {
        showError(String(event.payload.message));
      }
    }
    if (event.kind === "history" && (type === "newEntry" || type === "deleted" || type === "cleared")) {
      // Defer reload when History/Site Map are hidden so background traffic
      // never janks Proxy/Fuzz tab switches. The pending refresh runs on
      // next switch via changeTab's stale check.
      historyStale = true;
      if (activeTab === "History" || activeTab === "Site Map") scheduleHistoryLoad(true, 150);
    }
    if (event.kind === "repeater") {
      statusMessage = `Replay ${String(event.payload.status)}`;
      if (event.payload.status === "openTab" && Array.isArray(event.payload.raw)) {
        sendRawToReplay(
          new Uint8Array(event.payload.raw as number[]),
          typeof event.payload.tls === "boolean" ? event.payload.tls : true,
        );
      }
    }
    if (event.kind === "interception" && (type === "request" || type === "response")) {
      const raw = new Uint8Array(event.payload.raw as number[]);
      const requestRaw = type === "response"
        ? new Uint8Array(event.payload.requestRaw as number[])
        : undefined;
      const requestMetadata = parseRequestMetadata(requestRaw ?? raw, String(event.payload.url ?? ""));
      const status = type === "response" ? parseResponseStatus(raw) : null;
      const entry: InterceptEntry = {
        id: String(event.payload.id),
        kind: type,
        raw,
        requestRaw,
        url: requestMetadata.url,
        host: requestMetadata.host,
        method: requestMetadata.method,
        status,
        length: raw.length,
        receivedAt: Date.now(),
      };
      pendingIntercepts.push(entry);
      selectedInterceptId ??= entry.id;
    } else if (event.kind === "interception" && type === "resolved") {
      removeIntercept(String(event.payload.id));
    }
  }

  function parseRequestMetadataText(value: string, suppliedUrl: string) {
    return parseRequestMetadataTextLib(value, suppliedUrl);
  }

  function sameRequestHeader(cache: RequestMetadataCache | null, ownerId: number | string, value: string) {
    if (!cache || cache.ownerId !== ownerId) return false;
    if (cache.headerPrefix === null) return value.length === 0;
    return value.startsWith(cache.headerPrefix);
  }

  function updateReplayMetadata(tabId: number, value: string) {
    if (sameRequestHeader(replayMetadataCache, tabId, value)) return;
    replayMetadataCache = {
      ownerId: tabId,
      headerPrefix: requestHeaderPrefix(value),
      metadata: parseRequestMetadataText(value, ""),
    };
  }

  function updateInterceptMetadata(entryId: string, value: string, fallbackHost: string) {
    if (sameRequestHeader(interceptMetadataCache, entryId, value)) return;
    const metadata = parseRequestMetadataText(value, "");
    interceptMetadataCache = {
      ownerId: entryId,
      headerPrefix: requestHeaderPrefix(value),
      metadata: { ...metadata, host: metadata.host || fallbackHost },
    };
  }

  function parseRequestMetadata(raw: Uint8Array, suppliedUrl: string) {
    return parseRequestMetadataLib(raw, suppliedUrl);
  }

  function hostname(value: string) {
    return parseHostnameLib(value);
  }

  function requestHost(raw: Uint8Array) {
    return requestHostLib(raw);
  }

  function parseResponseStatus(raw: Uint8Array) {
    const startLine = new TextDecoder().decode(raw.slice(0, 256)).split(/\r?\n/, 1)[0] ?? "";
    const status = Number(startLine.split(/\s+/, 3)[1]);
    return Number.isFinite(status) ? status : null;
  }

  function clearPendingIntercepts() {
    pendingIntercepts = [];
    selectedInterceptId = null;
    interceptDraft = null;
    interceptDraftChanged = null;
    interceptMetadataCache = null;
  }

  async function stopProxyAndClearPending() {
    await commands.stopProxy();
    clearPendingIntercepts();
  }

  function beginInterceptResolution(id: string) {
    if (resolvingInterceptIds.has(id)) return false;
    resolvingInterceptIds.add(id);
    return true;
  }

  function endInterceptResolution(id: string) {
    resolvingInterceptIds.delete(id);
  }

  function removeIntercept(id: string) {
    if (interceptDraft?.entryId === id) interceptDraft = null;
    if (interceptDraftChanged?.entryId === id) interceptDraftChanged = null;
    if (interceptMetadataCache?.ownerId === id) interceptMetadataCache = null;
    const wasSelected = selectedInterceptId === id;
    pendingIntercepts = pendingIntercepts.filter((entry) => entry.id !== id);
    if (wasSelected || (selectedInterceptId && !pendingIntercepts.some((entry) => entry.id === selectedInterceptId))) {
      selectedInterceptId = pendingIntercepts[0]?.id ?? null;
    }
  }

  function updateInterceptDraft(value: string) {
    if (selectedIntercept) {
      interceptDraft = { entryId: selectedIntercept.id, value };
      updateInterceptMetadata(selectedIntercept.id, value, selectedIntercept.host);
    }
  }

  function updateInterceptDraftState(changed: boolean) {
    if (selectedIntercept) interceptDraftChanged = { entryId: selectedIntercept.id, changed };
  }

  function flushInterceptDraft() {
    const draft = interceptDraft;
    if (!draft) {
      interceptDraftChanged = null;
      return;
    }
    const entry = pendingIntercepts.find((candidate) => candidate.id === draft.entryId);
    const changed = interceptDraftChanged?.entryId === draft.entryId && interceptDraftChanged.changed;
    if (entry && changed) {
      updateInterceptMetadata(entry.id, draft.value, entry.host);
      entry.raw = encodeHttpText(draft.value);
    }
    interceptDraft = null;
    interceptDraftChanged = null;
  }

  function selectIntercept(id: string) {
    flushInterceptDraft();
    selectedInterceptId = id;
  }

  function updateReplayDraft(value: string) {
    if (activeReplay) {
      replayDraft = { tabId: activeReplay.id, value };
      updateReplayMetadata(activeReplay.id, value);
      markWorkspaceDirty();
    }
  }

  function replayText(tab: ReplayTab | undefined) {
    if (!tab) return "";
    return replayDraft?.tabId === tab.id ? replayDraft.value : decodeHttpText(tab.request);
  }

  function flushReplayDraft() {
    const draft = replayDraft;
    if (!draft) return;
    const tab = replayTabs.find((candidate) => candidate.id === draft.tabId);
    if (tab) {
      updateReplayMetadata(tab.id, draft.value);
      tab.request = encodeHttpText(draft.value);
    }
    replayDraft = null;
    markWorkspaceDirty();
  }

  function switchReplayTab(id: number) {
    flushReplayDraft();
    // Paint the newly selected tab first; persistence is debounced anyway.
    activeReplayId = id;
    queueMicrotask(() => markWorkspaceDirty());
  }

  function tabForWorkspace(workspace: TabWorkspace, id: number) {
    return workspace === "Replay"
      ? replayTabs.find((tab) => tab.id === id)
      : fuzz.tabs.find((tab) => tab.id === id);
  }

  function tabIdsInBar(workspace: TabWorkspace) {
    const entries = workspace === "Replay"
      ? buildTabBarEntries<ReplayTab>(replayTabs)
      : buildTabBarEntries<IntruderWorkspaceTab>(fuzz.tabs);
    return entries.flatMap((entry) =>
      entry.kind === "tab" ? [entry.tab.id] : entry.tabs.map((tab) => tab.id),
    );
  }

  function handleHistoryContextAction(id: string) {
    const entry = contextMenu?.entry;
    if (!entry) return;
    contextMenu = null;
    if (id === "send-replay") return void sendEntryToReplay(entry);
    if (id === "send-fuzz") return void sendEntryToFuzz(entry);
    if (id === "save-organizer") return void saveHistoryToOrganizer(entry);
    if (id === "compare-request") return void compareHistory(entry, "request");
    if (id === "compare-response") return void compareHistory(entry, "response");
    if (id === "copy-url") return void navigator.clipboard.writeText(entry.url).catch(showError);
    if (id === "copy-curl") return void copyAsCurl(entry);
    if (id === "delete-entry") return void deleteEntry(entry);
  }

  function openTabContextMenu(event: MouseEvent, workspace: TabWorkspace, tabId: number) {
    event.preventDefault();
    contextMenu = null;
    tabGroupContextMenu = null;
    tabContextMenu = { x: event.clientX, y: event.clientY, workspace, tabId };
  }

  function closeTabContextMenu() {
    tabContextMenu = null;
  }

  function openTabGroupContextMenu(event: MouseEvent, workspace: TabWorkspace, groupId: string) {
    event.preventDefault();
    contextMenu = null;
    tabContextMenu = null;
    tabGroupContextMenu = { x: event.clientX, y: event.clientY, workspace, groupId };
  }

  function closeTabGroupContextMenu() {
    tabGroupContextMenu = null;
  }

  function handleTabContextAction(id: string) {
    if (id.startsWith("restore-closed-tab:")) return restoreClosedTab(Number(id.slice("restore-closed-tab:".length)));
    if (id === "rename-tab") return openTabRenameDialog();
    if (id === "close-tab") return closeTabFromContext();
    if (id === "close-tabs-left") return closeTabsFromContext("left");
    if (id === "close-tabs-right") return closeTabsFromContext("right");
    if (id === "remove-tab-from-group") return assignTabToGroup(null);
    if (id === "create-tab-group") return openCreateTabGroupDialog();
    if (id.startsWith("tab-group:")) return assignTabToGroup(id.slice("tab-group:".length));
  }

  function handleTabGroupContextAction(id: string) {
    if (id === "edit-tab-group") return openEditTabGroupDialog();
    if (id === "ungroup-tabs") return ungroupTabGroup();
    if (id === "close-group-tabs") return closeAllTabsInGroup();
  }

  function closeTabFromContext() {
    const menu = tabContextMenu;
    if (!menu) return;
    if (menu.workspace === "Replay") closeReplayTab(menu.tabId);
    else closeFuzzTab(menu.tabId);
    closeTabContextMenu();
  }

  function closeTabsFromContext(direction: "left" | "right") {
    const menu = tabContextMenu;
    if (!menu) return;
    const ids = tabIdsInBar(menu.workspace);
    const index = ids.indexOf(menu.tabId);
    if (index < 0) return closeTabContextMenu();
    const idsToClose = direction === "left" ? ids.slice(0, index) : ids.slice(index + 1);
    for (const id of idsToClose) {
      if (menu.workspace === "Replay") closeReplayTab(id);
      else closeFuzzTab(id);
    }
    closeTabContextMenu();
  }

  function openTabRenameDialog() {
    const menu = tabContextMenu;
    const tab = menu ? tabForWorkspace(menu.workspace, menu.tabId) : undefined;
    if (!menu || !tab) return closeTabContextMenu();
    tabRenameValue = tab.title;
    tabRenameDialog = { ...menu };
    closeTabContextMenu();
  }

  function saveTabRename() {
    const dialog = tabRenameDialog;
    const title = tabRenameValue.trim();
    if (!dialog || !title) return;
    const tab = tabForWorkspace(dialog.workspace, dialog.tabId);
    if (tab) {
      tab.title = title;
      markWorkspaceDirty();
    }
    tabRenameDialog = null;
  }

  function assignTabToGroup(groupId: string | null) {
    const menu = tabContextMenu;
    const tab = menu ? tabForWorkspace(menu.workspace, menu.tabId) : undefined;
    if (!menu || !tab) return closeTabContextMenu();
    if (groupId && !tabGroups.some((group) => group.id === groupId)) return;
    tab.groupId = groupId;
    markWorkspaceDirty();
    closeTabContextMenu();
  }

  function openCreateTabGroupDialog() {
    const menu = tabContextMenu;
    const tab = menu ? tabForWorkspace(menu.workspace, menu.tabId) : undefined;
    if (!menu || !tab) return closeTabContextMenu();
    tabGroupName = "";
    tabGroupColor = tabGroupColors[0];
    tabGroupError = "";
    tabGroupSelectedTabIds = [menu.tabId];
    tabGroupDialog = { mode: "create", workspace: menu.workspace, tabId: menu.tabId };
    closeTabContextMenu();
  }

  function openEditTabGroupDialog() {
    const menu = tabGroupContextMenu;
    const group = menu ? tabGroups.find((candidate) => candidate.id === menu.groupId) : undefined;
    if (!menu || !group) return closeTabGroupContextMenu();
    tabGroupName = group.name;
    tabGroupColor = group.color;
    tabGroupError = "";
    tabGroupSelectedTabIds = tabsForWorkspace(menu.workspace)
      .filter((tab) => tab.groupId === group.id)
      .map((tab) => tab.id);
    tabGroupDialog = { mode: "edit", workspace: menu.workspace, groupId: group.id };
    closeTabGroupContextMenu();
  }

  function toggleTabGroupTab(tabId: number, selected: boolean) {
    tabGroupSelectedTabIds = selected
      ? [...new Set([...tabGroupSelectedTabIds, tabId])]
      : tabGroupSelectedTabIds.filter((id) => id !== tabId);
  }

  function toggleAllTabGroupTabs() {
    tabGroupSelectedTabIds = tabGroupAllTabsSelected ? [] : tabGroupDialogTabs.map((tab) => tab.id);
  }

  function createTabGroupFromDialog() {
    const dialog = tabGroupDialog;
    const name = tabGroupName.trim();
    if (!dialog) return;
    if (!name) {
      tabGroupError = "Enter a name for this tab group.";
      return;
    }
    if (dialog.mode === "edit") {
      const group = tabGroups.find((candidate) => candidate.id === dialog.groupId);
      if (!group) return (tabGroupDialog = null);
      group.name = name;
      group.color = tabGroupColorValue(tabGroupColor);
      const selectedIds = new Set(tabGroupSelectedTabIds);
      for (const tab of tabsForWorkspace(dialog.workspace)) {
        if (selectedIds.has(tab.id)) tab.groupId = group.id;
        else if (tab.groupId === group.id) tab.groupId = null;
      }
      if (!tabInAnyWorkspace(group.id)) {
        tabGroups = tabGroups.filter((candidate) => candidate.id !== group.id);
      }
      markWorkspaceDirty();
      tabGroupDialog = null;
      return;
    }
    const group: TabGroup = {
      id: uniqueExecutionId(),
      name,
      color: tabGroupColorValue(tabGroupColor),
      collapsed: false,
    };
    tabGroups.push(group);
    const selectedIds = new Set(tabGroupSelectedTabIds);
    const tabs = dialog.workspace === "Fuzz" ? fuzz.tabs : replayTabs;
    for (const tab of tabs) {
      if (selectedIds.has(tab.id)) tab.groupId = group.id;
    }
    markWorkspaceDirty();
    tabGroupDialog = null;
  }

  function tabsForWorkspace(workspace: TabWorkspace) {
    return workspace === "Replay" ? replayTabs : fuzz.tabs;
  }

  function ungroupTabGroup() {
    const menu = tabGroupContextMenu;
    const group = menu ? tabGroups.find((candidate) => candidate.id === menu.groupId) : undefined;
    if (!menu || !group) return closeTabGroupContextMenu();
    for (const tab of tabsForWorkspace(menu.workspace)) {
      if (tab.groupId === group.id) tab.groupId = null;
    }
    if (!tabInAnyWorkspace(group.id)) {
      tabGroups = tabGroups.filter((candidate) => candidate.id !== group.id);
    }
    markWorkspaceDirty();
    closeTabGroupContextMenu();
  }

  function closeAllTabsInGroup() {
    const menu = tabGroupContextMenu;
    const group = menu ? tabGroups.find((candidate) => candidate.id === menu.groupId) : undefined;
    if (!menu || !group) return closeTabGroupContextMenu();
    const ids = tabsForWorkspace(menu.workspace)
      .filter((tab) => tab.groupId === group.id)
      .map((tab) => tab.id);
    for (const id of ids) {
      if (menu.workspace === "Replay") closeReplayTab(id);
      else closeFuzzTab(id);
    }
    if (!tabInAnyWorkspace(group.id)) {
      tabGroups = tabGroups.filter((candidate) => candidate.id !== group.id);
    }
    markWorkspaceDirty();
    closeTabGroupContextMenu();
  }

  function tabInAnyWorkspace(groupId: string) {
    return replayTabs.some((tab) => tab.groupId === groupId) || fuzz.tabs.some((tab) => tab.groupId === groupId);
  }

  function toggleTabGroup(groupId: string) {
    const group = tabGroups.find((candidate) => candidate.id === groupId);
    if (!group) return;
    group.collapsed = !group.collapsed;
    markWorkspaceDirty();
  }

  function flushFuzzDraft() {
    const draft = fuzz.editorDraft;
    if (!draft) return;
    const tab = fuzz.tabs.find((candidate): candidate is IntruderTab => candidate.kind === "setup" && candidate.id === draft.tabId);
    if (tab) tab.request = encodeHttpText(draft.value);
    fuzz.editorDraft = null;
    markWorkspaceDirty();
  }

  function flushActiveEditorDraft() {
    if (activeTab === "Proxy") flushInterceptDraft();
    if (activeTab === "Replay") flushReplayDraft();
    if (activeTab === "Fuzz") flushFuzzDraft();
  }

  function changeTab(tab: Tab) {
    if (tab === "Logs" && !snapshot?.settings.showLogsTab) return;
    flushActiveEditorDraft();
    // Paint the new tab first; workspace persistence is debounced anyway.
    activeTab = tab;
    contextMenu = null;
    tabContextMenu = null;
    tabGroupContextMenu = null;
    queueMicrotask(() => markWorkspaceDirty());
    if (tab !== "History" && tab !== "Site Map") return;
    // Switching tabs is instant when the cached page is still fresh.
    // Reload only when the filter/project changed, background traffic
    // marked the cache stale, or no rows are cached yet.
    const key = currentHistoryCacheKey();
    if (!historyStale && key === historyCacheKey && history.length) return;
    void loadHistory(true);
  }

  function openSettings(section: SettingsSection) {
    settingsSection = section;
    markWorkspaceDirty();
    changeTab("Settings");
  }

  function scheduleHistoryLoad(reset = true, delay = 250) {
    clearTimeout(filterTimer);
    filterTimer = setTimeout(() => void loadHistory(reset), delay);
  }

  async function loadHistory(reset = false) {
    if (!isTauri() || historyLoading || !snapshot?.project.currentProjectPath) return;
    if (reset) {
      const key = currentHistoryCacheKey();
      if (!historyStale && key === historyCacheKey && history.length) return;
    }
    historyLoading = true;
    try {
      const offset = reset ? 0 : history.length;
      const page = await commands.queryHistory(historyFilter, offset, 500);
      history = reset ? page : [...history, ...page];
      historyCacheKey = currentHistoryCacheKey();
      historyStale = false;
      historyHasMore = page.length === 500;
      if (reset && historyDetail && !page.some((entry) => entry.id === historyDetail?.entry.id)) {
        historyDetail = null;
      }
      if (reset && restoredHistoryDetailId) {
        const entry = page.find((candidate) => candidate.id === restoredHistoryDetailId);
        if (entry) {
          restoredHistoryDetailId = null;
          void selectHistory(entry);
        }
      }
    } catch (reason) {
      if (!String(reason).includes("cancelled")) showError(reason);
    } finally {
      historyLoading = false;
    }
  }

  async function selectHistory(entry: HistoryEntry) {
    historyInspectorsVisible = true;
    restoredHistoryDetailId = entry.id;
    markWorkspaceDirty();
    try {
      historyDetail = await commands.getHistoryDetail(entry.id);
    } catch (reason) {
      showError(reason);
    }
  }

  function sortHistory(column: string) {
    historyFilter.sortDescending = historyFilter.sortBy === column ? !historyFilter.sortDescending : false;
    historyFilter.sortBy = column;
    markWorkspaceDirty();
    scheduleHistoryLoad(true, 0);
  }

  async function deleteEntry(entry: HistoryEntry) {
    if (!(await askForConfirmation({
      title: "Delete history entry?",
      message: `Delete this History entry for ${entry.method} ${entry.url}?`,
      confirmLabel: "Delete entry",
    }))) return;
    try {
      const deleted = await commands.deleteHistoryEntry(entry.id);
      if (!deleted) throw new Error("History entry was not found or could not be deleted");
      contextMenu = null;
      if (historyDetail?.entry.id === entry.id) historyDetail = null;
      markWorkspaceDirty();
    } catch (reason) {
      showError(reason);
    }
  }

  async function clearAllHistory() {
    if (!(await askForConfirmation({
      title: "Delete all history?",
      message: "Delete all captured history and body files from this project?",
      confirmLabel: "Delete all",
    }))) return;
    try {
      await commands.clearHistory();
      history = [];
      historyDetail = null;
      restoredHistoryDetailId = null;
      historyCacheKey = currentHistoryCacheKey();
      historyStale = false;
      historyHasMore = false;
      markWorkspaceDirty();
    } catch (reason) {
      showError(reason);
    }
  }

  async function sendEntryToReplay(entry: HistoryEntry) {
    try {
      const detail = await commands.getHistoryDetail(entry.id);
      if (detail) {
        await commands.openInRepeater(new Uint8Array(detail.request), entry.url.startsWith("https://"));
      }
      contextMenu = null;
    } catch (reason) {
      showError(reason);
    }
  }

  async function sendEntryToFuzz(entry: HistoryEntry) {
    try {
      const detail = await commands.getHistoryDetail(entry.id);
      if (detail) sendRawToFuzz(new Uint8Array(detail.request), entry.url.startsWith("https://"));
      contextMenu = null;
    } catch (reason) {
      showError(reason);
    }
  }

  async function saveHistoryToOrganizer(entry: HistoryEntry) {
    try {
      const detail = await commands.getHistoryDetail(entry.id);
      if (detail) {
        await saveToOrganizer(
          new Uint8Array(detail.request),
          new Uint8Array(detail.response),
          entry.url.startsWith("https://"),
          "History",
        );
      }
      contextMenu = null;
    } catch (reason) {
      showError(reason);
    }
  }

  async function saveToOrganizer(
    request: Uint8Array,
    response: Uint8Array = new Uint8Array(),
    tls = true,
    source = "Request",
  ) {
    if (!request.length) return;
    try {
      await commands.createOrganizerItem({
        title: "",
        folderId: null,
        stageId: null,
        request: Array.from(request),
        response: Array.from(response),
        tls,
        source,
        notes: "",
        tags: [],
      });
      organizerRevision += 1;
      statusMessage = `Saved to Organizer · ${requestHost(request) || "request"}`;
    } catch (reason) {
      showError(reason);
    }
  }

  function sendRawToReplay(raw: Uint8Array, tls?: boolean) {
    flushReplayDraft();
    const request = normalizeHttpLineEndingBytes(raw);
    const requestLine = new TextDecoder().decode(request.slice(0, 2_048)).split(/\r?\n/, 1)[0] ?? "";
    const target = requestLine.split(/\s+/, 3)[1] ?? "";
    const resolvedTls = tls ?? (target.startsWith("http://") ? false : true);
    const id = nextReplayId++;
    replayTabs.push(createReplayTab(id, request.slice(), resolvedTls));
    activeReplayId = id;
    markWorkspaceDirty();
    changeTab("Replay");
    return id;
  }

  function jsonPointerSegments(pointer: string) {
    if (pointer === "") return [];
    if (!pointer.startsWith("/")) throw new Error("JSON path must be a JSON Pointer beginning with '/'");
    return pointer
      .slice(1)
      .split("/")
      .map((segment) => segment.replaceAll("~1", "/").replaceAll("~0", "~"));
  }

  function replaceJsonPointer(root: unknown, pointer: string, value: string) {
    const segments = jsonPointerSegments(pointer);
    if (!segments.length) return value;
    let current: unknown = root;
    for (let index = 0; index < segments.length; index += 1) {
      const segment = segments[index];
      const last = index === segments.length - 1;
      if (Array.isArray(current)) {
        if (!/^0$|^[1-9]\d*$/.test(segment)) throw new Error(`JSON path segment '${segment}' is not an array index`);
        const arrayIndex = Number(segment);
        if (!Number.isSafeInteger(arrayIndex) || arrayIndex < 0 || arrayIndex >= current.length) {
          throw new Error(`JSON path index '${segment}' does not exist`);
        }
        if (last) current[arrayIndex] = value;
        else current = current[arrayIndex];
      } else if (current && typeof current === "object") {
        const object = current as Record<string, unknown>;
        if (!(segment in object)) throw new Error(`JSON path segment '${segment}' does not exist`);
        if (last) object[segment] = value;
        else current = object[segment];
      } else {
        throw new Error(`JSON path cannot continue through '${segment}'`);
      }
    }
    return root;
  }

  function createReplayTabsFromAgent(input: { jsonPath: string; values: string[]; titleTemplate?: string }) {
    flushReplayDraft();
    const source = activeReplay;
    if (!source?.request.length) throw new Error("Create a Replay request first");
    const sourceText = replayText(source);
    const message = splitHttpMessage(sourceText);
    if (!message.complete || !message.body.trim()) throw new Error("The active Replay request must contain a JSON body");
    let parsed: unknown;
    try {
      parsed = JSON.parse(message.body);
    } catch {
      throw new Error("The active Replay request body is not valid JSON");
    }
    if (!input.jsonPath || !input.values.length || input.values.length > 100) {
      throw new Error("A JSON path and between 1 and 100 values are required");
    }

    const created: ReplayTab[] = [];
    for (const [index, value] of input.values.entries()) {
      const replacement = replaceJsonPointer(cloneJson(parsed), input.jsonPath, value);
      const serialized = JSON.stringify(replacement, null, 2);
      const body = message.lineEnding === "\r\n"
        ? serialized.replaceAll("\n", "\r\n")
        : message.lineEnding === "\r"
          ? serialized.replaceAll("\n", "\r")
          : serialized;
      const request = encodeHttpText(synchronizeHttpContentLength(`${message.head}${message.separator}${body}`));
      const id = nextReplayId++;
      const tab = createReplayTab(id, request, source.tls);
      const titleTemplate = input.titleTemplate?.trim() || "value {value}";
      tab.title = titleTemplate.replaceAll("{value}", value).replaceAll("{index}", String(index + 1));
      created.push(tab);
    }
    replayTabs.push(...created);
    activeReplayId = created[0].id;
    markWorkspaceDirty();
    changeTab("Replay");
    statusMessage = `Created ${created.length} Replay tabs`;
    return { tabIds: created.map((tab) => tab.id), count: created.length };
  }

  function startFuzzFromAgent(input: { positionText: string; values: string[] }) {
    flushReplayDraft();
    const source = activeReplay;
    if (!source?.request.length) throw new Error("Create a Replay request first");
    const positionText = input.positionText;
    const values = input.values.filter((value) => typeof value === "string");
    if (!positionText.trim() || !values.length || values.length > 100) {
      throw new Error("A request value and between 1 and 100 test values are required");
    }
    const sourceText = replayText(source);
    const position = sourceText.indexOf(positionText);
    if (position < 0) throw new Error("The requested fuzz position was not found in the active Replay request");
    const marked = `${sourceText.slice(0, position)}§${sourceText.slice(position, position + positionText.length)}§${sourceText.slice(position + positionText.length)}`;
    const next = createIntruderTab(fuzz.nextTabId++, encodeHttpText(marked), source.tls);
    next.title = `Forge ${next.id}`;
    next.warehouse.type = "list";
    next.warehouse.list.text = values.join("\n");
    fuzz.tabs.push(next);
    fuzz.activeTabId = next.id;
    fuzz.editorDraft = null;
    aiFuzzLaunch = { id: uniqueExecutionId(), tabId: next.id, action: "start" };
    markWorkspaceDirty();
    changeTab("Fuzz");
    statusMessage = `Started Fuzz test with ${values.length} values`;
    return { tabId: next.id, values: values.length, position: positionText };
  }

  async function addScopeFromAgent(input: { pattern: string; isRegex?: boolean; includeSubdomains?: boolean; isInScope?: boolean }) {
    const entry = await commands.addScopeEntry(
      input.pattern.trim(),
      input.isRegex === true,
      input.includeSubdomains === true,
      input.isInScope !== false,
    );
    markWorkspaceDirty();
    statusMessage = `Scope ${entry.isInScope ? "include" : "exclude"} added`;
    return entry;
  }

  async function updateScopeFromAgent(input: { id: number; pattern: string; isRegex?: boolean; includeSubdomains?: boolean; isInScope?: boolean }) {
    const entry = await commands.updateScopeEntry(
      input.id,
      input.pattern.trim(),
      input.isRegex === true,
      input.includeSubdomains === true,
      input.isInScope !== false,
    );
    markWorkspaceDirty();
    statusMessage = "Scope entry updated";
    return entry;
  }

  async function removeScopeFromAgent(id: number) {
    if (!(await commands.removeScopeEntry(id))) throw new Error("Scope entry was not found");
    markWorkspaceDirty();
    statusMessage = "Scope entry removed";
    return { removed: true, id };
  }

  function redactAiJson(value: unknown): unknown {
    if (Array.isArray(value)) return value.map(redactAiJson);
    if (!value || typeof value !== "object") return value;
    const result: Record<string, unknown> = {};
    for (const [key, child] of Object.entries(value as Record<string, unknown>)) {
      result[key] = /pass(word)?|secret|token|credential|authorization|cookie|api[-_]?key/i.test(key)
        ? "[redacted]"
        : redactAiJson(child);
    }
    return result;
  }

  function redactAiRequestContext(value: string) {
    const message = splitHttpMessage(value);
    if (!message.complete) return value;
    const head = message.head
      .split(/\r\n|\n|\r/)
      .map((line) => /^(authorization|proxy-authorization|cookie|set-cookie|x-api-key|x-auth-token)\s*:/i.test(line)
        ? `${line.slice(0, line.indexOf(":"))}: [redacted]`
        : line)
      .join(message.lineEnding);
    let body = message.body;
    try {
      const parsed = JSON.parse(body);
      const redacted = JSON.stringify(redactAiJson(parsed), null, 2);
      body = message.lineEnding === "\r\n"
        ? redacted.replaceAll("\n", "\r\n")
        : message.lineEnding === "\r"
          ? redacted.replaceAll("\n", "\r")
          : redacted;
    } catch {
      // Non-JSON request content remains available for the user's request context.
    }
    return `${head}${message.separator}${body}`;
  }

  function buildAiContext() {
    const project = snapshot?.project;
    const parts = [
      `Project: ${project?.name ?? "Unnamed project"}`,
      `Active workspace tab: ${activeTab}`,
      `Active Replay tab: ${activeReplay?.title ?? "none"}`,
    ];
    if (activeReplayRequestText.trim()) parts.push(`Active Replay request:\n${redactAiRequestContext(activeReplayRequestText)}`);
    if (historyDetail) {
      parts.push(`Selected History entry ${historyDetail.entry.id}: ${historyDetail.entry.method} ${historyDetail.entry.url}`);
      parts.push(`Selected History request:\n${redactAiRequestContext(decodeHttpText(new Uint8Array(historyDetail.request)))}`);
    }
    return parts.join("\n\n");
  }

  type ForgeRecord = Record<string, unknown>;

  function forgeHas(args: ForgeRecord, key: string) {
    return Object.prototype.hasOwnProperty.call(args, key);
  }

  function forgeObject(value: unknown, label: string): ForgeRecord {
    if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error(`${label} must be an object`);
    return value as ForgeRecord;
  }

  function forgeRequiredString(args: ForgeRecord, key: string) {
    const value = args[key];
    if (typeof value !== "string") throw new Error(`${key} must be a string`);
    return value;
  }

  function forgeRequestText(value: unknown, label = "request"): string {
    if (typeof value === "string") return value;
    if (Array.isArray(value)) {
      if (value.every((item) => typeof item === "number" && Number.isInteger(item) && item >= 0 && item <= 255)) {
        return decodeHttpText(new Uint8Array(value as number[]));
      }
      throw new Error(`${label} must be a string`);
    }
    if (!value || typeof value !== "object") throw new Error(`${label} must be a string`);

    const source = value as ForgeRecord;
    for (const key of ["raw", "text", "request"]) {
      if (forgeHas(source, key) && source[key] !== value) {
        try {
          return forgeRequestText(source[key], label);
        } catch {
          // Continue to support a structured request object below.
        }
      }
    }

    const method = source.method;
    const suppliedUrl = source.url ?? source.path;
    if (typeof method !== "string" || typeof suppliedUrl !== "string") throw new Error(`${label} must be a string`);

    let target = suppliedUrl || "/";
    let host = typeof source.host === "string" ? source.host : "";
    if (/^https?:\/\//i.test(suppliedUrl)) {
      const parsed = new URL(suppliedUrl);
      target = `${parsed.pathname || "/"}${parsed.search}`;
      host ||= parsed.host;
    } else if (!target.startsWith("/")) {
      target = `/${target}`;
    }

    const lines = [`${method.toUpperCase()} ${target} HTTP/1.1`];
    let hasHost = false;
    if (source.headers && typeof source.headers === "object" && !Array.isArray(source.headers)) {
      for (const [name, headerValue] of Object.entries(source.headers as ForgeRecord)) {
        if (name.toLowerCase() === "host") hasHost = true;
        const text = Array.isArray(headerValue) ? headerValue.join(", ") : String(headerValue);
        lines.push(`${name}: ${text}`);
      }
    }
    if (host && !hasHost) lines.push(`Host: ${host}`);
    const bodyValue = source.body;
    const body = bodyValue === undefined || bodyValue === null
      ? ""
      : typeof bodyValue === "string" ? bodyValue : JSON.stringify(bodyValue);
    return synchronizeHttpContentLength(`${lines.join("\r\n")}\r\n\r\n${body}`);
  }

  function forgeOptionalString(args: ForgeRecord, key: string) {
    if (!forgeHas(args, key) || args[key] === null || args[key] === undefined) return undefined;
    return forgeRequiredString(args, key);
  }

  function forgeRequiredInteger(args: ForgeRecord, key: string) {
    const value = args[key];
    if (typeof value !== "number" || !Number.isSafeInteger(value)) throw new Error(`${key} must be an integer`);
    return value;
  }

  function forgeOptionalInteger(args: ForgeRecord, key: string) {
    if (!forgeHas(args, key) || args[key] === null || args[key] === undefined) return undefined;
    return forgeRequiredInteger(args, key);
  }

  function forgeOptionalBoolean(args: ForgeRecord, key: string, fallback: boolean) {
    if (!forgeHas(args, key) || args[key] === null || args[key] === undefined) return fallback;
    if (typeof args[key] !== "boolean") throw new Error(`${key} must be a boolean`);
    return args[key] as boolean;
  }

  function forgeStringArray(args: ForgeRecord, key: string) {
    const value = args[key];
    if (!Array.isArray(value) || value.some((item) => typeof item !== "string")) throw new Error(`${key} must be an array of strings`);
    return value as string[];
  }

  function forgeRequestArray(args: ForgeRecord, key: string) {
    const value = args[key];
    if (!Array.isArray(value)) throw new Error(`${key} must be an array`);
    return value.map((item, index) => forgeRequestText(item, `${key}[${index}]`));
  }

  function forgeIntegerArray(args: ForgeRecord, key: string) {
    const value = args[key];
    if (!Array.isArray(value) || value.some((item) => typeof item !== "number" || !Number.isSafeInteger(item))) throw new Error(`${key} must be an array of integers`);
    return value as number[];
  }

  function forgeObjectArray(args: ForgeRecord, key: string) {
    const value = args[key];
    if (!Array.isArray(value)) throw new Error(`${key} must be an array`);
    return value.map((item, index) => forgeObject(item, `${key}[${index}]`));
  }

  function forgeStrictObject(value: unknown, label: string, allowedKeys: readonly string[], requiredKeys: readonly string[] = allowedKeys) {
    const source = forgeObject(value, label);
    for (const key of Object.keys(source)) {
      if (!allowedKeys.includes(key)) throw new Error(`${label} contains unknown field '${key}'`);
    }
    for (const key of requiredKeys) {
      if (!forgeHas(source, key)) throw new Error(`${label}.${key} is required`);
    }
    return source;
  }

  function forgeEnum<T extends string>(value: unknown, label: string, values: readonly T[]): T {
    if (typeof value !== "string" || !values.includes(value as T)) throw new Error(`${label} must be one of: ${values.join(", ")}`);
    return value as T;
  }

  function forgeText(value: unknown, label: string, maxBytes?: number, allowEmpty = true) {
    if (typeof value !== "string") throw new Error(`${label} must be a string`);
    if (!allowEmpty && !value.trim()) throw new Error(`${label} must not be empty`);
    if (maxBytes !== undefined && new TextEncoder().encode(value).length > maxBytes) throw new Error(`${label} cannot exceed ${maxBytes} bytes`);
    return value;
  }

  const forgeInterceptionDirections = ["request", "response"] as const;
  const forgeInterceptionMatchTypes: InterceptionRuleMatchType[] = ["url", "domain", "ipAddress", "protocol", "fileExtension", "httpMethod", "contentType", "request", "cookieName", "cookieValue", "anyHeader", "body", "paramName", "paramValue", "listenerPort", "inScope"];
  const forgeInterceptionRelationships: InterceptionRuleRelationship[] = ["matches", "doesNotMatch", "contains", "doesNotContain", "isInScope", "isNotInScope"];
  const forgeStandardInterceptionRelationships: InterceptionRuleRelationship[] = ["matches", "doesNotMatch", "contains", "doesNotContain"];
  const forgeScopeInterceptionRelationships: InterceptionRuleRelationship[] = ["isInScope", "isNotInScope"];

  function forgeInterceptionDirection(args: ForgeRecord) {
    return forgeEnum(forgeRequiredString(args, "direction"), "direction", forgeInterceptionDirections);
  }

  function forgeRuleId(value: unknown, label: string) {
    return forgeText(value, label, 128, false);
  }

  function forgeInterceptionRule(value: unknown, label: string, existingId?: string): InterceptionRule {
    const source = forgeStrictObject(
      value,
      label,
      existingId === undefined
        ? ["id", "enabled", "operator", "matchType", "relationship", "condition"]
        : ["enabled", "operator", "matchType", "relationship", "condition"],
    );
    const id = existingId ?? forgeRuleId(source.id, `${label}.id`);
    const enabled = source.enabled;
    if (typeof enabled !== "boolean") throw new Error(`${label}.enabled must be a boolean`);
    const operator = forgeEnum(source.operator, `${label}.operator`, ["and", "or"] as const);
    const matchType = forgeEnum(source.matchType, `${label}.matchType`, forgeInterceptionMatchTypes);
    const relationship = forgeEnum(source.relationship, `${label}.relationship`, forgeInterceptionRelationships);
    const rawCondition = forgeText(source.condition, `${label}.condition`, 512);
    if (matchType === "inScope") {
      if (!forgeScopeInterceptionRelationships.includes(relationship)) throw new Error("inScope rules require isInScope or isNotInScope");
      if (rawCondition.trim()) throw new Error("inScope rules must have an empty condition");
      return { id, enabled, operator, matchType, relationship, condition: "" };
    }
    if (!forgeStandardInterceptionRelationships.includes(relationship)) throw new Error("Non-scope interception rules require matches, doesNotMatch, contains, or doesNotContain");
    const condition = rawCondition.trim();
    if (!condition) throw new Error("Non-scope interception rules require a condition");
    if (relationship === "matches" || relationship === "doesNotMatch") {
      try { new RegExp(condition, "i"); } catch (reason) { throw new Error(`Invalid interception rule regular expression: ${reason instanceof Error ? reason.message : String(reason)}`); }
    }
    return { id, enabled, operator, matchType, relationship, condition };
  }

  function forgeMatchReplaceTypes(): MatchReplaceRuleType[] {
    return ["requestHost", "requestHeader", "requestBody", "requestParamName", "requestParamValue", "responseHeader", "responseBody", "responseParamName", "responseParamValue"];
  }

  function forgeMatchReplaceRule(value: unknown, label: string, existingId?: string): MatchReplaceRule {
    const source = forgeStrictObject(
      value,
      label,
      existingId === undefined
        ? ["id", "enabled", "location", "type", "match", "replace", "isRegex"]
        : ["enabled", "location", "type", "match", "replace", "isRegex"],
      existingId === undefined
        ? ["id", "enabled", "type", "match", "replace", "isRegex"]
        : ["enabled", "type", "match", "replace", "isRegex"],
    );
    const id = existingId ?? forgeRuleId(source.id, `${label}.id`);
    if (typeof source.enabled !== "boolean") throw new Error(`${label}.enabled must be a boolean`);
    const type = forgeEnum(source.type, `${label}.type`, forgeMatchReplaceTypes());
    const match = forgeText(source.match, `${label}.match`, 2048, true);
    if (!match.length) throw new Error(`${label}.match must not be empty`);
    const replace = forgeText(source.replace, `${label}.replace`, 4096);
    if (typeof source.isRegex !== "boolean") throw new Error(`${label}.isRegex must be a boolean`);
    if (source.isRegex) {
      try { new RegExp(match); } catch (reason) { throw new Error(`Invalid match/replace regular expression: ${reason instanceof Error ? reason.message : String(reason)}`); }
    }
    const location = type.startsWith("response") ? "response" : "request";
    if (forgeHas(source, "location")) {
      const suppliedLocation = forgeEnum(source.location, `${label}.location`, ["request", "response"] as const);
      if (suppliedLocation !== location) throw new Error(`${label}.location conflicts with type ${type}`);
    }
    return { id, enabled: source.enabled, location, type, match, replace, isRegex: source.isRegex };
  }

  function forgeRuleIndex(value: unknown, label: string, length: number) {
    if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0 || value >= length) throw new Error(`${label} must be a zero-based index from 0 through ${Math.max(0, length - 1)}`);
    return value;
  }

  function forgeAssertUniqueRuleIds(rules: { id: string }[], label: string) {
    const ids = new Set<string>();
    for (const rule of rules) {
      if (ids.has(rule.id)) throw new Error(`${label} contains duplicate rule ID '${rule.id}'`);
      ids.add(rule.id);
    }
  }

  function forgeUniqueRuleId(rules: { id: string }[], id: string, label: string) {
    forgeAssertUniqueRuleIds(rules, label);
    if (rules.some((rule) => rule.id === id)) throw new Error(`${label} '${id}' already exists`);
    if (rules.length >= 100) throw new Error("At most 100 rules are supported");
  }

  function forgeExistingRule<T extends { id: string }>(rules: T[], id: string, label: string) {
    const index = rules.findIndex((rule) => rule.id === id);
    if (index < 0) throw new Error(`${label} '${id}' was not found`);
    return { rule: rules[index], index };
  }

  function forgeReorderedRules<T>(rules: T[], index: number, toIndex: number) {
    const next = rules.slice();
    const [moved] = next.splice(index, 1);
    next.splice(toIndex, 0, moved);
    return next;
  }

  async function updateForgeInterceptionRules(direction: "request" | "response", rules: InterceptionRule[]) {
    const settings = await updateSettings(direction === "request"
      ? { requestInterceptionRules: rules }
      : { responseInterceptionRules: rules });
    return direction === "request" ? settings.requestInterceptionRules : settings.responseInterceptionRules;
  }

  async function updateForgeMatchReplaceRules(rules: MatchReplaceRule[]) {
    const settings = await updateSettings({ matchReplaceRules: rules });
    return settings.matchReplaceRules;
  }

  function forgeCurrentInterceptionRules(direction: "request" | "response") {
    if (!snapshot) throw new Error("Settings are still loading");
    const rules = direction === "request" ? snapshot.settings.requestInterceptionRules : snapshot.settings.responseInterceptionRules;
    forgeAssertUniqueRuleIds(rules, `${direction} interception rules`);
    return rules;
  }

  function forgeCurrentMatchReplaceRules() {
    if (!snapshot) throw new Error("Settings are still loading");
    forgeAssertUniqueRuleIds(snapshot.settings.matchReplaceRules, "Match/replace rules");
    return snapshot.settings.matchReplaceRules;
  }

  type ForgePayloadGenerator =
    | { type: "list"; list: PayloadWarehouse["list"] }
    | { type: "numbers"; numbers: PayloadWarehouse["numbers"] }
    | { type: "null"; nullPayload: PayloadWarehouse["nullPayload"] }
    | { type: "bruteForce"; bruteForce: PayloadWarehouse["bruteForce"] }
    | { type: "dates"; dates: PayloadWarehouse["dates"] }
    | { type: "characterSubstitution"; characterSubstitution: PayloadWarehouse["characterSubstitution"] };

  const forgePayloadTypes: PayloadWarehouse["type"][] = ["list", "numbers", "null", "bruteForce", "dates", "characterSubstitution"];
  const forgePayloadProcessingTypes: PayloadProcessingRuleType[] = ["addPrefix", "addSuffix", "matchReplace", "substring", "reverseSubstring", "modifyCase", "encode", "decode", "hash"];

  function forgePayloadList(value: unknown, label: string): PayloadWarehouse["list"] {
    const source = forgeStrictObject(value, label, ["text", "builtin", "url"]);
    return { text: forgeText(source.text, `${label}.text`), builtin: forgeText(source.builtin, `${label}.builtin`), url: forgeText(source.url, `${label}.url`) };
  }

  function forgePayloadNumbers(value: unknown, label: string): PayloadWarehouse["numbers"] {
    const source = forgeStrictObject(value, label, ["mode", "from", "to", "step", "count"]);
    return {
      mode: forgeEnum(source.mode, `${label}.mode`, ["sequential", "random"] as const),
      from: forgeText(source.from, `${label}.from`),
      to: forgeText(source.to, `${label}.to`),
      step: forgeText(source.step, `${label}.step`),
      count: forgeText(source.count, `${label}.count`),
    };
  }

  function forgePayloadNull(value: unknown, label: string): PayloadWarehouse["nullPayload"] {
    const source = forgeStrictObject(value, label, ["mode", "count"]);
    return { mode: forgeEnum(source.mode, `${label}.mode`, ["count", "infinite"] as const), count: forgeText(source.count, `${label}.count`) };
  }

  function forgePayloadBruteForce(value: unknown, label: string): PayloadWarehouse["bruteForce"] {
    const source = forgeStrictObject(value, label, ["characterSet", "minLength", "maxLength"]);
    return {
      characterSet: forgeText(source.characterSet, `${label}.characterSet`),
      minLength: forgeText(source.minLength, `${label}.minLength`),
      maxLength: forgeText(source.maxLength, `${label}.maxLength`),
    };
  }

  function forgePayloadDates(value: unknown, label: string): PayloadWarehouse["dates"] {
    const source = forgeStrictObject(value, label, ["from", "to", "step", "unit", "formatMode", "format", "customFormat"]);
    return {
      from: forgeText(source.from, `${label}.from`),
      to: forgeText(source.to, `${label}.to`),
      step: forgeText(source.step, `${label}.step`),
      unit: forgeEnum(source.unit, `${label}.unit`, ["days", "weeks", "months", "years"] as const),
      formatMode: forgeEnum(source.formatMode, `${label}.formatMode`, ["preset", "custom"] as const),
      format: forgeText(source.format, `${label}.format`),
      customFormat: forgeText(source.customFormat, `${label}.customFormat`),
    };
  }

  function forgePayloadCharacterSubstitution(value: unknown, label: string): PayloadWarehouse["characterSubstitution"] {
    const source = forgeStrictObject(value, label, ["mappings", "caseSensitive", "itemsText", "newItem", "builtin"]);
    if (!Array.isArray(source.mappings)) throw new Error(`${label}.mappings must be an array`);
    const mappings = source.mappings.map((mapping, index) => {
      const item = forgeStrictObject(mapping, `${label}.mappings[${index}]`, ["from", "to"]);
      return { from: forgeText(item.from, `${label}.mappings[${index}].from`), to: forgeText(item.to, `${label}.mappings[${index}].to`) };
    });
    if (typeof source.caseSensitive !== "boolean") throw new Error(`${label}.caseSensitive must be a boolean`);
    return {
      mappings,
      caseSensitive: source.caseSensitive,
      itemsText: forgeText(source.itemsText, `${label}.itemsText`),
      newItem: forgeText(source.newItem, `${label}.newItem`),
      builtin: forgeText(source.builtin, `${label}.builtin`),
    };
  }

  function forgeProcessingInteger(value: string, label: string, positive: boolean) {
    const parsed = Number(value);
    if (!Number.isSafeInteger(parsed) || (positive ? parsed <= 0 : parsed < 0)) throw new Error(`${label} must be ${positive ? "a positive" : "a non-negative"} integer string`);
    if (positive && parsed > 5_000) throw new Error(`${label} cannot exceed 5,000`);
    return value;
  }

  function forgePayloadProcessingRule(value: unknown, label: string, existingId?: string): PayloadProcessingRule {
    const source = forgeStrictObject(
      value,
      label,
      existingId === undefined
        ? ["id", "enabled", "type", "value", "match", "replacement", "useRegex", "caseSensitive", "start", "length", "operation"]
        : ["enabled", "type", "value", "match", "replacement", "useRegex", "caseSensitive", "start", "length", "operation"],
    );
    const id = existingId ?? forgeRuleId(source.id, `${label}.id`);
    if (typeof source.enabled !== "boolean") throw new Error(`${label}.enabled must be a boolean`);
    const type = forgeEnum(source.type, `${label}.type`, forgePayloadProcessingTypes);
    const valueText = forgeText(source.value, `${label}.value`);
    const match = forgeText(source.match, `${label}.match`);
    const replacement = forgeText(source.replacement, `${label}.replacement`);
    if (typeof source.useRegex !== "boolean") throw new Error(`${label}.useRegex must be a boolean`);
    if (typeof source.caseSensitive !== "boolean") throw new Error(`${label}.caseSensitive must be a boolean`);
    const start = forgeText(source.start, `${label}.start`);
    const length = forgeText(source.length, `${label}.length`);
    const operation = forgeText(source.operation, `${label}.operation`);
    if (type === "addPrefix" || type === "addSuffix") {
      if (!valueText) throw new Error(`${label}.value must not be empty for ${type}`);
    }
    if (type === "matchReplace") {
      if (!match) throw new Error(`${label}.match must not be empty`);
      if (source.useRegex) {
        try { new RegExp(match, source.caseSensitive ? "g" : "gi"); } catch (reason) { throw new Error(`Invalid payload processing regular expression: ${reason instanceof Error ? reason.message : String(reason)}`); }
      }
    }
    if (type === "substring" || type === "reverseSubstring") {
      forgeProcessingInteger(start, `${label}.start`, false);
      forgeProcessingInteger(length, `${label}.length`, true);
    }
    if (type === "modifyCase") forgeEnum(operation, `${label}.operation`, ["lower", "capitalize", "upper"] as const);
    if (type === "encode" || type === "decode") forgeEnum(operation, `${label}.operation`, ["url", "base64", "hex"] as const);
    if (type === "hash") forgeEnum(operation, `${label}.operation`, ["sha1", "sha256", "sha512"] as const);
    return { id, enabled: source.enabled, type, value: valueText, match, replacement, useRegex: source.useRegex, caseSensitive: source.caseSensitive, start, length, operation };
  }

  function forgePayloadProcessingRules(value: unknown, label: string) {
    if (!Array.isArray(value)) throw new Error(`${label} must be an array`);
    if (value.length > 100) throw new Error(`${label} cannot contain more than 100 rules`);
    const rules = value.map((rule, index) => forgePayloadProcessingRule(rule, `${label}[${index}]`));
    const ids = new Set<string>();
    for (const rule of rules) {
      if (ids.has(rule.id)) throw new Error(`${label} contains duplicate rule ID '${rule.id}'`);
      ids.add(rule.id);
    }
    return rules;
  }

  function forgeFuzzGenerator(value: unknown, label: string): ForgePayloadGenerator {
    const source = forgeObject(value, label);
    const type = forgeEnum(source.type, `${label}.type`, forgePayloadTypes);
    if (type === "list") {
      const branch = forgeStrictObject(source, label, ["type", "list"]);
      return { type, list: forgePayloadList(branch.list, `${label}.list`) };
    }
    if (type === "numbers") {
      const branch = forgeStrictObject(source, label, ["type", "numbers"]);
      return { type, numbers: forgePayloadNumbers(branch.numbers, `${label}.numbers`) };
    }
    if (type === "null") {
      const branch = forgeStrictObject(source, label, ["type", "nullPayload"]);
      return { type, nullPayload: forgePayloadNull(branch.nullPayload, `${label}.nullPayload`) };
    }
    if (type === "bruteForce") {
      const branch = forgeStrictObject(source, label, ["type", "bruteForce"]);
      return { type, bruteForce: forgePayloadBruteForce(branch.bruteForce, `${label}.bruteForce`) };
    }
    if (type === "dates") {
      const branch = forgeStrictObject(source, label, ["type", "dates"]);
      return { type, dates: forgePayloadDates(branch.dates, `${label}.dates`) };
    }
    const branch = forgeStrictObject(source, label, ["type", "characterSubstitution"]);
    return { type, characterSubstitution: forgePayloadCharacterSubstitution(branch.characterSubstitution, `${label}.characterSubstitution`) };
  }

  function forgeFuzzWarehouse(value: unknown, label: string): PayloadWarehouse {
    const source = forgeStrictObject(value, label, ["type", "list", "numbers", "nullPayload", "bruteForce", "dates", "characterSubstitution", "processing"]);
    const type = forgeEnum(source.type, `${label}.type`, forgePayloadTypes);
    return {
      type,
      list: forgePayloadList(source.list, `${label}.list`),
      numbers: forgePayloadNumbers(source.numbers, `${label}.numbers`),
      nullPayload: forgePayloadNull(source.nullPayload, `${label}.nullPayload`),
      bruteForce: forgePayloadBruteForce(source.bruteForce, `${label}.bruteForce`),
      dates: forgePayloadDates(source.dates, `${label}.dates`),
      characterSubstitution: forgePayloadCharacterSubstitution(source.characterSubstitution, `${label}.characterSubstitution`),
      processing: forgePayloadProcessingRules(source.processing, `${label}.processing`),
    };
  }

  function forgeFuzzWarehouseTarget(tab: IntruderTab, args: ForgeRecord, materialize: boolean) {
    flushFuzzDraft();
    const positionIndex = forgeOptionalInteger(args, "positionIndex");
    if (positionIndex === undefined) {
      forgeAssertUniqueRuleIds(tab.warehouse.processing, "Payload processing rules");
      return { positionIndex: null, warehouse: tab.warehouse };
    }
    if (positionIndex < 0) throw new Error("positionIndex must be zero or greater");
    if (tab.mode !== "map" && tab.mode !== "combine") throw new Error("positionIndex is only valid for Map or Combine Fuzz modes");
    const positionCount = findTestPositions(decodeHttpText(tab.request)).length;
    if (positionIndex >= positionCount) throw new Error(`positionIndex ${positionIndex} is outside the ${positionCount} marked positions`);
    if (materialize) {
      ensureFuzzPositionWarehouses(tab, positionCount);
      forgeAssertUniqueRuleIds(tab.positionWarehouses[positionIndex].processing, "Payload processing rules");
      return { positionIndex, warehouse: tab.positionWarehouses[positionIndex] };
    }
    const warehouse = tab.positionWarehouses[positionIndex] ?? createPayloadWarehouse();
    forgeAssertUniqueRuleIds(warehouse.processing, "Payload processing rules");
    return { positionIndex, warehouse };
  }

  function setForgeFuzzWarehouse(tab: IntruderTab, positionIndex: number | null, warehouse: PayloadWarehouse) {
    if (positionIndex === null) tab.warehouse = warehouse;
    else {
      ensureFuzzPositionWarehouses(tab, positionIndex + 1);
      tab.positionWarehouses[positionIndex] = warehouse;
    }
  }

  function forgeFuzzRuleResult(tab: IntruderTab, positionIndex: number | null, warehouse: PayloadWarehouse, extra: Record<string, unknown> = {}) {
    return { tabId: tab.id, positionIndex, warehouse: clonePayloadWarehouse(warehouse), processingRules: warehouse.processing.map((rule) => ({ ...rule })), ...extra };
  }

  function forgeTabId(args: ForgeRecord, key = "tabId") {
    return forgeOptionalInteger(args, key);
  }

  function findReplayForForge(id: number | undefined) {
    const tab = id === undefined ? activeReplay : replayTabs.find((candidate) => candidate.id === id);
    if (!tab) throw new Error(`Replay tab ${id ?? "active"} was not found`);
    return tab;
  }

  function findFuzzForForge(id: number | undefined) {
    const tab = id === undefined
      ? activeFuzz
      : fuzz.tabs.find((candidate): candidate is IntruderTab => candidate.kind === "setup" && candidate.id === id);
    if (!tab) throw new Error(`Fuzz tab ${id ?? "active"} was not found`);
    return tab;
  }

  function fuzzForgeScanId(tab: IntruderTab, requestedTabId: number | undefined, requestedScanId: string | undefined) {
    if (requestedScanId !== undefined) return requestedScanId;
    if (requestedTabId === undefined && activeFuzz?.id === tab.id) {
      return activeFuzzScan?.session.id ?? tab.activeScanId;
    }
    return tab.activeScanId;
  }

  function replayForgeSummary(tab: ReplayTab) {
    const response = selectedReplayResponse(tab);
    const metadata = parseRequestMetadataText(replayText(tab).slice(0, HTTP_METADATA_LIMIT), "");
    return {
      id: tab.id,
      title: tab.title,
      host: metadata.host || null,
      tls: tab.tls,
      sending: tab.sending,
      requestLength: tab.request.length,
      responseLength: response.length,
      historyCount: tab.history.length,
      identityGroup: tab.identityConfig?.groupName ?? null,
      identityCount: tab.identityConfig?.identityIds.length ?? 0,
    };
  }

  function replayForgeDetail(tab: ReplayTab) {
    const response = selectedReplayResponse(tab);
    return {
      ...replayForgeSummary(tab),
      request: replayText(tab),
      response: decodeHttpText(response),
      history: tab.history.map((request, index) => ({ index, request: decodeHttpText(request), current: index === tab.historyIndex })),
      identityConfig: tab.identityConfig ? { ...tab.identityConfig, identityIds: [...tab.identityConfig.identityIds] } : null,
      identityResponses: Object.values(tab.identityResponses).map((item) => ({
        executionId: item.executionId,
        identityId: item.identityId,
        name: item.name,
        status: item.status,
        durationMs: item.durationMs,
        size: item.size,
        error: item.error,
        sending: item.sending,
        response: decodeHttpText(item.raw),
      })),
    };
  }

  function fuzzForgeScanSummary(scan: IntruderScan) {
    return {
      id: scan.session.id,
      name: scan.name,
      startedAt: scan.startedAt,
      completedAt: scan.completedAt,
      running: scan.running,
      stopped: scan.stopped,
      error: scan.error,
      mode: scan.session.mode,
      totalRequests: scan.session.totalRequests,
      nextPayloadIndex: scan.nextPayloadIndex,
      resultCount: scan.results.length,
      persistenceError: scan.persistenceError,
    };
  }

  function fuzzForgeSummary(tab: IntruderTab) {
    const request = fuzz.editorDraft?.tabId === tab.id ? fuzz.editorDraft.value : decodeHttpText(tab.request);
    let positions: string[] = [];
    try { positions = findTestPositions(request).map((position) => position.original); } catch { /* The detailed read reports malformed markers through the request text. */ }
    return {
      id: tab.id,
      title: tab.title,
      tls: tab.tls,
      mode: tab.mode,
      scanName: tab.scanName,
      requestLength: tab.request.length,
      positions,
      scanCount: tab.scans.length,
      running: tab.scans.some((scan) => scan.running),
      activeScanId: tab.activeScanId,
    };
  }

  function fuzzForgeDetail(tab: IntruderTab) {
    return {
      ...fuzzForgeSummary(tab),
      request: fuzz.editorDraft?.tabId === tab.id ? fuzz.editorDraft.value : decodeHttpText(tab.request),
      warehouse: cloneJson(tab.warehouse),
      positionWarehouses: cloneJson(tab.positionWarehouses),
      scans: tab.scans.map((scan) => ({
        ...fuzzForgeScanSummary(scan),
        results: scan.results.slice(0, 50).map((result) => ({
          id: result.id,
          sequence: result.sequence,
          payload: result.payload,
          status: result.status,
          length: result.length,
          durationMs: result.durationMs,
          error: result.error,
        })),
      })),
    };
  }

  function setJsonPointerValue(root: unknown, pointer: string, value: unknown) {
    const segments = jsonPointerSegments(pointer);
    if (!segments.length) return value;
    let current: unknown = root;
    for (let index = 0; index < segments.length; index += 1) {
      const segment = segments[index];
      const last = index === segments.length - 1;
      if (Array.isArray(current)) {
        if (!/^0$|^[1-9]\d*$/.test(segment)) throw new Error(`JSON path segment '${segment}' is not an array index`);
        const arrayIndex = Number(segment);
        if (!Number.isSafeInteger(arrayIndex) || arrayIndex < 0 || arrayIndex >= current.length) throw new Error(`JSON path index '${segment}' does not exist`);
        if (last) current[arrayIndex] = value;
        else current = current[arrayIndex];
      } else if (current && typeof current === "object") {
        const object = current as ForgeRecord;
        if (!(segment in object)) throw new Error(`JSON path segment '${segment}' does not exist`);
        if (last) object[segment] = value;
        else current = object[segment];
      } else {
        throw new Error(`JSON path cannot continue through '${segment}'`);
      }
    }
    return root;
  }

  function removeJsonPointerValue(root: unknown, pointer: string) {
    const segments = jsonPointerSegments(pointer);
    if (!segments.length) throw new Error("The JSON document itself cannot be removed");
    let current: unknown = root;
    for (let index = 0; index < segments.length - 1; index += 1) {
      const segment = segments[index];
      if (Array.isArray(current)) {
        if (!/^0$|^[1-9]\d*$/.test(segment)) throw new Error(`JSON path segment '${segment}' is not an array index`);
        const arrayIndex = Number(segment);
        if (!Number.isSafeInteger(arrayIndex) || arrayIndex < 0 || arrayIndex >= current.length) throw new Error(`JSON path index '${segment}' does not exist`);
        current = current[arrayIndex];
      } else if (current && typeof current === "object" && segment in (current as ForgeRecord)) {
        current = (current as ForgeRecord)[segment];
      } else {
        throw new Error(`JSON path segment '${segment}' does not exist`);
      }
    }
    const last = segments[segments.length - 1];
    if (Array.isArray(current)) {
      if (!/^0$|^[1-9]\d*$/.test(last)) throw new Error(`JSON path segment '${last}' is not an array index`);
      const arrayIndex = Number(last);
      if (!Number.isSafeInteger(arrayIndex) || arrayIndex < 0 || arrayIndex >= current.length) throw new Error(`JSON path index '${last}' does not exist`);
      current.splice(arrayIndex, 1);
    } else if (current && typeof current === "object" && last in (current as ForgeRecord)) {
      delete (current as ForgeRecord)[last];
    } else {
      throw new Error(`JSON path segment '${last}' does not exist`);
    }
    return root;
  }

  function updateReplayHeader(value: string, name: string, nextValue?: string) {
    const message = splitHttpMessage(value);
    const lines = message.head.split(/\r\n|\n|\r/);
    const normalizedName = name.trim().toLowerCase();
    if (!normalizedName) throw new Error("Header name cannot be empty");
    let found = false;
    const nextLines = lines.flatMap((line) => {
      const colon = line.indexOf(":");
      const lineName = colon > 0 ? line.slice(0, colon).trim().toLowerCase() : "";
      if (lineName !== normalizedName) return [line];
      found = true;
      if (nextValue === undefined) return [];
      return [`${line.slice(0, colon + 1)} ${nextValue}`];
    });
    if (!found && nextValue !== undefined) nextLines.push(`${name.trim()}: ${nextValue}`);
    const updated = `${nextLines.join(message.lineEnding)}${message.complete ? message.separator + message.body : ""}`;
    return message.complete ? synchronizeHttpContentLength(updated) : updated;
  }

  function normalizeReplayHost(value: string) {
    const trimmed = value.trim();
    if (!trimmed) throw new Error("host must not be empty");
    try {
      const parsed = new URL(trimmed.includes("://") ? trimmed : `http://${trimmed}`);
      if (!parsed.hostname) throw new Error("missing hostname");
      return parsed.host;
    } catch {
      throw new Error("host must be a hostname or hostname:port");
    }
  }

  function updateReplayQuery(value: string, name: string, nextValue?: string) {
    const firstLineEnd = value.search(/\r\n|\n|\r/);
    const firstLine = firstLineEnd < 0 ? value : value.slice(0, firstLineEnd);
    const match = /^(\s*\S+)(\s+)(\S+)(.*)$/.exec(firstLine);
    if (!match) throw new Error("Replay request line is incomplete");
    const target = match[3];
    const queryIndex = target.indexOf("?");
    const base = queryIndex < 0 ? target : target.slice(0, queryIndex);
    const query = queryIndex < 0 ? "" : target.slice(queryIndex + 1);
    const params = new URLSearchParams(query);
    if (nextValue === undefined) params.delete(name);
    else params.set(name, nextValue);
    const encoded = params.toString();
    const nextTarget = encoded ? `${base}?${encoded}` : base;
    const nextLine = `${match[1]}${match[2]}${nextTarget}${match[4]}`;
    return `${nextLine}${firstLineEnd < 0 ? "" : value.slice(firstLine.length)}`;
  }

  function patchReplayRequest(tab: ReplayTab, operations: ForgeRecord[]) {
    flushReplayDraft();
    let value = decodeHttpText(tab.request);
    for (const operation of operations) {
      const type = forgeRequiredString(operation, "type");
      if (type === "replaceText") {
        const find = forgeRequiredString(operation, "find");
        const replaceWith = forgeRequiredString(operation, "replaceWith");
        if (!find) throw new Error("replaceText requires a non-empty find value");
        value = operation.all === true ? value.split(find).join(replaceWith) : value.replace(find, replaceWith);
      } else if (type === "setHeader") {
        value = updateReplayHeader(value, forgeRequiredString(operation, "name"), forgeRequiredString(operation, "value"));
      } else if (type === "removeHeader") {
        value = updateReplayHeader(value, forgeRequiredString(operation, "name"));
      } else if (["setJsonValue", "removeJsonValue"].includes(type)) {
        const message = splitHttpMessage(value);
        if (!message.complete) throw new Error("JSON edits require a complete request with a header/body separator");
        let parsed: unknown;
        try { parsed = JSON.parse(message.body); } catch { throw new Error("JSON edits require a valid JSON body"); }
        if (type === "setJsonValue" && !forgeHas(operation, "value")) throw new Error("setJsonValue requires a value");
        parsed = type === "setJsonValue"
          ? setJsonPointerValue(parsed, forgeRequiredString(operation, "path"), operation.value)
          : removeJsonPointerValue(parsed, forgeRequiredString(operation, "path"));
        const serialized = JSON.stringify(parsed, null, 2);
        if (typeof serialized !== "string") throw new Error("The JSON edit produced an invalid document");
        const body = message.lineEnding === "\r\n" ? serialized.replaceAll("\n", "\r\n") : message.lineEnding === "\r" ? serialized.replaceAll("\n", "\r") : serialized;
        value = synchronizeHttpContentLength(`${message.head}${message.separator}${body}`);
      } else if (type === "setQueryParameter") {
        value = updateReplayQuery(value, forgeRequiredString(operation, "name"), forgeRequiredString(operation, "value"));
      } else if (type === "removeQueryParameter") {
        value = updateReplayQuery(value, forgeRequiredString(operation, "name"));
      } else {
        throw new Error(`Unsupported Replay edit operation '${type}'`);
      }
    }
    tab.request = encodeHttpText(value);
    tab.response = new Uint8Array();
    tab.identityResponses = {};
    tab.activeIdentityResponseId = null;
    markWorkspaceDirty();
    return value;
  }

  function createForgeReplayTab(request: string, tls: boolean | undefined, title: string | undefined) {
    const id = nextReplayId++;
    const raw = normalizeHttpLineEndingBytes(encodeHttpText(request));
    const tab = createReplayTab(id, raw, tls ?? inferRequestTls(raw));
    if (title?.trim()) tab.title = title.trim();
    replayTabs.push(tab);
    return tab;
  }

  function deleteForgeReplayTab(id: number) {
    const index = replayTabs.findIndex((tab) => tab.id === id);
    if (index < 0) throw new Error(`Replay tab ${id} was not found`);
    const tab = replayTabs[index];
    if (tab.sending) throw new Error(`Replay tab ${id} is sending; cancel it before removing it`);
    if (replayTabs.length === 1) {
      Object.assign(tab, createReplayTab(tab.id));
    } else {
      replayTabs.splice(index, 1);
      if (activeReplayId === id) activeReplayId = replayTabs[Math.max(0, index - 1)].id;
    }
    return id;
  }

  function ensureFuzzPositionWarehouses(tab: IntruderTab, count: number) {
    while (tab.positionWarehouses.length < count) tab.positionWarehouses.push(createPayloadWarehouse());
  }

  function deleteForgeFuzzTab(id: number) {
    const index = fuzz.tabs.findIndex((tab) => tab.id === id);
    if (index < 0) throw new Error(`Fuzz tab ${id} was not found`);
    const tab = fuzz.tabs[index];
    if (tab.kind === "result") {
      fuzz.tabs.splice(index, 1);
      if (fuzz.activeTabId === id) fuzz.activeTabId = fuzz.tabs[Math.max(0, index - 1)]?.id ?? fuzz.tabs[0]?.id ?? 0;
      return id;
    }
    if (tab.scans.some((scan) => scan.running)) throw new Error(`Fuzz tab ${id} has a running scan`);
    flushFuzzDraft();
    const linkedResultIds = new Set(
      fuzz.tabs.filter((candidate) => candidate.kind === "result" && candidate.sourceTabId === id).map((candidate) => candidate.id),
    );
    if (fuzz.tabs.length - linkedResultIds.size === 1) {
      fuzz.tabs = [createIntruderTab(id)];
      fuzz.activeTabId = id;
    } else {
      fuzz.tabs = fuzz.tabs.filter((candidate) => candidate.id !== id && !linkedResultIds.has(candidate.id));
      if (fuzz.activeTabId === id || linkedResultIds.has(fuzz.activeTabId)) fuzz.activeTabId = fuzz.tabs[Math.max(0, index - 1)]?.id ?? fuzz.tabs[0]?.id ?? 0;
    }
    return id;
  }

  async function fuzzPlanForForge(tab: IntruderTab) {
    flushFuzzDraft();
    const template = decodeHttpText(tab.request);
    const positions = findTestPositions(template);
    if (tab.mode === "single" && positions.length > 1) throw new Error("Single mode accepts one marked position");
    if (tab.mode === "spread" && !positions.length) throw new Error("Spread mode needs at least one marked position");
    if ((tab.mode === "map" || tab.mode === "combine") && positions.length < 2) throw new Error("Map and Combine modes need at least two marked positions");
    ensureFuzzPositionWarehouses(tab, positions.length);
    const warehouses = tab.mode === "map" || tab.mode === "combine"
      ? tab.positionWarehouses.slice(0, positions.length)
      : [tab.warehouse];
    const sets = [];
    for (const warehouse of warehouses) {
      const generated = generatePayloads(warehouse);
      sets.push({ payloads: await processPayloads(generated.payloads, warehouse.processing), repeatIndefinitely: generated.repeatIndefinitely });
    }
    const plan = planPayloadRows(tab.mode, sets, positions.length);
    return {
      positions: positions.map((position, index) => ({ index, value: position.original })),
      totalRequests: plan.repeatIndefinitely ? null : plan.rows.length,
      repeatIndefinitely: plan.repeatIndefinitely,
      sampleRows: plan.rows.slice(0, 10),
      generatedValueCounts: sets.map((set) => set.payloads.length),
    };
  }

  function organizerNullableId(value: unknown, key: string) {
    if (value === undefined || value === null) return null;
    if (typeof value !== "string") throw new Error(`${key} must be a string`);
    const normalized = value.trim();
    return normalized || null;
  }

  function organizerTags(value: unknown) {
    if (!Array.isArray(value)) return [];
    const result: string[] = [];
    for (const item of value) {
      if (typeof item !== "string") continue;
      const tag = item.trim();
      if (tag && !result.some((existing) => existing.toLocaleLowerCase() === tag.toLocaleLowerCase())) result.push(tag);
    }
    return result;
  }

  function organizerInput(value: unknown): OrganizerItemInput {
    const source = forgeObject(value, "Organizer item input");
    const request = typeof source.request === "string" ? Array.from(encodeHttpText(source.request)) : byteArray(source.request);
    const response = typeof source.response === "string" ? Array.from(encodeHttpText(source.response)) : byteArray(source.response);
    if (!request.length) throw new Error("Organizer item request must not be empty");
    return {
      title: typeof source.title === "string" ? source.title : "",
      folderId: organizerNullableId(source.folderId, "folderId"),
      stageId: organizerNullableId(source.stageId, "stageId"),
      request,
      response,
      tls: typeof source.tls === "boolean" ? source.tls : true,
      source: typeof source.source === "string" ? source.source : "Forge",
      notes: typeof source.notes === "string" ? source.notes : "",
      tags: organizerTags(source.tags),
    };
  }

  function organizerItemInputFromItem(item: OrganizerItem, folderId = item.folderId, stageId = item.stageId): OrganizerItemInput {
    return {
      title: item.title,
      folderId,
      stageId,
      request: [...item.request],
      response: [...item.response],
      tls: item.tls,
      source: item.source,
      notes: item.notes,
      tags: [...item.tags],
    };
  }

  function organizerSummary(item: OrganizerItem) {
    return {
      id: item.id,
      title: item.title,
      folderId: item.folderId,
      stageId: item.stageId,
      method: item.method,
      host: item.host,
      path: item.path,
      status: item.status,
      tls: item.tls,
      source: item.source,
      notes: item.notes,
      tags: [...item.tags],
      requestLength: item.request.length,
      responseLength: item.response.length,
      createdAt: item.createdAt,
      updatedAt: item.updatedAt,
    };
  }

  function organizerDetail(item: OrganizerItem) {
    return {
      ...organizerSummary(item),
      request: decodeHttpText(new Uint8Array(item.request)),
      response: decodeHttpText(new Uint8Array(item.response)),
    };
  }

  async function organizerItemForForge(id: string) {
    const bundle = await commands.getOrganizer();
    const item = bundle.items.find((candidate) => candidate.id === id);
    if (!item) throw new Error("Organizer item was not found");
    return { bundle, item };
  }

  function organizerFolderDescendants(id: string, bundle: OrganizerBundle): string[] {
    return bundle.folders
      .filter((folder) => folder.parentId === id)
      .flatMap((folder) => [folder.id, ...organizerFolderDescendants(folder.id, bundle)]);
  }

  function validateOrganizerPlacement(folderId: string | null, stageId: string | null, bundle: OrganizerBundle) {
    if (folderId && !bundle.folders.some((folder) => folder.id === folderId)) throw new Error("Organizer folder was not found");
    if (stageId && !organizerWorkspace.stages.some((stage) => stage.id === stageId)) throw new Error("Organizer stage was not found");
  }

  function organizerSort(value: string | undefined, fallback: OrganizerWorkspaceState["sort"] = "updated") {
    const next = value ?? fallback;
    if (!["updated", "created", "title", "host"].includes(next)) throw new Error("Organizer sort must be updated, created, title, or host");
    return next as OrganizerWorkspaceState["sort"];
  }

  function organizerColor(value: string | undefined, fallback = "#5794ef") {
    const color = (value ?? fallback).trim();
    if (!/^#[0-9a-f]{6}$/i.test(color)) throw new Error("Organizer color must be a six-digit hex color");
    return color.toLowerCase();
  }

  function organizerViewSnapshot() {
    return {
      selectedFolderId: organizerWorkspace.selectedFolderId,
      selectedTag: organizerWorkspace.selectedTag,
      query: organizerWorkspace.query,
      sort: organizerWorkspace.sort,
      selectedItemId: organizerWorkspace.selectedItemId,
    };
  }

  function updateOrganizerWorkspace(patch: Partial<OrganizerWorkspaceState>) {
    organizerWorkspace = { ...organizerWorkspace, ...patch };
    organizerRevision += 1;
    markWorkspaceDirty();
  }

  function ensureForgeOrganizerTags(tags: string[]) {
    const additions = tags
      .filter((tag) => !organizerWorkspace.tagDefinitions.some((definition) => definition.name.toLocaleLowerCase() === tag.toLocaleLowerCase()))
      .map((name) => ({ name, color: "#5794ef" }))
      .filter((definition, index, values) => values.findIndex((item) => item.name.toLocaleLowerCase() === definition.name.toLocaleLowerCase()) === index);
    if (!additions.length) return;
    organizerWorkspace = { ...organizerWorkspace, tagDefinitions: [...organizerWorkspace.tagDefinitions, ...additions] };
    markWorkspaceDirty();
  }

  function identityGroupInput(value: unknown): IdentityGroupInput {
    const source = forgeObject(value, "Identity group input");
    const injectionType = source.injectionType;
    if (!["cookie", "header", "queryParameter"].includes(String(injectionType))) throw new Error("Identity injectionType must be cookie, header, or queryParameter");
    return {
      name: typeof source.name === "string" ? source.name : "",
      description: typeof source.description === "string" ? source.description : "",
      injectionType: injectionType as IdentityGroupInput["injectionType"],
      injectionKey: typeof source.injectionKey === "string" ? source.injectionKey : "",
    };
  }

  function identityInput(value: unknown): IdentityInput {
    const source = forgeObject(value, "Identity input");
    return {
      groupId: typeof source.groupId === "string" ? source.groupId : "",
      name: typeof source.name === "string" ? source.name : "",
      color: typeof source.color === "string" ? source.color : "#5794ef",
      notes: typeof source.notes === "string" ? source.notes : "",
      authValue: typeof source.authValue === "string" ? source.authValue : "",
    };
  }

  async function executeForgeTool(name: string, args: Record<string, unknown>): Promise<unknown> {
    const input = args as ForgeRecord;
    switch (name) {
      case "capabilities_read":
        return { tools: forgeTools.map((entry) => ({ name: entry.function.name, description: entry.function.description })) };
      case "context_read":
        return { context: buildAiContext(), project: snapshot?.project ?? null, activeView: activeTab };
      case "navigate": {
        const view = forgeRequiredString(input, "view").toLowerCase();
        const viewMap: Record<string, Tab> = { forge: "AI", ai: "AI", proxy: "Proxy", history: "History", "site map": "Site Map", sitemap: "Site Map", replay: "Replay", fuzz: "Fuzz", organizer: "Organizer", "id+": "ID+", decoder: "Decoder", comparer: "Comparer", scope: "Scope", logs: "Logs", settings: "Settings" };
        const target = viewMap[view];
        if (!target) throw new Error(`Unknown workspace view '${view}'`);
        if (target === "Settings" && forgeOptionalString(input, "section")) {
          const section = forgeRequiredString(input, "section");
          if (!["proxy", "display", "storage", "certificates", "ai", "miscellaneous"].includes(section)) throw new Error(`Unknown Settings section '${section}'`);
          settingsSection = section as SettingsSection;
        }
        changeTab(target);
        return { view: target, section: target === "Settings" ? settingsSection : null };
      }
      case "workspace_read":
        return buildWorkspaceSnapshot();
      case "workspace_reset":
        resetWorkspaceState();
        markWorkspaceDirty();
        return { ok: true, activeView: activeTab };
      case "project_state_read":
        return snapshot?.project ?? null;
      case "project_list_recent":
        await refreshRecentProjects();
        return recentProjects;
      case "project_create": {
        const projectName = forgeRequiredString(input, "name").trim();
        const path = forgeRequiredString(input, "path").trim();
        if (!projectName || !path) throw new Error("Project name and path are required");
        await createProject(projectName, path, true);
        return snapshot?.project ?? { path };
      }
      case "project_open": {
        const path = forgeRequiredString(input, "path").trim();
        await openProject(path, true);
        return snapshot?.project ?? { path };
      }
      case "project_close":
        await closeProject();
        return { ok: true, project: snapshot?.project ?? null };
      case "project_create_temporary":
        await createTemporaryProject(true);
        return snapshot?.project ?? null;
      case "project_save": {
        await persistWorkspace();
        const destination = forgeOptionalString(input, "destination");
        await commands.saveProject(destination?.trim() || undefined);
        const value = await refreshSnapshot();
        return value.project;
      }
      case "project_delete": {
        const path = forgeRequiredString(input, "path");
        await commands.deleteProject(path);
        await refreshRecentProjects();
        return { ok: true, path };
      }
      case "proxy_status_read":
        return snapshot?.proxy ?? null;
      case "proxy_start": {
        await commands.startProxy();
        const value = await refreshSnapshot();
        return value.proxy;
      }
      case "proxy_stop": {
        await stopProxyAndClearPending();
        const value = await refreshSnapshot();
        return value.proxy;
      }
      case "proxy_settings_read":
        return snapshot ? { proxy: snapshot.proxy, settings: snapshot.settings } : null;
      case "proxy_settings_update": {
        const updated = await updateSettings(forgeObject(input.patch, "patch") as SettingsPatch);
        return { proxy: snapshot?.proxy ?? null, settings: updated };
      }
      case "proxy_interception_rule_create": {
        const direction = forgeInterceptionDirection(input);
        const rules = forgeCurrentInterceptionRules(direction);
        const rule = forgeInterceptionRule(input.rule, "rule");
        forgeUniqueRuleId(rules, rule.id, "Interception rule");
        const updatedRules = await updateForgeInterceptionRules(direction, [...rules, rule]);
        return { direction, rule, index: updatedRules.findIndex((candidate) => candidate.id === rule.id), rules: updatedRules };
      }
      case "proxy_interception_rule_update": {
        const direction = forgeInterceptionDirection(input);
        const id = forgeRuleId(input.id, "id");
        const rules = forgeCurrentInterceptionRules(direction);
        const current = forgeExistingRule(rules, id, "Interception rule");
        const rule = forgeInterceptionRule(input.rule, "rule", id);
        const next = rules.slice();
        next[current.index] = rule;
        const updatedRules = await updateForgeInterceptionRules(direction, next);
        return { direction, rule, index: current.index, rules: updatedRules };
      }
      case "proxy_interception_rule_delete": {
        const direction = forgeInterceptionDirection(input);
        const id = forgeRuleId(input.id, "id");
        const rules = forgeCurrentInterceptionRules(direction);
        const current = forgeExistingRule(rules, id, "Interception rule");
        const updatedRules = await updateForgeInterceptionRules(direction, rules.filter((_, index) => index !== current.index));
        return { direction, deletedId: id, rules: updatedRules };
      }
      case "proxy_interception_rule_reorder": {
        const direction = forgeInterceptionDirection(input);
        const id = forgeRuleId(input.id, "id");
        const rules = forgeCurrentInterceptionRules(direction);
        const current = forgeExistingRule(rules, id, "Interception rule");
        const toIndex = forgeRuleIndex(input.toIndex, "toIndex", rules.length);
        const updatedRules = await updateForgeInterceptionRules(direction, forgeReorderedRules(rules, current.index, toIndex));
        return { direction, rule: updatedRules[toIndex], index: toIndex, rules: updatedRules };
      }
      case "proxy_interception_rule_set_enabled": {
        const direction = forgeInterceptionDirection(input);
        const id = forgeRuleId(input.id, "id");
        const rules = forgeCurrentInterceptionRules(direction);
        const current = forgeExistingRule(rules, id, "Interception rule");
        if (typeof input.enabled !== "boolean") throw new Error("enabled must be a boolean");
        const rule = { ...current.rule, enabled: input.enabled };
        const next = rules.slice();
        next[current.index] = rule;
        const updatedRules = await updateForgeInterceptionRules(direction, next);
        return { direction, rule, index: current.index, rules: updatedRules };
      }
      case "proxy_match_replace_rule_create": {
        const rules = forgeCurrentMatchReplaceRules();
        const rule = forgeMatchReplaceRule(input.rule, "rule");
        forgeUniqueRuleId(rules, rule.id, "Match/replace rule");
        const updatedRules = await updateForgeMatchReplaceRules([...rules, rule]);
        return { rule, index: updatedRules.findIndex((candidate) => candidate.id === rule.id), rules: updatedRules };
      }
      case "proxy_match_replace_rule_update": {
        const id = forgeRuleId(input.id, "id");
        const rules = forgeCurrentMatchReplaceRules();
        const current = forgeExistingRule(rules, id, "Match/replace rule");
        const rule = forgeMatchReplaceRule(input.rule, "rule", id);
        const next = rules.slice();
        next[current.index] = rule;
        const updatedRules = await updateForgeMatchReplaceRules(next);
        return { rule, index: current.index, rules: updatedRules };
      }
      case "proxy_match_replace_rule_delete": {
        const id = forgeRuleId(input.id, "id");
        const rules = forgeCurrentMatchReplaceRules();
        const current = forgeExistingRule(rules, id, "Match/replace rule");
        const updatedRules = await updateForgeMatchReplaceRules(rules.filter((_, index) => index !== current.index));
        return { deletedId: id, rules: updatedRules };
      }
      case "proxy_match_replace_rule_reorder": {
        const id = forgeRuleId(input.id, "id");
        const rules = forgeCurrentMatchReplaceRules();
        const current = forgeExistingRule(rules, id, "Match/replace rule");
        const toIndex = forgeRuleIndex(input.toIndex, "toIndex", rules.length);
        const updatedRules = await updateForgeMatchReplaceRules(forgeReorderedRules(rules, current.index, toIndex));
        return { rule: updatedRules[toIndex], index: toIndex, rules: updatedRules };
      }
      case "proxy_match_replace_rule_set_enabled": {
        const id = forgeRuleId(input.id, "id");
        const rules = forgeCurrentMatchReplaceRules();
        const current = forgeExistingRule(rules, id, "Match/replace rule");
        if (typeof input.enabled !== "boolean") throw new Error("enabled must be a boolean");
        const rule = { ...current.rule, enabled: input.enabled };
        const next = rules.slice();
        next[current.index] = rule;
        const updatedRules = await updateForgeMatchReplaceRules(next);
        return { rule, index: current.index, rules: updatedRules };
      }
      case "intercept_queue_read":
        return pendingIntercepts.map((entry) => ({ id: entry.id, kind: entry.kind, method: entry.method, url: entry.url, host: entry.host, status: entry.status, length: entry.length, receivedAt: entry.receivedAt }));
      case "intercept_entry_read": {
        const id = forgeRequiredString(input, "id");
        const entry = pendingIntercepts.find((candidate) => candidate.id === id);
        if (!entry) throw new Error(`Intercept entry ${id} was not found`);
        return { id: entry.id, kind: entry.kind, method: entry.method, url: entry.url, host: entry.host, status: entry.status, raw: decodeHttpText(entry.raw), request: entry.requestRaw ? decodeHttpText(entry.requestRaw) : null };
      }
      case "intercept_entry_resolve": {
        const id = forgeRequiredString(input, "id");
        const action = forgeRequiredString(input, "action") as "forward" | "drop" | "modify";
        if (!["forward", "drop", "modify"].includes(action)) throw new Error("Interception action must be forward, drop, or modify");
        const entry = pendingIntercepts.find((candidate) => candidate.id === id);
        if (!entry) throw new Error(`Intercept entry ${id} was not found`);
        if (!beginInterceptResolution(id)) throw new Error("Interception is already being resolved");
        try {
          let raw = entry.raw;
          if (action === "modify") raw = encodeHttpText(forgeOptionalString(input, "raw") ?? decodeHttpText(entry.raw));
          if (action !== "drop" && entry.kind === "request") raw = finalizeHttpRequestBytes(raw);
          await commands.resolveInterception(id, action, action === "modify" ? raw : undefined);
          // A false backend result means another path already resolved the entry; either way it is no longer pending.
          removeIntercept(id);
          return { ok: true, id, action };
        } finally {
          endInterceptResolution(id);
        }
      }
      case "replay_tabs_list":
        return { activeTabId: activeReplayId, tabs: replayTabs.map(replayForgeSummary) };
      case "replay_tab_read":
        return replayForgeDetail(findReplayForForge(forgeRequiredInteger(input, "tabId")));
      case "replay_active_tab_read":
        return activeReplay ? replayForgeDetail(activeReplay) : null;
      case "replay_tab_create": {
        flushReplayDraft();
        const request = forgeRequestText(input.request);
        const tab = createForgeReplayTab(request, forgeOptionalBoolean(input, "tls", inferRequestTls(encodeHttpText(request))), forgeOptionalString(input, "title"));
        activeReplayId = tab.id; markWorkspaceDirty(); changeTab("Replay");
        return replayForgeDetail(tab);
      }
      case "replay_tabs_create": {
        flushReplayDraft();
        const created: ReplayTab[] = [];
        const globalTls = forgeOptionalBoolean(input, "tls", true);
        if (forgeHas(input, "requests")) {
          for (const [index, request] of forgeRequestArray(input, "requests").entries()) {
            if (created.length >= 100) throw new Error("At most 100 Replay tabs can be created at once");
            created.push(createForgeReplayTab(request, globalTls, `Forge ${nextReplayId}`));
          }
        }
        if (forgeHas(input, "tabs")) {
          for (const [index, item] of forgeObjectArray(input, "tabs").entries()) {
            if (created.length >= 100) throw new Error("At most 100 Replay tabs can be created at once");
            const request = forgeRequestText(item.request, `tabs[${index}].request`);
            created.push(createForgeReplayTab(request, forgeOptionalBoolean(item, "tls", globalTls), forgeOptionalString(item, "title") ?? `Forge ${nextReplayId}`));
          }
        }
        if (!created.length) throw new Error("Provide requests or tab definitions");
        activeReplayId = created[0].id; markWorkspaceDirty(); changeTab("Replay");
        return { count: created.length, tabIds: created.map((tab) => tab.id), tabs: created.map(replayForgeSummary) };
      }
      case "replay_tab_duplicate": {
        const source = findReplayForForge(forgeRequiredInteger(input, "tabId")); flushReplayDraft();
        const copy = createReplayTab(nextReplayId++, source.request.slice(), source.tls);
        copy.title = forgeOptionalString(input, "title")?.trim() || `${source.title} copy`;
        copy.identityConfig = source.identityConfig ? { ...source.identityConfig, identityIds: [...source.identityConfig.identityIds] } : null;
        replayTabs.push(copy); activeReplayId = copy.id; markWorkspaceDirty(); changeTab("Replay");
        return replayForgeDetail(copy);
      }
      case "replay_tabs_duplicate": {
        const created: ReplayTab[] = [];
        flushReplayDraft();
        for (const id of forgeIntegerArray(input, "tabIds")) {
          const source = findReplayForForge(id);
          if (created.length >= 100) throw new Error("At most 100 Replay tabs can be duplicated at once");
          const copy = createReplayTab(nextReplayId++, source.request.slice(), source.tls);
          copy.title = `${source.title} copy`;
          copy.identityConfig = source.identityConfig ? { ...source.identityConfig, identityIds: [...source.identityConfig.identityIds] } : null;
          replayTabs.push(copy); created.push(copy);
        }
        if (!created.length) throw new Error("At least one Replay tab ID is required");
        activeReplayId = created[0].id; markWorkspaceDirty(); changeTab("Replay");
        return { tabIds: created.map((tab) => tab.id), count: created.length };
      }
      case "replay_tab_update": {
        const tab = findReplayForForge(forgeRequiredInteger(input, "tabId")); flushReplayDraft();
        let updated = false;
        const title = forgeOptionalString(input, "title") ?? forgeOptionalString(input, "name");
        if (title !== undefined) {
          if (!title.trim()) throw new Error("Replay tab title must not be empty");
          tab.title = title.trim();
          updated = true;
        }
        if (forgeHas(input, "request")) {
          tab.request = normalizeHttpLineEndingBytes(encodeHttpText(forgeRequestText(input.request)));
          updated = true;
        }
        const operations = forgeHas(input, "operations") ? forgeObjectArray(input, "operations") : [];
        if (forgeHas(input, "host")) {
          operations.push({ type: "setHeader", name: "Host", value: normalizeReplayHost(forgeRequiredString(input, "host")) });
        }
        if (operations.length) {
          patchReplayRequest(tab, operations);
          updated = true;
        }
        if (forgeHas(input, "request") && !operations.length) {
          tab.response = new Uint8Array();
          tab.identityResponses = {};
          tab.activeIdentityResponseId = null;
        }
        if (forgeHas(input, "tls")) {
          tab.tls = forgeOptionalBoolean(input, "tls", tab.tls);
          updated = true;
        }
        if (!updated) throw new Error("Provide title, name, request, host, operations, or tls to update the Replay tab");
        markWorkspaceDirty();
        return replayForgeDetail(tab);
      }
      case "replay_protocol_set": {
        const tab = findReplayForForge(forgeRequiredInteger(input, "tabId"));
        tab.tls = forgeOptionalBoolean(input, "tls", tab.tls);
        tab.response = new Uint8Array();
        tab.identityResponses = {};
        tab.activeIdentityResponseId = null;
        markWorkspaceDirty();
        return replayForgeDetail(tab);
      }
      case "replay_tab_delete": {
        const id = forgeRequiredInteger(input, "tabId"); deleteForgeReplayTab(id); markWorkspaceDirty();
        return { ok: true, deleted: [id], activeTabId: activeReplayId };
      }
      case "replay_tabs_delete": {
        const deleted = forgeIntegerArray(input, "tabIds").filter((id, index, ids) => ids.indexOf(id) === index).map((id) => { deleteForgeReplayTab(id); return id; });
        markWorkspaceDirty(); return { ok: true, deleted, activeTabId: activeReplayId };
      }
      case "replay_tab_select": {
        const id = forgeRequiredInteger(input, "tabId"); findReplayForForge(id); flushReplayDraft(); activeReplayId = id; changeTab("Replay"); return { ok: true, activeTabId: id };
      }
      case "replay_request_patch": {
        const tab = findReplayForForge(forgeRequiredInteger(input, "tabId"));
        const operations = forgeObjectArray(input, "operations");
        if (operations.length > 100) throw new Error("At most 100 Replay edits can be applied at once");
        patchReplayRequest(tab, operations); return replayForgeDetail(tab);
      }
      case "replay_request_history_read": {
        const tab = findReplayForForge(forgeRequiredInteger(input, "tabId"));
        return { tabId: tab.id, currentIndex: tab.historyIndex, versions: tab.history.map((request, index) => ({ index, request: decodeHttpText(request), current: index === tab.historyIndex })) };
      }
      case "replay_request_history_restore": {
        const tab = findReplayForForge(forgeRequiredInteger(input, "tabId")); const index = forgeRequiredInteger(input, "index"); flushReplayDraft();
        if (index < 0 || index >= tab.history.length) throw new Error(`Replay request history index ${index} does not exist`);
        tab.request = tab.history[index].slice(); tab.historyIndex = index; tab.response = new Uint8Array(); markWorkspaceDirty(); return replayForgeDetail(tab);
      }
      case "replay_send": {
        const tab = findReplayForForge(forgeTabId(input)); activeReplayId = tab.id; changeTab("Replay"); return await runReplay(tab);
      }
      case "replay_cancel": {
        const tab = findReplayForForge(forgeTabId(input)); return await cancelReplay(tab);
      }
      case "replay_response_read": {
        const tab = findReplayForForge(forgeTabId(input)); const response = selectedReplayResponse(tab); return { tabId: tab.id, response: decodeHttpText(response), length: response.length, identityResponseId: tab.activeIdentityResponseId };
      }
      case "replay_response_clear": {
        const tab = findReplayForForge(forgeTabId(input)); tab.response = new Uint8Array(); tab.identityResponses = {}; tab.activeIdentityResponseId = null; markWorkspaceDirty(); return { ok: true, tabId: tab.id };
      }
      case "replay_identity_configure": {
        const tab = findReplayForForge(forgeRequiredInteger(input, "tabId")); const groupId = forgeRequiredString(input, "groupId"); const identityIds = forgeStringArray(input, "identityIds");
        const bundle = await commands.getIdentityGroups(); const group = bundle.groups.find((candidate) => candidate.id === groupId); const available = bundle.identities.filter((identity) => identity.groupId === groupId && identityIds.includes(identity.id));
        if (!group || !available.length || available.length !== identityIds.length) throw new Error("The selected identity group or identities do not exist");
        tab.identityConfig = { groupId, groupName: group.name, identityIds: available.map((identity) => identity.id) }; tab.response = new Uint8Array(); tab.identityResponses = {}; tab.activeIdentityResponseId = null; markWorkspaceDirty(); return replayForgeDetail(tab);
      }
      case "replay_open_from_history": {
        const detail = await commands.getHistoryDetail(forgeRequiredString(input, "historyId")); if (!detail) throw new Error("History entry was not found");
        const id = sendRawToReplay(new Uint8Array(detail.request), detail.entry.url.startsWith("https://")); return { tabId: id };
      }
      case "replay_open_from_organizer": {
        const bundle = await commands.getOrganizer(); const item = bundle.items.find((candidate) => candidate.id === forgeRequiredString(input, "itemId")); if (!item) throw new Error("Organizer item was not found");
        const id = sendRawToReplay(new Uint8Array(item.request), item.tls); return { tabId: id };
      }
      case "replay_send_to_fuzz": {
        const tab = findReplayForForge(forgeRequiredInteger(input, "tabId")); const id = sendRawToFuzz(tab.request, tab.tls); return { tabId: id };
      }
      case "replay_send_to_decoder": {
        const tab = findReplayForForge(forgeRequiredInteger(input, "tabId")); const part = (forgeOptionalString(input, "part") ?? "request").toLowerCase(); const raw = part === "response" ? selectedReplayResponse(tab) : tab.request; if (!["request", "response"].includes(part)) throw new Error("part must be request or response"); sendToDecoder(decodeHttpText(raw)); return { ok: true, part, length: raw.length };
      }
      case "replay_send_to_comparer": {
        const tab = findReplayForForge(forgeRequiredInteger(input, "tabId")); const part = (forgeOptionalString(input, "part") ?? "request").toLowerCase(); if (!["request", "response"].includes(part)) throw new Error("part must be request or response"); const raw = part === "response" ? selectedReplayResponse(tab) : tab.request; addToComparer(decodeHttpText(raw)); changeTab("Comparer"); return { ok: true, part, length: raw.length };
      }
      case "replay_save_to_organizer": {
        const tab = findReplayForForge(forgeRequiredInteger(input, "tabId"));
        const part = (forgeOptionalString(input, "part") ?? "request").toLowerCase();
        if (!["request", "response"].includes(part)) throw new Error("part must be request or response");
        const request = tab.request;
        const response = selectedReplayResponse(tab);
        if (part === "response" && !response.length) throw new Error("There is no response to save");
        if (!request.length) throw new Error("There is no request to save");
        const folderId = organizerNullableId(input.folderId, "folderId");
        const stageId = organizerNullableId(input.stageId, "stageId");
        const tags = forgeHas(input, "tags") ? forgeStringArray(input, "tags") : [];
        const bundle = await commands.getOrganizer();
        validateOrganizerPlacement(folderId, stageId, bundle);
        const item = await commands.createOrganizerItem({
          title: forgeOptionalString(input, "title") ?? tab.title,
          folderId,
          stageId,
          request: Array.from(request),
          response: Array.from(response),
          tls: tab.tls,
          source: "Replay",
          notes: forgeOptionalString(input, "notes") ?? "",
          tags: organizerTags(tags),
        });
        ensureForgeOrganizerTags(item.tags);
        organizerRevision += 1;
        return organizerDetail(item);
      }
      case "fuzz_tabs_list":
        return { activeTabId: activeFuzz?.id ?? fuzz.activeTabId, tabs: fuzz.tabs.filter((tab): tab is IntruderTab => tab.kind === "setup").map(fuzzForgeSummary) };
      case "fuzz_tab_read":
        return fuzzForgeDetail(findFuzzForForge(forgeRequiredInteger(input, "tabId")));
      case "fuzz_active_tab_read":
        return activeFuzz ? fuzzForgeDetail(activeFuzz) : null;
      case "fuzz_tab_create": {
        flushFuzzDraft(); const request = forgeRequestText(input.request); const raw = normalizeHttpLineEndingBytes(encodeHttpText(request)); const tab = createIntruderTab(fuzz.nextTabId++, raw, forgeOptionalBoolean(input, "tls", inferRequestTls(raw))); if (forgeOptionalString(input, "title")?.trim()) tab.title = forgeOptionalString(input, "title")!.trim(); if (forgeHas(input, "scanName")) tab.scanName = forgeRequiredString(input, "scanName").trim(); fuzz.tabs.push(tab); fuzz.activeTabId = tab.id; markWorkspaceDirty(); changeTab("Fuzz"); return fuzzForgeDetail(tab);
      }
      case "fuzz_tab_duplicate": {
        const source = findFuzzForForge(forgeRequiredInteger(input, "tabId")); flushFuzzDraft(); const copy = createIntruderTab(fuzz.nextTabId++, source.request.slice(), source.tls); copy.title = forgeOptionalString(input, "title")?.trim() || `${source.title} copy`; copy.mode = source.mode; copy.scanName = source.scanName; copy.warehouse = clonePayloadWarehouse(source.warehouse); copy.positionWarehouses = source.positionWarehouses.map(clonePayloadWarehouse); copy.selectedPayloadPosition = source.selectedPayloadPosition; fuzz.tabs.push(copy); fuzz.activeTabId = copy.id; markWorkspaceDirty(); changeTab("Fuzz"); return fuzzForgeDetail(copy);
      }
      case "fuzz_tab_update": {
        const tab = findFuzzForForge(forgeRequiredInteger(input, "tabId")); flushFuzzDraft(); if (forgeHas(input, "title")) tab.title = forgeRequiredString(input, "title"); if (forgeHas(input, "scanName")) tab.scanName = forgeRequiredString(input, "scanName").trim(); if (forgeHas(input, "request")) tab.request = normalizeHttpLineEndingBytes(encodeHttpText(forgeRequestText(input.request))); if (forgeHas(input, "tls")) tab.tls = forgeOptionalBoolean(input, "tls", tab.tls); if (forgeHas(input, "mode")) { const mode = forgeRequiredString(input, "mode"); if (!["single", "spread", "map", "combine"].includes(mode)) throw new Error("Unknown Fuzz mode"); tab.mode = mode as IntruderMode; } markWorkspaceDirty(); return fuzzForgeDetail(tab);
      }
      case "fuzz_tab_delete": {
        const id = forgeRequiredInteger(input, "tabId"); deleteForgeFuzzTab(id); markWorkspaceDirty(); return { ok: true, deleted: id, activeTabId: fuzz.activeTabId };
      }
      case "fuzz_tab_select": {
        const id = forgeRequiredInteger(input, "tabId"); findFuzzForForge(id); flushFuzzDraft(); fuzz.activeTabId = id; changeTab("Fuzz"); return { ok: true, activeTabId: id };
      }
      case "fuzz_positions_set": {
        const tab = findFuzzForForge(forgeRequiredInteger(input, "tabId")); flushFuzzDraft(); const values = forgeStringArray(input, "values"); if (!values.length) throw new Error("At least one position value is required"); let textValue = decodeHttpText(tab.request); let cursor = 0; for (const target of values) { if (!target || target.includes("§")) throw new Error("Position values must be non-empty and cannot contain §"); const index = textValue.indexOf(target, cursor); if (index < 0) throw new Error(`Position value '${target}' was not found after the previous position`); textValue = `${textValue.slice(0, index)}§${target}§${textValue.slice(index + target.length)}`; cursor = index + target.length + 2; } tab.request = encodeHttpText(textValue); fuzz.editorDraft = null; markWorkspaceDirty(); return fuzzForgeDetail(tab);
      }
      case "fuzz_positions_clear": {
        const tab = findFuzzForForge(forgeRequiredInteger(input, "tabId")); flushFuzzDraft(); tab.request = encodeHttpText(synchronizeHttpContentLength(decodeHttpText(tab.request).replaceAll("§", ""))); markWorkspaceDirty(); return fuzzForgeDetail(tab);
      }
      case "fuzz_mode_set": {
        const tab = findFuzzForForge(forgeRequiredInteger(input, "tabId")); const mode = forgeRequiredString(input, "mode"); if (!["single", "spread", "map", "combine"].includes(mode)) throw new Error("Unknown Fuzz mode"); tab.mode = mode as IntruderMode; markWorkspaceDirty(); return fuzzForgeSummary(tab);
      }
      case "fuzz_warehouse_read": {
        const tab = findFuzzForForge(forgeRequiredInteger(input, "tabId"));
        const target = forgeFuzzWarehouseTarget(tab, input, false);
        return forgeFuzzRuleResult(tab, target.positionIndex, target.warehouse);
      }
      case "fuzz_generator_set": {
        const tab = findFuzzForForge(forgeRequiredInteger(input, "tabId"));
        const generator = forgeFuzzGenerator(input.generator, "generator");
        const target = forgeFuzzWarehouseTarget(tab, input, true);
        const warehouse = clonePayloadWarehouse(target.warehouse);
        warehouse.type = generator.type;
        if (generator.type === "list") warehouse.list = generator.list;
        if (generator.type === "numbers") warehouse.numbers = generator.numbers;
        if (generator.type === "null") warehouse.nullPayload = generator.nullPayload;
        if (generator.type === "bruteForce") warehouse.bruteForce = generator.bruteForce;
        if (generator.type === "dates") warehouse.dates = generator.dates;
        if (generator.type === "characterSubstitution") warehouse.characterSubstitution = generator.characterSubstitution;
        setForgeFuzzWarehouse(tab, target.positionIndex, warehouse);
        markWorkspaceDirty();
        return forgeFuzzRuleResult(tab, target.positionIndex, warehouse, { generator: { ...generator } });
      }
      case "fuzz_warehouse_set": {
        const tab = findFuzzForForge(forgeRequiredInteger(input, "tabId"));
        const warehouse = forgeFuzzWarehouse(input.warehouse, "warehouse");
        const target = forgeFuzzWarehouseTarget(tab, input, true);
        setForgeFuzzWarehouse(tab, target.positionIndex, clonePayloadWarehouse(warehouse));
        markWorkspaceDirty();
        return forgeFuzzRuleResult(tab, target.positionIndex, warehouse);
      }
      case "fuzz_payload_processing_rules_read": {
        const tab = findFuzzForForge(forgeRequiredInteger(input, "tabId"));
        const target = forgeFuzzWarehouseTarget(tab, input, false);
        return forgeFuzzRuleResult(tab, target.positionIndex, target.warehouse);
      }
      case "fuzz_payload_processing_rule_create": {
        const tab = findFuzzForForge(forgeRequiredInteger(input, "tabId"));
        const rule = forgePayloadProcessingRule(input.rule, "rule");
        const target = forgeFuzzWarehouseTarget(tab, input, true);
        forgeUniqueRuleId(target.warehouse.processing, rule.id, "Payload processing rule");
        const warehouse = clonePayloadWarehouse(target.warehouse);
        warehouse.processing = [...warehouse.processing, rule];
        setForgeFuzzWarehouse(tab, target.positionIndex, warehouse);
        markWorkspaceDirty();
        return forgeFuzzRuleResult(tab, target.positionIndex, warehouse, { rule, index: warehouse.processing.length - 1 });
      }
      case "fuzz_payload_processing_rule_update": {
        const tab = findFuzzForForge(forgeRequiredInteger(input, "tabId"));
        const id = forgeRuleId(input.id, "id");
        const rule = forgePayloadProcessingRule(input.rule, "rule", id);
        const target = forgeFuzzWarehouseTarget(tab, input, true);
        const current = forgeExistingRule(target.warehouse.processing, id, "Payload processing rule");
        const warehouse = clonePayloadWarehouse(target.warehouse);
        warehouse.processing[current.index] = rule;
        setForgeFuzzWarehouse(tab, target.positionIndex, warehouse);
        markWorkspaceDirty();
        return forgeFuzzRuleResult(tab, target.positionIndex, warehouse, { rule, index: current.index });
      }
      case "fuzz_payload_processing_rule_delete": {
        const tab = findFuzzForForge(forgeRequiredInteger(input, "tabId"));
        const id = forgeRuleId(input.id, "id");
        const target = forgeFuzzWarehouseTarget(tab, input, true);
        const current = forgeExistingRule(target.warehouse.processing, id, "Payload processing rule");
        const warehouse = clonePayloadWarehouse(target.warehouse);
        warehouse.processing = warehouse.processing.filter((_, index) => index !== current.index);
        setForgeFuzzWarehouse(tab, target.positionIndex, warehouse);
        markWorkspaceDirty();
        return forgeFuzzRuleResult(tab, target.positionIndex, warehouse, { deletedId: id });
      }
      case "fuzz_payload_processing_rule_reorder": {
        const tab = findFuzzForForge(forgeRequiredInteger(input, "tabId"));
        const id = forgeRuleId(input.id, "id");
        const target = forgeFuzzWarehouseTarget(tab, input, true);
        const current = forgeExistingRule(target.warehouse.processing, id, "Payload processing rule");
        const toIndex = forgeRuleIndex(input.toIndex, "toIndex", target.warehouse.processing.length);
        const warehouse = clonePayloadWarehouse(target.warehouse);
        warehouse.processing = forgeReorderedRules(warehouse.processing, current.index, toIndex);
        setForgeFuzzWarehouse(tab, target.positionIndex, warehouse);
        markWorkspaceDirty();
        return forgeFuzzRuleResult(tab, target.positionIndex, warehouse, { rule: warehouse.processing[toIndex], index: toIndex });
      }
      case "fuzz_payload_processing_rule_set_enabled": {
        const tab = findFuzzForForge(forgeRequiredInteger(input, "tabId"));
        const id = forgeRuleId(input.id, "id");
        if (typeof input.enabled !== "boolean") throw new Error("enabled must be a boolean");
        const target = forgeFuzzWarehouseTarget(tab, input, true);
        const current = forgeExistingRule(target.warehouse.processing, id, "Payload processing rule");
        const warehouse = clonePayloadWarehouse(target.warehouse);
        const rule = { ...warehouse.processing[current.index], enabled: input.enabled };
        warehouse.processing[current.index] = rule;
        setForgeFuzzWarehouse(tab, target.positionIndex, warehouse);
        markWorkspaceDirty();
        return forgeFuzzRuleResult(tab, target.positionIndex, warehouse, { rule, index: current.index });
      }
      case "fuzz_plan_preview":
        return fuzzPlanForForge(findFuzzForForge(forgeRequiredInteger(input, "tabId")));
      case "fuzz_payload_preview": {
        const tab = findFuzzForForge(forgeRequiredInteger(input, "tabId"));
        const requestedLimit = forgeOptionalInteger(input, "sampleLimit");
        const sampleLimit = requestedLimit ?? 10;
        if (sampleLimit < 1 || sampleLimit > 50) throw new Error("sampleLimit must be between 1 and 50");
        const target = forgeFuzzWarehouseTarget(tab, input, false);
        const generated = generatePayloads(target.warehouse);
        const payloads = await processPayloads(generated.payloads, target.warehouse.processing);
        return {
          tabId: tab.id,
          positionIndex: target.positionIndex,
          type: target.warehouse.type,
          generatedCount: generated.repeatIndefinitely ? null : payloads.length,
          repeatIndefinitely: generated.repeatIndefinitely,
          samples: payloads.slice(0, sampleLimit),
        };
      }
      case "fuzz_start": {
        const tab = findFuzzForForge(forgeTabId(input)); if (forgeHas(input, "scanName")) tab.scanName = forgeRequiredString(input, "scanName").trim(); if (!tab.scanName) throw new Error("scanName is required"); fuzz.activeTabId = tab.id; aiFuzzLaunch = { id: uniqueExecutionId(), tabId: tab.id, action: "start" }; markWorkspaceDirty(); changeTab("Fuzz"); return { ok: true, scheduled: true, tabId: tab.id, scanName: tab.scanName };
      }
      case "fuzz_stop": {
        const requestedTabId = forgeTabId(input); const tab = findFuzzForForge(requestedTabId); const scanId = fuzzForgeScanId(tab, requestedTabId, forgeOptionalString(input, "scanId")); const scan = tab.scans.find((candidate) => candidate.session.id === scanId); if (!scan) throw new Error("Fuzz scan was not found"); if (!scan.running) return fuzzForgeScanSummary(scan); scan.stopRequested = true; scan.stopped = true; scan.running = false; if (scan.currentRequestId) await commands.cancelRepeaterRequest(scan.currentRequestId).catch(() => {}); markWorkspaceDirty(); return fuzzForgeScanSummary(scan);
      }
      case "fuzz_resume": {
        const requestedTabId = forgeTabId(input); const tab = findFuzzForForge(requestedTabId); const scanId = fuzzForgeScanId(tab, requestedTabId, forgeOptionalString(input, "scanId")); const scan = tab.scans.find((candidate) => candidate.session.id === scanId); if (!scan) throw new Error("Fuzz scan was not found"); if (scan.running) throw new Error("Fuzz scan is already running"); if (!scan.session.repeatIndefinitely && scan.nextPayloadIndex >= scan.session.payloadRows.length) throw new Error("Fuzz scan has no remaining rows"); fuzz.activeTabId = tab.id; aiFuzzLaunch = { id: uniqueExecutionId(), tabId: tab.id, action: "resume", scanId: scan.session.id }; markWorkspaceDirty(); changeTab("Fuzz"); return { ok: true, scheduled: true, tabId: tab.id, scanId: scan.session.id };
      }
      case "fuzz_scans_read": {
        const tab = findFuzzForForge(forgeRequiredInteger(input, "tabId")); return { tabId: tab.id, scans: tab.scans.map(fuzzForgeScanSummary) };
      }
      case "fuzz_results_read": {
        const tab = findFuzzForForge(forgeRequiredInteger(input, "tabId")); const scanId = forgeRequiredString(input, "scanId"); const scan = tab.scans.find((candidate) => candidate.session.id === scanId); if (!scan) throw new Error("Fuzz scan was not found"); const limit = Math.max(1, Math.min(500, forgeOptionalInteger(input, "limit") ?? 50)); return { tabId: tab.id, scanId, results: scan.results.slice(0, limit).map((result) => ({ id: result.id, sequence: result.sequence, position: result.position, payload: result.payload, payloads: result.payloads, status: result.status, length: result.length, durationMs: result.durationMs, error: result.error, request: decodeHttpText(result.request), response: decodeHttpText(result.response) })) };
      }
      case "fuzz_result_open_in_replay": {
        const tab = findFuzzForForge(forgeRequiredInteger(input, "tabId")); const scan = tab.scans.find((candidate) => candidate.session.id === forgeRequiredString(input, "scanId")); const result = scan?.results.find((candidate) => candidate.id === forgeRequiredString(input, "resultId")); if (!scan || !result) throw new Error("Fuzz result was not found"); return { tabId: sendRawToReplay(result.request, scan.session.tls) };
      }
      case "fuzz_result_save_to_organizer": {
        const tab = findFuzzForForge(forgeRequiredInteger(input, "tabId")); const scan = tab.scans.find((candidate) => candidate.session.id === forgeRequiredString(input, "scanId")); const result = scan?.results.find((candidate) => candidate.id === forgeRequiredString(input, "resultId")); if (!scan || !result) throw new Error("Fuzz result was not found"); const item = await commands.createOrganizerItem({ title: `${scan.name} · ${result.payload}`, folderId: null, stageId: null, request: Array.from(result.request), response: Array.from(result.response), tls: scan.session.tls, source: "Fuzz", notes: "", tags: [] }); organizerRevision += 1; return item;
      }
      case "history_search": {
        const filter: HistoryFilter = { ...historyFilter, search: forgeOptionalString(input, "search") ?? null, host: forgeOptionalString(input, "host") ?? null, method: forgeOptionalString(input, "method") ?? null, statusMin: forgeOptionalInteger(input, "statusMin") ?? null, statusMax: forgeOptionalInteger(input, "statusMax") ?? null };
        const limit = Math.max(1, Math.min(500, forgeOptionalInteger(input, "limit") ?? 100)); const entries = await commands.queryHistory(filter, 0, limit); history = entries; historyHasMore = entries.length === limit; return { entries };
      }
      case "history_read": {
        const detail = await commands.getHistoryDetail(forgeRequiredString(input, "historyId")); if (!detail) throw new Error("History entry was not found"); return { entry: detail.entry, request: decodeHttpText(new Uint8Array(detail.request)), response: decodeHttpText(new Uint8Array(detail.response)) };
      }
      case "history_delete": {
        const historyId = forgeRequiredString(input, "historyId"); if (!(await commands.deleteHistoryEntry(historyId))) throw new Error("History entry was not found"); history = history.filter((entry) => entry.id !== historyId); if (historyDetail?.entry.id === historyId) historyDetail = null; markWorkspaceDirty(); return { ok: true, historyId };
      }
      case "history_clear":
        await commands.clearHistory(); history = []; historyDetail = null; markWorkspaceDirty(); return { ok: true };
      case "history_open_in_fuzz": {
        const detail = await commands.getHistoryDetail(forgeRequiredString(input, "historyId")); if (!detail) throw new Error("History entry was not found"); return { tabId: sendRawToFuzz(new Uint8Array(detail.request), detail.entry.url.startsWith("https://")) };
      }
      case "history_save_to_organizer": {
        const detail = await commands.getHistoryDetail(forgeRequiredString(input, "historyId")); if (!detail) throw new Error("History entry was not found"); const item = await commands.createOrganizerItem({ title: detail.entry.url, folderId: null, stageId: null, request: detail.request, response: detail.response, tls: detail.entry.url.startsWith("https://"), source: "History", notes: "", tags: [] }); organizerRevision += 1; return item;
      }
      case "history_compare": {
        const detail = await commands.getHistoryDetail(forgeRequiredString(input, "historyId")); if (!detail) throw new Error("History entry was not found"); const part = (forgeOptionalString(input, "part") ?? "request").toLowerCase(); if (!["request", "response"].includes(part)) throw new Error("part must be request or response"); const value = decodeHttpText(new Uint8Array(part === "request" ? detail.request : detail.response)); addToComparer(value); changeTab("Comparer"); return { ok: true, part };
      }
      case "history_decode": {
        const detail = await commands.getHistoryDetail(forgeRequiredString(input, "historyId")); if (!detail) throw new Error("History entry was not found"); const part = (forgeOptionalString(input, "part") ?? "request").toLowerCase(); if (!["request", "response"].includes(part)) throw new Error("part must be request or response"); const value = decodeHttpText(new Uint8Array(part === "request" ? detail.request : detail.response)); sendToDecoder(value); return { ok: true, part, length: value.length };
      }
      case "site_map_read": {
        const filter: HistoryFilter = { ...historyFilter, search: null }; const entries = await commands.queryHistory(filter, 0, 5_000); const scope = await commands.getScope(); const branches = buildSiteMap(entries, forgeOptionalString(input, "search") ?? siteMapWorkspace.search, forgeOptionalBoolean(input, "inScopeOnly", siteMapWorkspace.inScopeOnly)); const limit = Math.max(1, Math.min(1_000, forgeOptionalInteger(input, "limit") ?? 200)); const rows = flattenSiteMap(branches, new Set<string>(), true); return { rows: rows.slice(0, limit).map((row) => row.kind === "endpoint" ? { kind: row.kind, key: row.key, label: row.label, depth: row.depth, entry: row.entry, inScope: isHostInScope(row.entry.host, scope.entries) } : { kind: row.kind, key: row.key, label: row.label, depth: row.depth, host: row.node.host, path: row.node.path, url: row.node.url, entryCount: row.node.entries.length }) };
      }
      case "site_map_endpoint_open": {
        const detail = await commands.getHistoryDetail(forgeRequiredString(input, "historyId")); if (!detail) throw new Error("Site Map endpoint was not found"); return { tabId: sendRawToReplay(new Uint8Array(detail.request), detail.entry.url.startsWith("https://")) };
      }
      case "scope_read":
        return await commands.getScope();
      case "scope_entry_add":
        return await addScopeFromAgent({ pattern: forgeRequiredString(input, "pattern"), isRegex: forgeOptionalBoolean(input, "isRegex", false), includeSubdomains: forgeOptionalBoolean(input, "includeSubdomains", false), isInScope: forgeOptionalBoolean(input, "isInScope", true) });
      case "scope_entry_update":
        return await updateScopeFromAgent({ id: forgeRequiredInteger(input, "id"), pattern: forgeRequiredString(input, "pattern"), isRegex: forgeOptionalBoolean(input, "isRegex", false), includeSubdomains: forgeOptionalBoolean(input, "includeSubdomains", false), isInScope: forgeOptionalBoolean(input, "isInScope", true) });
      case "scope_entry_delete":
        return await removeScopeFromAgent(forgeRequiredInteger(input, "id"));
      case "scope_entries_import":
        return await commands.importScopeEntries(forgeStringArray(input, "entries"));
      case "organizer_read":
        return await commands.getOrganizer();
      case "organizer_state_read": {
        const bundle = await commands.getOrganizer();
        return {
          folders: bundle.folders,
          items: bundle.items.map(organizerSummary),
          tagDefinitions: organizerWorkspace.tagDefinitions.map((tag) => ({ ...tag })),
          stages: organizerWorkspace.stages.map((stage) => ({ ...stage })),
          view: organizerViewSnapshot(),
        };
      }
      case "organizer_items_list": {
        const bundle = await commands.getOrganizer();
        const rawFolder = forgeHas(input, "folderId") ? forgeRequiredString(input, "folderId").trim() : "all";
        const folderId = !rawFolder || rawFolder.toLocaleLowerCase() === "all"
          ? "all"
          : rawFolder.toLocaleLowerCase() === "unfiled" ? "unfiled" : rawFolder;
        if (folderId !== "all" && folderId !== "unfiled" && !bundle.folders.some((folder) => folder.id === folderId)) throw new Error("Organizer folder was not found");
        const stageFilter = forgeHas(input, "stageId") ? organizerNullableId(input.stageId, "stageId") : undefined;
        if (stageFilter && !organizerWorkspace.stages.some((stage) => stage.id === stageFilter)) throw new Error("Organizer stage was not found");
        const tagFilter = forgeHas(input, "tag") ? forgeRequiredString(input, "tag").trim().toLocaleLowerCase() : "";
        const needle = forgeHas(input, "search") ? forgeRequiredString(input, "search").trim().toLocaleLowerCase() : "";
        const sort = organizerSort(forgeHas(input, "sort") ? forgeRequiredString(input, "sort") : undefined);
        const limit = Math.max(1, Math.min(500, forgeOptionalInteger(input, "limit") ?? 100));
        const folderIds = folderId !== "all" && folderId !== "unfiled"
          ? new Set([folderId, ...organizerFolderDescendants(folderId, bundle)])
          : null;
        const filtered = bundle.items.filter((item) => {
          if (folderId === "unfiled" && item.folderId) return false;
          if (folderIds && (!item.folderId || !folderIds.has(item.folderId))) return false;
          if (stageFilter !== undefined && item.stageId !== stageFilter) return false;
          if (tagFilter && !item.tags.some((tag) => tag.toLocaleLowerCase() === tagFilter)) return false;
          if (!needle) return true;
          return [
            item.title, item.method, item.host, item.path, item.source, item.notes, ...item.tags,
            decodeHttpText(new Uint8Array(item.request)), decodeHttpText(new Uint8Array(item.response)),
          ].some((value) => value.toLocaleLowerCase().includes(needle));
        }).sort((left, right) => {
          if (sort === "title") return left.title.localeCompare(right.title) || left.updatedAt.localeCompare(right.updatedAt);
          if (sort === "host") return left.host.localeCompare(right.host) || left.path.localeCompare(right.path);
          const field = sort === "created" ? "createdAt" : "updatedAt";
          return right[field].localeCompare(left[field]);
        });
        return { count: filtered.length, items: filtered.slice(0, limit).map(organizerSummary) };
      }
      case "organizer_item_read": {
        const { item } = await organizerItemForForge(forgeRequiredString(input, "id"));
        return organizerDetail(item);
      }
      case "organizer_view_update": {
        let selectedFolderId = organizerWorkspace.selectedFolderId;
        if (forgeHas(input, "folderId")) {
          const rawFolder = forgeRequiredString(input, "folderId").trim();
          if (!rawFolder || rawFolder.toLocaleLowerCase() === "all") selectedFolderId = "all";
          else if (rawFolder.toLocaleLowerCase() === "unfiled") selectedFolderId = "unfiled";
          else {
            const bundle = await commands.getOrganizer();
            if (!bundle.folders.some((folder) => folder.id === rawFolder)) throw new Error("Organizer folder was not found");
            selectedFolderId = rawFolder;
          }
        }
        const selectedTag = forgeHas(input, "tag") ? forgeRequiredString(input, "tag").trim() : organizerWorkspace.selectedTag;
        const query = forgeHas(input, "query") ? forgeRequiredString(input, "query") : organizerWorkspace.query;
        const sort = organizerSort(forgeHas(input, "sort") ? forgeRequiredString(input, "sort") : organizerWorkspace.sort);
        updateOrganizerWorkspace({ selectedFolderId, selectedTag, query, sort });
        changeTab("Organizer");
        return organizerViewSnapshot();
      }
      case "organizer_folder_create":
        { const folder = await commands.createOrganizerFolder(forgeRequiredString(input, "name"), forgeOptionalString(input, "parentId") ?? null); organizerRevision += 1; return folder; }
      case "organizer_folder_update": {
        const id = forgeRequiredString(input, "id");
        const bundle = await commands.getOrganizer();
        const current = bundle.folders.find((candidate) => candidate.id === id);
        if (!current) throw new Error("Organizer folder was not found");
        const parentId = forgeHas(input, "parentId") ? forgeOptionalString(input, "parentId") ?? null : current.parentId;
        const folder = await commands.updateOrganizerFolder(id, forgeRequiredString(input, "name"), parentId);
        organizerRevision += 1;
        return folder;
      }
      case "organizer_folder_delete":
        { const id = forgeRequiredString(input, "id"); if (!(await commands.deleteOrganizerFolder(id))) throw new Error("Organizer folder was not found"); organizerRevision += 1; return { ok: true, id }; }
      case "organizer_item_create": {
        const itemInput = organizerInput(input.input);
        const bundle = await commands.getOrganizer();
        validateOrganizerPlacement(itemInput.folderId, itemInput.stageId, bundle);
        const item = await commands.createOrganizerItem(itemInput);
        ensureForgeOrganizerTags(item.tags);
        organizerRevision += 1;
        return organizerDetail(item);
      }
      case "organizer_item_update": {
        const itemInput = organizerInput(input.input);
        const bundle = await commands.getOrganizer();
        validateOrganizerPlacement(itemInput.folderId, itemInput.stageId, bundle);
        const item = await commands.updateOrganizerItem(forgeRequiredString(input, "id"), itemInput);
        if (!item) throw new Error("Organizer item was not found");
        ensureForgeOrganizerTags(item.tags);
        organizerRevision += 1;
        return organizerDetail(item);
      }
      case "organizer_item_patch": {
        const { bundle, item } = await organizerItemForForge(forgeRequiredString(input, "id"));
        const patch = forgeObject(input.patch, "Organizer item patch");
        const itemInput = organizerItemInputFromItem(item);
        if (forgeHas(patch, "title")) itemInput.title = forgeRequiredString(patch, "title");
        if (forgeHas(patch, "folderId")) itemInput.folderId = organizerNullableId(patch.folderId, "folderId");
        if (forgeHas(patch, "stageId")) itemInput.stageId = organizerNullableId(patch.stageId, "stageId");
        if (forgeHas(patch, "request")) itemInput.request = Array.from(encodeHttpText(forgeRequiredString(patch, "request")));
        if (forgeHas(patch, "response")) itemInput.response = Array.from(encodeHttpText(forgeRequiredString(patch, "response")));
        if (forgeHas(patch, "tls")) itemInput.tls = forgeOptionalBoolean(patch, "tls", itemInput.tls);
        if (forgeHas(patch, "source")) itemInput.source = forgeRequiredString(patch, "source");
        if (forgeHas(patch, "notes")) itemInput.notes = forgeRequiredString(patch, "notes");
        if (forgeHas(patch, "tags")) itemInput.tags = organizerTags(forgeStringArray(patch, "tags"));
        if (!itemInput.request.length) throw new Error("Organizer item request must not be empty");
        validateOrganizerPlacement(itemInput.folderId, itemInput.stageId, bundle);
        const updated = await commands.updateOrganizerItem(item.id, itemInput);
        ensureForgeOrganizerTags(updated.tags);
        organizerRevision += 1;
        return organizerDetail(updated);
      }
      case "organizer_item_delete":
        { const id = forgeRequiredString(input, "id"); if (!(await commands.deleteOrganizerItem(id))) throw new Error("Organizer item was not found"); organizerRevision += 1; return { ok: true, id }; }
      case "organizer_item_move": {
        const { bundle, item } = await organizerItemForForge(forgeRequiredString(input, "id"));
        const folderId = forgeHas(input, "folderId") ? organizerNullableId(input.folderId, "folderId") : item.folderId;
        const stageId = forgeHas(input, "stageId") ? organizerNullableId(input.stageId, "stageId") : item.stageId;
        validateOrganizerPlacement(folderId, stageId, bundle);
        const updated = await commands.updateOrganizerItem(item.id, organizerItemInputFromItem(item, folderId, stageId));
        organizerRevision += 1;
        return organizerDetail(updated);
      }
      case "organizer_tag_create": {
        const name = forgeRequiredString(input, "name").trim();
        if (!name) throw new Error("Organizer tag name must not be empty");
        if (organizerWorkspace.tagDefinitions.some((tag) => tag.name.toLocaleLowerCase() === name.toLocaleLowerCase())) throw new Error("Organizer tag already exists");
        const tag = { name, color: organizerColor(forgeOptionalString(input, "color")) };
        updateOrganizerWorkspace({ tagDefinitions: [...organizerWorkspace.tagDefinitions, tag] });
        return tag;
      }
      case "organizer_tag_update": {
        const name = forgeRequiredString(input, "name").trim();
        const current = organizerWorkspace.tagDefinitions.find((tag) => tag.name.toLocaleLowerCase() === name.toLocaleLowerCase());
        if (!current) throw new Error("Organizer tag was not found");
        const nextName = forgeOptionalString(input, "newName")?.trim() || current.name;
        if (!nextName) throw new Error("Organizer tag name must not be empty");
        if (nextName.toLocaleLowerCase() !== current.name.toLocaleLowerCase() && organizerWorkspace.tagDefinitions.some((tag) => tag.name.toLocaleLowerCase() === nextName.toLocaleLowerCase())) throw new Error("Organizer tag already exists");
        const tag = { name: nextName, color: organizerColor(forgeOptionalString(input, "color"), current.color) };
        updateOrganizerWorkspace({ tagDefinitions: organizerWorkspace.tagDefinitions.map((item) => item.name === current.name ? tag : item) });
        return tag;
      }
      case "organizer_tag_delete": {
        const name = forgeRequiredString(input, "name").trim();
        const current = organizerWorkspace.tagDefinitions.find((tag) => tag.name.toLocaleLowerCase() === name.toLocaleLowerCase());
        if (!current) throw new Error("Organizer tag was not found");
        const bundle = await commands.getOrganizer();
        const usedCount = bundle.items.filter((item) => item.tags.some((tag) => tag.toLocaleLowerCase() === current.name.toLocaleLowerCase())).length;
        updateOrganizerWorkspace({
          selectedTag: organizerWorkspace.selectedTag.toLocaleLowerCase() === current.name.toLocaleLowerCase() ? "" : organizerWorkspace.selectedTag,
          tagDefinitions: organizerWorkspace.tagDefinitions.filter((tag) => tag.name !== current.name),
        });
        return { ok: true, name: current.name, usedCount };
      }
      case "organizer_stage_create": {
        const name = forgeRequiredString(input, "name").trim();
        if (!name) throw new Error("Organizer stage name must not be empty");
        if (organizerWorkspace.stages.some((stage) => stage.name.toLocaleLowerCase() === name.toLocaleLowerCase())) throw new Error("Organizer stage already exists");
        const stage = { id: `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`, name, color: organizerColor(forgeOptionalString(input, "color")) };
        updateOrganizerWorkspace({ stages: [...organizerWorkspace.stages, stage] });
        return stage;
      }
      case "organizer_stage_update": {
        const id = forgeRequiredString(input, "id");
        const current = organizerWorkspace.stages.find((stage) => stage.id === id);
        if (!current) throw new Error("Organizer stage was not found");
        const name = forgeOptionalString(input, "name")?.trim() || current.name;
        if (!name) throw new Error("Organizer stage name must not be empty");
        if (name.toLocaleLowerCase() !== current.name.toLocaleLowerCase() && organizerWorkspace.stages.some((stage) => stage.name.toLocaleLowerCase() === name.toLocaleLowerCase())) throw new Error("Organizer stage already exists");
        const stage = { id, name, color: organizerColor(forgeOptionalString(input, "color"), current.color) };
        updateOrganizerWorkspace({ stages: organizerWorkspace.stages.map((item) => item.id === id ? stage : item) });
        return stage;
      }
      case "organizer_stage_delete": {
        const id = forgeRequiredString(input, "id");
        const stage = organizerWorkspace.stages.find((candidate) => candidate.id === id);
        if (!stage) throw new Error("Organizer stage was not found");
        const bundle = await commands.getOrganizer();
        const usedCount = bundle.items.filter((item) => item.stageId === id).length;
        if (usedCount) throw new Error(`Organizer stage is assigned to ${usedCount} entr${usedCount === 1 ? "y" : "ies"}; unassign them first`);
        updateOrganizerWorkspace({ stages: organizerWorkspace.stages.filter((candidate) => candidate.id !== id) });
        return { ok: true, id, name: stage.name };
      }
      case "organizer_stage_reorder": {
        const id = forgeRequiredString(input, "id");
        const beforeId = forgeOptionalString(input, "beforeId")?.trim() || "";
        const currentIndex = organizerWorkspace.stages.findIndex((stage) => stage.id === id);
        if (currentIndex < 0) throw new Error("Organizer stage was not found");
        if (beforeId && !organizerWorkspace.stages.some((stage) => stage.id === beforeId)) throw new Error("Organizer target stage was not found");
        const stages = [...organizerWorkspace.stages];
        const [moved] = stages.splice(currentIndex, 1);
        const targetIndex = beforeId ? stages.findIndex((stage) => stage.id === beforeId) : stages.length;
        stages.splice(targetIndex < 0 ? stages.length : targetIndex, 0, moved);
        updateOrganizerWorkspace({ stages });
        return { stages };
      }
      case "organizer_import":
        { const count = await commands.importOrganizer(input.bundle as OrganizerBundle); organizerRevision += 1; return { ok: true, count }; }
      case "organizer_export":
        return await commands.exportOrganizerJson();
      case "identity_groups_read":
        return await commands.getIdentityGroups();
      case "identity_group_create":
        return await commands.createIdentityGroup(identityGroupInput(input.input));
      case "identity_group_update":
        return await commands.updateIdentityGroup(forgeRequiredString(input, "id"), identityGroupInput(input.input));
      case "identity_group_delete":
        { const id = forgeRequiredString(input, "id"); if (!(await commands.deleteIdentityGroup(id))) throw new Error("Identity group was not found"); return { ok: true, id }; }
      case "identity_create":
        return await commands.createIdentity(identityInput(input.input));
      case "identity_update":
        return await commands.updateIdentity(forgeRequiredString(input, "id"), identityInput(input.input));
      case "identity_delete":
        { const id = forgeRequiredString(input, "id"); if (!(await commands.deleteIdentity(id))) throw new Error("Identity was not found"); return { ok: true, id }; }
      case "identity_injection_preview":
        return await commands.resolveIdentityInjection(forgeRequiredString(input, "identityId"));
      case "decoder_state_read":
        return { input: decoderInput, workspace: cloneJson(decoderWorkspace), output: decoderWorkspace.stageOutputs.at(-1) ?? decoderInput };
      case "decoder_input_set":
        decoderInput = forgeRequiredString(input, "input"); decoderWorkspace.input = decoderInput; decoderWorkspace.stageOutputs = []; markWorkspaceDirty(); return { ok: true, input: decoderInput };
      case "decoder_transform":
        { const value = forgeRequiredString(input, "input"); const operation = forgeRequiredString(input, "operation"); const result = await commands.decoderTransform(value, operation, forgeOptionalBoolean(input, "padding", true)); decoderInput = value; decoderWorkspace.input = value; decoderWorkspace.stageOutputs = [result.output]; decoderWorkspace.detected = result.detected; markWorkspaceDirty(); return result; }
      case "decoder_output_use_in_replay":
        { const output = decoderWorkspace.stageOutputs.at(-1) ?? decoderInput; const id = sendRawToReplay(encodeHttpText(output), true); return { tabId: id }; }
      case "decoder_output_use_in_fuzz":
        { const output = decoderWorkspace.stageOutputs.at(-1) ?? decoderInput; const id = sendRawToFuzz(encodeHttpText(output), true); return { tabId: id }; }
      case "comparer_state_read":
        return cloneJson(comparerWorkspace);
      case "comparer_inputs_set":
        if (forgeHas(input, "left")) comparerLeft = forgeRequiredString(input, "left"); if (forgeHas(input, "right")) comparerRight = forgeRequiredString(input, "right"); comparerWorkspace = { ...comparerWorkspace, left: comparerLeft, right: comparerRight }; markWorkspaceDirty(); return cloneJson(comparerWorkspace);
      case "comparer_run":
        { const granularity = (forgeOptionalString(input, "granularity") ?? comparerWorkspace.granularity) as ComparerWorkspaceState["granularity"]; if (!["character", "line", "word"].includes(granularity)) throw new Error("Unknown comparison granularity"); return await commands.compareText(comparerLeft, comparerRight, granularity); }
      case "settings_read":
        return snapshot?.settings ?? null;
      case "settings_update":
        return await updateSettings(forgeObject(input.patch, "patch") as SettingsPatch);
      case "certificate_generate":
        return await generateCaCertificate();
      case "logs_read":
        return await commands.getLogs();
      case "logs_clear":
        await commands.clearLogs(); return { ok: true };
      default:
        throw new Error(`Forge tool '${name}' is not implemented`);
    }
  }

  async function requestReplayTab(raw: Uint8Array, tls = true) {
    try { await commands.openInRepeater(normalizeHttpLineEndingBytes(raw), tls); }
    catch (reason) { showError(reason); }
  }

  async function persistFuzzScan(sourceTabId: number, scan: IntruderScan) {
    const lifecycleToken = projectLifecycleToken;
    const projectPath = snapshot?.project.currentProjectPath;
    if (!isTauri() || !projectPath || lifecycleToken !== projectLifecycleToken) return;
    try {
      await commands.createFuzzScan(scan.session.id, sourceTabId, scan.name, scan.startedAt);
    } catch (reason) {
      if (lifecycleToken === projectLifecycleToken) showError(reason);
      throw reason;
    }
  }

  async function persistFuzzScanCompletion(scan: IntruderScan) {
    const lifecycleToken = projectLifecycleToken;
    const projectPath = snapshot?.project.currentProjectPath;
    if (!isTauri() || !projectPath || !scan.completedAt || lifecycleToken !== projectLifecycleToken) return;
    try {
      await commands.completeFuzzScan(scan.session.id, scan.completedAt);
    } catch (reason) {
      if (lifecycleToken === projectLifecycleToken) showError(reason);
      throw reason;
    }
  }

  function sendRawToFuzz(raw: Uint8Array, tls = true) {
    flushFuzzDraft();
    const next = createIntruderTab(fuzz.nextTabId++, normalizeHttpLineEndingBytes(raw), tls);
    fuzz.tabs.push(next);
    fuzz.activeTabId = next.id;
    markWorkspaceDirty();
    changeTab("Fuzz");
    return next.id;
  }

  function closeFuzzTab(id: number) {
    const index = fuzz.tabs.findIndex((candidate) => candidate.id === id);
    if (index < 0) return;
    const tab = fuzz.tabs[index];
    if (tab.kind === "result") {
      fuzz.tabs.splice(index, 1);
      if (fuzz.activeTabId === id) fuzz.activeTabId = fuzz.tabs[Math.max(0, index - 1)]?.id ?? fuzz.tabs[0]?.id ?? 0;
      markWorkspaceDirty();
      return;
    }
    if (tab.scans.some((scan) => scan.running)) {
      tab.error = "Stop running scans before closing this tab.";
      fuzz.activeTabId = id;
      return;
    }
    if (fuzz.editorDraft?.tabId === id) flushFuzzDraft();
    recordClosedTab("Fuzz", tab);
    const linkedResultIds = new Set(
      fuzz.tabs.filter((candidate) => candidate.kind === "result" && candidate.sourceTabId === id).map((candidate) => candidate.id),
    );
    const setupCount = fuzz.tabs.filter((candidate) => candidate.kind === "setup").length;
    if (setupCount === 1) {
      const replacement = createIntruderTab(fuzz.nextTabId++);
      fuzz.tabs = [replacement];
      fuzz.activeTabId = replacement.id;
    } else {
      fuzz.tabs = fuzz.tabs.filter((candidate) => candidate.id !== id && !linkedResultIds.has(candidate.id));
      if (fuzz.activeTabId === id || linkedResultIds.has(fuzz.activeTabId)) fuzz.activeTabId = fuzz.tabs[Math.max(0, index - 1)]?.id ?? fuzz.tabs[0]?.id ?? 0;
    }
    markWorkspaceDirty();
  }

  function sendToDecoder(value: string) {
    decoderInput = value;
    markWorkspaceDirty();
    changeTab("Decoder");
    statusMessage = `Sent ${value.length ? "value" : "empty value"} to Decoder`;
  }

  async function sendHistoryEntryToDecoder(entry: HistoryEntry) {
    try {
      const detail = historyDetail?.entry.id === entry.id
        ? historyDetail
        : await commands.getHistoryDetail(entry.id);
      if (!detail) return;
      sendToDecoder(selectedTextOr(new Uint8Array(detail.request)));
    } catch (reason) {
      showError(reason);
    }
  }

  function resetFuzz() {
    flushFuzzDraft();
    fuzz.tabs = [createIntruderTab(1)];
    fuzz.activeTabId = 1;
    fuzz.nextTabId = 2;
    fuzz.editorDraft = null;
    markWorkspaceDirty();
  }

  function uniqueExecutionId() {
    return typeof crypto?.randomUUID === "function"
      ? crypto.randomUUID()
      : `${Date.now()}-${Math.random().toString(16).slice(2)}`;
  }

  async function openIdentityConfig() {
    const tab = activeReplay;
    if (!tab) return;
    identityConfigDialog = true;
    identityDialogLoading = true;
    identityDialogError = "";
    try {
      const bundle: IdentityBundle = await commands.getIdentityGroups();
      identityGroups = bundle.groups.map((group) => ({
        id: group.id,
        name: group.name,
        identities: bundle.identities
          .filter((identity) => identity.groupId === group.id)
          .map(({ id, groupId, name, color }) => ({ id, groupId, name, color })),
      }));
      const savedGroup = identityGroups.find((group) => group.id === tab.identityConfig?.groupId);
      if (savedGroup) {
        identityDialogGroupId = savedGroup.id;
        identityDialogIds = tab.identityConfig!.identityIds.filter((id) => savedGroup.identities.some((identity) => identity.id === id));
      } else {
        identityDialogGroupId = null;
        identityDialogIds = [];
      }
    } catch (reason) {
      identityDialogError = String(reason);
      showError(reason);
    } finally {
      identityDialogLoading = false;
    }
  }

  function chooseIdentityGroup(group: IdentityGroupChoice) {
    identityDialogGroupId = group.id;
    identityDialogIds = group.identities.map((identity) => identity.id);
  }

  function toggleIdentitySelection(identityId: string, selected: boolean) {
    identityDialogIds = selected
      ? [...new Set([...identityDialogIds, identityId])]
      : identityDialogIds.filter((id) => id !== identityId);
  }

  function toggleAllIdentities() {
    if (!identityDialogGroup) return;
    identityDialogIds = identityDialogAllSelected ? [] : identityDialogGroup.identities.map((identity) => identity.id);
  }

  function saveIdentityConfig() {
    const tab = activeReplay;
    const group = identityDialogGroup;
    const identityIds = identityDialogIds.filter((id) => group?.identities.some((identity) => identity.id === id));
    if (!tab || !group || !identityIds.length) return;
    tab.identityConfig = { groupId: group.id, groupName: group.name, identityIds };
    tab.response = new Uint8Array();
    tab.identityResponses = {};
    tab.activeIdentityResponseId = null;
    tab.pendingRequestIds = [];
    identityConfigDialog = false;
    markWorkspaceDirty();
    statusMessage = `ID+ configured for ${group.name} · ${identityIds.length} ${identityIds.length === 1 ? "identity" : "identities"}`;
  }

  async function runReplayInternal(target: ReplayTab | undefined = activeReplay) {
    const tab = target;
    const operationToken = projectLifecycleToken;
    flushReplayDraft();
    if (!tab?.request.length) return { ok: false, error: "Replay tab has no request", tabId: tab?.id ?? null };
    if (tab.sending) return { ok: false, error: "Replay tab is already sending", tabId: tab.id };
    if (projectTransitioning) return { ok: false, error: "Replay request cancelled because a project transition is in progress", tabId: tab.id };
    const request = finalizeHttpRequestBytes(tab.request);
    tab.request = request;
    if (!tab.identityConfig) {
      tab.sending = true;
      tab.history.push(request.slice());
      tab.historyIndex = tab.history.length - 1;
      markWorkspaceDirty();
      try {
        const response = await commands.sendRepeaterRequest(String(tab.id), request, tab.tls);
        if (operationToken !== projectLifecycleToken) {
          return { ok: false, tabId: tab.id, error: "Replay request superseded by a project transition" };
        }
        tab.response = new Uint8Array(response.raw);
        return { ok: true, tabId: tab.id, status: response.status, durationMs: response.durationMs, size: response.size };
      } catch (reason) {
        showError(reason);
        return { ok: false, tabId: tab.id, error: String(reason) };
      } finally {
        tab.sending = false;
        markWorkspaceDirty();
      }
    }

    const tls = tab.tls;
    const config = { ...tab.identityConfig, identityIds: [...tab.identityConfig.identityIds] };
    let bundle: IdentityBundle;
    try {
      bundle = await commands.getIdentityGroups();
    } catch (reason) {
      showError(reason);
      return { ok: false, tabId: tab.id, error: String(reason) };
    }
    if (operationToken !== projectLifecycleToken) {
      return { ok: false, tabId: tab.id, error: "Replay request superseded by a project transition" };
    }
    const group = bundle.groups.find((candidate) => candidate.id === config.groupId);
    const identities = bundle.identities.filter((identity) =>
      identity.groupId === config.groupId && config.identityIds.includes(identity.id),
    );
    if (!group || !identities.length) {
      const message = "No configured identities are available. Refresh the ID+ selection before sending.";
      showError(message);
      statusMessage = message;
      return { ok: false, tabId: tab.id, error: message };
    }

    tab.identityConfig = { groupId: group.id, groupName: group.name, identityIds: identities.map((identity) => identity.id) };
    tab.sending = true;
    tab.history.push(request.slice());
    tab.historyIndex = tab.history.length - 1;
    markWorkspaceDirty();
    tab.response = new Uint8Array();
    const runs = identities.map((identity) => {
      const executionId = uniqueExecutionId();
      const response: IdentityResponse = {
        executionId,
        identityId: identity.id,
        name: identity.name,
        color: identity.color,
        raw: new Uint8Array(),
        status: null,
        durationMs: null,
        size: null,
        error: null,
        sending: true,
      };
      return { identity, executionId, response };
    });
    tab.identityResponses = Object.fromEntries(runs.map(({ executionId, response }) => [executionId, response]));
    tab.activeIdentityResponseId = runs[0]?.executionId ?? null;
    tab.pendingRequestIds = runs.map(({ executionId }) => executionId);

    await Promise.allSettled(runs.map(async ({ identity, executionId, response }) => {
      try {
        const injection = await commands.resolveIdentityInjection(identity.id);
        const result = await commands.sendRepeaterRequest(executionId, request, tls, injection);
        if (operationToken !== projectLifecycleToken) return;
        tab.identityResponses[executionId] = {
          ...response,
          raw: new Uint8Array(result.raw),
          status: result.status,
          durationMs: result.durationMs,
          size: result.size,
          sending: false,
        };
      } catch (reason) {
        if (operationToken === projectLifecycleToken) {
          tab.identityResponses[executionId] = { ...response, error: String(reason), sending: false };
        }
      }
    }));
    tab.pendingRequestIds = [];
    tab.sending = false;
    markWorkspaceDirty();
    const responses = Object.values(tab.identityResponses);
    return {
      ok: true,
      tabId: tab.id,
      identityCount: responses.length,
      completed: responses.filter((response) => !response.error && !response.sending).length,
      errors: responses.filter((response) => response.error).map((response) => ({ name: response.name, error: response.error })),
    };
  }

  async function runReplay(target: ReplayTab | undefined = activeReplay) {
    const tab = target;
    if (!tab) return runReplayInternal(target);
    const operation = runReplayInternal(tab);
    replayOperations.set(tab.id, operation);
    try {
      return await operation;
    } finally {
      if (replayOperations.get(tab.id) === operation) replayOperations.delete(tab.id);
      if (replayCloseRequests.delete(tab.id) && !tab.sending) removeReplayTab(tab.id);
    }
  }

  async function cancelReplay(target: ReplayTab | undefined = activeReplay) {
    const tab = target;
    if (!tab?.sending) return { ok: false, error: "Replay tab is not sending", tabId: tab?.id ?? null };
    try {
      if (tab.identityConfig) {
        await Promise.allSettled(tab.pendingRequestIds.map((id) => commands.cancelRepeaterRequest(id)));
      } else {
        await commands.cancelRepeaterRequest(String(tab.id));
      }
      return { ok: true, tabId: tab.id };
    } catch (reason) { showError(reason); return { ok: false, tabId: tab.id, error: String(reason) }; }
  }

  function duplicateReplayTab() {
    if (!activeReplay) return;
    flushReplayDraft();
    sendRawToReplay(activeReplay.request, activeReplay.tls);
  }

  function removeReplayTab(id: number) {
    const tab = replayTabs.find((candidate) => candidate.id === id);
    if (!tab) return;
    if (activeReplayId === id) flushReplayDraft();
    recordClosedTab("Replay", tab);
    if (replayTabs.length === 1) {
      const replacement = createReplayTab(nextReplayId++);
      replayTabs.splice(0, 1, replacement);
      activeReplayId = replacement.id;
      markWorkspaceDirty();
      return;
    }
    const index = replayTabs.findIndex((tab) => tab.id === id);
    replayTabs.splice(index, 1);
    if (activeReplayId === id) activeReplayId = replayTabs[Math.max(0, index - 1)].id;
    markWorkspaceDirty();
  }

  function closeReplayTab(id: number) {
    const tab = replayTabs.find((candidate) => candidate.id === id);
    if (!tab) return;
    if (tab.sending) {
      replayCloseRequests.add(id);
      void cancelReplay(tab);
      return;
    }
    removeReplayTab(id);
  }

  function navigateReplayHistory(direction: number) {
    const tab = activeReplay;
    flushReplayDraft();
    if (!tab?.history.length) return;
    tab.historyIndex = Math.max(0, Math.min(tab.history.length - 1, tab.historyIndex + direction));
    tab.request = tab.history[tab.historyIndex].slice();
    markWorkspaceDirty();
  }

  function selectedReplayResponse(tab: ReplayTab) {
    return tab.identityConfig
      ? tab.activeIdentityResponseId ? tab.identityResponses[tab.activeIdentityResponseId]?.raw ?? new Uint8Array() : new Uint8Array()
      : tab.response;
  }

  function sendResponseToComparer(response?: Uint8Array) {
    const value = response ?? (activeReplay ? selectedReplayResponse(activeReplay) : new Uint8Array());
    if (!value.length) return;
    addToComparer(new TextDecoder().decode(value));
    changeTab("Comparer");
  }

  function addToComparer(value: string) {
    if (!comparerLeft) comparerLeft = value;
    else if (!comparerRight) comparerRight = value;
    else { comparerLeft = comparerRight; comparerRight = value; }
    comparerWorkspace = { ...comparerWorkspace, left: comparerLeft, right: comparerRight };
    markWorkspaceDirty();
  }

  async function compareHistory(entry: HistoryEntry, kind: "request" | "response") {
    try {
      const detail = await commands.getHistoryDetail(entry.id);
      if (detail) addToComparer(new TextDecoder().decode(new Uint8Array(detail[kind])));
      contextMenu = null;
      changeTab("Comparer");
    } catch (reason) { showError(reason); }
  }

  async function copyAsCurl(entry: HistoryEntry) {
    try {
      const detail = await commands.getHistoryDetail(entry.id);
      if (!detail) return;
      const raw = new TextDecoder().decode(new Uint8Array(detail.request));
      const [head, ...bodyParts] = raw.split("\r\n\r\n");
      const lines = head.split(/\r?\n/);
      const method = lines.shift()?.split(" ")[0] || entry.method;
      const quote = (value: string) => `'${value.replaceAll("'", `'\\''`)}'`;
      const parts = ["curl", "-X", quote(method), quote(entry.url)];
      for (const header of lines) {
        if (header && !header.toLowerCase().startsWith("content-length:")) parts.push("-H", quote(header));
      }
      const body = bodyParts.join("\r\n\r\n");
      if (body) parts.push("--data-binary", quote(body));
      await navigator.clipboard.writeText(parts.join(" "));
      contextMenu = null;
      statusMessage = "cURL command copied";
    } catch (reason) { showError(reason); }
  }

  async function resolveIntercept(action: "forward" | "drop" | "modify") {
    const entry = selectedIntercept;
    if (!entry || !beginInterceptResolution(entry.id)) return;
    try {
      const shouldModify = action === "modify" || (action === "forward" && interceptDraftChanged?.entryId === entry.id && interceptDraftChanged.changed);
      const resolution = shouldModify ? "modify" : action;
      flushInterceptDraft();
      const finalized = resolution !== "drop" && entry.kind === "request"
        ? finalizeHttpRequestBytes(entry.raw)
        : entry.raw;
      if (resolution !== "drop" && entry.kind === "request") entry.raw = finalized;
      await commands.resolveInterception(
        entry.id,
        resolution,
        resolution === "modify" ? finalized : undefined,
      );
      // A false backend result means another path already resolved the entry; either way it is no longer pending.
      removeIntercept(entry.id);
    } catch (reason) {
      showError(reason);
    } finally {
      endInterceptResolution(entry.id);
    }
  }

  async function resolveAllIntercepts(action: "forward" | "drop") {
    if (bulkResolving) return;
    const entries = [...pendingIntercepts];
    if (!entries.length) return;
    if (action === "drop" && !(await askForConfirmation({
      title: "Drop pending interceptions?",
      message: `Drop all ${entries.length} pending interception${entries.length === 1 ? "" : "s"}?`,
      confirmLabel: "Drop all",
      danger: true,
    }))) return;
    bulkResolving = true;
    const changedEntryId = interceptDraftChanged?.changed ? interceptDraftChanged.entryId : null;
    flushInterceptDraft();
    try {
      for (const entry of entries) {
        const current = pendingIntercepts.find((candidate) => candidate.id === entry.id);
        if (!current || !beginInterceptResolution(current.id)) continue;
        try {
          const raw = action === "forward" && current.kind === "request"
            ? finalizeHttpRequestBytes(current.raw)
            : current.raw;
          if (action === "forward" && current.kind === "request") current.raw = raw;
          const resolution = action === "forward" && current.kind === "request" && current.id === changedEntryId
            ? "modify"
            : action;
          await commands.resolveInterception(
            current.id,
            resolution,
            resolution === "modify" ? raw : undefined,
          );
          // A false backend result means another path already resolved the entry; either way it is no longer pending.
          removeIntercept(current.id);
        } catch (reason) {
          showError(reason);
        } finally {
          endInterceptResolution(current.id);
        }
      }
    } finally {
      bulkResolving = false;
    }
  }

  async function ensureLauncherWindowChrome() {
    if (!isTauri()) return;
    // Boot already uses the launcher geometry from tauri.conf.json (720x560,
    // centered). Touching size/position here makes the window visibly shrink
    // on Windows when the splash is replaced, so only enforce chrome.
    window.sessionStorage.removeItem("witness.window-defaults-applied");
    const appWindow = getCurrentWindow();
    await appWindow.setResizable(false);
    await appWindow.setMinSize(new LogicalSize(620, 480));
  }

  async function setProjectWindowMode(projectOpen: boolean) {
    if (!isTauri()) return;
    const sizingKey = "witness.window-defaults-applied";
    const appWindow = getCurrentWindow();
    await appWindow.setResizable(projectOpen);
    if (projectOpen) {
      if (window.sessionStorage.getItem(sizingKey)) return;
      window.sessionStorage.setItem(sizingKey, "true");
    } else {
      window.sessionStorage.removeItem(sizingKey);
    }
    const size = projectOpen ? new LogicalSize(1280, 820) : new LogicalSize(720, 560);
    const minimum = projectOpen ? new LogicalSize(1024, 720) : new LogicalSize(620, 480);
    if (!projectOpen) {
      await appWindow.setFullscreen(false);
      await appWindow.unmaximize();
    }
    await appWindow.setMinSize(minimum);
    await appWindow.setSize(size);
    if (projectOpen) {
      await appWindow.center();
      return;
    }

    await new Promise<void>((resolve) => window.requestAnimationFrame(() => resolve()));
    const [monitor, outerSize] = await Promise.all([currentMonitor(), appWindow.outerSize()]);
    if (!monitor) {
      await appWindow.center();
      return;
    }
    await appWindow.setPosition(new PhysicalPosition(
      Math.round(monitor.position.x + (monitor.size.width - outerSize.width) / 2),
      Math.round(monitor.position.y + (monitor.size.height - outerSize.height) / 2),
    ));
  }

  async function refreshRecentProjects() {
    if (!isTauri()) return;
    recentProjects = await commands.getRecentProjects();
  }

  async function deleteRecentProject(path: string) {
    const project = recentProjects.find((candidate) => candidate.path === path);
    if (!project) return;
    busy = true;
    try {
      await commands.deleteProject(path);
      await refreshRecentProjects();
      statusMessage = `Deleted project: ${project.name}`;
    } catch (reason) {
      showError(reason);
    } finally {
      busy = false;
    }
  }

  function resetClosedProjectState() {
    workspaceReady = false;
    resetTransientProjectState();
    history = [];
    historyDetail = null;
    historyStale = true;
    historyCacheKey = "";
    resetWorkspaceState();
  }

  async function finishProjectOpen(path: string, temporary = false) {
    endProjectTransition();
    await refreshSnapshot();
    workspaceReady = false;
    resetTransientProjectState();
    history = [];
    historyDetail = null;
    historyStale = true;
    historyCacheKey = "";
    resetWorkspaceState();
    let workspaceInvalid = false;
    const workspace = await commands.getWorkspace();
    if (workspace) {
      try { restoreWorkspaceSnapshot(workspace); }
      catch (reason) { workspaceInvalid = true; statusMessage = "Project opened with a fresh workspace; saved workspace data was invalid"; showError(reason); }
    }
    workspaceReady = true;
    statusMessage = temporary ? "Temporary session ready" : `Project open: ${path}`;
    await refreshRecentProjects();
    await setProjectWindowMode(true);
    if (activeTab === "History" || activeTab === "Site Map") void loadHistory(true);
    if (!workspaceInvalid) void persistWorkspace().catch(showError);
  }

  async function finishProjectClose() {
    endProjectTransition();
    const value = await refreshSnapshot();
    resetClosedProjectState();
    await refreshRecentProjects();
    await setProjectWindowMode(false);
    return value;
  }

  async function recoverProjectTransition() {
    if (projectTransitioning) endProjectTransition();
    else snapshotRequestToken += 1;
    try {
      const value = await refreshSnapshot();
      if (!value.project.currentProjectPath) {
        resetClosedProjectState();
        await refreshRecentProjects();
        await setProjectWindowMode(false);
      }
    } catch (reason) {
      snapshot = null;
      resetClosedProjectState();
      statusMessage = "Project state could not be refreshed after the failed transition";
      await setProjectWindowMode(false).catch(() => {});
      showError(reason);
    }
  }

  async function prepareProjectTransition() {
    beginProjectTransition();
    await stopRunningFuzzScans();
    await stopRunningReplayRequests();
    if (snapshot?.proxy.running) await stopProxyAndClearPending();
    await persistWorkspace();
  }

  async function ensureProjectTransitionPrepared() {
    if (!projectTransitioning) await prepareProjectTransition();
  }

  async function createProject(name: string, path: string, rethrow = false) {
    busy = true;
    try {
      await prepareProjectTransition();
      await commands.createProject(name, path);
      await finishProjectOpen(path);
    } catch (reason) {
      await recoverProjectTransition();
      showError(reason);
      if (rethrow) throw reason;
    } finally {
      busy = false;
    }
  }

  async function pickProjectPath() {
    try {
      return await commands.pickProjectSavePath();
    } catch (reason) {
      showError(reason);
      return null;
    }
  }

  async function openProject(path?: string, rethrow = false) {
    const selected = path ?? await commands.pickProjectFile().catch((reason) => {
      showError(reason);
      return null;
    });
    if (!selected) {
      if (rethrow) throw new Error("No project was selected");
      return;
    }
    busy = true;
    try {
      await prepareProjectTransition();
      await commands.openProject(selected);
      await finishProjectOpen(selected);
    } catch (reason) {
      await recoverProjectTransition();
      showError(reason);
      await refreshRecentProjects();
      if (rethrow) throw reason;
    } finally {
      busy = false;
    }
  }

  async function createTemporaryProject(rethrow = false) {
    busy = true;
    try {
      await prepareProjectTransition();
      await commands.createTemporaryProject();
      await finishProjectOpen("", true);
    } catch (reason) {
      await recoverProjectTransition();
      showError(reason);
      if (rethrow) throw reason;
    } finally {
      busy = false;
    }
  }

  async function handleQuickTour() {
    if (busy) return;
    busy = true;
    try {
      await prepareProjectTransition();
      await commands.createTemporaryProject();
      await finishProjectOpen("", true);
      // No delay — show first card immediately after temp session ready
      await new Promise<void>((r) => requestAnimationFrame(() => r()));
      startTutorial(undefined, async () => {
        try {
          await closeProject();
        } catch (e) {
          showError(e);
        }
      });
    } catch (reason) {
      await recoverProjectTransition();
      showError(reason);
    } finally {
      busy = false;
    }
  }

  async function browseTemporarySavePath() {
    const selected = await pickProjectPath();
    if (selected) temporaryProjectPath = selected;
  }

  function openTemporarySaveDialog(closeAfterSave = false) {
    if (!snapshot?.project.currentProjectPath || !snapshot.project.temporary || busy) return;
    temporaryProjectName = "Witness Project";
    temporaryProjectPath = "";
    closeAfterTemporarySave = closeAfterSave || closeWindowAfterTemporaryAction;
    temporarySaveDialog = true;
  }

  async function saveTemporaryProject() {
    if (!temporaryProjectName.trim() || !temporaryProjectPath.trim()) return;
    busy = true;
    const shouldCloseAfterSave = closeAfterTemporarySave;
    const shouldDestroyWindow = closeWindowAfterTemporaryAction;
    let projectClosed = false;
    try {
      await ensureProjectTransitionPrepared();
      await commands.saveTemporaryProject(temporaryProjectName.trim(), temporaryProjectPath.trim());
      await refreshRecentProjects();
      if (shouldCloseAfterSave) {
        await commands.closeProject();
        await finishProjectClose();
        projectClosed = true;
        temporarySaveDialog = false;
        closeAfterTemporarySave = false;
        closeWindowAfterTemporaryAction = false;
        if (shouldDestroyWindow) await getCurrentWindow().destroy();
      } else {
        endProjectTransition();
        const value = await refreshSnapshot();
        temporarySaveDialog = false;
        closeAfterTemporarySave = false;
        closeWindowAfterTemporaryAction = false;
        statusMessage = `Temporary session saved to ${value.project.archivePath ?? temporaryProjectPath.trim()}`;
        resolveUpdateSaveWaiter(true);
      }
    } catch (reason) {
      await recoverProjectTransition();
      showError(reason);
      if (closeWindowAfterTemporaryAction) closeWindowAfterTemporaryAction = false;
      resolveUpdateSaveWaiter(false);
    } finally {
      if (projectClosed) closeAfterTemporarySave = false;
      busy = false;
    }
  }

  async function closeNativeProject(destroyWindow = false) {
    await ensureProjectTransitionPrepared();
    try {
      await commands.closeProject();
      await finishProjectClose();
      if (destroyWindow) await getCurrentWindow().destroy();
      return true;
    } catch (reason) {
      await recoverProjectTransition();
      throw reason;
    }
  }

  async function closeProject(destroyWindow = false) {
    if (busy) return false;
    busy = true;
    try {
      return await closeNativeProject(destroyWindow);
    } catch (reason) {
      showError(reason);
      return false;
    } finally {
      busy = false;
    }
  }

  async function stopRunningReplayRequests() {
    const active = [...replayOperations.keys()];
    replayOperations.clear();
    const activeTabs = replayTabs.filter((tab) => tab.sending);
    await Promise.all(activeTabs.map(async (tab) => {
      await cancelReplay(tab);
    }));
    await Promise.all(active.map(async (tabId) => {
      const operation = replayOperations.get(tabId);
      if (operation) {
        await operation.catch(() => {});
      }
    }));
  }

  async function stopRunningFuzzScans() {
    const scans = fuzz.tabs
      .filter((item): item is IntruderTab => item.kind === "setup")
      .flatMap((item) => item.scans)
      .filter((item) => item.running);
    for (const scan of scans) {
      scan.stopRequested = true;
      scan.stopped = true;
      if (scan.currentRequestId) {
        await commands.cancelRepeaterRequest(scan.currentRequestId).catch(() => {});
      }
    }
    const deadline = Date.now() + 30_000;
    while (fuzz.tabs
      .filter((item): item is IntruderTab => item.kind === "setup")
      .flatMap((item) => item.scans)
      .some((item) => item.running)) {
      if (Date.now() >= deadline) throw new Error("Timed out while stopping Fuzz scans");
      await new Promise<void>((resolve) => window.setTimeout(resolve, 25));
    }
    const persistenceFailures = fuzz.tabs
      .filter((item): item is IntruderTab => item.kind === "setup")
      .flatMap((item) => item.scans)
      .filter((scan) => scan.persistenceError);
    const failed = persistenceFailures[0];
    if (failed) throw new Error(`Fuzz scan could not be persisted: ${failed.persistenceError}`);
  }

  function requestProjectClose() {
    if (snapshot?.project.temporary) {
      closeTemporaryDialog = true;
      return;
    }
    void closeProject();
  }

  function cancelTemporaryClose() {
    closeTemporaryDialog = false;
    const pendingWindowClose = closeWindowAfterTemporaryAction;
    closeWindowAfterTemporaryAction = false;
    if (pendingWindowClose && projectTransitioning) endProjectTransition();
  }

  function cancelTemporarySaveDialog() {
    temporarySaveDialog = false;
    closeAfterTemporarySave = false;
    const pendingWindowClose = closeWindowAfterTemporaryAction;
    closeWindowAfterTemporaryAction = false;
    if (pendingWindowClose && projectTransitioning) endProjectTransition();
    resolveUpdateSaveWaiter(false);
  }

  async function confirmTemporaryClose() {
    closeTemporaryDialog = false;
    const shouldDestroyWindow = closeWindowAfterTemporaryAction;
    const closed = await closeProject();
    if (!closed) {
      closeWindowAfterTemporaryAction = false;
      return;
    }
    if (shouldDestroyWindow) {
      closeWindowAfterTemporaryAction = false;
      await getCurrentWindow().destroy();
    }
  }

  // ---- Auto-update: unsaved-work guard (§4.1) + background check (§3.1) ----

  function askUpdateGuardChoice(version: string | null): Promise<"save" | "nosave" | "cancel"> {
    updateGuardVersion = version;
    updateGuardDialog = true;
    return new Promise((resolve) => {
      updateGuardChoice = resolve;
    });
  }

  function settleUpdateGuardChoice(choice: "save" | "nosave" | "cancel") {
    updateGuardDialog = false;
    const resolve = updateGuardChoice;
    updateGuardChoice = null;
    resolve?.(choice);
  }

  function waitForTemporarySaveForUpdate(): Promise<boolean> {
    return new Promise((resolve) => {
      updateSaveWaiter = resolve;
      openTemporarySaveDialog(false);
    });
  }

  function resolveUpdateSaveWaiter(saved: boolean) {
    const resolve = updateSaveWaiter;
    updateSaveWaiter = null;
    resolve?.(saved);
  }

  /** §4.1 guard. Returns true when it is safe to download/install. */
  async function runUpdateGuard(version: string | null): Promise<boolean> {
    if (!snapshot?.project.currentProjectPath) return true;
    if (snapshot.project.temporary) {
      const choice = await askUpdateGuardChoice(version);
      if (choice === "cancel") return false;
      if (choice === "nosave") return true;
      return await waitForTemporarySaveForUpdate();
    }
    try {
      await persistWorkspace();
      await commands.saveProject();
      await refreshSnapshot();
      return true;
    } catch (reason) {
      showErrorToast("Could not save project — update cancelled. You can keep using this version.");
      showError(reason);
      return false;
    }
  }

  async function discardBackgroundUpdate() {
    const previous = pendingBgUpdate;
    pendingBgUpdate = null;
    updateAvailableVersion = null;
    updateAvailableDialog = false;
    bgUpdateProgress = null;
    await closeUpdate(previous);
  }

  async function runBackgroundUpdateInstall(update: Update): Promise<boolean> {
    if (bgUpdateBusy) return false;
    bgUpdateBusy = true;
    bgUpdateProgress = 0;
    statusMessage = `Downloading update v${update.version}… 0%`;
    try {
      await installUpdateAndRelaunch(update, (percent) => {
        if (percent !== null) bgUpdateProgress = percent;
        statusMessage =
          percent === 100
            ? "Installing update… the app will restart."
            : `Downloading update v${update.version}… ${percent ?? 0}%`;
      });
      statusMessage = "Restarting into the new version…";
      return true;
    } catch (reason) {
      showErrorToast(reason);
      statusMessage = "Update failed — you can keep using this version.";
      bgUpdateBusy = false;
      bgUpdateProgress = null;
      return false;
    }
  }

  async function startUpdateFromDialog() {
    const update = pendingBgUpdate;
    if (!update || bgUpdateBusy) return;
    updateAvailableDialog = false;
    const allowed = await runUpdateGuard(update.version);
    if (!allowed) {
      statusMessage = "Update cancelled — staying on this version.";
      return;
    }
    await runBackgroundUpdateInstall(update);
  }

  async function backgroundUpdateCheck() {
    if (!isTauri() || bgUpdateBusy) return;
    const mode = getUpdateMode();
    if (mode === "manual") return;
    if (pendingBgUpdate || updateAvailableDialog || updateGuardDialog) return;
    let update: Update | null = null;
    try {
      update = await checkForUpdate();
    } catch {
      return; // Background failures stay silent; manual check shows errors.
    }
    if (!update) return;
    if (mode === "auto-check") {
      pendingBgUpdate = update;
      updateAvailableVersion = update.version;
      updateAvailableDialog = true;
      showInfoToast(`Version ${update.version} is available.`);
      return;
    }
    // auto-update: proceed without an Update Now click (guard still applies).
    pendingBgUpdate = update;
    showInfoToast(`Update v${update.version} found — preparing…`);
    const allowed = await runUpdateGuard(update.version);
    if (!allowed) {
      await discardBackgroundUpdate();
      statusMessage = "Update cancelled — staying on this version.";
      return;
    }
    await runBackgroundUpdateInstall(update);
  }

  function scheduleBackgroundUpdateCheck() {
    window.setTimeout(() => {
      void backgroundUpdateCheck();
    }, 5_000);
  }

  function openProjectExport(closeAfterSave = false) {
    if (!snapshot?.project.currentProjectPath) return;
    const currentArchive = snapshot.project.archivePath;
    exportPath = currentArchive ?? `${snapshot.project.currentProjectPath}.wns`;
    closeAfterProjectSave = closeAfterSave;
    exportDialog = true;
  }

  async function saveProjectShortcut() {
    if (!snapshot?.project.currentProjectPath) return false;
    if (snapshot.project.temporary) {
      openTemporarySaveDialog();
      return true;
    }
    if (!snapshot.project.archivePath) {
      showError(new Error("Persistent project has no archive destination"));
      return false;
    }
    try {
      await persistWorkspace();
      await commands.saveProject();
      await refreshSnapshot();
      statusMessage = "Project saved";
      return true;
    } catch (reason) {
      showError(reason);
      return false;
    }
  }

  async function browseProjectSavePath() {
    try {
      const selected = await commands.pickProjectSavePath();
      if (selected) exportPath = selected;
    } catch (reason) {
      showError(reason);
    }
  }

  async function runProjectExport() {
    if (!exportPath.trim() || exportBusy) return;
    exportBusy = true;
    const shouldCloseAfterSave = closeAfterProjectSave;
    try {
      await persistWorkspace();
      await commands.saveProject(exportPath.trim());
      const value = await refreshSnapshot();
      exportDialog = false;
      closeAfterProjectSave = false;
      statusMessage = `Project saved to ${value.project.archivePath ?? exportPath.trim()}`;
      if (shouldCloseAfterSave) await closeProject();
    } catch (reason) {
      if (!String(reason).includes("cancelled")) showError(reason);
    } finally { exportBusy = false; }
  }

  function openImportDialog() {
    if (activeTab === "Proxy") return;
    importTarget = activeTab === "Replay" ? "Replay" : "Fuzz";
    curlImportCommand = "";
    curlImportError = "";
    importDialog = "options";
  }

  function inferRequestTls(raw: Uint8Array) {
    const requestLine = new TextDecoder().decode(raw.slice(0, 2_048)).split(/\r?\n/, 1)[0] ?? "";
    return !requestLine.split(/\s+/, 3)[1]?.startsWith("http://");
  }

  function importRawRequest(raw: Uint8Array, tls = inferRequestTls(raw)) {
    if (importTarget === "Replay") {
      sendRawToReplay(raw, tls);
      statusMessage = "Request imported into a new Replay tab";
    } else if (importTarget === "Fuzz") {
      sendRawToFuzz(raw, tls);
      statusMessage = "Request imported into a new Fuzz tab";
    }
    importDialog = null;
  }

  async function importRequest(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;
    try {
      importRawRequest(new Uint8Array(await file.arrayBuffer()));
    } catch (reason) {
      showError(reason);
    } finally {
      input.value = "";
    }
  }

  function importCurl() {
    try {
      const request = curlToHttpRequest(curlImportCommand);
      importRawRequest(request.raw, request.tls);
    } catch (reason) {
      curlImportError = String(reason);
    }
  }

  async function toggleProxy() {
    if (!snapshot || busy) return;
    busy = true;
    try {
      if (snapshot.proxy.running) {
        await stopProxyAndClearPending();
      } else {
        await commands.startProxy();
      }
    } catch (reason) {
      showError(reason);
    } finally {
      busy = false;
    }
  }

  function showError(reason: unknown) {
    showErrorToast(reason);
    statusMessage = "Action failed";
  }

  function askForConfirmation(request: Omit<ConfirmationRequest, "resolve">) {
    return new Promise<boolean>((resolve) => {
      confirmationRequest = { ...request, resolve };
    });
  }

  function settleConfirmation(accepted: boolean) {
    const request = confirmationRequest;
    confirmationRequest = null;
    request?.resolve(accepted);
  }

  async function setInterceptInScopeOnly(enabled: boolean) {
    if (!snapshot) return;
    const previous = snapshot.settings.interceptInScopeOnly;
    snapshot.settings.interceptInScopeOnly = enabled;
    try {
      snapshot.settings = await commands.updateSettings({ interceptInScopeOnly: enabled });
      clearPendingIntercepts();
    } catch (reason) { snapshot.settings.interceptInScopeOnly = previous; showError(reason); }
  }

  async function updateSettings(patch: SettingsPatch) {
    if ("theme" in patch && (patch as { theme?: string }).theme === "light") {
      showInfoToast("Light mode is under development!");
      throw new Error("Light mode is under development!");
    }
    if (!snapshot) throw new Error("Settings are still loading");
    const normalizedPatch: SettingsPatch = "shortcutModifier" in patch
      ? { ...patch, shortcutModifier: normalizeShortcutModifier(patch.shortcutModifier, shortcutPlatform) }
      : patch;
    const previous = snapshot.settings;
    const settings = { ...previous, ...normalizedPatch };
    const networkChanged = previous.proxyPort !== settings.proxyPort
      || previous.proxyBindAddress !== settings.proxyBindAddress
      || previous.certificateDirectory !== settings.certificateDirectory;
    const restartProxy = snapshot.proxy.running && networkChanged;
    const interceptionSettingsChanged = [
      "proxyIntercepting",
      "proxyInterceptMode",
      "interceptContentTypes",
      "requestInterceptionRules",
      "responseInterceptionRules",
      "interceptInScopeOnly",
    ].some((key) => key in normalizedPatch);
    if (restartProxy && !(await askForConfirmation({
      title: "Restart the running proxy?",
      message: "Apply this change and restart the running proxy?",
      confirmLabel: "Apply and restart",
      danger: false,
    }))) {
      throw new Error("Settings change cancelled");
    }
    try {
      if (restartProxy) await stopProxyAndClearPending();
      snapshot.settings = await commands.updateSettings(normalizedPatch);
      if (interceptionSettingsChanged && !restartProxy) clearPendingIntercepts();
      if (!snapshot.settings.showLogsTab && activeTab === "Logs") activeTab = "Proxy";
      snapshot.proxy.port = snapshot.settings.proxyPort;
      snapshot.proxy.bindAddress = snapshot.settings.proxyBindAddress;
      snapshot.proxy.intercepting = snapshot.settings.proxyIntercepting && snapshot.settings.proxyInterceptMode !== "none";
      if (restartProxy) await commands.startProxy();
      statusMessage = "Setting saved";
      return snapshot.settings;
    } catch (reason) { showError(reason); throw reason; }
  }

  async function toggleTheme() {
    showInfoToast("Light mode is under development!");
    previewTheme = "dark";
    if (snapshot) snapshot.settings.theme = "dark";
    return;
  }

  async function generateCaCertificate() {
    const certificate = await commands.generateCaCertificate();
    if (snapshot) snapshot.proxy.certificateStatus = "present; install witness-ca.pem in your browser";
    statusMessage = certificate.generated ? "CA certificate generated" : "CA certificate is ready";
    return certificate;
  }

  async function setProxyInterception(enabled: boolean) {
    if (!snapshot?.proxy.running) return;
    try {
      await updateSettings(
        enabled && snapshot?.settings.proxyInterceptMode === "none"
          ? { proxyIntercepting: true, proxyInterceptMode: "allRequests" }
          : { proxyIntercepting: enabled },
      );
      if (!enabled) {
        clearPendingIntercepts();
        statusMessage = "Interception disabled — pending messages forwarded";
      }
    } catch {
      // updateSettings has already surfaced the failure and retained the persisted value.
    }
  }

  function resizeHistory(event: PointerEvent) {
    if (!snapshot || !historyWorkspace) return;
    const workspace = historyWorkspace;
    event.preventDefault();
    const move = (pointer: PointerEvent) => {
      const bounds = workspace.getBoundingClientRect();
      snapshot!.settings.layoutSplitPercent = Math.round(Math.max(20, Math.min(75, ((pointer.clientY - bounds.top) / bounds.height) * 100)));
    };
    const stop = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", stop);
      if (snapshot) void commands.updateSettings({ layoutSplitPercent: snapshot.settings.layoutSplitPercent }).catch(showError);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", stop, { once: true });
  }

  function resizeHistoryByKeyboard(event: KeyboardEvent) {
    if (!snapshot || !["ArrowUp", "ArrowDown"].includes(event.key)) return;
    event.preventDefault();
    const change = event.key === "ArrowUp" ? -2 : 2;
    snapshot.settings.layoutSplitPercent = Math.max(20, Math.min(75, snapshot.settings.layoutSplitPercent + change));
    void commands.updateSettings({ layoutSplitPercent: snapshot.settings.layoutSplitPercent }).catch(showError);
  }

  function minimizeHistoryInspectors() {
    historyInspectorsVisible = false;
    markWorkspaceDirty();
  }
</script>

<svelte:head>
  <title>{windowTitle}</title>
  <meta
    name="description"
    content="Witness is a fast, focused manual web security testing suite."
  />
</svelte:head>

<ErrorToast />
<InfoToast />

{#if !snapshot?.project.currentProjectPath}
  <ProjectLauncher
    {recentProjects}
    ready={Boolean(snapshot)}
    {busy}
    onOpen={(path) => void openProject(path)}
    onCreate={(name, path) => void createProject(name, path)}
    onTemporary={() => void createTemporaryProject()}
    onBrowse={pickProjectPath}
    onConfigureAi={() => { activeTab = "Settings"; settingsSection = "ai"; }}
    onTour={() => void handleQuickTour()}
    onDelete={(path) => void deleteRecentProject(path)}
  />
  {#if activeTab === "Settings" && snapshot}
    <div
      class="launcher-settings-panel"
      style={`--interface-font-size:${snapshot.settings.fontSize ?? 14}px;--message-editor-font-size:${snapshot.settings.messageEditorFontSize ?? 12}px`}
    >
      <header class="launcher-ai-header">
        <button class="launcher-ai-back" type="button" aria-label="Back to launcher" onclick={() => (activeTab = "Proxy")}>
          <svg viewBox="0 0 32 24" width="24" height="18" fill="none" stroke="#fff" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M3 12h26M3 12l8-8M3 12l8 8" />
          </svg>
        </button>
        <h1>Configure AI Inference</h1>
      </header>
      <div class="launcher-ai-settings">
        <AiSettings settings={snapshot.settings} onUpdate={updateSettings} />
      </div>
    </div>
  {/if}
{:else}
<div
  class="app-shell"
  data-theme={currentTheme}
  style={`--interface-font-size:${snapshot?.settings.fontSize ?? 14}px;--message-editor-font-size:${snapshot?.settings.messageEditorFontSize ?? 12}px`}
>
  <header class="titlebar">
    <div class="brand" aria-label="Witness">
      <span class="brand-mark" aria-hidden="true"><img src={currentTheme === "light" ? "/witness_app_icon.png" : "/witness_app_icon.png"} alt="" /></span>
    </div>
    <nav class="toolbar" aria-label="Primary tools" data-tour="toolbar">
      {#each tabs as tab}
        <button
          data-tour={`tab-${tab}`}
          class:active={activeTab === tab}
          aria-current={activeTab === tab ? "page" : undefined}
          aria-label={tab === "Proxy" && activeTab !== "Proxy" && pendingIntercepts.length ? `Proxy — ${pendingIntercepts.length} intercepted ${pendingIntercepts.length === 1 ? "message" : "messages"} waiting` : tabLabel(tab)}
          onclick={() => changeTab(tab)}
        >
          {tabLabel(tab)}
          {#if tab === "Proxy" && activeTab !== "Proxy" && pendingIntercepts.length}
            <span class="proxy-pending-badge" aria-hidden="true">{pendingIntercepts.length > 99 ? "99+" : pendingIntercepts.length}</span>
          {/if}
        </button>
      {/each}
    </nav>
    <span class="toolbar-spacer"></span>
    <div class:online={snapshot?.proxy.running} class="proxy-pill">
      <span class="status-dot" aria-hidden="true"></span>
      {snapshot?.proxy.running ? "Proxy on" : "Proxy off"}
    </div>
    {#if activeTab === "Proxy" || activeTab === "Replay" || activeTab === "Fuzz"}
      <span
        class="import-request-control"
        data-tooltip={activeTab === "Proxy" ? "Import unavailable in Proxy" : "Import request"}
      >
        <button
          class="icon-button import-request"
          aria-label={activeTab === "Proxy" ? "Import request unavailable in Proxy" : "Import request"}
          disabled={activeTab === "Proxy"}
          onclick={openImportDialog}
        >
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 3v11"/><path d="m8 10 4 4 4-4"/><path d="M5 14v6h14v-6"/></svg>
        </button>
      </span>
    {/if}
    <span
      class="project-save-control"
      data-tooltip={snapshot?.project.currentProjectPath && snapshot.project.temporary === false ? "Autosave enabled" : "Save project"}
    >
      <button
        class="icon-button project-save"
        data-tour="project-save"
        aria-label={snapshot?.project.currentProjectPath && snapshot.project.temporary === false ? "Save unavailable; autosave enabled" : "Save project"}
        disabled={busy || !snapshot?.project.currentProjectPath || snapshot?.project.temporary === false}
        onclick={() => openTemporarySaveDialog()}
      >
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 3h12l2 2v16H5Z"/><path d="M8 3v6h8V3"/><path d="M8 21v-7h8v7"/></svg>
      </button>
    </span>
    <button
      class="icon-button theme-toggle"
      aria-label={`Switch to ${currentTheme === "light" ? "dark" : "light"} mode`}
      data-tooltip={`Switch to ${currentTheme === "light" ? "dark" : "light"} mode`}
      onclick={() => void toggleTheme()}
    >
      {#if currentTheme === "light"}
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M20.2 15.6A8.7 8.7 0 0 1 8.4 3.8 8.7 8.7 0 1 0 20.2 15.6Z"/></svg>
      {:else}
        <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="3.5"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/></svg>
      {/if}
    </button>
    <button class="icon-button" aria-label="Open settings" data-tooltip="Settings" onclick={() => openSettings("display")}>
      <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.9l.1.1-2.8 2.8-.1-.1a1.7 1.7 0 0 0-1.9-.3 1.7 1.7 0 0 0-1 1.6v.2h-4V21a1.7 1.7 0 0 0-1-1.6 1.7 1.7 0 0 0-1.9.3l-.1.1L4.2 17l.1-.1a1.7 1.7 0 0 0 .3-1.9A1.7 1.7 0 0 0 3 14H2.8v-4H3a1.7 1.7 0 0 0 1.6-1 1.7 1.7 0 0 0-.3-1.9L4.2 7 7 4.2l.1.1A1.7 1.7 0 0 0 9 4.6a1.7 1.7 0 0 0 1-1.6v-.2h4V3a1.7 1.7 0 0 0 1 1.6 1.7 1.7 0 0 0 1.9-.3l.1-.1L19.8 7l-.1.1a1.7 1.7 0 0 0-.3 1.9 1.7 1.7 0 0 0 1.6 1h.2v4H21a1.7 1.7 0 0 0-1.6 1Z"/></svg>
    </button>
    <button class="icon-button shutdown-button" aria-label="Close project" data-tooltip="Close project" data-tooltip-align="end" onclick={requestProjectClose}>
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 2v10"/><path d="M6.3 5.7a8 8 0 1 0 11.4 0"/></svg>
    </button>
  </header>

  <main>
    {#if snapshot && workspaceReady}
      <div
        class="forge-panel-host"
        data-tour="forge"
        hidden={activeTab !== "AI"}
        aria-hidden={activeTab !== "AI"}
      >
        <AiController
          bind:this={forgeController}
          settings={snapshot.settings}
          context={activeTab === "AI" ? buildAiContext() : ""}
          workspace={forgeWorkspace}
          trustTools={forgeTrustTools}
          onTrustToolsChange={(value) => (forgeTrustTools = value)}
          onExecuteTool={executeForgeTool}
          onWorkspaceChange={(workspace) => { forgeWorkspace = workspace; markWorkspaceDirty(); }}
        />
      </div>
    {/if}
    {#if activeTab === "Proxy"}
      <section class="proxy-workspace" aria-labelledby="proxy-title" data-tour="proxy-workspace">
        <div class="proxy-controls" data-tour="proxy-controls">
          <div class="proxy-control-title">
            <span class:online={snapshot?.proxy.running} class="status-dot" aria-hidden="true"></span>
            <div>
              <h1 id="proxy-title">Intercept</h1>
              <span>{snapshot ? `${snapshot.settings.proxyBindAddress}:${snapshot.settings.proxyPort}` : "Loading proxy settings…"}</span>
            </div>
          </div>
          <div class="proxy-control-actions">
            <label class:disabled={!snapshot || !snapshot.proxy.running} class="intercept-switch">
              <span>Intercept <span class="intercept-state">{snapshot?.proxy.running && snapshot.settings.proxyIntercepting ? "on" : "off"}</span></span>
              <Toggle
                checked={snapshot?.proxy.running && snapshot.settings.proxyIntercepting}
                disabled={!snapshot || !snapshot.proxy.running}
                ariaLabel="Intercept"
                onchange={(event) => void setProxyInterception(event.currentTarget.checked)}
              />
            </label>
            <label class:disabled={!snapshot} class="intercept-switch scope-intercept-switch" data-tooltip="Pause only traffic that matches an in-scope rule">
              <span>In-scope only <span class="intercept-state">{snapshot?.settings.interceptInScopeOnly ? "on" : "off"}</span></span>
              <Toggle
                checked={snapshot?.settings.interceptInScopeOnly ?? false}
                disabled={!snapshot}
                ariaLabel="Intercept only in-scope traffic"
                onchange={(event) => void setInterceptInScopeOnly(event.currentTarget.checked)}
              />
            </label>
            <button
              class:danger={snapshot?.proxy.running}
              class="text-button primary-action"
              disabled={!snapshot || busy}
              onclick={toggleProxy}
            >
              {#if busy}<span class="spinner" aria-hidden="true"></span>{/if}{busy ? "Working…" : snapshot?.proxy.running ? "Stop proxy" : "Start proxy"}
            </button>
            <button class="text-button proxy-settings-button" type="button" onclick={() => openSettings("proxy")}>Proxy settings</button>
          </div>
        </div>

        {#if !snapshot}
          <div class="proxy-state">
            <svg viewBox="0 0 64 64" aria-hidden="true">
              <circle cx="32" cy="32" r="23"></circle>
              <path d="M32 20v13M32 43h.01"></path>
            </svg>
            <h2>Loading proxy</h2>
            <p>Connecting to the desktop bridge…</p>
          </div>
        {:else if !snapshot.proxy.running}
          <div class="proxy-state state-off">
            <svg viewBox="0 0 64 64" aria-hidden="true">
              <path d="M21 18h22a7 7 0 0 1 7 7v14a7 7 0 0 1-7 7H21a7 7 0 0 1-7-7V25a7 7 0 0 1 7-7Z"></path>
              <path d="M24 27l16 10M20 13l24 38"></path>
              <circle cx="23" cy="27" r="2"></circle>
              <circle cx="41" cy="37" r="2"></circle>
            </svg>
            <h2>Proxy is off</h2>
            <p>Start the proxy to begin capturing traffic.</p>
          </div>
        {:else if !snapshot.settings.proxyIntercepting}
          <div class="proxy-state state-off">
            <svg viewBox="0 0 64 64" aria-hidden="true">
              <path d="M32 10l19 8v12c0 12-7.7 20.7-19 25-11.3-4.3-19-13-19-25V18l19-8Z"></path>
              <path d="M24 25v14M40 25v14"></path>
              <path d="M18 14l28 36"></path>
            </svg>
            <h2>Interception is off</h2>
            <p>Turn on interception to pause requests and responses.</p>
          </div>
        {:else if !pendingIntercepts.length}
          <div class="proxy-state state-waiting">
            <svg viewBox="0 0 64 64" aria-hidden="true">
              <circle cx="32" cy="32" r="5"></circle>
              <path d="M22.5 22.5a13.5 13.5 0 0 0 0 19M41.5 22.5a13.5 13.5 0 0 1 0 19"></path>
              <path d="M15.5 15.5a23.5 23.5 0 0 0 0 33M48.5 15.5a23.5 23.5 0 0 1 0 33"></path>
            </svg>
            <h2>Waiting for a request</h2>
            <p>Intercepted traffic will appear here automatically.</p>
          </div>
        {:else}
          <div class="proxy-interception-ui">
            <div class="proxy-queue" data-tour="proxy-intercept-table">
              <InterceptTable
                entries={pendingIntercepts}
                selectedId={selectedInterceptId}
                onSelect={(entry) => selectIntercept(entry.id)}
              />
            </div>

            <div class="intercept-heading" data-tour="proxy-intercept-actions">
              <span><strong>Paused {selectedIntercept?.kind ?? "message"}</strong> · {pendingIntercepts.length} waiting</span>
              <div class="intercept-actions">
                <div class="intercept-action-group intercept-action-group--drop">
                  <button
                    class="intercept-all intercept-all--drop"
                    type="button"
                    disabled={!pendingIntercepts.length || bulkResolving}
                    aria-label={pendingIntercepts.length ? `Drop all ${pendingIntercepts.length} pending interception${pendingIntercepts.length === 1 ? "" : "s"}` : "No pending requests to drop"}
                    data-tooltip={pendingIntercepts.length ? `Drop all ${pendingIntercepts.length} pending (Ctrl+Shift+D)` : "No pending requests"}
                    onclick={() => resolveAllIntercepts("drop")}
                  ><span class="intercept-all-label" aria-hidden="true">ALL</span></button>
                  <button
                    class="text-button danger intercept-drop intercept-main"
                    type="button"
                    disabled={!selectedIntercept || bulkResolving}
                    aria-label="Drop selected interception"
                    onclick={() => resolveIntercept("drop")}
                  >Drop</button>
                </div>
                <div class="intercept-action-group intercept-action-group--forward">
                  <button
                    class="intercept-all intercept-all--forward"
                    type="button"
                    disabled={!pendingIntercepts.length || bulkResolving}
                    aria-label={pendingIntercepts.length ? `Forward all ${pendingIntercepts.length} pending interception${pendingIntercepts.length === 1 ? "" : "s"}` : "No pending requests to forward"}
                    data-tooltip={pendingIntercepts.length ? `Forward all ${pendingIntercepts.length} pending (Ctrl+Shift+F)` : "No pending requests"}
                    onclick={() => resolveAllIntercepts("forward")}
                  ><span class="intercept-all-label" aria-hidden="true">ALL</span></button>
                  <button
                    class="text-button forward-action intercept-main"
                    type="button"
                    disabled={!selectedIntercept || bulkResolving}
                    aria-label="Forward selected interception"
                    onclick={() => resolveIntercept("forward")}
                  >
                    Forward
                  </button>
                </div>
              </div>
            </div>

            {#if selectedIntercept}
              <div class:split={selectedIntercept.kind === "response"} class="intercept-preview" data-tour="proxy-message-viewer">
                {#if selectedIntercept.kind === "response"}
                  <MessageViewer
                    title="Request"
                    kind="request"
                    raw={selectedIntercept.requestRaw ?? new Uint8Array()}
                    metadata={selectedIntercept.host}
                    onSendReplay={(raw) => void requestReplayTab(raw, selectedIntercept?.url.startsWith("https://") ?? true)}
                    onSendFuzz={(raw) => sendRawToFuzz(raw, selectedIntercept?.url.startsWith("https://") ?? true)}
                    onSendDecoder={sendToDecoder}
                    onSaveOrganizer={(raw) => void saveToOrganizer(raw, selectedIntercept?.raw ?? new Uint8Array(), selectedIntercept?.url.startsWith("https://") ?? true, "Proxy")}
                  />
                {/if}
                  <MessageViewer
                    title={selectedIntercept.kind === "request" ? "Request" : "Response"}
                    kind={selectedIntercept.kind === "request" ? "request" : "response"}
                    raw={selectedIntercept.raw}
                    metadata={selectedInterceptMetadata}
                    editable
                    normalizeRequest={selectedIntercept.kind === "request"}
                    onTextChange={updateInterceptDraft}
                    onDirtyChange={updateInterceptDraftState}
                    onSendReplay={selectedIntercept.kind === "request" ? (raw) => void requestReplayTab(raw, selectedIntercept?.url.startsWith("https://") ?? true) : undefined}
                    onSendFuzz={selectedIntercept.kind === "request" ? (raw) => sendRawToFuzz(raw, selectedIntercept?.url.startsWith("https://") ?? true) : undefined}
                    onSendDecoder={sendToDecoder}
                    onSaveOrganizer={selectedIntercept.kind === "request" ? (raw) => void saveToOrganizer(raw, new Uint8Array(), selectedIntercept?.url.startsWith("https://") ?? true, "Proxy") : undefined}
                    onProxyAction={(action) => void resolveIntercept(action)}
                  />
              </div>
            {:else}
              <div class="intercept-preview-empty">
                <span aria-hidden="true">↗↙</span>
                <strong>No message selected</strong>
                <small>Select a pending message from the table.</small>
              </div>
            {/if}
          </div>
        {/if}
      </section>
    {:else if activeTab === "History"}
      <section
        class="history-workspace"
        bind:this={historyWorkspace}
        data-tour="history-workspace"
        aria-label="HTTP history workspace"
        style={`grid-template-rows:${historyInspectorsVisible ? `${snapshot?.settings.layoutSplitPercent ?? 46}% 5px minmax(180px, 1fr)` : "1fr"}`}
      >
        {#if !snapshot?.project.currentProjectPath}
          <div class="no-project">
            <div class="empty-icon" aria-hidden="true">▤</div>
            <h1>No project is open</h1>
            <p>Close this workspace to return to the project launcher.</p>
          </div>
        {:else}
          <div class="history-panel" data-tour="history-filter">
            <FilterBar filter={historyFilter} onChange={() => { markWorkspaceDirty(); scheduleHistoryLoad(true); }} onClearHistory={clearAllHistory} />
            <div data-tour="history-table">
              <HistoryTable
                entries={history}
                selectedId={historyDetail?.entry.id}
                loading={historyLoading}
                onSelect={selectHistory}
                onSort={sortHistory}
                onNeedMore={() => { if (historyHasMore && !historyLoading) void loadHistory(false); }}
                onContext={(event, entry) => { tabContextMenu = null; tabGroupContextMenu = null; contextMenu = { x: event.clientX, y: event.clientY, entry }; }}
              />
            </div>
          </div>
          {#if historyInspectorsVisible}<button class="pane-divider" aria-label="Resize history and inspector panes" data-tooltip="Drag to resize" onpointerdown={resizeHistory} onkeydown={resizeHistoryByKeyboard}></button>{/if}
          {#if historyInspectorsVisible}<div class="inspectors" data-tour="history-inspectors">
            {#if historyDetail}
              <div data-tour="history-request-viewer">
                <MessageViewer
                  title="Request"
                  kind="request"
                  raw={new Uint8Array(historyDetail.request)}
                  metadata={historyDetail.entry.host}
                  onSendReplay={(raw) => void requestReplayTab(raw, historyDetail?.entry.url.startsWith("https://") ?? true)}
                  onSendFuzz={(raw) => sendRawToFuzz(raw, historyDetail?.entry.url.startsWith("https://") ?? true)}
                  onSendDecoder={sendToDecoder}
                  onSaveOrganizer={(raw) => void saveToOrganizer(raw, new Uint8Array(historyDetail?.response ?? []), historyDetail?.entry.url.startsWith("https://") ?? true, "History")}
                  search={historyFilter.search ?? ""}
                  onMinimize={minimizeHistoryInspectors}
                />
              </div>
              <div data-tour="history-response-viewer">
                <MessageViewer
                  title="Response"
                  kind="response"
                  raw={new Uint8Array(historyDetail.response)}
                  metadata={historyDetail.entry.host}
                  search={historyFilter.search ?? ""}
                  onSendDecoder={sendToDecoder}
                  onMinimize={minimizeHistoryInspectors}
                />
              </div>
            {:else}
              <div class="select-prompt">Select a history row to inspect its request and response.</div>
            {/if}
          </div>{/if}
        {/if}
      </section>
    {:else if activeTab === "Site Map"}
      <div data-tour="site-map">
        <SiteMap
          bind:this={siteMapController}
          entries={history}
          workspace={siteMapWorkspace}
          onWorkspaceChange={(state) => { siteMapWorkspace = state; markWorkspaceDirty(); }}
          onSelect={(entry) => { siteMapWorkspace = { ...siteMapWorkspace, selectedEntryId: entry.id }; void selectHistory(entry); changeTab("History"); }}
          onSendReplay={sendEntryToReplay}
          onDelete={(entry) => void deleteEntry(entry)}
        />
      </div>
    {:else if activeTab === "Replay"}
      <section class="repeater-workspace" aria-label="Replay workspace" data-tour="replay-workspace">
        <div class="repeater-tabs" role="tablist" aria-label="Replay tabs" data-tour="replay-tabs">
          <div class="repeater-tab-viewport">
            <div class="repeater-tab-scroll" bind:this={replayTabScrollElement} onscroll={updateReplayTabScrollState}>
              <div class="repeater-tab-strip">
            {#each replayTabBarEntries as entry (entry.kind === "tab" ? "tab-" + entry.tab.id : "group-" + entry.group.id)}
            {#if entry.kind === "group"}
              <div class="repeater-tab-group" class:collapsed={entry.group.collapsed}>
                <button
                  class="tab-group-marker"
                  style={"--tab-group-color:" + entry.group.color}
                  aria-label={(entry.group.collapsed ? "Expand " : "Collapse ") + entry.group.name + " tab group"}
                  aria-expanded={!entry.group.collapsed}
                  onclick={() => toggleTabGroup(entry.group.id)}
                  oncontextmenu={(event) => openTabGroupContextMenu(event, "Replay", entry.group.id)}
                >
                  <span class="tab-group-color" aria-hidden="true"></span>
                  <span>{entry.group.name}</span>
                  <small>{entry.tabs.length}</small>
                  <span class="tab-group-chevron" aria-hidden="true">{entry.group.collapsed ? "▸" : "▾"}</span>
                </button>
                {#if !entry.group.collapsed}
                  <div class="repeater-group-tabs" style={`--tab-group-color:${entry.group.color}`}>
                    {#each entry.tabs as tab (tab.id)}
                      <div role="presentation" class:active={tab.id === activeReplayId} class="repeater-tab" onclick={() => switchReplayTab(tab.id)} oncontextmenu={(event) => openTabContextMenu(event, "Replay", tab.id)}>
                        <button role="tab" aria-selected={tab.id === activeReplayId} onclick={(event) => { event.stopPropagation(); switchReplayTab(tab.id); }}>{tab.title}</button>
                        <button
                          class="tab-close-button"
                          class:inactive={tab.id !== activeReplayId}
                          aria-hidden={tab.id !== activeReplayId}
                          tabindex={tab.id === activeReplayId ? 0 : -1}
                          aria-label={"Close " + tab.title}
                  data-tooltip={"Close " + tab.title}
                          onclick={(event) => { event.stopPropagation(); closeReplayTab(tab.id); }}
                        >×</button>
                      </div>
                    {/each}
                  </div>
                {/if}
              </div>
            {:else}
              {@const tab = entry.tab}
              <div role="presentation" class:active={tab.id === activeReplayId} class="repeater-tab" onclick={() => switchReplayTab(tab.id)} oncontextmenu={(event) => openTabContextMenu(event, "Replay", tab.id)}>
                <button role="tab" aria-selected={tab.id === activeReplayId} onclick={(event) => { event.stopPropagation(); switchReplayTab(tab.id); }}>{tab.title}</button>
                <button
                  class="tab-close-button"
                  class:inactive={tab.id !== activeReplayId}
                  aria-hidden={tab.id !== activeReplayId}
                  tabindex={tab.id === activeReplayId ? 0 : -1}
                  aria-label={"Close " + tab.title}
                  data-tooltip={"Close " + tab.title}
                  onclick={(event) => { event.stopPropagation(); closeReplayTab(tab.id); }}
                >×</button>
              </div>
            {/if}
          {/each}
          </div>
          </div>
          {#if replayTabsCanScrollRight}
            <button class="tab-scroll-indicator" type="button" aria-label="Scroll to more Replay tabs" data-tooltip="More tabs" onclick={scrollReplayTabsRight}>&gt;</button>
          {/if}
          </div>
          <div class="repeater-tab-actions" aria-label="Replay tab actions">
            <button class="icon-button tab-search-button" type="button" style="--svgbuttonsize:28px" data-tooltip="Search Replay tabs" aria-label="Search Replay tabs" onclick={openReplaySearch}>
              <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="10.8" cy="10.8" r="6.3"></circle><path d="m16 16 4.5 4.5"></path></svg>
            </button>
            <button class="new-tab" type="button" data-tooltip="New Replay tab" aria-label="New Replay tab" onclick={() => sendRawToReplay(new Uint8Array(), true)}>+</button>
          </div>
        </div>
        <div class="repeater-toolbar">
          <div class="repeater-toolbar-leading">
            <div class="repeater-actions repeater-navigation">
              <button
                disabled={!activeReplay?.history.length}
                aria-label="Previous request"
                data-tooltip="Previous request"
                onclick={() => navigateReplayHistory(-1)}
              >←</button>
              <button
                disabled={!activeReplay?.history.length}
                aria-label="Next request"
                data-tooltip="Next request"
                onclick={() => navigateReplayHistory(1)}
              >→</button>
              <label class="repeater-protocol">
                <span>Target</span>
                <select
                  value={activeReplay?.tls ? "https" : "http"}
                  disabled={activeReplay?.sending}
                  onchange={(event) => { if (activeReplay) { activeReplay.tls = event.currentTarget.value === "https"; markWorkspaceDirty(); } }}
                >
                  <option value="https">HTTPS</option>
                  <option value="http">HTTP</option>
                </select>
              </label>
            </div>
            <div class="repeater-toolbar-context">
              {#if activeReplay?.identityConfig}
                <strong class="identity-config-summary">ID+ configured : Group “{activeReplay.identityConfig.groupName}” • {activeReplay.identityConfig.identityIds.length} IDs</strong>
              {:else}
                <p class="eyebrow">REPLAY</p>
              {/if}
            </div>
          </div>
          <div class="repeater-actions repeater-toolbar-trailing">
            <DuplicateButton label="Duplicate Replay tab" onclick={duplicateReplayTab} />
            {#if activeReplay?.sending}<button class="text-button cancel-request" onclick={() => void cancelReplay()}>Cancel</button>{/if}
            <button class="text-button primary-action" data-tour="replay-send" disabled={!activeReplayRequestText.length || activeReplay?.sending} onclick={() => void runReplay()}>{#if activeReplay?.sending}<span class="spinner" aria-hidden="true"></span>{/if}{activeReplay?.sending ? "Sending…" : "Send"}</button>
          </div>
        </div>
        {#if activeReplay}
          <div data-tour="replay-request-editor">
            <MessageViewer
              title="Request"
              kind="request"
              raw={activeReplay.request}
            metadata={activeReplayMetadata}
            editable
            normalizeRequest
            onTextChange={updateReplayDraft}
            onConfigureIdentities={() => void openIdentityConfig()}
            onDuplicate={duplicateReplayTab}
            onSendFuzz={(raw) => sendRawToFuzz(raw, activeReplay.tls)}
            onSendDecoder={sendToDecoder}
            onSaveOrganizer={(raw) => void saveToOrganizer(raw, selectedReplayResponse(activeReplay), activeReplay.tls, "Replay")}
          />
          </div>
          {#if activeReplay.identityConfig}
            <section class="repeater-response-panel" aria-label="Identity responses">
              <div class="identity-response-view">
                {#if activeIdentityResponse?.sending}
                  <div class="identity-response-state" aria-live="polite"><strong>Sending as {activeIdentityResponse.name}…</strong><span>Waiting for this identity response.</span></div>
                {:else if activeIdentityResponse?.error}
                  <div class="identity-response-state error" role="alert"><strong>{activeIdentityResponse.name} failed</strong><span>{activeIdentityResponse.error}</span></div>
                {:else if activeIdentityResponse}
                  <MessageViewer kind="response" title={`Response · ${activeIdentityResponse.name}`} raw={activeIdentityResponse.raw} metadata={activeReplayMetadata} onCompareResponse={sendResponseToComparer} onSendDecoder={sendToDecoder} />
                {:else}
                  <div class="identity-response-state"><strong>No identity response selected</strong><span>Choose an identity below or send the request to begin.</span></div>
                {/if}
              </div>
              <div class="identity-response-tabs" role="tablist" aria-label="Identity response tabs">
                {#each Object.values(activeReplay.identityResponses) as response (response.executionId)}
                  <button
                    class:active={response.executionId === activeReplay.activeIdentityResponseId}
                    role="tab"
                    aria-selected={response.executionId === activeReplay.activeIdentityResponseId}
                    onclick={() => { activeReplay.activeIdentityResponseId = response.executionId; markWorkspaceDirty(); }}
                  >
                    <span class="identity-color" style={`background:${response.color}`} aria-hidden="true"></span>
                    <span class="identity-response-tab-copy"><strong>{response.name}</strong><small>{response.sending ? "Sending…" : response.error ? "Error" : response.status === null ? "No response" : `${response.status} · ${response.durationMs ?? 0} ms · ${response.size ?? response.raw.length} B`}</small></span>
                  </button>
                {/each}
                {#if !Object.keys(activeReplay.identityResponses).length}
                  <span class="identity-tabs-empty">Send to view identity responses.</span>
                {/if}
              </div>
            </section>
          {:else}
            {#key activeReplay.id}
              <MessageViewer kind="response" title="Response" raw={activeReplay.response} metadata={activeReplayMetadata} onCompareResponse={sendResponseToComparer} onSendDecoder={sendToDecoder} />
            {/key}
          {/if}
        {/if}
      </section>
    {:else if activeTab === "Fuzz"}
      <div data-tour="fuzz-workspace">
        <Fuzz
          bind:this={fuzzController}
          state={fuzz}
          lifecycleToken={projectLifecycleToken}
          projectTransitioning={projectTransitioning}
          theme={currentTheme === "light" ? "light" : "dark"}
          launchRequest={aiFuzzLaunch}
          onLaunchRequestHandled={(id) => { if (aiFuzzLaunch?.id === id) aiFuzzLaunch = null; }}
          onScanCreated={(sourceTabId, scan) => persistFuzzScan(sourceTabId, scan)}
          onScanUpdated={(sourceTabId, scan) => persistFuzzScanCompletion(scan)}
          onSaveOrganizer={(request, response, tls) => void saveToOrganizer(request, response, tls, "Fuzz")}
          onSendReplay={(raw, tls) => sendRawToReplay(raw, tls)}
          onSendDecoder={sendToDecoder}
          onStateChange={markWorkspaceDirty}
          {tabGroups}
          onTabContextMenu={(event, tabId) => openTabContextMenu(event, "Fuzz", tabId)}
          onTabGroupContextMenu={(event, groupId) => openTabGroupContextMenu(event, "Fuzz", groupId)}
          onToggleTabGroup={toggleTabGroup}
          onCloseTab={closeFuzzTab}
        />
      </div>
    {:else if activeTab === "Organizer"}
      <div data-tour="organizer">
        <Organizer
          bind:this={organizerController}
          revision={organizerRevision}
          workspace={organizerWorkspace}
          onWorkspaceChange={(state) => { organizerWorkspace = state; markWorkspaceDirty(); }}
          onSendReplay={(raw, tls) => sendRawToReplay(raw, tls)}
          onSendFuzz={(raw, tls) => sendRawToFuzz(raw, tls)}
          onSendDecoder={sendToDecoder}
          onStatus={(message) => (statusMessage = message)}
          onError={showError}
        />
      </div>
    {:else if activeTab === "ID+"}
      <div data-tour="identity">
        <IdentityPlus
          bind:this={identityController}
          workspace={identityWorkspace}
          onWorkspaceChange={(state) => { identityWorkspace = state; markWorkspaceDirty(); }}
          onStatus={(message) => (statusMessage = message)}
          onError={showError}
        />
      </div>
    {:else if activeTab === "Decoder"}
      <div data-tour="decoder">
        <Decoder
          bind:this={decoderController}
          bind:input={decoderInput}
          workspace={decoderWorkspace}
          onWorkspaceChange={(state) => { decoderWorkspace = state; decoderInput = state.input; markWorkspaceDirty(); }}
          onStatus={(message) => (statusMessage = message)}
        />
      </div>
    {:else if activeTab === "Comparer"}
      <div data-tour="comparer">
        <Comparer bind:this={comparerController} workspace={comparerWorkspace} onStateChange={(state) => { comparerWorkspace = state; comparerLeft = state.left; comparerRight = state.right; markWorkspaceDirty(); }} />
      </div>
    {:else if activeTab === "Scope"}
      <div data-tour="scope">
        <ScopeManager
          bind:this={scopeController}
          projectOpen={Boolean(snapshot?.project.currentProjectPath)}
          onError={showError}
        />
      </div>
    {:else if activeTab === "Settings" && snapshot}
      <div data-tour="settings-panel">
        <SettingsPanel
          settings={snapshot.settings}
          proxyRunning={snapshot.proxy.running}
          selectedSection={settingsSection}
          onSectionChange={(section) => { settingsSection = section; markWorkspaceDirty(); }}
          onUpdate={updateSettings}
          onGenerateCertificate={generateCaCertificate}
          onReplayTutorial={() => void startTutorial()}
          onBeforeInstall={(version) => runUpdateGuard(version)}
        />
      </div>
    {:else if activeTab === "Logs"}
      <div data-tour="logs"><LogViewer bind:this={logController} /></div>
    {/if}
  </main>

  <footer class="statusbar">
    <span aria-live="polite"><span class:online={snapshot?.proxy.running} class="status-dot"></span>&nbsp;&nbsp;{statusMessage}</span>
    <span>{snapshot?.proxy.connectionCount ?? 0} connections</span>
    <span>{eventCount} events</span>
    <span class="status-metric" aria-live="off" data-tooltip="Witness process memory usage">Memory {memoryUsage}</span>
    <button
      class="clock-button"
      type="button"
      aria-label={`Switch to ${use24HourClock ? "12-hour" : "24-hour"} time`}
      data-tooltip={`Switch to ${use24HourClock ? "12-hour" : "24-hour"} time`}
      onclick={() => { use24HourClock = !use24HourClock; markWorkspaceDirty(); }}
    >{clockTime}</button>
  </footer>

  {#if contextMenu}
    <ContextMenu
      x={contextMenu.x}
      y={contextMenu.y}
      items={historyContextMenuItems}
      onAction={handleHistoryContextAction}
      onClose={() => (contextMenu = null)}
      ariaLabel="History actions"
    />
  {/if}

  {#if tabContextMenu}
    <ContextMenu
      x={tabContextMenu.x}
      y={tabContextMenu.y}
      items={tabContextMenuItems}
      onAction={handleTabContextAction}
      onClose={closeTabContextMenu}
      ariaLabel="Tab actions"
    />
  {/if}

  {#if tabGroupContextMenu}
    <ContextMenu
      x={tabGroupContextMenu.x}
      y={tabGroupContextMenu.y}
      items={tabGroupContextMenuItems}
      onAction={handleTabGroupContextAction}
      onClose={closeTabGroupContextMenu}
      ariaLabel="Tab group actions"
    />
  {/if}

  <ConfirmDialog
    open={closeTemporaryDialog}
    title="Close this temporary session?"
    message="This temporary session and all captured data will be permanently deleted when it is closed. This action cannot be undone."
    secondaryLabel="Save and close"
    confirmLabel="Yes, close session"
    busy={busy}
    onSecondary={() => { closeTemporaryDialog = false; openTemporarySaveDialog(true); }}
    onConfirm={() => void confirmTemporaryClose()}
    onCancel={cancelTemporaryClose}
  />
  <ConfirmDialog
    open={updateAvailableDialog}
    title="Update available"
    message={updateAvailableVersion ? `Version ${updateAvailableVersion} is available. Update now? The app will restart after installation.` : "A new version is available. Update now?"}
    confirmLabel="Update Now"
    cancelLabel="Later"
    danger={false}
    busy={bgUpdateBusy}
    onConfirm={() => void startUpdateFromDialog()}
    onCancel={() => void discardBackgroundUpdate()}
  />
  <ConfirmDialog
    open={updateGuardDialog}
    title="Save temporary session before updating?"
    message={updateGuardVersion ? `Version ${updateGuardVersion} is ready. Save your work before the app restarts, or update without saving and lose the temporary session data.` : "An update is ready. Save your work before the app restarts?"}
    confirmLabel="Update without saving"
    secondaryLabel="Save…"
    cancelLabel="Cancel"
    danger={false}
    busy={busy}
    onConfirm={() => settleUpdateGuardChoice("nosave")}
    onSecondary={() => settleUpdateGuardChoice("save")}
    onCancel={() => settleUpdateGuardChoice("cancel")}
  />
</div>

{#if confirmationRequest}
  <ConfirmDialog
    open={true}
    title={confirmationRequest.title}
    message={confirmationRequest.message}
    confirmLabel={confirmationRequest.confirmLabel ?? "Confirm"}
    danger={confirmationRequest.danger ?? true}
    onConfirm={() => settleConfirmation(true)}
    onCancel={() => settleConfirmation(false)}
  />
{/if}

{#if tabRenameDialog}
  <div class="modal-backdrop" data-theme={currentTheme} role="presentation" onclick={(event) => { if (event.target === event.currentTarget) tabRenameDialog = null; }}>
    <div class="tab-group-dialog tab-rename-dialog" role="dialog" aria-modal="true" aria-labelledby="tab-rename-dialog-title">
      <form class="tab-group-dialog-form" onsubmit={(event) => { event.preventDefault(); saveTabRename(); }}>
        <p class="tab-dialog-eyebrow">{tabRenameDialog.workspace.toUpperCase()}</p>
        <h2 id="tab-rename-dialog-title">Rename tab</h2>
        <label class="tab-group-name-label" for="tab-rename-name">
          <span>Tab name</span>
          <input id="tab-rename-name" class="tab-group-name-input" bind:value={tabRenameValue} autocomplete="off" />
        </label>
        <div class="tab-group-dialog-footer">
          <button class="text-button" type="button" onclick={() => (tabRenameDialog = null)}>Cancel</button>
          <button class="text-button primary-action" type="submit" disabled={!tabRenameValue.trim()}>Rename</button>
        </div>
      </form>
    </div>
  </div>
{/if}

{#if tabGroupDialog}
  <div class="modal-backdrop" data-theme={currentTheme} role="presentation" onclick={(event) => { if (event.target === event.currentTarget) tabGroupDialog = null; }}>
    <div class="tab-group-dialog" role="dialog" aria-modal="true" aria-labelledby="tab-group-dialog-title">
      <form class="tab-group-dialog-form" onsubmit={(event) => { event.preventDefault(); createTabGroupFromDialog(); }}>
        <p class="tab-dialog-eyebrow">{tabGroupDialog.workspace.toUpperCase()}</p>
        <h2 id="tab-group-dialog-title">{tabGroupDialog.mode === "edit" ? "Edit tab group" : "Create tab group"}</h2>
        <label class="tab-group-name-label" for="tab-group-name">
          <span>Group name</span>
          <input id="tab-group-name" class="tab-group-name-input" bind:value={tabGroupName} placeholder="e.g. Authentication" autocomplete="off" />
        </label>

        <div class="tab-group-tabs-header">
          <span>Add tabs to group</span>
          <button class="tab-group-select-all" type="button" onclick={toggleAllTabGroupTabs}>{tabGroupAllTabsSelected ? "Deselect all" : "Select all"}</button>
        </div>
        <div class="tab-group-tab-list" aria-label="Tabs to add to group">
          {#each tabGroupDialogTabs as tab (tab.id)}
            <label class="tab-group-tab-option">
              <Toggle checked={tabGroupSelectedTabIds.includes(tab.id)} ariaLabel={tab.title} onchange={(event) => toggleTabGroupTab(tab.id, event.currentTarget.checked)} />
              <span>{tab.title}</span>
            </label>
          {:else}
            <span class="tab-group-empty">No tabs available.</span>
          {/each}
        </div>

        {#if tabGroupError}<p class="tab-group-error" role="alert">{tabGroupError}</p>{/if}
        <label class="tab-group-color-field" for="tab-group-color">
          <span>Group color</span>
          <input id="tab-group-color" type="color" bind:value={tabGroupColor} aria-label="Group color" />
        </label>
        <div class="tab-group-dialog-footer">
          <button class="text-button" type="button" onclick={() => (tabGroupDialog = null)}>Cancel</button>
          <button class="text-button primary-action" type="submit" disabled={!tabGroupName.trim()}>{tabGroupDialog.mode === "edit" ? "Save" : "Create"}</button>
        </div>
      </form>
    </div>
  </div>
{/if}

{#if exportDialog}
  <div class="modal-backdrop" data-theme={currentTheme} role="presentation" onclick={(event) => { if (event.target === event.currentTarget && !exportBusy) exportDialog = false; }}>
    <form class="project-dialog" onsubmit={(event) => { event.preventDefault(); void runProjectExport(); }}>
      <p class="eyebrow">PROJECT</p>
      <h2>Save Witness project</h2>
      <label><span>Destination .wns path</span><span class="folder-picker"><input bind:value={exportPath} disabled={exportBusy} /><button class="text-button" type="button" disabled={exportBusy} onclick={browseProjectSavePath}>Browse…</button></span></label>
      <p>The file includes captured traffic, project data, Replay and Fuzz workspaces, and saved tool state. It may contain credentials or tokens.</p>
      <div class="dialog-actions">
        {#if exportBusy}<button class="text-button" type="button" disabled>Saving…</button>{:else}<button class="text-button" type="button" onclick={() => { exportDialog = false; closeAfterProjectSave = false; }}>Cancel</button>{/if}
        <button class="text-button primary-action" type="submit" disabled={exportBusy || !exportPath.trim()}>{exportBusy ? "Saving…" : "Save .wns"}</button>
      </div>
    </form>
  </div>
{/if}

{#if importDialog}
  <div class="modal-backdrop" data-theme={currentTheme} role="presentation" onclick={(event) => { if (event.target === event.currentTarget) importDialog = null; }}>
    {#if importDialog === "options"}
      <div class="request-import-dialog" role="dialog" aria-modal="true" aria-labelledby="request-import-title">
        <div><h2 id="request-import-title">Import request</h2></div>
        <div class="request-import-options">
          <label class="request-import-option">
            <input type="file" accept=".http,.txt,application/octet-stream" onchange={importRequest} />
            Import .http request
          </label>
          <button class="text-button" type="button" onclick={() => (importDialog = "curl")}>Paste cURL command</button>
        </div>
        <footer><button class="text-button" type="button" onclick={() => (importDialog = null)}>Cancel</button></footer>
      </div>
    {:else}
      <form class="request-import-dialog request-import-curl" onsubmit={(event) => { event.preventDefault(); importCurl(); }} aria-labelledby="curl-import-title">
        <div><h2 id="curl-import-title">Paste cURL command</h2></div>
        <textarea bind:value={curlImportCommand} aria-label="cURL command" placeholder="curl https://example.com/path" spellcheck={false}></textarea>
        {#if curlImportError}<p class="import-error" role="alert">{curlImportError}</p>{/if}
        <footer>
          <button class="text-button" type="button" onclick={() => { curlImportError = ""; importDialog = "options"; }}>Back</button>
          <button class="text-button primary-action" type="submit" disabled={!curlImportCommand.trim()}>Import request</button>
        </footer>
      </form>
    {/if}
  </div>
{/if}

{#if identityConfigDialog}
  <div class="modal-backdrop" data-theme={currentTheme} role="presentation" onclick={(event) => { if (event.target === event.currentTarget && !identityDialogLoading) identityConfigDialog = false; }}>
    <div class="identity-config-dialog" role="dialog" aria-modal="true" aria-labelledby="identity-config-title" aria-busy={identityDialogLoading}>
      <header>
        <div><p class="eyebrow">ID+</p><h2 id="identity-config-title">Configure identities</h2></div>
        <button type="button" aria-label="Close identity configuration" disabled={identityDialogLoading} onclick={() => (identityConfigDialog = false)}>×</button>
      </header>
      {#if identityDialogLoading}
        <div class="identity-dialog-state">Loading identity groups…</div>
      {:else if identityDialogError}
        <div class="identity-dialog-state error" role="alert">{identityDialogError}</div>
      {:else if !identityDialogGroup}
        <div class="identity-group-choices" aria-label="Choose an identity group">
          <p>Select an identity group for this Replay tab.</p>
          {#each identityGroups as group (group.id)}
            <button type="button" onclick={() => chooseIdentityGroup(group)}>
              <strong>{group.name}</strong><span>{group.identities.length} {group.identities.length === 1 ? "identity" : "identities"}</span>
            </button>
          {:else}
            <div class="identity-dialog-state">No identity groups are available. Create one in ID+ first.</div>
          {/each}
        </div>
      {:else}
        <div class="identity-selection">
          <div class="identity-selection-heading">
            <div><span>Group</span><strong>{identityDialogGroup.name}</strong></div>
            <button class="text-button" type="button" onclick={() => { identityDialogGroupId = null; identityDialogIds = []; }}>Change group</button>
          </div>
          <div class="identity-selection-actions">
            <span>Select at least one identity.</span>
            <button class="text-button" type="button" onclick={toggleAllIdentities}>{identityDialogAllSelected ? "Untick all" : "Tick all"}</button>
          </div>
          <div class="identity-checkboxes">
            {#each identityDialogGroup.identities as identity (identity.id)}
              <label>
                <Toggle checked={identityDialogIds.includes(identity.id)} ariaLabel={identity.name} onchange={(event) => toggleIdentitySelection(identity.id, event.currentTarget.checked)} />
                <span class="identity-color" style={`background:${identity.color}`} aria-hidden="true"></span>
                <span>{identity.name}</span>
              </label>
            {:else}
              <div class="identity-dialog-state">This group has no identities.</div>
            {/each}
          </div>
        </div>
      {/if}
      <footer>
        <button class="text-button" type="button" disabled={identityDialogLoading} onclick={() => (identityConfigDialog = false)}>Cancel</button>
        <button class="text-button primary-action" type="button" disabled={identityDialogLoading || !identityDialogGroup || !identityDialogIds.length} onclick={saveIdentityConfig}>Send</button>
      </footer>
    </div>
  </div>
{/if}

{#if temporarySaveDialog}
  <div class="modal-backdrop" data-theme={currentTheme} role="presentation" onclick={(event) => { if (event.target === event.currentTarget && !busy) cancelTemporarySaveDialog(); }}>
    <form class="project-dialog" onsubmit={(event) => { event.preventDefault(); void saveTemporaryProject(); }}>
      <p class="eyebrow">TEMPORARY SESSION</p>
      <h2>{closeAfterTemporarySave ? "Save and close" : "Save project"}</h2>
      <label><span>Project name</span><input bind:value={temporaryProjectName} disabled={busy} /></label>
      <label><span>Destination .wns file</span><span class="folder-picker"><input bind:value={temporaryProjectPath} placeholder="/path/to/project.wns" disabled={busy} /><button class="text-button" type="button" disabled={busy} onclick={browseTemporarySavePath}>Browse…</button></span></label>
      <p>All captured traffic, scope entries, workspace state, and project data will be saved in one unencrypted .wns file.</p>
      <div class="dialog-actions"><button class="text-button" type="button" disabled={busy} onclick={cancelTemporarySaveDialog}>Cancel</button><button class="text-button primary-action" class:success={closeAfterTemporarySave} type="submit" disabled={busy || !temporaryProjectName.trim() || !temporaryProjectPath.trim()}>{busy ? "Saving…" : closeAfterTemporarySave ? "Save and close" : "Save .wns"}</button></div>
    </form>
  </div>
{/if}

{/if}

{#if replaySearchOpen}
  <div class="modal-backdrop" data-theme={currentTheme} role="presentation" onclick={(event) => { if (event.target === event.currentTarget) closeReplaySearch(); }}>
    <div
      bind:this={replaySearchDialogElement}
      class="replay-search-dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby="replay-search-title"
    >
      <header class="replay-search-heading">
        <div><p class="eyebrow">REPLAY</p><h2 id="replay-search-title">Search tabs</h2></div>
        <button class="icon-button" type="button" aria-label="Close Replay tab search" data-tooltip="Close Replay tab search" onclick={closeReplaySearch}>×</button>
      </header>
      <label class="replay-search-field" for="replay-search-input">
        <span>Search request and response context</span>
        <input id="replay-search-input" bind:value={replaySearchQuery} autocomplete="off" placeholder="Search across Replay tabs…" />
      </label>
      <div class="replay-search-results" aria-live="polite">
        {#if !debouncedReplaySearchQuery.trim()}
          <p class="replay-search-empty">Type to search request, response, history, and identity-response content.</p>
        {:else if replaySearchResults.length}
          {#each replaySearchResults as result (result.tab.id)}
            {@const highlightedSnippet = highlightSearchText(result.snippet, debouncedReplaySearchQuery)}
            <button class="replay-search-result" type="button" onclick={() => openReplaySearchResult(result.tab.id)}>
              <span class="replay-search-snippet">{#each highlightedSnippet as part}{#if part.match}<strong>{part.text}</strong>{:else}{part.text}{/if}{/each}</span>
              <span class="replay-search-tab-badge">{result.tab.title}</span>
            </button>
          {/each}
        {:else}
          <p class="replay-search-empty">No Replay tabs contain “{debouncedReplaySearchQuery.trim()}”.</p>
        {/if}
      </div>
    </div>
  </div>
{/if}

{#if showShortcuts}
  <div class="modal-backdrop" data-theme={currentTheme} role="presentation" onclick={(event) => { if (event.target === event.currentTarget) closeShortcuts(); }}>
    <div
      bind:this={shortcutDialogElement}
      class={`shortcut-dialog shortcut-columns-${shortcutColumnCount}`}
      role="dialog"
      aria-modal="true"
      aria-labelledby="shortcut-dialog-title"
      tabindex="-1"
    >
      <div class="shortcut-dialog-heading"><div><p class="eyebrow">KEYBOARD</p><h2 id="shortcut-dialog-title">Shortcuts</h2></div><button type="button" class="text-button compact" onclick={() => { closeShortcuts(); openSettings("keyboard"); }}>Full reference</button></div>
      <section class="shortcut-section" aria-labelledby="shortcut-global-heading">
        <h3 id="shortcut-global-heading" class="shortcut-section-title">Global shortcuts</h3>
        {#each globalShortcutDefinitions as shortcut}
          <div class="shortcut-entry"><kbd aria-label={formatShortcut(shortcut, shortcutPlatform, activeShortcutModifier)}>{#each formatShortcutParts(shortcut, shortcutPlatform, activeShortcutModifier) as part, index}{#if index > 0 && shortcutPlatform !== "macos"}<span class="shortcut-key-separator" aria-hidden="true">+</span>{/if}<span class="shortcut-key-part">{part}</span>{/each}</kbd><span class="shortcut-description">{shortcut.label}</span></div>
        {/each}
      </section>
      <section class="shortcut-section" aria-labelledby="shortcut-tab-heading">
        <h3 id="shortcut-tab-heading" class="shortcut-section-title">{tabLabel(activeTab)} shortcuts</h3>
        {#each activeTabShortcutDefinitions as shortcut}
          <div class="shortcut-entry"><kbd aria-label={formatShortcut(shortcut, shortcutPlatform, activeShortcutModifier)}>{#each formatShortcutParts(shortcut, shortcutPlatform, activeShortcutModifier) as part, index}{#if index > 0 && shortcutPlatform !== "macos"}<span class="shortcut-key-separator" aria-hidden="true">+</span>{/if}<span class="shortcut-key-part">{part}</span>{/each}</kbd><span class="shortcut-description">{shortcut.label}</span></div>
        {/each}
      </section>
    </div>
  </div>
{/if}

<style>
  :global(:root) {
    --font-size-compact: 10px;
    --font-size-body: 12px;
    --font-size-heading: 14px;
    --font-size-title: 18px;
    --font-size-editor: 12px;
  }
  :global(*) { box-sizing: border-box; }
  :global(html, body) { margin: 0; min-width: 0; min-height: 0; overflow: hidden; background: #090b12; user-select: none; -webkit-user-select: none; }
  :global(input, textarea, [contenteditable="true"], .cm-editor, .cm-scroller, .cm-content, .cm-line, .highlighted, .hex-view) { user-select: text !important; -webkit-user-select: text !important; }
  .app-shell :global(input::selection),
  .app-shell :global(textarea::selection),
  .app-shell :global([contenteditable="true"]::selection),
  .app-shell :global(.highlighted::selection),
  .app-shell :global(.hex-view::selection) { color: var(--selection-text) !important; background: var(--selection-bg) !important; }
  :global(button, input, select, textarea) { font: inherit; }
  :global(button:has(> svg):not(:has(> :not(svg)))) {
    display: inline-grid;
    place-items: center;
    width: var(--svgbuttonsize) !important;
    min-width: var(--svgbuttonsize) !important;
    height: var(--svgbuttonsize) !important;
    min-height: var(--svgbuttonsize) !important;
    padding: 0 !important;
  }
  :global(button:has(> svg):not(:has(> :not(svg))) > svg) {
    width: calc(var(--svgbuttonsize) - 6px) !important;
    height: calc(var(--svgbuttonsize) - 6px) !important;
  }
  :global(button:focus-visible, input:focus-visible, select:focus-visible, textarea:focus-visible) { outline: 2px solid var(--border-strong, #64748b); outline-offset: 2px; }
  .app-shell {
    --font-size-compact: max(8px, calc(var(--interface-font-size, 14px) - 4px));
    --font-size-body: max(10px, calc(var(--interface-font-size, 14px) - 2px));
    --font-size-heading: var(--interface-font-size, 14px);
    --font-size-title: calc(var(--interface-font-size, 14px) + 4px);
    --font-size-editor: var(--message-editor-font-size, 12px);
  }
  .launcher-settings-panel {
    --interface-font-size: 14px;
    --message-editor-font-size: 12px;
    --font-size-compact: max(8px, calc(var(--interface-font-size) - 4px));
    --font-size-body: max(10px, calc(var(--interface-font-size) - 2px));
    --font-size-heading: var(--interface-font-size);
    --font-size-title: calc(var(--interface-font-size) + 4px);
    --font-size-editor: var(--message-editor-font-size);
    --launcher-base: #232323;
    --launcher-surface: color-mix(in srgb, var(--launcher-base), white 6%);
    --border: color-mix(in srgb, var(--launcher-base), white 20%);
    --border-strong: var(--border);
    position: fixed;
    inset: 14px;
    z-index: 20;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    border: 0;
    border-radius: 0;
    color: #f4f6fb;
    background:
      linear-gradient(
        rgb(from var(--launcher-surface) r g b / 0.94),
        rgb(from var(--launcher-surface) r g b / 0.94)
      ),
      radial-gradient(
        circle at 50% -20%,
        rgb(255 255 255 / 8%),
        transparent 47%
      ),
      var(--launcher-base);
    font: var(--font-size-body)/1.45 Inter, "SF Pro Text", ui-sans-serif, system-ui, -apple-system, sans-serif;
  }
  .launcher-ai-header {
    display: flex;
    flex: 0 0 auto;
    align-items: center;
    gap: 14px;
    min-height: 58px;
    padding: 0 20px;
  }
  .launcher-ai-header h1 {
    margin: 0;
    font-size: calc(var(--font-size-heading) + 2px);
    font-weight: 600;
    letter-spacing: -0.025em;
  }
  .launcher-ai-back {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    min-height: 30px;
    padding: 0 8px 0 0;
    color: #97a1b4;
    border: 0;
    background: transparent;
    font: inherit;
    cursor: pointer;
  }
  .launcher-ai-back:hover { color: #f4f6fb; }
  .launcher-ai-back span { font-size: 18px; line-height: 1; }
  .launcher-ai-settings {
    flex: 1 1 auto;
    min-height: 0;
    padding: 0;
    overflow: auto;
    background: transparent;
    padding-top: 10px;
    overflow-y: auto;
    scrollbar-width: none;
    -ms-overflow-style: none;
  }
  .launcher-ai-settings::-webkit-scrollbar {
  display: none;
  }
  .launcher-ai-settings :global(.ai-settings .ai-card),
  .launcher-ai-settings :global(.ai-settings div),
  .launcher-ai-settings :global(.ai-settings input:not([type="checkbox"])) {
    background: transparent !important;
  }
  .launcher-ai-settings :global(.ai-settings .section-kicker),
  .launcher-ai-settings :global(.ai-settings .card-heading p:not(.section-kicker)),
  .launcher-ai-settings :global(.ai-settings .ai-toggle-copy small),
  .launcher-ai-settings :global(.ai-settings label),
  .launcher-ai-settings :global(.ai-settings label em),
  .launcher-ai-settings :global(.ai-settings label small),
  .launcher-ai-settings :global(.ai-settings .connection-state),
  .launcher-ai-settings :global(.ai-settings .saved-key-heading),
  .launcher-ai-settings :global(.ai-settings .saved-key-card small),
  .launcher-ai-settings :global(.ai-settings .replace-note),
  .launcher-ai-settings :global(.ai-settings .key-state),
  .launcher-ai-settings :global(.ai-settings .delete-key-confirm),
  .launcher-ai-settings :global(.ai-settings .ai-footnote) {
    font-size: var(--font-size-compact) !important;
  }
  .launcher-ai-settings :global(.ai-settings .card-heading h3),
  .launcher-ai-settings :global(.ai-settings .ai-toggle-copy strong),
  .launcher-ai-settings :global(.ai-settings input),
  .launcher-ai-settings :global(.ai-settings .saved-key-card code) {
    font-size: var(--font-size-body) !important;
  }
  /*
   * Theme palette
   *
   * These are the semantic colors to customize. The background, surface,
   * hover, and soft status colors below are derived from this small palette.
   */
  .app-shell,
  .modal-backdrop {
    --base: #2b2b2b;
    --art: rgb(255, 132, 0);
    --editor: #2b2b2b;
    --accent: #c4c4c4;
    --border: #4d525e;
    --border: #424141ff;
    --text: #f4f6fb;
    --muted: #cacbccff;
    --success: #279630;
    --danger: #d92d4b;
    --warning: #f7b955;
    --selection: #2f7a45;
    --selection-text: #f4f6fb;
    --accent-contrast: #171a23;
    --syntax-method: #7dd3fc;
    --syntax-target: #c4b5fd;
    --syntax-protocol: #94a3b8;
    --syntax-status-success: #86efac;
    --syntax-status-redirect: #fcd34d;
    --syntax-status-client-error: #fb923c;
    --syntax-status-server-error: #fda4af;
    --syntax-status-reason: #cbd5e1;
    --syntax-header-name: #f9a8d4;
    --syntax-header-important: #fdba74;
    --syntax-header-delimiter: #94a3b8;
    --syntax-header-value: #a7f3d0;
    --syntax-json-key: #93c5fd;
    --syntax-json-value: #a3f705;
    --syntax-json-string: #a7f3d0;
    --syntax-json-number: #fdba74;
    --syntax-json-boolean: #c4b5fd;
    --syntax-json-null: #94a3b8;
    --syntax-json-punctuation: #cbd5e1;
    --svgbuttonsize: 24px;
    --svgbuttonstrokewidth: 1.2;

    /* Derived tokens — keep these in sync with the semantic palette above. */
    --bg: var(--base);
    --surface: color-mix(in srgb, var(--base) 96%, #000);
    --surface-2: var(--surface);
    --surface-3: color-mix(in srgb, var(--base) 90%, #fff);
    --nestedtabsbg: var(--surface-3);
    --tab-group-bg-mix: 24%;
    --border-strong: color-mix(in srgb, var(--border) 78%, var(--base));
    --accent-hover: color-mix(in srgb, var(--accent) 82%, #fff);
    --accent-soft: color-mix(in srgb, var(--accent) 18%, var(--base));
    --selection-bg: color-mix(in srgb, var(--selection) 72%, var(--base));
    --input: var(--base);
    --success-soft: color-mix(in srgb, var(--success) 22%, var(--base));
    --danger-soft: color-mix(in srgb, var(--danger) 16%, var(--base));
    --shadow: 0 18px 50px rgb(0 0 0 / 28%);
    --shadow-soft: 0 8px 24px rgb(0 0 0 / 16%);
    --radius: 10px;
    color-scheme: dark;
  }
  .app-shell[data-theme="light"],
  .modal-backdrop[data-theme="light"] {
    --base: #f5f6fa;
    --editor: #ffffff;
    --accent: #4b5563;
    --border: #dfe3ec;
    --text: #171a23;
    --muted: #687386;
    --success: #168a63;
    --danger: #d92d4b;
    --warning: #b66b09;
    --selection: #a7e8ae;
    --selection-text: #122318;
    --accent-contrast: #fff;
    --syntax-method: #0369a1;
    --syntax-target: #6d28d9;
    --syntax-protocol: #64748b;
    --syntax-status-success: #15803d;
    --syntax-status-redirect: #a16207;
    --syntax-status-client-error: #c2410c;
    --syntax-status-server-error: #be123c;
    --syntax-status-reason: #475569;
    --syntax-header-name: #be185d;
    --syntax-header-important: #c2410c;
    --syntax-header-delimiter: #64748b;
    --syntax-header-value: #047857;
    --syntax-json-key: #1d4ed8;
    --syntax-json-value: #a3f705;
    --syntax-json-string: #047857;
    --syntax-json-number: #c2410c;
    --syntax-json-boolean: #6d28d9;
    --syntax-json-null: #64748b;
    --syntax-json-punctuation: #475569;

    --bg: var(--base);
    --surface: color-mix(in srgb, var(--base) 94%, #fff);
    --surface-2: color-mix(in srgb, var(--base) 97%, #000);
    --surface-3: color-mix(in srgb, var(--base) 92%, #000);
    --nestedtabsbg: var(--surface-3);
    --tab-group-bg-mix: 12%;
    --border-strong: color-mix(in srgb, var(--border) 78%, var(--base));
    --accent-hover: color-mix(in srgb, var(--accent) 82%, #000);
    --accent-soft: color-mix(in srgb, var(--accent) 14%, var(--base));
    --selection-bg: var(--selection);
    --input: var(--surface);
    --success-soft: color-mix(in srgb, var(--success) 12%, var(--base));
    --danger-soft: color-mix(in srgb, var(--danger) 12%, var(--base));
    --shadow: 0 18px 50px rgb(32 38 54 / 14%);
    --shadow-soft: 0 8px 24px rgb(32 38 54 / 9%);
    color-scheme: light;
  }
  .app-shell {
    position: fixed; inset: 0; height: 100dvh; display: flex; flex-direction: column; overflow: hidden;
    color: var(--text); background: var(--bg); font: var(--font-size-body)/1.45 Inter, "SF Pro Text", ui-sans-serif, system-ui, -apple-system, sans-serif;
    -webkit-font-smoothing: antialiased;
  }
  .titlebar, .toolbar, .statusbar { display: flex; align-items: center; border-color: var(--border); }
  .titlebar { flex: 0 0 50px; padding: 0 14px; background: #0e1115; border-bottom: 1px solid var(--border); }
  .brand { display: flex; align-items: center; gap: 9px; font-weight: 700; letter-spacing: .02em; }
  .brand-mark { display: grid; place-items: center; width: 30px; height: 24px; overflow: hidden; }
  .brand-mark img { display: block; width: 30px; height: 30px; object-fit: contain; }
  .import-request:disabled { opacity: .45; cursor: not-allowed; }
  .icon-button { border: 0; background: transparent; color: var(--muted); cursor: pointer; }
  .toolbar { min-width: 0; padding: 0; gap: 4px; overflow: hidden; }
  .toolbar button { height: 31px; padding: 0 13px; border: 0; border-radius: 5px; color: var(--muted); background: transparent; cursor: pointer; }
  .toolbar button:hover { color: var(--text); background: var(--surface-2); }
  .toolbar button.active { color: var(--text); background: #242932; }
  .toolbar-spacer { flex: 1; }
  .proxy-pill { display: flex; align-items: center; gap: 7px; color: var(--muted); font-size: var(--font-size-body); }
  .proxy-pill.online { color: #a7f3d0; }
  .status-dot { display: inline-block; width: 7px; height: 7px; margin-right: 7px; border-radius: 50%; background: #647083; }
  .status-dot.online, .online .status-dot { background: #34d399; box-shadow: 0 0 0 3px #123a2f; }
  main { flex: 1 1 0; width: 100%; min-height: 0; overflow: auto; }
  .eyebrow { margin: 0 0 7px; color: var(--accent); font-size: var(--font-size-body); font-weight: 800; letter-spacing: .15em; }
  h1 { margin: 0; font-size: var(--font-size-title); line-height: 1.2; letter-spacing: -.02em; }
  h2 { margin: 13px 0 4px; font-size: var(--font-size-heading); }
  button.primary-action { padding: 9px 17px; border: 1px solid #d8860b; border-radius: 6px; color: #1b1308; background: var(--accent); font-weight: 700; cursor: pointer; }
  button.primary-action.danger { color: #fecaca; background: #3d171b; border-color: #7f1d1d; }
  button.primary-action:disabled { opacity: .45; cursor: wait; }
  .spinner { display: inline-block; width: 12px; height: 12px; margin-right: 7px; border: 2px solid #1b130855; border-top-color: #1b1308; border-radius: 50%; vertical-align: -2px; animation: spin .7s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  input { min-width: 0; padding: 8px 10px; border: 1px solid #343b46; border-radius: 5px; color: var(--text); background: #0c0f13; }
  input:disabled { opacity: .65; }
  .intercept-switch { position: relative; display: inline-flex; align-items: center; gap: 4px; color: var(--text); font-weight: 700; line-height: 1; cursor: pointer; }
  .intercept-switch.disabled { opacity: .55; cursor: not-allowed; }
  .intercept-state { display: inline-block; width: 2em; text-align: left; }
  .empty-icon { display: grid; place-items: center; width: 48px; height: 48px; border-radius: 50%; color: var(--accent); background: var(--accent-soft); font-size: var(--font-size-title); }
  .intercept-heading { display: flex; align-items: center; justify-content: space-between; padding: 0 10px; border: 1px solid #5e4317; border-bottom: 0; border-radius: 6px 6px 0 0; color: #d5d9df; background: #2a210f; font-size: var(--font-size-body); }
  .intercept-heading .intercept-actions { display: flex; gap: 5px; align-items: stretch; }
  .intercept-heading button { padding: 6px 9px; border: 1px solid #3b424d; border-radius: 4px; color: #cbd1da; background: #171b21; cursor: pointer; }
  .intercept-action-group { display: flex; align-items: stretch; }
  .intercept-action-group .intercept-main {
    box-sizing: border-box;
    width: 96px;
    justify-content: center;
    border-left: 0 !important;
    border-radius: 0 4px 4px 0 !important;
  }
  .intercept-heading button.intercept-all {
    display: grid; place-items: center;
    box-sizing: border-box;
    width: 25px; min-width: 25px; padding: 0 4px 0 2px;
    border: 3px solid #3b424d; border-radius: 4px 0 0 4px;
    background: #171b21; color: inherit; cursor: pointer;
    transition: background-color .16s ease, border-color .16s ease, color .16s ease, opacity .16s ease;
  }
  .intercept-all:disabled { opacity: .45; cursor: not-allowed; }
  .intercept-all:focus-visible { outline: 2px solid var(--accent, #9ca3af); outline-offset: 2px; }
  .intercept-all-label {
    writing-mode: vertical-rl;
    transform: rotate(180deg);
    font-size: 9px; font-weight: 800; letter-spacing: .08em; line-height: 1;
    text-orientation: mixed; user-select: none;
  }
  .intercept-heading button.intercept-all--forward {
    color: #fff;
    border-color: var(--success, #279630);
    border-right: 3px solid rgba(255,255,255,0.55);
    background: var(--success, #279630);
  }
  .intercept-all--forward:hover:not(:disabled) {
    color: #fff;
    border-color: color-mix(in srgb, var(--success, #279630) 82%, #fff);
    border-right-color: rgba(255,255,255,0.55);
    background: color-mix(in srgb, var(--success, #279630) 82%, #000);
  }
  .intercept-heading button.intercept-all--drop {
    color: #fff;
    border-color: var(--danger, #d92d4b);
    border-right: 3px solid rgba(255,255,255,0.55);
    background: var(--danger, #d92d4b);
  }
  .intercept-all--drop:hover:not(:disabled) {
    color: #fff;
    border-color: color-mix(in srgb, var(--danger, #d92d4b) 82%, #fff);
    border-right-color: rgba(255,255,255,0.55);
    background: color-mix(in srgb, var(--danger, #d92d4b) 82%, #000);
  }
  .history-workspace { display: grid; height: 100%; min-height: 0; overflow: hidden; }
  .history-panel { display: grid; grid-template-rows: auto minmax(0, 1fr); min-height: 0; border-bottom: 1px solid var(--border); }
  .pane-divider { width: 100%; height: 5px; padding: 0; border: 0; border-top: 1px solid var(--border); background: #171b21; cursor: row-resize; transition: background .15s ease; }
  .pane-divider:hover, .pane-divider:focus-visible { background: var(--accent); }
  .inspectors { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); gap: 7px; min-height: 0; min-width: 0; overflow: hidden; padding: 7px; background: #0b0e12; }
  .inspectors > div { display: grid; min-height: 0; min-width: 0; overflow: hidden; }
  .inspectors > div > :global(.message-viewer) { min-height: 0; height: 100%; }
  .history-panel [data-tour="history-table"] { display: grid; min-height: 0; min-width: 0; overflow: hidden; }
  .select-prompt { grid-column: 1 / -1; display: grid; place-items: center; color: var(--muted); border-radius: 6px; }
  .repeater-workspace { display: grid; grid-template-columns: 1fr 1fr; grid-template-rows: 31px 52px 1fr; gap: 7px; height: 100%; min-height: 0; padding: 7px; overflow: hidden; }
  .repeater-workspace [data-tour="replay-request-editor"] { grid-column: 1; grid-row: 3; min-height: 0; overflow: hidden; }
  .repeater-workspace [data-tour="replay-request-editor"] > :global(.message-viewer) { height: 100%; }
  .repeater-workspace .repeater-response-panel { grid-column: 2; grid-row: 3; min-height: 0; overflow: hidden; }
  .repeater-workspace > :global(.message-viewer) { grid-column: 2; grid-row: 3; min-height: 0; overflow: hidden; }
  .repeater-tabs {
    display: flex;
    min-width: 0;
    overflow: hidden;
    border-bottom: 1px solid var(--border);
  }
  .repeater-tab-viewport {
    position: relative;
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
  }
  .repeater-tab-scroll {
    width: 100%;
    height: 100%;
    min-width: 0;
    overflow-x: auto;
    scrollbar-width: none;
    -ms-overflow-style: none;
  }
  .repeater-tab-scroll::-webkit-scrollbar { display: none; }
  .repeater-tab-strip { display: flex; gap: 3px; min-width: max-content; min-height: 28px; }
  .tab-scroll-indicator {
    position: absolute;
    z-index: 2;
    top: 0;
    right: 0;
    display: grid;
    place-items: center;
    width: 24px;
    min-width: 24px;
    height: 28px;
    min-height: 28px;
    padding: 0;
    border: 0;
    border-left: 1px solid var(--border);
    border-radius: 3px 0 0 0;
    color: var(--muted);
    background: var(--surface);
    cursor: pointer;
  }
  .tab-scroll-indicator:hover { color: var(--text); background: var(--surface-2); }
  .repeater-tab-actions {
    display: flex;
    flex: 0 0 auto;
    align-items: flex-start;
    gap: 3px;
    padding-left: 3px;
    border-left: 1px solid var(--border);
    background: var(--surface);
  }
  .repeater-tab { display: flex; align-items: center; min-width: 108px; height: 28px; border: 1px solid var(--border); border-bottom: 2px solid transparent; border-radius: 3px 3px 0 0; background: var(--nestedtabsbg, var(--surface)); }
  .repeater-tab.active { color: var(--text); border-color: color-mix(in srgb, var(--accent) 40%, var(--border)); background: var(--nestedtabsbg, var(--surface)); }
  .repeater-tab button { height: 100%; padding: 0 7px; border: 0; color: var(--muted); background: transparent; font-size: var(--font-size-body); cursor: pointer; }
  .repeater-tab button:first-child { flex: 1; text-align: left; }
  .repeater-tab .tab-close-button { font-size: var(--font-size-heading); }
  .repeater-tab .tab-close-button.inactive { visibility: hidden; pointer-events: none; }
  .repeater-tab.active button:first-child { color: var(--text); }
  .repeater-tab-actions > button { border: 1px solid var(--border); border-bottom: 0; border-radius: 3px 3px 0 0; color: var(--muted); background: var(--surface); cursor: pointer; }
  .repeater-tab-actions > button.new-tab { width: 28px; min-width: 28px; height: 28px; padding: 0; justify-content: center; }
  .repeater-tab-actions .tab-search-button { border: 1px solid var(--border); }
  .repeater-tab-actions .tab-search-button:hover { color: var(--text); background: var(--surface-2); }
  .repeater-toolbar { display: flex; align-items: center; justify-content: space-between; padding: 0 10px; border: 1px solid var(--border); border-radius: 6px; background: var(--surface); }
  .repeater-toolbar .eyebrow { margin-bottom: 1px; }
  .repeater-toolbar-leading { display: flex; align-items: center; gap: 12px; min-width: 0; }
  .repeater-toolbar-context { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .repeater-toolbar-trailing { margin-left: auto; }
  .repeater-actions { display: flex; gap: 5px; }
  .repeater-protocol { display: flex; align-items: center; gap: 5px; color: var(--muted); font-size: var(--font-size-compact); }
  .repeater-protocol select { height: 28px; padding: 0 7px; border: 1px solid #343b45; border-radius: 4px; color: var(--text); background: #171b21; }
  .repeater-actions > button:not(.primary-action) { padding: 6px 9px; border: 1px solid #343b45; border-radius: 4px; color: #bdc5cf; background: #171b21; cursor: pointer; }
  .no-project { grid-row: 1 / -1; display: grid; place-content: center; justify-items: center; gap: 8px; color: var(--muted); text-align: center; }
  .no-project h1 { color: var(--text); }
  .no-project p { margin: 0 0 8px; }
  .no-project div:last-child { display: flex; gap: 7px; }
  .statusbar { z-index: 2; flex: 0 0 30px; gap: 22px; padding: 0 12px; color: var(--muted); background: #0e1115; border-top: 1px solid var(--border); font-size: var(--font-size-body); }
  .statusbar span:first-child { flex: 1; }
  .status-metric, .clock-button { white-space: nowrap; font-variant-numeric: tabular-nums; }
  .clock-button { padding: 0; border: 0; color: inherit; background: transparent; cursor: pointer; }
  .clock-button:hover { color: var(--text); }
  .clock-button:focus-visible { outline: 2px solid var(--border-strong); outline-offset: 2px; }
  @media (prefers-contrast: more) { .app-shell { --border: #697386; --muted: #c4cad3; } }
  .tab-group-color { display: inline-block; width: 8px; height: 8px; flex: 0 0 auto; border-radius: 50%; background: var(--tab-group-color, var(--art)); }
  .modal-backdrop {
    position: fixed; inset: 0; z-index: 40; display: grid; place-items: center; background: #05070ac7;
    font: var(--font-size-body)/1.35 Inter, "SF Pro Text", ui-sans-serif, system-ui, -apple-system, sans-serif;
  }
  .project-dialog { width: 440px; padding: 22px; border: 1px solid #343b45; border-radius: 8px; color: var(--text); background: #13171d; box-shadow: 0 20px 60px #000b; }
  .tab-group-dialog { display: grid; gap: 10px; width: min(380px, calc(100vw - 32px)); padding: 22px; border: 1px solid var(--border-strong); border-radius: 8px; color: var(--text); background: var(--surface); box-shadow: var(--shadow); }
  .tab-group-dialog h2 { margin: 0 0 8px; font-size: var(--font-size-title); }
  .tab-group-dialog label { display: grid; gap: 6px; color: var(--muted); font-size: var(--font-size-body); }
  .tab-group-dialog label input:not([type="color"]) { width: 100%; }
  .tab-group-error { margin: 0; color: var(--danger); font-size: var(--font-size-body); }
  .request-import-dialog { display: grid; gap: 15px; width: min(350px, calc(100vw - 40px)); padding: 20px; border: 1px solid var(--border-strong); border-radius: 7px; color: var(--text); background: var(--surface); box-shadow: var(--shadow); }
  .request-import-dialog h2 { margin: 0; font-size: var(--font-size-heading); }
  .request-import-options { display: grid; gap: 5px; }
  .request-import-option, .request-import-options button, .request-import-dialog footer button { min-height: 31px; padding: 4px 8px; border: 1px solid var(--border-strong); border-radius: 4px; color: var(--text); background: var(--surface-2); text-align: left; cursor: pointer; }
  .request-import-option { display: flex; align-items: center; }
  .request-import-option input { display: none; }
  .request-import-option:hover, .request-import-options button:hover { border-color: color-mix(in srgb, var(--text) 45%, var(--border-strong)); }
  .request-import-dialog footer { display: flex; justify-content: flex-end; gap: 7px; }
  .request-import-dialog footer .primary-action { color: var(--accent-contrast); background: var(--accent); }
  .request-import-curl textarea { width: 100%; min-height: 140px; resize: vertical; }
  .import-error { margin: 0; color: var(--danger); font-size: var(--font-size-body); }
  .project-dialog h2 { margin: 0 0 18px; font-size: var(--font-size-title); }
  .project-dialog label { display: grid; gap: 6px; margin: 11px 0; color: #aab2bd; font-size: var(--font-size-body); }
  .project-dialog label input { width: 100%; }
  .folder-picker { display: flex; gap: 7px; } .folder-picker input { flex: 1; } .folder-picker button { padding: 7px 10px; border: 1px solid #353c47; border-radius: 5px; color: #cbd1da; background: #1a1f26; cursor: pointer; }
  .project-dialog > p:not(.eyebrow) { color: var(--muted); font-size: var(--font-size-body); }
  .dialog-actions { display: flex; justify-content: flex-end; gap: 7px; margin-top: 19px; }
  .dialog-actions button { padding: 7px 12px; border: 1px solid #353c47; border-radius: 5px; color: #cbd1da; background: #1a1f26; cursor: pointer; }
  .dialog-actions button.primary-action { color: #1b1308; background: var(--accent); border-color: #d8860b; }
  .replay-search-dialog {
    display: grid;
    grid-template-rows: auto auto minmax(120px, 1fr);
    gap: 14px;
    width: min(680px, calc(100vw - 32px));
    max-height: min(620px, calc(100vh - 32px));
    padding: 20px;
    overflow: hidden;
    border: 1px solid var(--border-strong);
    border-radius: 8px;
    color: var(--text);
    background: var(--surface);
    box-shadow: var(--shadow);
  }
  .replay-search-heading { display: flex; align-items: flex-start; justify-content: space-between; gap: 12px; }
  .replay-search-heading .eyebrow { margin-bottom: 3px; }
  .replay-search-heading h2 { margin: 0; font-size: var(--font-size-title); }
  .replay-search-heading .icon-button { border: 1px solid transparent; border-radius: 4px; font-size: var(--font-size-title); }
  .replay-search-heading .icon-button:hover { color: var(--text); border-color: var(--border-strong); background: var(--surface-2); }
  .replay-search-field { display: grid; gap: 6px; color: var(--muted); font-size: var(--font-size-compact); }
  .replay-search-field input { width: 100%; height: 34px; }
  .replay-search-results { min-height: 0; overflow: auto; border: 1px solid var(--border); border-radius: 5px; background: var(--surface-2); }
  .replay-search-result { display: grid; grid-template-columns: minmax(0, 1fr) max-content; align-items: center; gap: 14px; width: 100%; min-height: 48px; padding: 9px 11px; border: 0; border-bottom: 1px solid var(--border); color: var(--text); background: transparent; text-align: left; cursor: pointer; }
  .replay-search-result:last-child { border-bottom: 0; }
  .replay-search-result:hover, .replay-search-result:focus-visible { background: var(--accent-soft); }
  .replay-search-snippet { min-width: 0; overflow: hidden; color: var(--muted); text-overflow: ellipsis; white-space: nowrap; }
  .replay-search-snippet strong { color: var(--text); font-weight: 800; }
  .replay-search-tab-badge { padding: 3px 7px; border: 1px solid color-mix(in srgb, var(--accent) 38%, var(--border)); border-radius: 999px; color: var(--text); background: var(--accent-soft); font-size: var(--font-size-compact); font-weight: 700; }
  .replay-search-empty { display: grid; place-items: center; min-height: 110px; margin: 0; padding: 18px; color: var(--muted); text-align: center; }
  .shortcut-dialog { width: min(430px, calc(100vw - 32px)); padding: 22px; border: 1px solid #343b45; border-radius: 8px; color: var(--text); background: #13171d; box-shadow: 0 20px 60px #000b; }
  .shortcut-dialog.shortcut-columns-2 { width: min(680px, calc(100vw - 32px)); }
  .shortcut-dialog.shortcut-columns-3 { width: min(900px, calc(100vw - 32px)); }
  .shortcut-dialog h2 { margin: 0 0 14px; }
  .shortcut-dialog-heading { display: flex !important; align-items: start !important; justify-content: space-between; gap: 12px; border-bottom: 0 !important; }
  .shortcut-dialog-heading .eyebrow { margin-bottom: 3px; }
  .shortcut-dialog-heading h2 { margin-bottom: 8px; }
  .shortcut-section { display: grid; }
  .shortcut-section + .shortcut-section { margin-top: 16px; }
  .shortcut-section-title { grid-column: 1 / -1; margin: 0; padding: 0 0 7px; border-bottom: 1px solid #252b33; color: var(--muted); font-size: var(--font-size-compact); font-weight: 800; letter-spacing: .1em; text-transform: uppercase; }
  .shortcut-entry { display: grid; grid-template-columns: max-content minmax(0, 1fr); column-gap: 8px; align-items: center; min-height: 48px; border-bottom: 1px solid #252b33; color: #aeb6c1; font-size: var(--font-size-body); }
  .shortcut-entry > span { display: block; min-width: 0; align-self: center; }
  .shortcut-description { color: var(--text); font-size: var(--font-size-body); font-weight: 400; line-height: 1.25; }
  .shortcut-dialog.shortcut-columns-2 .shortcut-section { grid-template-columns: repeat(2, minmax(0, 1fr)); column-gap: 28px; }
  .shortcut-dialog.shortcut-columns-3 .shortcut-section { grid-template-columns: repeat(3, minmax(0, 1fr)); column-gap: 22px; }
  .shortcut-dialog kbd { display: inline-flex; align-items: center; justify-content: center; gap: .3em; box-sizing: border-box; width: fit-content; min-width: 76px; min-height: 36px; padding: 7px 11px; border: 1px solid #39414c; border-radius: 6px; color: #d6dbe2; background: #0c0f13; box-shadow: inset 0 -1px 0 #39414c, 0 1px 2px #0007; font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; font-size: var(--font-size-body); font-weight: 800; letter-spacing: .035em; line-height: 1; white-space: nowrap; }
  .shortcut-key-part, .shortcut-key-separator { display: inline-flex; align-items: center; }
  .shortcut-key-separator { color: var(--muted); font-weight: 650; }
  /* Shared modern interaction system */
  .app-shell button, .app-shell :global(button) {
    transition: transform .16s ease, color .16s ease, background-color .16s ease, border-color .16s ease, box-shadow .16s ease, opacity .16s ease;
  }
  .app-shell button:not(:disabled):active, .app-shell :global(button:not(:disabled):active) { transform: translateY(1px); }
  .app-shell :global(.message-viewer .wrap-icon-action:active),
  .app-shell :global(.message-viewer .mode-cycle:active) { transform: none; }
  .titlebar {
    padding: 0 18px;
    background: color-mix(in srgb, var(--surface) 92%, transparent);
    border-bottom: 1px solid var(--border);
    box-shadow: 0 1px 0 rgb(255 255 255 / 2%);
  }
  .brand { gap: 10px; font-size: var(--font-size-heading); font-weight: 760; letter-spacing: -.01em; }
  .brand-mark {
    width: 30px; height: 30px; border-radius: 0; box-shadow: none;
  }
  .brand-mark img { width: 36px; height: 36px; }
  .icon-button {
    position: relative; display: grid; place-items: center; width: 32px; height: 32px; padding: 0; border: 1px solid transparent;
    border-radius: 9px; color: var(--muted); background: transparent;
  }
  .icon-button:hover { color: var(--text); border-color: var(--border); background: var(--surface-2); }
  .icon-button svg { width: 17px; height: 17px; fill: none; stroke: currentColor; stroke-width: var(--svgbuttonstrokewidth, 1.5); stroke-linecap: round; stroke-linejoin: round; }
  .theme-toggle { margin-left: 3px; color: var(--muted); background: transparent; }
  .project-save { color: var(--muted); background: transparent; }
  .project-save:hover, .theme-toggle:hover { color: var(--text); background: var(--surface-2); }
  .project-save:disabled { opacity: .45; cursor: not-allowed; }
  .project-save:disabled:hover { color: var(--muted); border-color: transparent; background: transparent; }
  .shutdown-button { margin-left: 2px; color: var(--danger); }
  .shutdown-button:hover { color: var(--danger); border-color: color-mix(in srgb, var(--danger) 45%, var(--border)); background: var(--danger-soft); }

  .toolbar {
    gap: 3px; padding: 0; background: transparent; border: 0; box-shadow: none;
  }
  .toolbar > button {
    position: relative; height: 36px; padding: 0 13px; border: 1px solid transparent; border-radius: 9px;
    color: var(--muted); font-size: var(--font-size-body); font-weight: 600;
  }
  .toolbar > button:hover { color: var(--text); border-color: var(--border); background: transparent;}
  .toolbar > button.active { color: var(--text); border-color: var(--border); background: var(--surface-2); }
  .toolbar > button.active::after { content: none; }
  .proxy-pill {
    min-height: 30px; padding: 0 10px; border: 1px solid var(--border); border-radius: 999px;
    background: var(--surface-2); font-size: var(--font-size-body); font-weight: 600;
  }
  .proxy-pill.online { color: var(--success); border-color: color-mix(in srgb, var(--success) 25%, var(--border)); background: var(--success-soft); }
  .proxy-pill:not(.online) { color: var(--danger); border-color: color-mix(in srgb, var(--danger) 25%, var(--border)); }
  .proxy-pill:not(.online) .status-dot { background: var(--danger); }
  .status-dot { width: 7px; height: 7px; margin: 0; background: var(--muted); }
  .status-dot.online, .online .status-dot { background: var(--success); box-shadow: 0 0 0 3px color-mix(in srgb, var(--success) 18%, transparent); }

  main { background: var(--bg); }
  main > [data-tour="settings-panel"],
  main > [data-tour="comparer"],
  main > [data-tour="decoder"],
  main > [data-tour="fuzz-workspace"],
  main > [data-tour="scope"],
  main > [data-tour="organizer"],
  main > [data-tour="identity"] { height: 100%; min-height: 0; overflow: hidden; }
  .forge-panel-host { height: 100%; min-height: 0; overflow: hidden; }
  .eyebrow { margin-bottom: 8px; color: var(--accent); font-size: var(--font-size-compact); letter-spacing: .18em; }
  h1 { font-size: var(--font-size-title); font-weight: 720; letter-spacing: -.035em; }
  h2 { font-weight: 680; letter-spacing: -.02em; }
  button.primary-action, .app-shell :global(button.run), .app-shell :global(.add > button) {
    min-height: 36px; padding: 0 16px; border: 0; border-radius: 9px; color: var(--accent-contrast);
    background: linear-gradient(135deg, var(--accent-hover), var(--accent)); font-weight: 700;
    box-shadow: 0 8px 20px color-mix(in srgb, var(--accent) 28%, transparent), inset 0 1px rgb(255 255 255 / 18%);
  }
  button.primary-action:not(:disabled):hover, .app-shell :global(button.run:not(:disabled):hover), .app-shell :global(.add > button:not(:disabled):hover) {
    transform: translateY(-1px); filter: saturate(1.08); box-shadow: 0 11px 26px color-mix(in srgb, var(--accent) 34%, transparent);
  }
  button.primary-action.danger {
    color: #fff; background: linear-gradient(135deg, #f0526b, #d92d4b);
    box-shadow: 0 8px 20px rgb(217 45 75 / 22%);
  }
  input, select,
  .app-shell :global(input), .app-shell :global(select), .app-shell :global(textarea) {
    border-color: var(--border-strong); border-radius: 8px; color: var(--text); background: var(--input);
  }
  /* Avoid platform-native select bevels and side glows; keep a simple flat control. */
  select, .app-shell :global(select) {
    appearance: none;
    -webkit-appearance: none;
    padding-right: 27px;
    background-color: var(--input);
    background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 12 12'%3E%3Cpath d='M3 4.5h6L6 1.5zM3 7.5h6l-3 3z' fill='%239ca3af'/%3E%3C/svg%3E");
    background-repeat: no-repeat;
    background-position: right 8px center;
    background-size: 12px;
    box-shadow: none;
  }
  input:hover, select:hover,
  .app-shell :global(input:hover), .app-shell :global(select:hover), .app-shell :global(textarea:hover) { border-color: color-mix(in srgb, var(--accent) 45%, var(--border-strong)); }
  input:focus, select:focus,
  .app-shell :global(input:focus), .app-shell :global(select:focus), .app-shell :global(textarea:focus) {
    border-color: var(--accent); box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 13%, transparent);
  }
  .repeater-actions > button:not(.primary-action), .intercept-heading button,
  .dialog-actions button, .folder-picker button {
    min-height: 32px; padding: 0 11px; border: 1px solid var(--border-strong); border-radius: 8px;
    color: var(--text); background: var(--surface-2); font-weight: 550;
  }
  .repeater-actions > button:not(.primary-action):hover, .dialog-actions button:hover {
    border-color: color-mix(in srgb, var(--accent) 42%, var(--border-strong)); background: var(--surface-3);
  }
  .dialog-actions button.primary-action {
    border: 0; color: var(--accent-contrast); background: linear-gradient(135deg, var(--accent-hover), var(--accent));
    box-shadow: 0 7px 18px color-mix(in srgb, var(--accent) 24%, transparent);
  }
  .empty-icon { width: 54px; height: 54px; color: var(--accent); background: var(--accent-soft); box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent) 15%, transparent); }
  .intercept-heading {
    border-color: color-mix(in srgb, var(--warning) 35%, var(--border)); border-radius: 10px 10px 0 0;
    color: var(--text); background: color-mix(in srgb, var(--warning) 10%, var(--surface));
  }
  .inspectors { gap: 10px; padding: 10px; background: var(--bg); }
  .pane-divider { background: var(--surface-2); }
  .repeater-workspace { grid-template-rows: 37px 60px minmax(190px, 1fr) minmax(190px, 1fr); gap: 10px; padding: 10px; }
  .repeater-tabs { gap: 3px; border-bottom: 1px solid var(--border); }
  .repeater-tab {
    min-width: 108px; height: 28px; border: 1px solid var(--border); border-bottom: 2px solid transparent; border-radius: 3px 3px 0 0;
    background: var(--nestedtabsbg, var(--surface));
  }
  .repeater-tab.active { color: var(--text); border-color: color-mix(in srgb, var(--accent) 40%, var(--border)); background: var(--nestedtabsbg, var(--surface)); }
  .repeater-tab-actions > button, .repeater-tab-actions > button.new-tab {
    width: 28px; min-width: 28px; height: 28px; padding: 0; border: 1px solid var(--border); border-bottom: 0; border-radius: 3px 3px 0 0;
    color: var(--muted); background: var(--surface);
  }
  .repeater-toolbar { padding: 0 14px; border-radius: 11px; box-shadow: var(--shadow-soft); }
  .repeater-actions { align-items: center; gap: 7px; }
  .repeater-protocol select { height: 32px; border-color: var(--border-strong); border-radius: 8px; color: var(--text); background: var(--input); }
  .statusbar { gap: 24px; padding: 0 16px; color: var(--muted); background: var(--surface); border-top-color: var(--border); font-size: var(--font-size-compact); }

  .project-dialog, .tab-group-dialog, .shortcut-dialog {
    border-color: var(--border-strong); border-radius: 12px; color: var(--text); background: var(--surface); box-shadow: var(--shadow);
  }
  .modal-backdrop { background: rgb(5 7 12 / 24%); backdrop-filter: blur(2px); }
  .project-dialog, .tab-group-dialog, .shortcut-dialog { padding: 26px; }
  .project-dialog label { color: var(--muted); }
  .shortcut-dialog .shortcut-section-title, .shortcut-dialog .shortcut-entry { border-bottom-color: var(--border); color: var(--muted); }
  .shortcut-dialog kbd { border-color: var(--border-strong); border-radius: 6px; color: var(--text); background: var(--surface-2); }
  @media (max-width: 700px) {
    .shortcut-dialog.shortcut-columns-2, .shortcut-dialog.shortcut-columns-3 { width: min(430px, calc(100vw - 32px)); }
    .shortcut-dialog.shortcut-columns-2 .shortcut-section, .shortcut-dialog.shortcut-columns-3 .shortcut-section { grid-template-columns: 1fr; }
  }

  /* Cross-component surfaces and controls */
  .app-shell :global(.decoder-tool),
  .app-shell :global(.comparer-tool),
  .app-shell :global(.scope-tool),
  .app-shell :global(.site-map),
  .app-shell :global(.logs-tool),
  .app-shell :global(.settings-tool) { color: var(--text); }
  .app-shell :global(.comparer-tool header p),
  .app-shell :global(.scope-tool header p),
  .app-shell :global(.site-map header p),
  .app-shell :global(.logs-tool header p),
  .app-shell :global(.settings-tool header p) { color: var(--accent); }
  .app-shell :global(.history-table),
  .app-shell :global(.filter-bar),
  .app-shell :global(.tree),
  .app-shell :global(.list),
  .app-shell :global(.log-list),
  .app-shell :global(.operation-bar),
  .app-shell :global(fieldset) {
    color: var(--text); border-color: var(--border); background: var(--surface);
  }
  .app-shell :global(.message-viewer) {
    color: var(--text); border-color: var(--border); background: var(--editor);
  }
  .app-shell :global(.message-viewer),
  .app-shell :global(.tree),
  .app-shell :global(.list),
  .app-shell :global(.log-list),
  .app-shell :global(fieldset) { border-radius: 11px; box-shadow: var(--shadow-soft); }
  .app-shell :global(.table-header),
  .app-shell :global(.filter-bar) { color: var(--text); border-color: var(--border); background: var(--surface-2); }
  .app-shell :global(.message-viewer > header),
  .app-shell :global(.hex-toolbar) { color: var(--text); border-color: var(--border); background: var(--editor) !important; }
  .app-shell :global(.metadata),
  .app-shell :global(.detected),
  .app-shell :global(.inputs label),
  .app-shell :global(.panels label),
  .app-shell :global(.global),
  .app-shell :global(.node em),
  .app-shell :global(.endpoint),
  .app-shell :global(.empty),
  .app-shell :global(.loading),
  .app-shell :global(.log-entry),
  .app-shell :global(.settings-tool label),
  .app-shell :global(.settings-tool footer) { color: var(--muted); }
  .app-shell :global(.detected strong), .app-shell :global(.table-header button) { color: var(--text); }
  .app-shell :global(.message-viewer button),
  .app-shell :global(.filter-bar button),
  .app-shell :global(.hex-toolbar button),
  .app-shell :global(.scope-tool button),
  .app-shell :global(.scope-tool .import),
  .app-shell :global(.site-map .header-actions button),
  .app-shell :global(.logs-tool button),
  .app-shell :global(.comparer-tool button) {
    min-height: 30px; border: 1px solid transparent; border-radius: 8px;
    color: var(--muted); background: transparent;
  }
  .app-shell :global(.message-viewer button:hover),
  .app-shell :global(.filter-bar button:hover),
  .app-shell :global(.scope-tool button:hover),
  .app-shell :global(.scope-tool .import:hover),
  .app-shell :global(.site-map .header-actions button:hover),
  .app-shell :global(.logs-tool button:hover),
  .app-shell :global(.comparer-tool button:hover) { color: var(--text); border-color: transparent; background: transparent; }
  .app-shell :global(.message-viewer button.active) { color: var(--text); border-color: var(--border); background: var(--surface-2); }
  .app-shell :global(.message-viewer button.accent) { color: var(--text); border-color: var(--border); }
  .app-shell :global(.viewport),
  .app-shell :global(.pretty),
  .app-shell :global(.highlighted),
  .app-shell :global(.hex-view),
  .app-shell :global(.diff > div),
  .app-shell :global(.cm-editor),
  .app-shell :global(.cm-scroller),
  .app-shell :global(.comparer-tool textarea) { color: var(--text) !important; background: var(--input) !important; }
  .app-shell :global(.pretty),
  .app-shell :global(.hex-view),
  .app-shell :global(.cm-editor),
  .app-shell :global(.cm-scroller),
  .app-shell :global(.cm-content),
  .app-shell :global(.message-viewer .pretty input),
  .app-shell :global(.message-viewer .pretty textarea) { background: var(--editor) !important; }
  .app-shell :global(.cm-gutters) { color: var(--muted) !important; border-color: var(--border) !important; background: var(--editor) !important; }
  .app-shell :global(.cm-activeLine), .app-shell :global(.cm-activeLineGutter) { background: var(--editor) !important; }
  .app-shell :global(.table-row) { border-color: color-mix(in srgb, var(--border) 65%, transparent); color: var(--text); }
  .app-shell :global(.table-row:hover), .app-shell :global(.node:hover), .app-shell :global(.endpoint:hover) { background: var(--surface-2); }
  .app-shell :global(.table-row.selected) { background: var(--accent-soft); box-shadow: inset 3px 0 var(--accent); }
  .app-shell :global(.node), .app-shell :global(.endpoint), .app-shell :global(.rule), .app-shell :global(.log-entry) { border-color: var(--border); }
  .app-shell :global(.path), .app-shell :global(.rule code), .app-shell :global(.directory-node) { color: var(--text); }
  .app-shell :global(.node-menu) { color: var(--text); border-color: var(--border-strong); border-radius: 10px; background: var(--surface); box-shadow: var(--shadow); }
  .app-shell :global(.node-menu button:hover) { background: var(--surface-2); }
  .app-shell :global(.custom-range) { border-color: var(--border-strong); border-radius: 10px; color: var(--text); background: var(--surface); box-shadow: var(--shadow-soft); }
  .app-shell :global(.save-state) { color: var(--success); }
  .app-shell :global(.certificate-state) { color: var(--success); }
  .app-shell :global(.certificate-actions button) {
    border: 0; border-radius: 8px; color: var(--accent-contrast); background: linear-gradient(135deg, var(--accent-hover), var(--accent));
    box-shadow: 0 7px 18px color-mix(in srgb, var(--accent) 22%, transparent);
  }
  .app-shell :global(.method-get) { color: #23b889; }
  .app-shell :global(.method-post) { color: #5794ef; }
  .app-shell :global(.method-delete), .app-shell :global(.danger), .app-shell :global(.delete) { color: var(--danger); }

  .app-shell[data-theme="light"] :global(.endpoint .method) { color: #4b5563; background: #e5e7eb; }
  .app-shell[data-theme="light"] :global(.endpoint .method.get) { color: #087859; background: #e2f7ef; }
  .app-shell[data-theme="light"] :global(.endpoint .method.post) { color: #1767be; background: #e7f1ff; }
  .app-shell[data-theme="light"] :global(.diff span.add) { color: #087859; background: #dff6ed; }
  .app-shell[data-theme="light"] :global(.diff span.delete) { color: #b4233e; background: #ffe5ea; }
  .app-shell[data-theme="light"] :global(.highlighted .match) { color: #3d2600; background: #ffd978; }

  /* Dense desktop-tool layout */
  .app-shell {
    --radius: 4px;
    font-size: var(--font-size-body);
    line-height: 1.35;
  }
  .titlebar { flex-basis: 40px; gap: 2px; padding: 0 8px; box-shadow: none; }
  .brand { gap: 7px; font-size: var(--font-size-body); }
  .brand-mark { width: 30px; height: 30px; border-radius: 0; box-shadow: none; }
  .brand-mark img { width: 30px; height: 30px; }
  .icon-button { width: 27px; height: 27px; border-radius: 4px; }
  .icon-button svg { width: 15px; height: 15px; }
  .theme-toggle { margin-left: 2px; }

  .toolbar { gap: 0; padding: 0 8px; box-shadow: none; }
  .toolbar > button {
    height: 31px; padding: 0 11px; box-sizing: border-box; border: 1px solid transparent; border-radius: 2px;
    border-bottom: 2px solid transparent;
    color: var(--muted); background: var(--surface); font-size: var(--font-size-body); font-weight: 520; transition-duration: 0s;
  }
  .toolbar > button:hover { color: var(--text); border-color: transparent; background: var(--surface-2); }
  .toolbar > button.active { color: var(--text); background: var(--surface); border-color: transparent; border-bottom: 2px solid var(--art); }
  .toolbar > button.active::after { right: 8px; bottom: -5px; left: 8px; height: 2px; box-shadow: none; }
  .proxy-pending-badge {
    position: absolute; top: 2px; right: 3px; min-width: 13px; height: 13px; padding: 0 3px;
    color: #fff; border-radius: 999px; background: #279630; font-size: var(--font-size-compact); font-weight: 750; line-height: 13px;
    text-align: center; pointer-events: none;
  }
  .proxy-pill { width: 80px; min-height: 25px; justify-content: center; padding: 0 8px; border-radius: 4px; font-size: var(--font-size-compact); white-space: nowrap; }

  main { background: var(--bg); }
  h1 { font-size: var(--font-size-heading); letter-spacing: -.015em; }
  h2 { font-size: var(--font-size-heading); }
  button.primary-action, .app-shell :global(button.run), .app-shell :global(.add > button) {
    min-height: 28px; padding: 0 11px; border-radius: 4px;
    background: var(--accent); box-shadow: none;
  }
  .proxy-control-actions > button.primary-action { width: 104px; }
  button.primary-action:not(:disabled):hover, .app-shell :global(button.run:not(:disabled):hover), .app-shell :global(.add > button:not(:disabled):hover) {
    transform: none; background: var(--accent-hover); box-shadow: none;
  }
  button.primary-action.danger { background: var(--danger); box-shadow: none; }
  .proxy-control-actions > button.primary-action:not(.danger) { color: #fff; background: var(--success); }
  .proxy-control-actions > button.primary-action:not(.danger):hover { background: color-mix(in srgb, var(--success) 82%, #000); }
  .proxy-control-actions > button.primary-action.danger { color: #fff; background: var(--danger); }
  .proxy-control-actions > button.primary-action.danger:hover { background: color-mix(in srgb, var(--danger) 82%, #000); }
  input, select,
  .app-shell :global(input), .app-shell :global(select), .app-shell :global(textarea) { border-radius: 4px; }
  input:focus, select:focus,
  .app-shell :global(input:focus), .app-shell :global(select:focus), .app-shell :global(textarea:focus) { box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent) 14%, transparent); }
  select:focus, .app-shell :global(select:focus) { box-shadow: none; }
  .repeater-actions > button:not(.primary-action), .intercept-heading button,
  .dialog-actions button, .folder-picker button {
    min-height: 27px; padding: 0 8px; border-radius: 4px; font-weight: 500;
  }
  .empty-icon { width: 38px; height: 38px; font-size: var(--font-size-heading); box-shadow: none; }
  .intercept-heading { border-radius: 3px 3px 0 0; }
  .dialog-actions button.primary-action { background: var(--accent); box-shadow: none; }

  .history-panel { border-bottom-color: var(--border); }
  .inspectors { gap: 4px; padding: 0px;}
  .pane-divider { height: 5px; }
  .app-shell :global(.filter-bar) { gap: 4px; padding: 4px; }
  .app-shell :global(.filter-bar input),
  .app-shell :global(.filter-bar select),
  .app-shell :global(.filter-bar button) { height: 27px; min-height: 27px; border-radius: 3px; }
  .app-shell :global(.history-table) { grid-template-rows: 28px minmax(0, 1fr); border-radius: 0; box-shadow: none; }
  .app-shell :global(.table-row) { height: 28px; }
  .app-shell :global(.table-header button) { padding: 0 6px; }

  .repeater-workspace {
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    grid-template-rows: 30px 40px minmax(0, 1fr);
    gap: 4px; padding: 4px;
  }
  .repeater-tabs, .repeater-toolbar { grid-column: 1 / -1; }
  .repeater-workspace :global(.message-viewer) { grid-row: 3; }
  .repeater-tabs { gap: 3px; }
  .repeater-tab { min-width: 108px; height: 28px; border-radius: 3px 3px 0 0; }
  .repeater-tab-group { display: flex; align-items: stretch; flex: 0 0 auto; }
  .repeater-group-tabs {
    display: flex;
    align-items: stretch;
    position: relative;
    flex: 0 0 auto;
    border-bottom: 2px solid var(--tab-group-color, var(--art));
  }
  .tab-group-marker {
    display: flex; align-items: center; gap: 6px; height: 28px; padding: 0 8px; border: 1px solid var(--border); border-bottom: 0;
    border-radius: 3px 3px 0 0; color: var(--text); background: color-mix(in srgb, var(--tab-group-color, var(--art)) var(--tab-group-bg-mix, 12%), var(--surface));
    font-size: var(--font-size-compact); font-weight: 650; white-space: nowrap; cursor: pointer;
  }
  .tab-group-marker:hover { border-color: color-mix(in srgb, var(--tab-group-color, var(--art)) 50%, var(--border)); }
  .tab-group-marker small { color: var(--muted); font-size: var(--font-size-compact); font-weight: 500; }
  .tab-group-chevron { color: var(--muted); font-size: 12px; line-height: 1; }
  .repeater-tab-group.collapsed .tab-group-marker { border-bottom: 1px solid var(--border); }

  .tab-group-dialog {
    display: grid;
    gap: 0;
    width: min(352px, calc(100vw - 24px));
    padding: 0;
    overflow: hidden;
    border-radius: 8px;
  }
  .tab-group-dialog-form { display: grid; gap: 0; }
  .tab-group-name-label {
    display: block !important;
    margin: 0 0 4px !important;
    color: var(--muted) !important;
    font-size: var(--font-size-compact) !important;
  }
  .tab-group-name-input {
    width: 100%;
    height: 23px;
    min-height: 23px;
    padding: 0 7px;
    border-radius: 4px !important;
    font-size: var(--font-size-compact);
  }
  .tab-group-tabs-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-top: 10px;
    padding-top: 8px;
    border-top: 1px solid var(--border);
    color: var(--muted);
    font-size: var(--font-size-compact);
  }
  .tab-group-select-all {
    min-height: 0 !important;
    padding: 0 !important;
    border: 0 !important;
    color: var(--art) !important;
    background: transparent !important;
    font-size: var(--font-size-compact);
    text-decoration: underline;
    cursor: pointer;
  }
  .tab-group-tab-list {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-start;
    column-gap: 10px;
    row-gap: 0;
    max-height: 151px;
    margin-top: 4px;
    overflow-y: auto;
  }
  .tab-group-tab-option {
    display: flex !important;
    align-items: center;
    gap: 7px;
    flex: 0 1 auto;
    width: max-content;
    max-width: 100%;
    min-height: 23px;
    min-width: 30px;
    margin: 0 !important;
    color: var(--text) !important;
    font-size: var(--font-size-compact) !important;
    cursor: pointer;
  }
  .tab-group-tab-option span {
    width: max-content;
    min-width: 0;
    max-width: calc(100% - 20px);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tab-group-empty { padding: 5px 0; color: var(--muted); font-size: var(--font-size-compact); }
  .tab-group-error { margin: 7px 0 0; font-size: var(--font-size-compact); }
  .tab-group-color-field {
    display: flex !important;
    align-items: center;
    justify-content: space-between;
    margin: 8px 0 0 !important;
    padding-top: 8px;
    border-top: 1px solid var(--border);
    color: var(--muted) !important;
    font-size: var(--font-size-compact) !important;
  }
  .tab-group-color-field input[type="color"] {
    width: 42px;
    height: 22px;
    min-height: 22px;
    padding: 2px;
    border: 1px solid var(--border-strong);
    border-radius: 3px;
    background: transparent;
    cursor: pointer;
  }
  .tab-group-dialog-footer {
    display: flex;
    justify-content: flex-end;
    gap: 6px;
    padding: 7px 8px;
    border-top: 1px solid var(--border);
  }
  .tab-group-dialog-footer button {
    min-width: 78px;
    min-height: 27px;
    padding: 0 10px;
    border: 1px solid var(--border-strong);
    border-radius: 4px;
    color: var(--text);
    background: var(--surface-2);
    font-size: var(--font-size-compact);
    font-weight: 600;
    cursor: pointer;
  }
  .tab-group-dialog-footer button.primary-action {
    border-color: var(--art);
    color: #1b1308;
    background: var(--art);
    box-shadow: none;
  }
  .tab-group-dialog-footer button:disabled { opacity: .45; cursor: not-allowed; }
  .tab-group-dialog {
    display: block;
    width: min(380px, calc(100vw - 32px));
    padding: 20px;
    overflow: visible;
    border: 1px solid var(--border-strong);
    border-radius: 8px;
    background: var(--surface);
    box-shadow: var(--shadow);
  }
  .tab-group-dialog-form { display: grid; gap: 10px; }
  .tab-dialog-eyebrow {
    margin: 0;
    color: var(--accent);
    font-size: var(--font-size-compact);
    font-weight: 800;
    letter-spacing: .13em;
  }
  .tab-group-dialog h2 { margin: -3px 0 0; font-size: var(--font-size-heading); }
  .tab-group-name-label {
    display: grid !important;
    gap: 5px !important;
    margin: 0 !important;
    color: var(--muted) !important;
    font-size: var(--font-size-compact) !important;
  }
  .tab-group-name-input {
    width: 100%;
    height: 34px;
    min-height: 34px;
    padding: 0 9px;
    border: 1px solid var(--border-strong) !important;
    border-radius: 3px !important;
    box-shadow: none !important;
    background: var(--input) !important;
    font-size: var(--font-size-body);
  }
  .tab-group-name-input:focus { border-color: var(--accent) !important; box-shadow: none !important; }
  .tab-group-tabs-header {
    margin-top: 4px;
    padding-top: 10px;
    border-top: 1px solid var(--border);
    font-size: var(--font-size-compact);
  }
  .tab-group-color-field {
    margin-top: 4px !important;
    padding-top: 10px;
    border-top: 1px solid var(--border);
    font-size: var(--font-size-compact) !important;
  }
  .tab-group-dialog-footer {
    display: flex;
    justify-content: flex-end;
    gap: 5px;
    margin-top: 4px;
    padding: 0;
    border-top: 0;
  }
  .tab-group-dialog-footer button {
    min-height: 27px;
    padding: 0 10px;
    border: 1px solid var(--border-strong);
    border-radius: 3px;
    color: var(--text);
    background: var(--surface-2);
    font-size: var(--font-size-compact);
  }
  .repeater-tab-actions > button, .repeater-tab-actions > button.new-tab {
    width: 28px; min-width: 28px; height: 28px; border-radius: 3px 3px 0 0;
  }
  .repeater-tab,
  .app-shell :global(.intruder-tab) {
    flex: 0 0 auto;
    min-width: 48px;
    background: var(--nestedtabsbg, var(--surface));
  }
  .repeater-tab.active,
  .app-shell :global(.intruder-tab.active) {
    background: var(--nestedtabsbg, var(--surface));
    border-bottom: 2px solid var(--art);
  }
  .repeater-tab button:first-child,
  .app-shell :global(.intruder-tab button:first-child) {
    flex: 0 0 auto;
    white-space: nowrap;
  }
  .repeater-toolbar { padding: 0 7px; border-radius: 3px; box-shadow: none; }
  .repeater-toolbar .eyebrow { display: none; }
  .repeater-toolbar strong { font-size: var(--font-size-body); }
  .repeater-toolbar-leading { gap: 6px; }
  .repeater-actions { gap: 4px; }
  .repeater-protocol select { height: 27px; border-radius: 3px; }

  .app-shell :global(.message-viewer) {
    grid-template-rows: 32px minmax(0, 1fr); border-radius: 3px; box-shadow: none;
  }
  .app-shell :global(.message-viewer > header) { padding: 0 6px; }
  .app-shell :global(.message-viewer button),
  .app-shell :global(.hex-toolbar button),
  .app-shell :global(.scope-tool button),
  .app-shell :global(.scope-tool .import),
  .app-shell :global(.site-map .header-actions button),
  .app-shell :global(.logs-tool button),
  .app-shell :global(.comparer-tool button) {
    min-height: 25px; padding: 0 7px; border-radius: 3px;
  }
  .app-shell :global(.pretty), .app-shell :global(.highlighted), .app-shell :global(.hex-view) { padding: 7px; }

  .app-shell :global(.site-map),
  .app-shell :global(.logs-tool) {
    width: 100%; max-width: none; margin: 0; padding: 0 6px 6px;
  }
  .app-shell :global(.comparer-tool),
  .app-shell :global(.scope-tool) {
    width: 100%; max-width: none; margin: 0; padding: 0 10px 10px; gap: 8px;
  }
  .app-shell :global(.settings-tool) { width: 100%; max-width: none; margin: 0; padding: 0; }
  .app-shell :global(.comparer-tool) { grid-template-rows: 44px minmax(150px, .7fr) 30px minmax(190px, 1fr); }
  .app-shell :global(.scope-tool) { grid-template-rows: 44px minmax(0, 1fr) 38px; }
  .app-shell :global(.site-map) { grid-template-rows: 44px minmax(0, 1fr); }
  .app-shell :global(.settings-tool > header) { height: 52px; }
  .app-shell :global(.comparer-tool header p),
  .app-shell :global(.scope-tool header p),
  .app-shell :global(.site-map header p),
  .app-shell :global(.logs-tool header p) { display: none; }
  .app-shell :global(.comparer-tool h1),
  .app-shell :global(.scope-tool h1),
  .app-shell :global(.site-map h1),
  .app-shell :global(.logs-tool h1),
  .app-shell :global(.settings-tool h1) { font-size: var(--font-size-heading); letter-spacing: 0; }
  .app-shell :global(.operation-bar) { padding: 4px; border-radius: 3px; }
  .app-shell :global(.tree),
  .app-shell :global(.list),
  .app-shell :global(.log-list),
  .app-shell :global(fieldset) { border-radius: 3px; box-shadow: none; }
  .app-shell :global(fieldset) { padding: 10px; }
  .app-shell :global(.settings-form) { gap: 6px; }
  .app-shell :global(.scope-tool .scope-add button[type="submit"]) {
    color: var(--accent-contrast); border-color: var(--accent); background: var(--accent);
  }
  .app-shell :global(.scope-tool .scope-add .out-action) {
    color: #fecaca; border-color: color-mix(in srgb, var(--danger) 55%, var(--border)); background: var(--danger-soft);
  }
  .app-shell :global(.comparer-tool .inputs label > span button) {
    height: 23px; min-height: 23px; border-color: transparent; background: transparent;
  }
  .statusbar { gap: 18px; padding: 0 8px; font-size: var(--font-size-compact); }

  .proxy-workspace {
    display: grid;
    grid-template-rows: 44px minmax(0, 1fr);
    gap: 4px;
    width: 100%;
    height: 100%;
    min-height: 0;
    padding: 4px;
    overflow: hidden;
  }
  .proxy-controls {
    display: flex;
    align-items: stretch;
    justify-content: space-between;
    min-width: 0;
    padding: 0 7px 0 10px;
    border-bottom: 1px solid var(--border);
    border-radius: 3px;
    background: var(--surface);
  }
  .proxy-control-title, .proxy-control-actions {
    display: flex;
    align-items: center;
    height: 100%;
  }
  .proxy-control-title { min-width: 0; gap: 4px; }
  .proxy-control-title h1 { margin: 0; font-size: var(--font-size-body); font-weight: 650; letter-spacing: 0; }
  .proxy-control-title > div { display: flex; align-items: baseline; gap: 9px; min-width: 0; }
  .proxy-control-title > div > span { overflow: hidden; color: var(--muted); text-overflow: ellipsis; white-space: nowrap; font-size: var(--font-size-compact); }
  .proxy-control-actions { gap: 6px; }
  .proxy-control-actions .intercept-switch { margin-right: 4px; font-size: var(--font-size-compact); }
  .proxy-settings-button {
    min-height: 28px;
    padding: 0 9px;
    color: var(--text);
    border: 1px solid var(--border-strong);
    border-radius: 4px;
    background: var(--surface-2);
    font-size: var(--font-size-compact);
    cursor: pointer;
  }
  .proxy-settings-button:hover { border-color: var(--accent); background: var(--surface-3); }
  .proxy-state {
    display: grid;
    place-content: center;
    justify-items: center;
    gap: 7px;
    min-height: 0;
    color: var(--muted);
    text-align: center;
  }
  .proxy-state svg {
    width: 72px;
    height: 72px;
    margin-bottom: 8px;
    color: var(--muted);
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .proxy-state h2 { margin: 0; color: var(--text); font-size: var(--font-size-heading); }
  .proxy-state p { margin: 0; font-size: var(--font-size-body); }
  .proxy-state.state-off svg { color: color-mix(in srgb, var(--muted) 72%, transparent); }
  .proxy-state.state-waiting svg { color: var(--accent); }
  .proxy-state.state-waiting svg circle:first-child {
    fill: color-mix(in srgb, var(--accent) 18%, transparent);
    animation: waiting-pulse 1.8s ease-in-out infinite;
    transform-origin: center;
  }
  @keyframes waiting-pulse {
    50% { transform: scale(1.35); opacity: .65; }
  }
  .proxy-interception-ui {
    display: grid;
    grid-template-rows: minmax(110px, 28%) 35px minmax(220px, 1fr);
    gap: 4px;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
  }
  .proxy-queue {
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    border: 1px solid var(--border);
    border-radius: 3px;
    background: var(--surface);
  }
  .proxy-queue :global(.intercept-table) { height: 100%; }
  .proxy-interception-ui > .intercept-heading {
    min-width: 0;
    padding: 0 7px 0 10px;
    border: 1px solid var(--border);
    border-radius: 3px 3px 0 0;
    color: var(--text);
    background: var(--surface);
  }
  .proxy-interception-ui > .intercept-heading > span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .intercept-preview {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 0;
    min-width: 0;
    min-height: 0;
    margin-top: -4px;
    overflow: hidden;
  }
  .intercept-preview.split { grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 4px; }
  .intercept-preview :global(.message-viewer) { border-radius: 0 0 3px 3px; }
  .intercept-preview-empty {
    display: grid;
    place-content: center;
    justify-items: center;
    gap: 5px;
    min-height: 0;
    margin-top: -4px;
    color: var(--muted);
    border: 1px solid var(--border);
    border-top: 0;
    border-radius: 0 0 3px 3px;
    background: var(--surface);
  }
  .intercept-preview-empty > span { color: var(--accent); font-size: var(--font-size-title); }
  .intercept-preview-empty strong { color: var(--text); font-size: var(--font-size-body); }
  .intercept-preview-empty small { font-size: var(--font-size-compact); }

  .identity-config-summary { color: var(--text); font-size: var(--font-size-compact); font-weight: 650; white-space: nowrap; }
  .repeater-response-panel {
    grid-column: 2;
    grid-row: 3;
    display: grid;
    grid-template-rows: minmax(0, 1fr) 37px;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
  }
  .identity-response-view { min-width: 0; min-height: 0; }
  .identity-response-view :global(.message-viewer) { height: 100%; min-height: 0; }
  .identity-response-state {
    display: grid;
    place-content: center;
    gap: 5px;
    width: 100%;
    height: 100%;
    min-height: 0;
    padding: 14px;
    color: var(--muted);
    border: 1px solid var(--border);
    border-radius: 3px 3px 0 0;
    background: var(--surface);
    text-align: center;
  }
  .identity-response-state strong { color: var(--text); font-size: var(--font-size-body); }
  .identity-response-state.error { color: var(--danger); }
  .identity-response-tabs {
    display: flex;
    align-items: stretch;
    min-width: 0;
    height: 37px;
    overflow-x: auto;
    scrollbar-width: none;
    -ms-overflow-style: none;
    border: 1px solid var(--border);
    border-top: 0;
    border-radius: 0 0 3px 3px;
    background: var(--surface-2);
  }
  .identity-response-tabs::-webkit-scrollbar { display: none; }
  .identity-response-tabs > button {
    display: flex;
    flex: 0 0 145px;
    align-items: center;
    gap: 6px;
    min-width: 145px;
    padding: 3px 7px;
    border: 0;
    border-right: 1px solid var(--border);
    color: var(--muted);
    background: transparent;
    text-align: left;
    cursor: pointer;
  }
  .identity-response-tabs > button.active { color: var(--text); background: var(--accent-soft); box-shadow: inset 0 -2px var(--accent); }
  .identity-color { flex: 0 0 auto; width: 8px; height: 8px; border-radius: 50%; }
  .identity-response-tab-copy { display: grid; min-width: 0; gap: 1px; }
  .identity-response-tab-copy strong, .identity-response-tab-copy small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .identity-response-tab-copy strong { font-size: var(--font-size-compact); }
  .identity-response-tab-copy small { color: var(--muted); font-size: 9px; }
  .identity-tabs-empty { display: grid; place-items: center; min-width: 100%; color: var(--muted); font-size: var(--font-size-compact); }
  .identity-config-dialog {
    display: grid;
    grid-template-rows: auto minmax(140px, 1fr) auto;
    gap: 12px;
    width: min(440px, calc(100vw - 40px));
    min-height: 280px;
    max-height: min(620px, calc(100dvh - 40px));
    padding: 18px;
    border: 1px solid var(--border-strong);
    border-radius: 5px;
    color: var(--text);
    background: var(--surface);
    box-shadow: var(--shadow);
  }
  .identity-config-dialog > header, .identity-config-dialog > footer, .identity-selection-heading, .identity-selection-actions { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  .identity-config-dialog h2 { margin: 0; font-size: var(--font-size-heading); }
  .identity-config-dialog .eyebrow { margin: 0 0 3px; }
  .identity-config-dialog > header > button, .identity-selection-heading button, .identity-selection-actions button, .identity-config-dialog > footer > button:not(.primary-action) {
    min-height: 27px;
    padding: 0 8px;
    border: 1px solid var(--border-strong);
    border-radius: 3px;
    color: var(--text);
    background: var(--surface-2);
    cursor: pointer;
  }
  .identity-config-dialog > header > button { width: 27px; padding: 0; font-size: var(--font-size-heading); }
  .identity-config-dialog > footer { justify-content: flex-end; }
  .identity-group-choices, .identity-selection { min-height: 0; overflow: auto; }
  .identity-group-choices > p, .identity-selection-actions > span { margin: 0 0 8px; color: var(--muted); font-size: var(--font-size-compact); }
  .identity-group-choices { display: grid; align-content: start; gap: 6px; }
  .identity-group-choices > button {
    display: flex;
    align-items: center;
    justify-content: space-between;
    min-height: 42px;
    padding: 7px 9px;
    border: 1px solid var(--border);
    border-radius: 3px;
    color: var(--text);
    background: var(--surface-2);
    text-align: left;
    cursor: pointer;
  }
  .identity-group-choices > button:hover { border-color: var(--accent); }
  .identity-group-choices > button span { color: var(--muted); font-size: var(--font-size-compact); }
  .identity-selection { display: grid; grid-template-rows: auto auto minmax(0, 1fr); gap: 9px; }
  .identity-selection-heading > div { display: grid; gap: 2px; }
  .identity-selection-heading span { color: var(--muted); font-size: var(--font-size-compact); }
  .identity-selection-actions { align-items: baseline; }
  .identity-selection-actions > span { margin: 0; }
  .identity-checkboxes { display: grid; align-content: start; gap: 3px; overflow: auto; }
  .identity-checkboxes > label { display: flex; align-items: center; gap: 7px; min-height: 29px; padding: 3px 5px; border-radius: 3px; cursor: pointer; }
  .identity-checkboxes > label:hover { background: var(--surface-2); }
  .identity-dialog-state { display: grid; place-content: center; min-height: 100px; color: var(--muted); text-align: center; }
  .identity-dialog-state.error { color: var(--danger); }

  /* One shared treatment for visible text actions. Icon-only and structural buttons stay specialized. */
  :global(button.text-button) {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    min-width: 0;
    min-height: 28px;
    padding: 0 10px;
    border: 1px solid var(--border-strong, #3a424d);
    border-radius: 4px;
    color: var(--text, #dbe1e8);
    background: var(--surface-2, #1b2028);
    font-family: Inter, "SF Pro Text", ui-sans-serif, system-ui, sans-serif;
    font-size: var(--font-size-body, 12px);
    font-weight: 600;
    line-height: 1.2;
    letter-spacing: 0;
    text-align: center;
    text-transform: none;
    white-space: nowrap;
    cursor: pointer;
    user-select: none;
    transition: transform .16s ease, color .16s ease, background-color .16s ease, border-color .16s ease, box-shadow .16s ease, opacity .16s ease;
  }
  :global(button.text-button:hover:not(:disabled)) {
    color: var(--text, #dbe1e8);
    border-color: color-mix(in srgb, var(--accent, #9ca3af) 48%, var(--border-strong, #3a424d));
    background: var(--surface-3, #242b34);
  }
  :global(button.text-button:focus-visible) {
    outline: 2px solid var(--accent, #9ca3af);
    outline-offset: 2px;
  }
  :global(button.text-button:active:not(:disabled)) { transform: translateY(1px); }
  :global(button.text-button:disabled) { opacity: .45; cursor: not-allowed; }
  :global(button.text-button.compact) {
    min-height: 25px;
    padding: 0 8px;
    border-radius: 3px;
    font-size: var(--font-size-compact, 10px);
  }
  .intercept-heading button.forward-action {
    box-sizing: border-box;
    width: 96px;
    color: #fff;
    border-color: var(--success, #279630);
    background: var(--success, #279630);
    font-weight: 700;
  }
  .intercept-heading button.forward-action:hover:not(:disabled) {
    color: #fff;
    border-color: color-mix(in srgb, var(--success, #279630) 82%, #fff);
    background: color-mix(in srgb, var(--success, #279630) 82%, #000);
  }
  .intercept-heading button.intercept-drop {
    box-sizing: border-box;
    width: 96px;
    color: #fff;
  }

  :global(button.text-button.primary-action),
  :global(button.text-button.accent),
  :global(button.text-button.run),
  :global(button.text-button.start),
  :global(button.text-button.save),
  :global(button.text-button.done),
  :global(button.text-button.save-button),
  :global(button.text-button.confirm),
  :global(button.text-button.confirm-folder) {
    color: var(--accent-contrast, #171a23);
    border-color: var(--accent, #9ca3af);
    background: var(--accent, #9ca3af);
    font-weight: 700;
    box-shadow: none;
  }
  :global(button.text-button.primary-action:hover:not(:disabled)),
  :global(button.text-button.accent:hover:not(:disabled)),
  :global(button.text-button.run:hover:not(:disabled)),
  :global(button.text-button.start:hover:not(:disabled)),
  :global(button.text-button.save:hover:not(:disabled)),
  :global(button.text-button.done:hover:not(:disabled)),
  :global(button.text-button.save-button:hover:not(:disabled)),
  :global(button.text-button.confirm:hover:not(:disabled)),
  :global(button.text-button.confirm-folder:hover:not(:disabled)) {
    color: var(--accent-contrast, #171a23);
    border-color: var(--accent-hover, #d1d5db);
    background: var(--accent-hover, #d1d5db);
    box-shadow: none;
  }
  :global(button.text-button.primary-action.success:not(:disabled)) {
    color: #fff;
    border-color: var(--success, #279630);
    background: var(--success, #279630);
  }
  :global(button.text-button.primary-action.success:hover:not(:disabled)) {
    color: #fff;
    border-color: color-mix(in srgb, var(--success, #279630) 82%, #fff);
    background: color-mix(in srgb, var(--success, #279630) 82%, #000);
  }
  :global(button.text-button.danger),
  :global(button.text-button.destructive),
  :global(button.text-button.delete-button),
  :global(button.text-button.delete-confirm),
  :global(button.text-button.stop) {
    color: #fff;
    border-color: var(--danger, #d92d4b);
    background: var(--danger, #d92d4b);
    font-weight: 700;
    box-shadow: none;
  }
  :global(button.text-button.danger:hover:not(:disabled)),
  :global(button.text-button.destructive:hover:not(:disabled)),
  :global(button.text-button.delete-button:hover:not(:disabled)),
  :global(button.text-button.delete-confirm:hover:not(:disabled)),
  :global(button.text-button.stop:hover:not(:disabled)) {
    color: #fff;
    border-color: color-mix(in srgb, var(--danger, #d92d4b) 82%, #fff);
    background: color-mix(in srgb, var(--danger, #d92d4b) 82%, #000);
  }
  .app-shell :global(.proxy-control-actions > button.text-button.primary-action:not(.danger)) {
    color: #fff;
    border-color: var(--success);
    background: var(--success);
  }
  .app-shell :global(.proxy-control-actions > button.text-button.primary-action:not(.danger):hover:not(:disabled)) {
    color: #fff;
    border-color: color-mix(in srgb, var(--success) 82%, #fff);
    background: color-mix(in srgb, var(--success) 82%, #000);
  }
  :global(.node-menu button.text-button) {
    width: 100%;
    justify-content: flex-start;
    min-height: 28px;
    padding: 0 8px;
    border-color: transparent;
    border-radius: 3px;
    background: transparent;
    text-align: left;
  }
  :global(.node-menu button.text-button:hover:not(:disabled)) {
    border-color: transparent;
    background: var(--surface-2, #242b34);
  }
  :global(.export-options button.text-button) { justify-content: flex-start; text-align: left; }

  @media (max-width: 900px) {
    .repeater-workspace { grid-template-columns: 1fr; grid-template-rows: 31px 52px 1fr 1fr; }
    .repeater-workspace [data-tour="replay-request-editor"],
    .repeater-workspace .repeater-response-panel,
    .repeater-workspace > :global(.message-viewer) { grid-column: 1; grid-row: auto; }
  }

  @media (prefers-reduced-motion: reduce) {
    :global(*) { transition: none !important; }
    .proxy-state.state-waiting svg circle:first-child { animation: none; }
  }
</style>
