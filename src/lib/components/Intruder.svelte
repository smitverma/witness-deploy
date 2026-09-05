<script lang="ts">
  import { untrack } from "svelte";
  import { commands } from "$lib/api";
  import { showErrorToast } from "$lib/errorToast";
  import { decodeHttpText, encodeHttpText, finalizeHttpRequest, HTTP_METADATA_LIMIT, normalizeHttpLineEndings, requestHeaderPrefix } from "$lib/http-message";
  import {
    MAX_INTRUDER_REQUESTS,
    clonePayloadWarehouse,
    createIntruderResultTab,
    createIntruderTab,
    createPayloadWarehouse,
    findTestPositions,
    generatePayloads,
    planPayloadRows,
    processPayloads,
    requestHost,
    requestHostText,
    synchronizeContentLength,
    renderTestRequestValues,
    renderTestRequestValuesWithRanges,
    finalizeRenderedTestRequest,
  } from "$lib/intruder";
  import type {
    IntruderResult,
    IntruderResultTab,
    IntruderScan,
    IntruderSession,
    IntruderState,
    IntruderTab,
    IntruderWorkspaceTab,
    TabGroup,
    PayloadWarehouse as PayloadWarehouseState,
  } from "$lib/types";
  import IntruderResults from "./IntruderResults.svelte";
  import DuplicateButton from "./DuplicateButton.svelte";
  import MessageViewer from "./MessageViewer.svelte";
  import PayloadWarehouse from "./PayloadWarehouse.svelte";
  import { highlightSearchText } from "$lib/text-search";
  import { buildTabBarEntries } from "$lib/tab-groups";

  type EditorSelection = { from: number; to: number; value: string };
  type FuzzSearchResult = { tab: IntruderTab; snippet: string };

  function isSetupTab(item: IntruderWorkspaceTab | undefined): item is IntruderTab {
    return item?.kind === "setup";
  }

  function isResultTab(item: IntruderWorkspaceTab | undefined): item is IntruderResultTab {
    return item?.kind === "result";
  }

  // highlightSearchText imported from $lib/text-search.
  type RequestMetadataCache = {
    tabId: number;
    headerPrefix: string | null;
    host: string;
  };

  let {
    state: model,
    theme = "dark",
    onSaveOrganizer,
    onSendReplay,
    onSendDecoder,
    onStateChange,
    tabGroups = [],
    onTabContextMenu,
    onTabGroupContextMenu,
    onToggleTabGroup,
    onCloseTab,
    onScanCreated,
    onScanUpdated,
    lifecycleToken = 0,
    projectTransitioning = false,
    launchRequest = null,
    onLaunchRequestHandled,
  }: {
    state: IntruderState;
    theme?: "dark" | "light";
    onSaveOrganizer?: (
      request: Uint8Array,
      response: Uint8Array,
      tls: boolean,
    ) => void;
    onSendReplay?: (request: Uint8Array, tls: boolean) => void;
    onSendDecoder?: (value: string) => void;
    onStateChange?: () => void;
    tabGroups?: TabGroup[];
    onTabContextMenu?: (event: MouseEvent, tabId: number) => void;
    onTabGroupContextMenu?: (event: MouseEvent, groupId: string) => void;
    onToggleTabGroup?: (groupId: string) => void;
    onCloseTab?: (tabId: number) => void;
    onScanCreated?: (sourceTabId: number, scan: IntruderScan) => void | Promise<void>;
    onScanUpdated?: (sourceTabId: number, scan: IntruderScan) => void | Promise<void>;
    lifecycleToken?: number;
    projectTransitioning?: boolean;
    launchRequest?: { id: string; tabId: number; action?: "start" | "resume"; scanId?: string } | null;
    onLaunchRequestHandled?: (id: string) => void;
  } = $props();

  let editorSelection = $state<EditorSelection>({ from: 0, to: 0, value: "" });
  let launching = $state(false);
  const sessionOperations = new Map<string, Promise<void>>();
  let lastControllerLaunchId = "";
  let showScanModal = $state(false);
  let showSearchModal = $state(false);
  let searchDialogElement = $state<HTMLElement | null>(null);
  let searchOpener = $state<HTMLElement | null>(null);
  let searchQuery = $state("");
  let tabScrollElement = $state<HTMLElement | null>(null);
  let tabsCanScrollRight = $state(false);
  let reportedPositionError = $state("");
  const modeOptions = [
    { value: "single", label: "Single" },
    { value: "spread", label: "Spread" },
    { value: "map", label: "Map" },
    { value: "combine", label: "Combine" },
  ] as const;
  const activeEntry = $derived(
    model.tabs.find((item) => item.id === model.activeTabId) ?? model.tabs.find(isSetupTab),
  );
  const tab = $derived.by(() => {
    if (isSetupTab(activeEntry)) return activeEntry;
    if (isResultTab(activeEntry)) {
      return model.tabs.find((item): item is IntruderTab => item.kind === "setup" && item.id === activeEntry.sourceTabId);
    }
    return model.tabs.find(isSetupTab);
  });
  const tabBarEntries = $derived.by(() => buildTabBarEntries(model.tabs, tabGroups));
  // Searching decodes every tab's full request body, so debounce the query:
  // typing stays instant, results fill in after pause.
  let debouncedSearchQuery = $state("");
  $effect(() => {
    const next = searchQuery;
    const timer = setTimeout(() => {
      debouncedSearchQuery = next;
    }, 150);
    return () => clearTimeout(timer);
  });
  const searchResults = $derived.by((): FuzzSearchResult[] => {
    const query = debouncedSearchQuery.trim().toLocaleLowerCase();
    if (!query) return [];
    return model.tabs.filter(isSetupTab).flatMap((item) => {
      const searchable = requestText(item);
      const matchIndex = searchable.toLocaleLowerCase().indexOf(query);
      if (matchIndex < 0) return [];
      const start = Math.max(0, matchIndex - 48);
      const end = Math.min(searchable.length, matchIndex + query.length + 92);
      const snippet = searchable.slice(start, end).replace(/\s+/g, " ").trim();
      return [{ tab: item, snippet: `${start > 0 ? "…" : ""}${snippet}${end < searchable.length ? "…" : ""}` }];
    });
  });
  let requestMetadataCache = $state<RequestMetadataCache | null>(null);
  const requestMetadata = $derived(
    tab && requestMetadataCache?.tabId === tab.id
      ? requestMetadataCache.host
      : "",
  );
  const activeScan = $derived.by(() => {
    if (isResultTab(activeEntry)) {
      return tab?.scans.find((scan) => scan.session.id === activeEntry.scanId) ?? null;
    }
    return tab?.scans.find((scan) => scan.session.id === tab.activeScanId) ?? null;
  });
  const positionSummary = $derived.by(() => {
    if (!tab) return { count: 0, positions: [], error: "" };
    try {
      const positions = findTestPositions(
        requestText(tab),
      );
      return { count: positions.length, positions, error: "" };
    } catch (reason) {
      return { count: 0, positions: [], error: String(reason) };
    }
  });
  const positionOptions = $derived(
    positionSummary.positions.map((position, index) => ({
      number: index + 1,
      text: position.original,
    })),
  );
  const usesPositionWarehouses = $derived(
    tab?.mode === "map" || tab?.mode === "combine",
  );
  const activeWarehouse = $derived(
    usesPositionWarehouses
      ? (tab?.positionWarehouses[tab.selectedPayloadPosition] ?? tab?.warehouse)
      : tab?.warehouse,
  );
  // Generation is capped at MAX_INTRUDER_REQUESTS (throws fast on
  // brute-force explosions), so this derived stays cheap (<10ms) and keeps
  // exact deep-reactivity deps. launchTest() recomputes its own plan anyway.
  const planSummary = $derived.by(() => {
    if (!tab) return { rows: [], repeatIndefinitely: false, error: "" };
    try {
      const warehouses = usesPositionWarehouses
        ? tab.positionWarehouses.slice(0, positionSummary.count)
        : [tab.warehouse];
      const sets = warehouses.map((warehouse) => generatePayloads(warehouse));
      return {
        ...planPayloadRows(tab.mode, sets, positionSummary.count),
        error: "",
      };
    } catch (reason) {
      return {
        rows: [],
        repeatIndefinitely: false,
        error: reason instanceof Error ? reason.message : String(reason),
      };
    }
  });
  const totalRequests = $derived(
    planSummary.repeatIndefinitely ? null : planSummary.rows.length,
  );

  let saveTimer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    // JSON.stringify deliberately walks the deeply reactive model so changes
    // inside payload warehouses and result rows trigger the workspace saver.
    // Debounced so typing in the fuzz editor stays instant and tab switches
    // never serialize multi-MB scan results synchronously.
    JSON.stringify(model, (_key, value) => value instanceof Uint8Array ? `bytes:${value.length}` : value);
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => onStateChange?.(), 500);
    return () => clearTimeout(saveTimer);
  });

  $effect(() => {
    const item = tab;
    if (!item) return;
    const raw = item.request;
    untrack(() => updateRequestMetadata(item.id, decodeHttpText(raw.slice(0, HTTP_METADATA_LIMIT))));
  });

  $effect(() => {
    if (!tab) return;
    while (tab.positionWarehouses.length < positionSummary.count) {
      tab.positionWarehouses.push(createPayloadWarehouse());
    }
    if (
      positionSummary.count > 0 &&
      tab.selectedPayloadPosition >= positionSummary.count
    ) {
      tab.selectedPayloadPosition = positionSummary.count - 1;
    }
    if (positionSummary.count === 0) tab.selectedPayloadPosition = 0;
  });

  $effect(() => {
    if (
      positionSummary.error &&
      positionSummary.error !== reportedPositionError
    ) {
      showErrorToast(positionSummary.error);
    }
    reportedPositionError = positionSummary.error;
  });

  $effect(() => {
    const request = launchRequest;
    if (!request || request.id === lastControllerLaunchId) return;
    lastControllerLaunchId = request.id;
    const target = model.tabs.find((item): item is IntruderTab => item.kind === "setup" && item.id === request.tabId);
    if (!target) {
      onLaunchRequestHandled?.(request.id);
    } else if (request.action === "resume") {
      const scan = target.scans.find((item) => item.session.id === request.scanId);
      if (scan) {
        resumeScan(scan);
        onLaunchRequestHandled?.(request.id);
      } else {
        void launchTest(target).finally(() => onLaunchRequestHandled?.(request.id));
      }
    } else {
      void launchTest(target).finally(() => onLaunchRequestHandled?.(request.id));
    }
  });

  function reportTabError(item: IntruderTab, reason: unknown) {
    item.error = reason instanceof Error ? reason.message : String(reason);
    showErrorToast(item.error);
  }

  function reportScanError(scan: IntruderScan, reason: unknown) {
    scan.error = reason instanceof Error ? reason.message : String(reason);
    showErrorToast(scan.error);
  }

  function reportScanPersistenceError(scan: IntruderScan, reason: unknown) {
    scan.persistenceError = reason instanceof Error ? reason.message : String(reason);
    showErrorToast(scan.persistenceError);
  }

  async function persistScanUpdate(sourceTabId: number, scan: IntruderScan) {
    try {
      await onScanUpdated?.(sourceTabId, scan);
      scan.persistenceError = "";
    } catch (reason) {
      reportScanPersistenceError(scan, reason);
    }
  }

  function requestText(item: IntruderTab) {
    return model.editorDraft?.tabId === item.id
      ? model.editorDraft.value
      : decodeHttpText(item.request);
  }

  function updateRequestMetadata(tabId: number, value: string) {
    const cached = requestMetadataCache;
    const headerUnchanged = cached?.tabId === tabId && (
      cached.headerPrefix === null
        ? value.length === 0
        : value.startsWith(cached.headerPrefix)
    );
    if (headerUnchanged) return;
    requestMetadataCache = {
      tabId,
      headerPrefix: requestHeaderPrefix(value),
      host: requestHostText(value),
    };
  }

  function updateRequestText(value: string) {
    if (tab) {
      model.editorDraft = { tabId: tab.id, value };
      updateRequestMetadata(tab.id, value);
    }
  }

  function flushEditorDraft(item: IntruderTab | undefined = tab) {
    const draft = model.editorDraft;
    if (!draft) return;
    const target = model.tabs.find((candidate): candidate is IntruderTab => candidate.kind === "setup" && candidate.id === draft.tabId);
    if (target) {
      updateRequestMetadata(target.id, draft.value);
      target.request = encodeHttpText(draft.value);
    }
    model.editorDraft = null;
    if (item && item.id === draft.tabId) editorSelection = { ...editorSelection, value: draft.value };
  }

  function updateTabScrollState() {
    const element = tabScrollElement;
    tabsCanScrollRight = Boolean(element && element.scrollLeft + element.clientWidth < element.scrollWidth - 1);
  }

  function scrollTabsRight() {
    tabScrollElement?.scrollBy({ left: 120, behavior: "smooth" });
  }

  $effect(() => {
    tabBarEntries.map((entry) => entry.kind === "tab"
      ? entry.tab.title
      : `${entry.group.name}:${entry.group.collapsed}:${entry.tabs.map((item) => item.title).join(",")}`).join("|");
    model.tabs.length;
    const element = tabScrollElement;
    if (!element) return;
    updateTabScrollState();
    const observer = new ResizeObserver(updateTabScrollState);
    observer.observe(element);
    return () => observer.disconnect();
  });

  function closeSearch() {
    if (!showSearchModal) return;
    showSearchModal = false;
    searchQuery = "";
    const opener = searchOpener;
    searchOpener = null;
    window.requestAnimationFrame(() => {
      if (opener?.isConnected) opener.focus();
    });
  }

  function openSearch(event?: Event) {
    searchOpener = event?.currentTarget instanceof HTMLElement
      ? event.currentTarget
      : document.activeElement instanceof HTMLElement ? document.activeElement : null;
    showSearchModal = true;
    showScanModal = false;
  }

  $effect(() => {
    if (!showSearchModal) return;
    window.requestAnimationFrame(() => {
      searchDialogElement?.querySelector<HTMLInputElement>("input")?.focus();
    });
  });

  function openSearchResult(tabId: number) {
    selectTab(tabId);
    closeSearch();
  }

  function selectTab(id: number) {
    flushEditorDraft();
    model.activeTabId = id;
    editorSelection = { from: 0, to: 0, value: "" };
    showScanModal = false;
  }

  function createTab(request = new Uint8Array(), tls = true) {
    flushEditorDraft();
    const next = createIntruderTab(model.nextTabId++, request, tls);
    model.tabs.push(next);
    model.activeTabId = next.id;
    editorSelection = { from: 0, to: 0, value: "" };
    showScanModal = false;
  }

  function duplicateTab() {
    if (!tab) return;
    flushEditorDraft();
    const next = createIntruderTab(model.nextTabId++, tab.request, tab.tls);
    next.mode = tab.mode;
    next.scanName = tab.scanName;
    next.warehouse = clonePayloadWarehouse(tab.warehouse);
    next.positionWarehouses = tab.positionWarehouses.map(clonePayloadWarehouse);
    next.selectedPayloadPosition = tab.selectedPayloadPosition;
    model.tabs.push(next);
    model.activeTabId = next.id;
    editorSelection = { from: 0, to: 0, value: "" };
    showScanModal = false;
  }

  function closeTab(item: IntruderWorkspaceTab) {
    if (onCloseTab) {
      onCloseTab(item.id);
      return;
    }
    if (item.kind === "result") {
      const index = model.tabs.findIndex((candidate) => candidate.id === item.id);
      if (index >= 0) model.tabs.splice(index, 1);
      if (model.activeTabId === item.id) {
        model.activeTabId = model.tabs[Math.max(0, index - 1)]?.id ?? model.tabs[0]?.id ?? 0;
      }
      return;
    }
    if (item.scans.some((scan) => scan.running)) {
      reportTabError(item, "Stop running scans before closing this tab.");
      model.activeTabId = item.id;
      return;
    }
    const linkedResults = model.tabs.filter(
      (candidate) => candidate.kind === "result" && candidate.sourceTabId === item.id,
    );
    if (linkedResults.length) {
      reportTabError(item, "Close this tab's result tabs before closing the setup tab.");
      model.activeTabId = item.id;
      return;
    }
    if (model.tabs.length === 1) {
      if (item.id === model.editorDraft?.tabId) flushEditorDraft(item);
      const replacement = createIntruderTab(item.id);
      model.tabs.splice(0, 1, replacement);
      model.activeTabId = replacement.id;
      return;
    }
    if (item.id === model.editorDraft?.tabId) flushEditorDraft(item);
    const index = model.tabs.findIndex((candidate) => candidate.id === item.id);
    model.tabs.splice(index, 1);
    if (model.activeTabId === item.id) {
      model.activeTabId = model.tabs[Math.max(0, index - 1)]?.id ?? model.tabs[0]?.id ?? 0;
    }
  }

  function setSelection(
    selection: { from: number; to: number },
    value: string,
  ) {
    editorSelection = { ...selection, value };
  }

  function addPosition() {
    if (!tab) return;
    const { from, to, value } = editorSelection;
    if (from === to) {
      reportTabError(
        tab,
        "Select request text first, then choose Add position.",
      );
      return;
    }
    const marked = `${value.slice(0, from)}§${value.slice(from, to)}§${value.slice(to)}`;
    tab.request = encodeHttpText(marked);
    model.editorDraft = null;
    editorSelection = { from: 0, to: 0, value: marked };
    tab.error = "";
  }

  function clearPositions() {
    if (!tab) return;
    const template = requestText(tab);
    try {
      tab.request = encodeHttpText(
        normalizeHttpLineEndings(synchronizeContentLength(template.replaceAll("§", ""))),
      );
      model.editorDraft = null;
      editorSelection = { from: 0, to: 0, value: "" };
      tab.error = "";
    } catch (reason) {
      reportTabError(tab, reason);
    }
  }

  async function validateSession(item: IntruderTab): Promise<IntruderSession> {
    const template = finalizeHttpRequest(requestText(item));
    item.request = encodeHttpText(template);
    model.editorDraft = null;
    const positions = findTestPositions(template);
    if (item.mode === "single") {
      if (positions.length === 0 && item.warehouse.type !== "null") {
        throw new Error("Add one §value position§ to the request");
      }
      if (positions.length > 1) {
        throw new Error("Single mode accepts one marked position");
      }
    } else if (item.mode === "spread") {
      if (!positions.length)
        throw new Error("Spread mode needs at least one marked position");
    } else if (positions.length < 2) {
      throw new Error("Map and Combine modes need at least two marked positions");
    }
    const warehouses: PayloadWarehouseState[] =
      item.mode === "map" || item.mode === "combine"
        ? item.positionWarehouses.slice(0, positions.length)
        : [item.warehouse];
    if (
      warehouses.length !==
      (item.mode === "map" || item.mode === "combine"
        ? positions.length
        : 1)
    ) {
      throw new Error("Every marked position needs a warehouse configuration");
    }
    const sets = [];
    for (const warehouse of warehouses) {
      const generated = generatePayloads(warehouse);
      const values = await processPayloads(
        generated.payloads,
        warehouse.processing,
      );
      sets.push({
        payloads: values,
        repeatIndefinitely: generated.repeatIndefinitely,
      });
    }
    const plan = planPayloadRows(item.mode, sets, positions.length);
    const count = plan.repeatIndefinitely ? null : plan.rows.length;
    if (count !== null && count > MAX_INTRUDER_REQUESTS) {
      throw new Error(
        `This test is limited to ${MAX_INTRUDER_REQUESTS.toLocaleString()} requests per run`,
      );
    }
    if (positions.length)
      renderTestRequestValues(template, positions, plan.rows[0]);
    else synchronizeContentLength(template);

    return {
      id: `${Date.now()}-${crypto.randomUUID().slice(0, 8)}`,
      template: Array.from(item.request),
      tls: item.tls,
      mode: item.mode,
      payloadRows: plan.rows,
      totalRequests: count,
      repeatIndefinitely: plan.repeatIndefinitely,
      theme,
    };
  }

  async function launchTest(item: IntruderTab | undefined = tab) {
    if (!item || launching || projectTransitioning) return;
    item.error = "";
    let session: IntruderSession;
    try {
      if (!item.scanName.trim()) throw new Error("Scan Name is required");
      session = await validateSession(item);
    } catch (reason) {
      reportTabError(item, reason);
      return;
    }

    launching = true;
    const scan: IntruderScan = {
      name: item.scanName.trim(),
      session,
      startedAt: new Date().toISOString(),
      completedAt: null,
      running: true,
      stopped: false,
      stopRequested: false,
      currentRequestId: null,
      nextPayloadIndex: 0,
      results: [],
      selectedResultId: null,
      error: "",
      persistenceError: "",
    };
    item.scanName = scan.name;
    item.scans.unshift(scan);
    const reactiveScan = item.scans[0];
    item.activeScanId = session.id;
    const resultTab = createIntruderResultTab(model.nextTabId++, item.id, reactiveScan);
    model.tabs.push(resultTab);
    model.activeTabId = resultTab.id;
    launching = false;
    try {
      await onScanCreated?.(item.id, reactiveScan);
    } catch (reason) {
      reportScanError(reactiveScan, reason);
      reactiveScan.running = false;
      reactiveScan.stopped = true;
      reactiveScan.completedAt = new Date().toISOString();
      await persistScanUpdate(item.id, reactiveScan);
      return;
    }
    if (projectTransitioning) {
      reactiveScan.running = false;
      reactiveScan.stopped = true;
      reactiveScan.completedAt = new Date().toISOString();
      await persistScanUpdate(item.id, reactiveScan);
      reportScanError(reactiveScan, "Scan cancelled because a project transition is in progress");
      return;
    }
    void startSession(reactiveScan, item.id);
  }

  async function runSession(scan: IntruderScan, sourceTabId: number) {
    const runLifecycleToken = lifecycleToken;
    const template = decodeHttpText(
      new Uint8Array(scan.session.template),
    );
    let positions;
    try {
      positions = findTestPositions(template);
    } catch (reason) {
      reportScanError(scan, reason);
      scan.running = false;
      scan.completedAt = new Date().toISOString();
      await persistScanUpdate(sourceTabId, scan);
      return;
    }

    let sequence = scan.results.reduce((highest, result) => Math.max(highest, result.sequence), 0);
    const sendOne = async (values: string[], payloadIndex: number) => {
      if (runLifecycleToken !== lifecycleToken) {
        scan.stopRequested = true;
        return;
      }
      sequence += 1;
      const rendered = positions.length
        ? renderTestRequestValuesWithRanges(template, positions, values)
        : { value: synchronizeContentLength(template), ranges: [] };
      const finalized = finalizeRenderedTestRequest(rendered);
      const request = encodeHttpText(finalized.value);
      const modifiedRanges = finalized.ranges;
      const requestId = `${scan.session.id}-${sequence}`;
      scan.currentRequestId = requestId;
      const started = performance.now();
      let result: IntruderResult;
      try {
        const response = await commands.sendRepeaterRequest(
          requestId,
          request,
          scan.session.tls,
        );
        result = {
          id: requestId,
          sequence,
          position:
            scan.session.mode === "single" && positions.length ? 1 : null,
          payload: values.join(" | "),
          payloads: [...values],
          modifiedRanges: modifiedRanges.map((range) => ({ ...range })),
          status: response.status,
          length: response.size,
          durationMs: response.durationMs,
          error: "",
          request,
          response: new Uint8Array(response.raw),
        };
      } catch (reason) {
        if (scan.stopRequested) return;
        result = {
          id: requestId,
          sequence,
          position:
            scan.session.mode === "single" && positions.length ? 1 : null,
          payload: values.join(" | "),
          payloads: [...values],
          modifiedRanges: modifiedRanges.map((range) => ({ ...range })),
          status: null,
          length: 0,
          durationMs: Math.round(performance.now() - started),
          error: reason instanceof Error ? reason.message : String(reason),
          request,
          response: new Uint8Array(),
        };
      }
      if (runLifecycleToken !== lifecycleToken) {
        scan.stopRequested = true;
        return;
      }
      scan.results.push(result);
      scan.selectedResultId ??= result.id;
      scan.nextPayloadIndex = payloadIndex + 1;
    };

    try {
      if (scan.session.repeatIndefinitely) {
        while (!scan.stopRequested) {
          await sendOne(scan.session.payloadRows[0] ?? [], scan.nextPayloadIndex);
        }
      } else {
        for (let index = scan.nextPayloadIndex; index < scan.session.payloadRows.length; index += 1) {
          if (scan.stopRequested) return;
          await sendOne(scan.session.payloadRows[index], index);
        }
      }
    } catch (reason) {
      reportScanError(scan, reason);
    } finally {
      scan.currentRequestId = null;
      scan.stopped = scan.stopRequested;
      scan.completedAt = new Date().toISOString();
      await persistScanUpdate(sourceTabId, scan);
      scan.stopRequested = false;
      scan.running = false;
    }
  }

  function startSession(scan: IntruderScan, sourceTabId: number) {
    const operation = runSession(scan, sourceTabId);
    sessionOperations.set(scan.session.id, operation);
    void operation
      .catch((reason) => showErrorToast(reason))
      .finally(() => {
        if (sessionOperations.get(scan.session.id) === operation) {
          sessionOperations.delete(scan.session.id);
        }
      });
    return operation;
  }

  async function stopScan(scan: IntruderScan) {
    if (!scan.running || scan.stopRequested) return;
    scan.stopRequested = true;
    scan.stopped = true;
    for (let attempt = 0; attempt < 4; attempt += 1) {
      const requestId = scan.currentRequestId;
      if (!requestId) break;
      try {
        if (await commands.cancelRepeaterRequest(requestId)) break;
      } catch (reason) {
        reportScanError(scan, reason);
        break;
      }
      await new Promise<void>((resolve) => window.setTimeout(resolve, 25));
    }
    await sessionOperations.get(scan.session.id)?.catch((reason) => showErrorToast(reason));
  }

  function resultTabForScan(sourceTabId: number, scanId: string) {
    return model.tabs.find(
      (item): item is IntruderResultTab =>
        item.kind === "result" && item.sourceTabId === sourceTabId && item.scanId === scanId,
    );
  }

  function activateResultTab(source: IntruderTab, scan: IntruderScan) {
    let resultTab = resultTabForScan(source.id, scan.session.id);
    if (!resultTab) {
      resultTab = createIntruderResultTab(model.nextTabId++, source.id, scan);
      model.tabs.push(resultTab);
    }
    resultTab.title = scan.name;
    model.activeTabId = resultTab.id;
    showScanModal = false;
  }

  function resumeScan(scan: IntruderScan) {
    if (!tab || scan.running || scan.stopRequested || scan.currentRequestId || projectTransitioning) return;
    if (!scan.session.repeatIndefinitely && scan.nextPayloadIndex >= scan.session.payloadRows.length) return;
    scan.error = "";
    scan.stopped = false;
    scan.stopRequested = false;
    scan.currentRequestId = null;
    scan.running = true;
    scan.completedAt = null;
    tab.activeScanId = scan.session.id;
    activateResultTab(tab, scan);
    void startSession(scan, tab.id);
  }

  function openScan(scan: IntruderScan) {
    if (!tab) return;
    tab.activeScanId = scan.session.id;
    activateResultTab(tab, scan);
  }

  function scanCanResume(scan: IntruderScan) {
    return !scan.running && (scan.session.repeatIndefinitely || scan.nextPayloadIndex < scan.session.payloadRows.length);
  }

  function tabHasRunningScan(item: IntruderWorkspaceTab) {
    if (item.kind === "setup") return item.scans.some((scan) => scan.running);
    return model.tabs
      .find((candidate): candidate is IntruderTab => candidate.kind === "setup" && candidate.id === item.sourceTabId)
      ?.scans.some((scan) => scan.session.id === item.scanId && scan.running) ?? false;
  }

  function scanTarget(scan: IntruderScan) {
    return (
      requestHost(new Uint8Array(scan.session.template)) || "Unknown target"
    );
  }

  function scanTime(value: string) {
    return new Intl.DateTimeFormat(undefined, {
      dateStyle: "medium",
      timeStyle: "short",
    }).format(new Date(value));
  }

  export function handleShortcut(action: string): boolean {
    if (!tab) return false;
    if (action === "transient.close") {
      if (showSearchModal) {
        closeSearch();
        return true;
      }
      if (!showScanModal) return false;
      showScanModal = false;
      return true;
    }
    if (action === "fuzz.search") {
      openSearch();
      return true;
    }
    if (action === "fuzz.launch") {
      if (activeEntry?.kind !== "setup" || launching || activeScan?.running) return false;
      if (activeScan && activeScan.stopped && (activeScan.session.repeatIndefinitely || activeScan.nextPayloadIndex < activeScan.session.payloadRows.length)) resumeScan(activeScan);
      else void launchTest(tab);
      return true;
    }
    if (action === "fuzz.stop") {
      if (!activeScan?.running) return false;
      void stopScan(activeScan);
      return true;
    }
    if (action === "fuzz.results") {
      if (activeEntry?.kind !== "setup" || !tab.scans.length) return false;
      if (activeScan) openScan(activeScan);
      else showScanModal = true;
      return true;
    }
    if (action === "fuzz.newTab") {
      createTab();
      return true;
    }
    if (action === "fuzz.duplicateTab") {
      duplicateTab();
      return true;
    }
    return false;
  }
</script>

<section class="intruder-shell" aria-label="Fuzz">
  <div class="intruder-tabs" role="tablist" aria-label="Fuzz tabs">
    <div class="intruder-tab-viewport">
      <div class="intruder-tab-scroll" bind:this={tabScrollElement} onscroll={updateTabScrollState}>
        <div class="intruder-tab-strip">
        {#each tabBarEntries as entry (entry.kind === "tab" ? "tab-" + entry.tab.id : "group-" + entry.group.id)}
      {#if entry.kind === "group"}
        <div class="intruder-tab-group" class:collapsed={entry.group.collapsed}>
          <button
            class="tab-group-marker"
            style={"--tab-group-color:" + entry.group.color}
            aria-label={(entry.group.collapsed ? "Expand " : "Collapse ") + entry.group.name + " tab group"}
            aria-expanded={!entry.group.collapsed}
            onclick={() => onToggleTabGroup?.(entry.group.id)}
            oncontextmenu={(event) => { event.preventDefault(); onTabGroupContextMenu?.(event, entry.group.id); }}
          >
            <span class="tab-group-color" aria-hidden="true"></span>
            <span>{entry.group.name}</span>
            <small>{entry.tabs.length}</small>
            <span class="tab-group-chevron" aria-hidden="true">{entry.group.collapsed ? "▸" : "▾"}</span>
          </button>
          {#if !entry.group.collapsed}
            <div class="intruder-group-tabs" style={`--tab-group-color:${entry.group.color}`}>
              {#each entry.tabs as item (item.id)}
                <div role="presentation" class:active={item.id === model.activeTabId} class="intruder-tab" onclick={() => selectTab(item.id)} oncontextmenu={(event) => { event.preventDefault(); onTabContextMenu?.(event, item.id); }}>
                  <button
                    role="tab"
                    aria-selected={item.id === model.activeTabId}
                    onclick={(event) => { event.stopPropagation(); selectTab(item.id); }}>{item.title}</button
                  >
                  {#if tabHasRunningScan(item)}<span
                      class="running-dot"
                      data-tooltip="Scan running"
                    ></span>{/if}
                  <button
                    class="tab-close-button"
                    class:inactive={item.id !== model.activeTabId}
                    aria-hidden={item.id !== model.activeTabId}
                    tabindex={item.id === model.activeTabId ? 0 : -1}
                    aria-label={"Close " + item.title}
                    data-tooltip={"Close " + item.title}
                    onclick={(event) => { event.stopPropagation(); closeTab(item); }}>×</button
                  >
                </div>
              {/each}
            </div>
          {/if}
        </div>
      {:else}
        {@const item = entry.tab}
        <div role="presentation" class:active={item.id === model.activeTabId} class="intruder-tab" onclick={() => selectTab(item.id)} oncontextmenu={(event) => { event.preventDefault(); onTabContextMenu?.(event, item.id); }}>
          <button
            role="tab"
            aria-selected={item.id === model.activeTabId}
            onclick={(event) => { event.stopPropagation(); selectTab(item.id); }}>{item.title}</button
          >
          {#if tabHasRunningScan(item)}<span
              class="running-dot"
              data-tooltip="Scan running"
            ></span>{/if}
          <button
            class="tab-close-button"
            class:inactive={item.id !== model.activeTabId}
            aria-hidden={item.id !== model.activeTabId}
            tabindex={item.id === model.activeTabId ? 0 : -1}
            aria-label={"Close " + item.title}
            onclick={(event) => { event.stopPropagation(); closeTab(item); }}>×</button
          >
        </div>
      {/if}
    {/each}
        </div>
      </div>
      {#if tabsCanScrollRight}
        <button class="tab-scroll-indicator" type="button" aria-label="Scroll to more Fuzz tabs" data-tooltip="More tabs" onclick={scrollTabsRight}>&gt;</button>
      {/if}
    </div>
    <div class="intruder-tab-actions" aria-label="Fuzz tab actions">
      <button class="icon-button tab-search-button" type="button" style="--svgbuttonsize:28px" data-tooltip="Search Fuzz tabs" aria-label="Search Fuzz tabs" onclick={openSearch}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="10.8" cy="10.8" r="6.3"></circle><path d="m16 16 4.5 4.5"></path></svg>
      </button>
      <button
        class="new-tab"
        type="button"
        data-tooltip="New Fuzz tab"
        aria-label="New Fuzz tab"
        onclick={() => createTab()}>+</button>
    </div>
  </div>

  <div class="intruder-content">
    {#if isResultTab(activeEntry) && activeScan}
      <IntruderResults
        scan={activeScan}
        onStop={() => void stopScan(activeScan)}
        onResume={() => resumeScan(activeScan)}
        {onSaveOrganizer}
        {onSendReplay}
        {onSendDecoder}
      />
    {:else if tab && activeEntry?.kind === "setup"}
      <section
        class="intruder-workspace"
        aria-label="Test configuration workspace"
      >
        <header class="intruder-toolbar">
          <div class="intruder-toolbar-left">
            <label>
              <span>Mode</span>
              <select bind:value={tab.mode} disabled={launching}>
                {#each modeOptions as option}<option value={option.value}
                    >{option.label}</option
                  >{/each}
              </select>
            </label>
            <label>
              <span>Target</span>
              <select
                value={tab.tls ? "https" : "http"}
                disabled={launching}
                onchange={(event) =>
                  (tab.tls = event.currentTarget.value === "https")}
              >
                <option value="https">HTTPS</option>
                <option value="http">HTTP</option>
              </select>
            </label>
          </div>

          <div class="intruder-toolbar-middle">
            <span class="summary">
              {positionSummary.count} positions ·
              {planSummary.repeatIndefinitely
                ? "continuous"
                : `${planSummary.rows.length} value rows`} ·
              {totalRequests === null
                ? "until stopped"
                : `${totalRequests} requests`}
            </span>
          </div>

          <div class="intruder-toolbar-right">
            <DuplicateButton label="Duplicate Fuzz tab" onclick={duplicateTab} />
            <button class="text-button view-results" onclick={() => (showScanModal = true)}>
              Results{tab.scans.filter((scan) => scan.results.length > 0).length
                ? ` (${tab.scans.filter((scan) => scan.results.length > 0).length})`
                : ""}
            </button>
            <button
              class="text-button start"
              disabled={!requestText(tab).length || launching}
              onclick={() => void launchTest()}
            >
              {launching ? "Preparing…" : "Launch"}
            </button>
          </div>
        </header>

        <div class="intruder-setup">
          <div class="request-pane">
            <div class="pane-title">
              <span>Request template</span>
              <div>
                <button
                  disabled={launching ||
                    editorSelection.from === editorSelection.to}
                  class="text-button compact"
                  onclick={addPosition}>Add §</button
                >
                <button
                  disabled={launching || positionSummary.count === 0}
                  class="text-button compact"
                  onclick={clearPositions}>Clear §</button
                >
              </div>
            </div>
            <MessageViewer
              title="Request"
              kind="request"
              raw={tab.request}
              metadata={requestMetadata}
              editable={!launching}
              normalizeRequest
              onDuplicate={duplicateTab}
              onSendReplay={onSendReplay ? (raw) => onSendReplay?.(raw, tab.tls) : undefined}
              onSendDecoder={onSendDecoder}
              onTextChange={updateRequestText}
              onSelectionChange={setSelection}
              onSaveOrganizer={(raw) =>
                onSaveOrganizer?.(raw, new Uint8Array(), tab.tls)}
            />
          </div>

          <div class="payload-pane">
            <label class="scan-name-field">
              <span>Scan Name</span>
              <input
                type="text"
                bind:value={tab.scanName}
                disabled={launching}
                placeholder="Name this scan"
                autocomplete="off"
              />
            </label>
            {#if activeWarehouse}
              <PayloadWarehouse
                warehouse={activeWarehouse}
                disabled={launching}
                positions={positionOptions}
                bind:selectedPosition={tab.selectedPayloadPosition}
              />
            {/if}
          </div>
        </div>
      </section>
    {/if}
  </div>

  {#if showSearchModal}
    <div
      class="modal-backdrop"
      role="presentation"
      onclick={(event) => {
        if (event.target === event.currentTarget) closeSearch();
      }}
    >
      <div
        bind:this={searchDialogElement}
        class="search-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="fuzz-search-title"
      >
        <header>
          <div>
            <p class="search-eyebrow">FUZZ</p>
            <h2 id="fuzz-search-title">Search tabs</h2>
          </div>
          <button class="close-modal" type="button" aria-label="Close Fuzz tab search" onclick={closeSearch}>×</button>
        </header>
        <label class="search-field" for="fuzz-search-input">
          <span>Search request content</span>
          <input id="fuzz-search-input" bind:value={searchQuery} autocomplete="off" placeholder="Search across Fuzz tabs…" />
        </label>
        <div class="search-results" aria-live="polite">
          {#if !debouncedSearchQuery.trim()}
            <p class="search-empty">Type to search request content across Fuzz tabs.</p>
          {:else if searchResults.length}
            {#each searchResults as result (result.tab.id)}
              {@const highlightedSnippet = highlightSearchText(result.snippet, debouncedSearchQuery)}
              <button class="search-result" type="button" onclick={() => openSearchResult(result.tab.id)}>
                <span class="search-snippet">{#each highlightedSnippet as part}{#if part.match}<strong>{part.text}</strong>{:else}{part.text}{/if}{/each}</span>
                <span class="search-tab-badge">{result.tab.title}</span>
              </button>
            {/each}
          {:else}
            <p class="search-empty">No Fuzz tabs contain “{debouncedSearchQuery.trim()}”.</p>
          {/if}
        </div>
      </div>
    </div>
  {/if}

  {#if showScanModal && tab}
    <div
      class="modal-backdrop"
      role="presentation"
      onclick={(event) => {
        if (event.target === event.currentTarget) showScanModal = false;
      }}
    >
      <div
        class="scan-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="scan-modal-title"
      >
        <header>
          <div>
            <h2 id="scan-modal-title">{tab.title} results</h2>
            <p>Running and past scans for this tab</p>
          </div>
          <button
            class="close-modal"
            aria-label="Close results list"
            onclick={() => (showScanModal = false)}>×</button
          >
        </header>
        <div class="scan-list">
          {#each tab.scans as scan (scan.session.id)}
            <button class="scan-row" onclick={() => openScan(scan)}>
              <span
                class:running={scan.running}
                class:paused={!scan.running && !scan.error && scan.stopped && scanCanResume(scan)}
                class:stopped={!scan.running && (Boolean(scan.error) || (scan.stopped && !scanCanResume(scan)))}
                class="scan-status"
                aria-hidden="true"
              ></span>
              <span class="scan-details"
                ><strong>{scan.name}</strong><small
                  >{scanTarget(scan)} · {scanTime(scan.startedAt)}</small
                ></span
              >
              <span class="scan-progress">
                <strong
                  >{scan.running
                    ? "Running"
                    : scan.error
                      ? "Finished with errors"
                      : scanCanResume(scan) && scan.stopped
                        ? "Paused"
                        : scan.stopped
                          ? "Stopped"
                          : "Complete"}</strong
                >
                <small
                  >{scan.results.length} / {scan.session.totalRequests ?? "∞"} requests</small
                >
              </span>
              <span class="open-arrow" aria-hidden="true">→</span>
            </button>
          {:else}
            <div class="empty-scans">
              <span aria-hidden="true">▤</span><strong>No results yet</strong
              ><small>Launch a test from this tab to create a result set.</small
              >
            </div>
          {/each}
        </div>
        <footer>
          <span>{tab.scans.filter((scan) => scan.running).length} running</span
          ><span>{tab.scans.length} total</span>
        </footer>
      </div>
    </div>
  {/if}
</section>

<style>
  .intruder-shell {
    position: relative;
    display: grid;
    grid-template-rows: 30px minmax(0, 1fr);
    gap: 4px;
    height: 100%;
    min-height: 0;
    padding: 4px;
    color: var(--text);
    overflow: hidden;
  }
  .intruder-tabs {
    display: flex;
    min-width: 0;
    overflow: hidden;
    border-bottom: 1px solid var(--border);
  }
  .intruder-tab-viewport {
    position: relative;
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
  }
  .intruder-tab-scroll {
    width: 100%;
    height: 100%;
    min-width: 0;
    overflow-x: auto;
    scrollbar-width: none;
    -ms-overflow-style: none;
  }
  .intruder-tab-scroll::-webkit-scrollbar { display: none; }
  .intruder-tab-strip { display: flex; gap: 3px; min-width: max-content; min-height: 28px; }
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
  .intruder-tab-actions {
    display: flex;
    flex: 0 0 auto;
    align-items: flex-start;
    gap: 3px;
    padding-left: 3px;
    border-left: 1px solid var(--border);
    background: var(--surface);
  }
  .intruder-tab-actions .tab-search-button {
    display: grid;
    place-items: center;
    width: 28px;
    min-width: 28px;
    height: 28px;
    min-height: 28px;
    padding: 0;
    border: 1px solid var(--border);
    border-bottom: 0;
    border-radius: 3px 3px 0 0;
    color: var(--muted);
    background: var(--surface);
    cursor: pointer;
  }
  .intruder-tab-actions .tab-search-button svg {
    width: 17px;
    height: 17px;
    fill: none;
    stroke: currentColor;
    stroke-width: var(--svgbuttonstrokewidth, 1.5);
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .intruder-tab-actions .tab-search-button:hover { color: var(--text); background: var(--surface-2); }
  .intruder-tab {
    display: flex;
    align-items: center;
    min-width: 48px;
    flex: 0 0 auto;
    height: 28px;
    border: 1px solid var(--border);
    border-bottom: 2px solid transparent;
    border-radius: 3px 3px 0 0;
    background: var(--nestedtabsbg, var(--surface));
  }
  .intruder-tab.active {
    border-color: color-mix(in srgb, var(--accent) 40%, var(--border));
    border-bottom: 2px solid var(--art);
    background: var(--nestedtabsbg, var(--surface));
  }
  .intruder-tab button {
    height: 100%;
    padding: 0 7px;
    border: 0;
    color: var(--muted);
    background: transparent;
    font-size: var(--font-size-body);
    cursor: pointer;
  }
  .intruder-tab button:first-child {
    flex: 0 0 auto;
    text-align: left;
    white-space: nowrap;
  }
  .intruder-tab .tab-close-button {
    font-size: var(--font-size-heading);
  }
  .intruder-tab .tab-close-button.inactive {
    visibility: hidden;
    pointer-events: none;
  }
  .intruder-tab.active button:first-child {
    color: var(--text);
  }
  .intruder-tab-group {
    display: flex;
    align-items: stretch;
    flex: 0 0 auto;
  }
  .intruder-group-tabs {
    display: flex;
    align-items: stretch;
    position: relative;
    flex: 0 0 auto;
    border-bottom: 2px solid var(--tab-group-color, #ffa500);
  }
  .tab-group-marker {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 28px;
    padding: 0 8px;
    border: 1px solid var(--border);
    border-bottom: 0;
    border-radius: 3px 3px 0 0;
    color: var(--text);
    background: color-mix(in srgb, var(--tab-group-color, #ffa500) var(--tab-group-bg-mix, 12%), var(--surface));
    font-size: var(--font-size-compact);
    font-weight: 650;
    white-space: nowrap;
    cursor: pointer;
  }
  .tab-group-marker:hover { border-color: color-mix(in srgb, var(--tab-group-color, #ffa500) 50%, var(--border)); }
  .tab-group-color { display: inline-block; width: 8px; height: 8px; flex: 0 0 auto; border-radius: 50%; background: var(--tab-group-color, #ffa500); }
  .tab-group-marker small { color: var(--muted); font-size: var(--font-size-compact); font-weight: 500; }
  .tab-group-chevron { color: var(--muted); font-size: 12px; line-height: 1; }
  .intruder-tab-group.collapsed .tab-group-marker { border-bottom: 1px solid var(--border); }
  .running-dot {
    width: 6px;
    height: 6px;
    flex: 0 0 auto;
    border-radius: 50%;
    background: var(--success);
    box-shadow: 0 0 0 2px var(--success-soft);
  }
  .new-tab {
    width: 28px;
    min-width: 28px;
    height: 28px;
    padding: 0;
    border: 1px solid var(--border);
    border-bottom: 0;
    border-radius: 3px 3px 0 0;
    color: var(--muted);
    background: var(--surface);
    cursor: pointer;
  }
  .intruder-content {
    min-height: 0;
    overflow: hidden;
  }
  .intruder-content :global(.results-screen) {
    height: 100%;
  }
  .intruder-workspace {
    position: relative;
    display: grid;
    grid-template-rows: 42px minmax(0, 1fr);
    gap: 4px;
    height: 100%;
    min-height: 0;
    overflow: hidden;
  }
  .intruder-toolbar,
  .pane-title {
    display: flex;
    align-items: center;
    border: 1px solid var(--border);
    background: var(--surface);
  }
  .intruder-toolbar {
    display: grid;
  grid-template-columns: 1fr auto 1fr;
  align-items: center;
    padding: 0 7px;
    border-radius: 3px;
  }
  .intruder-toolbar-left {
    justify-self: start;
  }
  .intruder-toolbar-middle {
    justify-self: center;
  }
  .intruder-toolbar-right {
    justify-self: end;
  }
  .intruder-toolbar > div {
    display: flex;
    align-items: center;
    gap: 7px;
  }
  .intruder-toolbar > div span,
  .summary {
    color: var(--muted);
    font-size: var(--font-size-compact);
  }
  .intruder-toolbar label {
    display: flex;
    align-items: center;
    gap: 5px;
    color: var(--muted);
    font-size: var(--font-size-compact);
  }
  .intruder-toolbar select {
    height: 27px;
    padding: 0 6px;
    border: 1px solid var(--border-strong);
    color: var(--text);
    background: var(--input);
  }
  button {
    min-height: 25px;
    padding: 0 8px;
    border: 1px solid var(--border-strong);
    border-radius: 3px;
    color: var(--text);
    background: var(--surface-2);
    cursor: pointer;
  }
  button:disabled {
    opacity: 0.45;
    cursor: default;
  }
  button.start {
    color: var(--text);
    border-color: var(--accent);
    background: var(--accent);
    font-weight: 700;
  }
  button.view-results {
    font-weight: 650;
  }
  .intruder-setup {
    display: grid;
    grid-template-columns: minmax(0, 1.45fr) minmax(410px, 0.95fr);
    gap: 4px;
    height: 100%;
    min-height: 0;
    overflow: hidden;
  }
  .payload-pane {
    display: grid;
    grid-template-rows: 42px minmax(0, 1fr);
    gap: 4px;
    width: 100%;
    height: 100%;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
  }
  .scan-name-field {
    display: grid;
    grid-template-columns: max-content minmax(0, 1fr);
    align-items: center;
    gap: 8px;
    min-width: 0;
    padding: 0 8px;
    color: var(--muted);
    border: 1px solid var(--border);
    border-radius: 3px;
    background: var(--surface);
    font-size: var(--font-size-compact);
  }
  .scan-name-field input {
    width: 100%;
    min-width: 0;
    height: 27px;
    padding: 0 7px;
    border: 1px solid var(--border-strong);
    color: var(--text);
    background: var(--input);
  }
  .request-pane {
    display: grid;
    grid-template-rows: 31px minmax(0, 1fr);
    width: 100%;
    height: 100%;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
  }
  .pane-title {
    justify-content: space-between;
    min-height: 31px;
    padding: 0 7px;
    font-size: var(--font-size-compact);
  }
  .pane-title > div {
    display: flex;
    gap: 4px;
  }
  .request-pane :global(.message-viewer) {
    width: 100%;
    height: 100%;
    min-height: 0;
    border-top: 0;
    border-radius: 0 0 3px 3px;
  }
  .modal-backdrop {
    position: absolute;
    z-index: 50;
    inset: 0;
    display: grid;
    place-items: center;
    padding: 24px;
    background: color-mix(in srgb, var(--bg) 72%, transparent);
    backdrop-filter: blur(2px);
  }
  .scan-modal {
    display: grid;
    grid-template-rows: auto minmax(180px, 1fr) 34px;
    width: min(720px, 100%);
    max-height: min(620px, calc(100% - 24px));
    overflow: hidden;
    color: var(--text);
    border: 1px solid var(--border-strong);
    border-radius: 8px;
    background: var(--surface);
    box-shadow: 0 20px 55px #0008;
  }
  .search-modal {
    display: grid;
    grid-template-rows: auto auto minmax(120px, 1fr);
    gap: 14px;
    width: min(680px, 100%);
    max-height: min(620px, calc(100% - 24px));
    padding: 18px;
    overflow: hidden;
    color: var(--text);
    border: 1px solid var(--border-strong);
    border-radius: 8px;
    background: var(--surface);
    box-shadow: 0 20px 55px #0008;
  }
  .search-modal > header { display: flex; align-items: flex-start; justify-content: space-between; gap: 12px; }
  .search-eyebrow { margin: 0 0 3px; color: var(--accent); font-size: var(--font-size-compact); font-weight: 800; letter-spacing: .13em; }
  .search-modal h2 { margin: 0; font-size: var(--font-size-heading); }
  .search-field { display: grid; gap: 6px; color: var(--muted); font-size: var(--font-size-compact); }
  .search-field input { width: 100%; height: 34px; }
  .search-results { min-height: 0; overflow: auto; border: 1px solid var(--border); border-radius: 5px; background: var(--surface-2); }
  .search-result { display: grid; grid-template-columns: minmax(0, 1fr) max-content; align-items: center; gap: 14px; width: 100%; min-height: 48px; padding: 9px 11px; border: 0; border-bottom: 1px solid var(--border); color: var(--text); background: transparent; text-align: left; cursor: pointer; }
  .search-result:last-child { border-bottom: 0; }
  .search-result:hover, .search-result:focus-visible { background: var(--accent-soft); }
  .search-snippet { min-width: 0; overflow: hidden; color: var(--muted); text-overflow: ellipsis; white-space: nowrap; }
  .search-snippet strong { color: var(--text); font-weight: 800; }
  .search-tab-badge { padding: 3px 7px; border: 1px solid color-mix(in srgb, var(--accent) 38%, var(--border)); border-radius: 999px; color: var(--text); background: var(--accent-soft); font-size: var(--font-size-compact); font-weight: 700; }
  .search-empty { display: grid; place-items: center; min-height: 110px; margin: 0; padding: 18px; color: var(--muted); text-align: center; }
  .scan-modal > header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 18px;
    border-bottom: 1px solid var(--border);
  }
  .scan-modal h2 {
    margin: 0 0 2px;
    font-size: var(--font-size-heading);
  }
  .scan-modal p {
    margin: 0;
    color: var(--muted);
    font-size: var(--font-size-compact);
  }
  .close-modal {
    width: 30px;
    padding: 0;
    font-size: var(--font-size-title);
  }
  .scan-list {
    min-height: 0;
    padding: 8px;
    overflow: auto;
  }
  .scan-row {
    display: grid;
    grid-template-columns: 10px minmax(0, 1fr) auto 20px;
    align-items: center;
    gap: 11px;
    width: 100%;
    min-height: 58px;
    padding: 8px 10px;
    border: 0;
    border-bottom: 1px solid var(--border);
    border-radius: 3px;
    text-align: left;
    background: transparent;
  }
  .scan-row:hover {
    background: var(--accent-soft);
  }
  .scan-status {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--muted);
  }
  .scan-status.running {
    background: var(--success);
    box-shadow: 0 0 0 3px var(--success-soft);
  }
  .scan-status.paused {
    background: var(--warning);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--warning) 20%, transparent);
  }
  .scan-status.stopped {
    background: var(--danger);
    box-shadow: 0 0 0 3px var(--danger-soft);
  }
  .scan-details,
  .scan-progress {
    display: grid;
    gap: 3px;
    min-width: 0;
  }
  .scan-details strong {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--font-size-body);
  }
  .scan-details small,
  .scan-progress small {
    color: var(--muted);
    font-size: var(--font-size-compact);
  }
  .scan-progress {
    justify-items: end;
  }
  .scan-progress strong {
    color: var(--muted);
    font-size: var(--font-size-compact);
    font-weight: 650;
  }
  .open-arrow {
    color: var(--accent);
    font-size: var(--font-size-heading);
  }
  .empty-scans {
    display: grid;
    place-content: center;
    justify-items: center;
    gap: 5px;
    height: 200px;
    color: var(--muted);
    font-size: var(--font-size-compact);
  }
  .empty-scans > span {
    color: var(--accent);
    font-size: var(--font-size-title);
  }
  .empty-scans strong {
    color: var(--text);
    font-size: var(--font-size-body);
  }
  .scan-modal > footer {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 0 18px;
    color: var(--muted);
    border-top: 1px solid var(--border);
    font-size: var(--font-size-compact);
  }
  @media (max-width: 1050px) {
    .intruder-setup {
      grid-template-columns: minmax(0, 1fr) minmax(330px, 0.9fr);
    }
    .intruder-toolbar > div span {
      display: none;
    }
  }
</style>
