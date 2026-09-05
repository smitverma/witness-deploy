<script lang="ts">
  import { showErrorToast } from "$lib/errorToast";
  import { requestHost } from "$lib/intruder";
  import type { IntruderScan } from "$lib/types";
  import MessageViewer from "./MessageViewer.svelte";

  let {
    scan,
    onStop,
    onResume,
    onSaveOrganizer,
    onSendReplay,
    onSendDecoder,
  }: {
    scan: IntruderScan;
    onStop: () => void;
    onResume: () => void;
    onSaveOrganizer?: (request: Uint8Array, response: Uint8Array, tls: boolean) => void;
    onSendReplay?: (request: Uint8Array, tls: boolean) => void;
    onSendDecoder?: (value: string) => void;
  } = $props();

  const selectedResult = $derived(
    scan.results.find((result) => result.id === scan.selectedResultId) ?? null,
  );
  const progress = $derived(
    scan.session.totalRequests === null
      ? `${scan.results.length}/∞`
      : `${scan.results.length}/${scan.session.totalRequests}`,
  );
  let reportedResultId = $state<string | null>(null);
  type ResultSortKey = "sequence" | "position" | "payload" | "status" | "length" | "durationMs";
  type ResultSortDirection = "asc" | "desc";
  const resultColumns: { label: string; key: ResultSortKey }[] = [
    { label: "#", key: "sequence" },
    { label: "Pos", key: "position" },
    { label: "Input value", key: "payload" },
    { label: "Status", key: "status" },
    { label: "Length", key: "length" },
    { label: "Time", key: "durationMs" },
  ];
  let sortKey = $state<ResultSortKey>("sequence");
  let sortDirection = $state<ResultSortDirection>("asc");
  const canResume = $derived(
    !scan.running && (scan.session.repeatIndefinitely || scan.nextPayloadIndex < scan.session.payloadRows.length),
  );
  const scanState = $derived(
    scan.running
      ? "running"
      : scan.error
        ? "error"
        : canResume && scan.stopped
          ? "paused"
          : scan.stopped
            ? "stopped"
            : "complete",
  );
  const sortedResults = $derived.by(() => {
    const key = sortKey;
    const direction = sortDirection;
    return [...scan.results].sort((left, right) => compareResults(left, right, key, direction));
  });

  $effect(() => {
    if (selectedResult?.error && selectedResult.id !== reportedResultId) {
      showErrorToast(selectedResult.error);
      reportedResultId = selectedResult.id;
    } else if (!selectedResult?.error) {
      reportedResultId = null;
    }
  });

  function sortResults(nextKey: ResultSortKey) {
    if (sortKey === nextKey) {
      sortDirection = sortDirection === "asc" ? "desc" : "asc";
    } else {
      sortKey = nextKey;
      sortDirection = "asc";
    }
  }

  function resultSortValue(result: typeof scan.results[number], key: ResultSortKey): number | string | null {
    if (key === "position") return result.position;
    if (key === "payload") return result.payload;
    if (key === "status") return result.status;
    if (key === "length") return result.length;
    if (key === "durationMs") return result.durationMs;
    return result.sequence;
  }

  function compareResults(
    left: typeof scan.results[number],
    right: typeof scan.results[number],
    key: ResultSortKey,
    direction: ResultSortDirection,
  ) {
    const leftValue = resultSortValue(left, key);
    const rightValue = resultSortValue(right, key);
    if (leftValue === null || rightValue === null) {
      if (leftValue === rightValue) return left.sequence - right.sequence;
      return leftValue === null ? 1 : -1;
    }
    const comparison = typeof leftValue === "number" && typeof rightValue === "number"
      ? leftValue - rightValue
      : String(leftValue).localeCompare(String(rightValue), undefined, { numeric: true, sensitivity: "base" });
    if (comparison !== 0) return direction === "asc" ? comparison : -comparison;
    return left.sequence - right.sequence;
  }

</script>

<section class="results-screen" aria-label="Test results">
  <header class="results-toolbar">
    <div class="title">
      <span
        class:running={scanState === "running"}
        class:paused={scanState === "paused"}
        class:stopped={scanState === "stopped" || scanState === "error"}
        class="status-dot"
        aria-hidden="true"
      ></span>
      <div>
        <h1>{scan.name}</h1>
        <span>
          {scan.running
            ? `Running ${progress}`
            : scanState === "paused"
              ? `Paused · ${scan.results.length} requests`
              : scanState === "stopped"
                ? `Stopped · ${scan.results.length} requests`
                : scanState === "error"
                  ? `Error · ${scan.results.length} requests`
                  : scan.results.length
                    ? `Complete · ${scan.results.length} requests`
                    : "No completed requests"}
        </span>
      </div>
    </div>
    <div class="actions">
      {#if selectedResult}
        <span class="highlight-key"><i></i> Modified Value</span>
      {/if}
      {#if scan.running}
        <button class="text-button scan-control pause" onclick={onStop}>Pause</button>
      {:else if canResume}
        <button class="text-button scan-control resume" onclick={onResume}>Resume</button>
      {/if}
    </div>
  </header>

  <div class="results-content">
    <section class="results-table-panel" aria-label="Test result table">
      <div class="result-row result-header" role="row">
        {#each resultColumns as column}
          <button
            type="button"
            role="columnheader"
            aria-sort={sortKey === column.key ? (sortDirection === "asc" ? "ascending" : "descending") : "none"}
            onclick={() => sortResults(column.key)}
          >
            {column.label}
            <span aria-hidden="true">{sortKey === column.key ? (sortDirection === "asc" ? "↑" : "↓") : "↕"}</span>
          </button>
        {/each}
      </div>
      <div class="result-scroll">
        {#each sortedResults as result (result.id)}
          <button
            class:active={result.id === scan.selectedResultId}
            class:failed={Boolean(result.error)}
            class="result-row"
            role="row"
            onclick={() => (scan.selectedResultId = result.id)}
          >
            <span>{result.sequence}</span>
            <span>{result.position ?? "All"}</span>
            <span>{result.payload || "(empty)"}</span>
            <span>{result.status ?? "ERR"}</span>
            <span>{result.length}</span>
            <span>{result.durationMs} ms</span>
          </button>
        {/each}
        {#if !scan.results.length}
          <div class="empty-table">
            <span class:active={scan.running} class="pulse" aria-hidden="true"></span>
            <strong>{scan.running ? "Waiting for the first result" : "No results were recorded"}</strong>
            <small>{scan.running ? "Rows appear here as requests finish." : "Select a setup tab to start another test."}</small>
          </div>
        {/if}
      </div>
    </section>

    <section class="inspectors" aria-label="Selected request and response">
      {#if selectedResult}
        <MessageViewer
          title="Request"
          kind="request"
          raw={selectedResult.request}
          metadata={requestHost(selectedResult.request)}
          highlightRanges={selectedResult.modifiedRanges}
          onSendReplay={(raw) => onSendReplay?.(raw, scan.session.tls)}
          onSendDecoder={onSendDecoder}
          onSaveOrganizer={(raw) => onSaveOrganizer?.(raw, selectedResult.response, scan.session.tls)}
        />
        <MessageViewer
          title="Response"
          kind="response"
          raw={selectedResult.response}
          metadata={selectedResult.error ? "Request failed" : requestHost(selectedResult.request)}
          onSendDecoder={onSendDecoder}
        />
      {:else}
        <div class="empty-preview">
          <span aria-hidden="true">↗↙</span>
          <strong>No result selected</strong>
          <small>Select a row above to inspect its request and response.</small>
        </div>
      {/if}
    </section>
  </div>

  <footer>
    <span>{scan.session.tls ? "HTTPS" : "HTTP"}</span>
    <span>{scan.session.payloadRows.length} value rows</span>
    <span>{progress} complete</span>
  </footer>
</section>

<style>
  .results-screen {
    position: relative;
    display: grid;
    grid-template-rows: 50px minmax(0, 1fr) 30px;
    height: 100%;
    min-height: 0;
    color: var(--text);
    background: var(--bg);
    overflow: hidden;
  }
  .results-toolbar {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    gap: 12px;
    min-width: 0;
    padding: 0 10px;
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }
  .actions button {
    min-height: 28px;
    padding: 0 10px;
    color: var(--text);
    border: 1px solid var(--border-strong);
    border-radius: 4px;
    background: var(--surface-2);
    cursor: pointer;
  }
  .title, .title > div, .actions { display: flex; align-items: center; }
  .title { min-width: 0; gap: 8px; }
  .title > div { align-items: baseline; gap: 10px; min-width: 0; }
  .title h1 { margin: 0; font-size: var(--font-size-heading); font-weight: 650; }
  .title > div > span, .highlight-key { color: var(--muted); font-size: var(--font-size-compact); }
  .status-dot { width: 8px; height: 8px; flex: 0 0 auto; border-radius: 50%; background: var(--muted); }
  .status-dot.running { background: var(--success); box-shadow: 0 0 0 3px var(--success-soft); }
  .status-dot.paused { background: var(--warning); box-shadow: 0 0 0 3px color-mix(in srgb, var(--warning) 20%, transparent); }
  .status-dot.stopped { background: var(--danger); box-shadow: 0 0 0 3px var(--danger-soft); }
  .actions { justify-content: flex-end; gap: 8px; }
  .actions button.scan-control { box-sizing: border-box; display: inline-flex; align-items: center; width: 76px; justify-content: center; }
  .actions button.pause { color: #fff; border-color: var(--warning); background: var(--warning); }
  .actions button.pause:hover:not(:disabled) { color: #fff; border-color: color-mix(in srgb, var(--warning) 82%, #fff); background: color-mix(in srgb, var(--warning) 82%, #000); }
  .highlight-key { display: flex; align-items: center; gap: 5px; }
  .highlight-key i { width: 12px; height: 9px; border-radius: 2px; background: var(--warning); }
  .results-content {
    display: grid;
    grid-template-rows: minmax(150px, 32%) minmax(260px, 1fr);
    gap: 4px;
    min-height: 0;
    padding: 4px;
    overflow: hidden;
  }
  .results-table-panel {
    display: grid;
    grid-template-rows: 28px minmax(0, 1fr);
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    border: 1px solid var(--border);
    border-radius: 3px;
    background: var(--surface);
  }
  .result-scroll { min-height: 0; overflow: auto; }
  .result-row {
    display: grid;
    grid-template-columns: 52px 52px minmax(120px, 1.25fr) minmax(76px, .85fr) minmax(90px, 1fr) minmax(96px, 1fr);
    align-items: center;
    width: 100%;
    min-height: 28px;
    padding: 0;
    border: 0;
    border-bottom: 1px solid var(--border);
    border-radius: 0;
    text-align: left;
    font-size: var(--font-size-compact);
  }
  .result-row span { min-width: 0; padding: 0 8px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .result-header { color: var(--muted); background: var(--surface-2); font-weight: 700; }
  .result-header > button { display: flex; align-items: center; min-width: 0; gap: 4px; padding: 0 8px; border: 0; border-radius: 0; color: inherit; background: transparent; font: inherit; text-align: left; cursor: pointer; }
  .result-header > button:hover, .result-header > button:focus-visible { color: var(--text); background: var(--surface); outline: none; }
  .result-header > button span { min-width: 0; padding: 0; overflow: visible; }
  button.result-row { color: var(--text); background: transparent; cursor: pointer; }
  button.result-row:hover, button.result-row.active { background: var(--accent-soft); }
  button.result-row.active { box-shadow: inset 3px 0 var(--accent); }
  button.result-row.failed { color: var(--danger); }
  .empty-table, .empty-preview {
    display: grid;
    place-content: center;
    justify-items: center;
    gap: 5px;
    height: 100%;
    color: var(--muted);
    text-align: center;
    font-size: var(--font-size-compact);
  }
  .empty-table strong, .empty-preview strong { color: var(--text); font-size: var(--font-size-body); }
  .pulse { width: 9px; height: 9px; margin-bottom: 4px; border-radius: 50%; background: var(--muted); }
  .pulse.active { background: var(--success); animation: pulse 1.25s ease-in-out infinite; }
  .inspectors {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 4px;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
  }
  .inspectors :global(.message-viewer) { height: 100%; border-radius: 3px; }
  .empty-preview { grid-column: 1 / -1; border: 1px solid var(--border); border-radius: 3px; background: var(--surface); }
  .empty-preview > span { color: var(--accent); font-size: var(--font-size-title); }
  footer {
    display: flex;
    align-items: center;
    gap: 22px;
    padding: 0 10px;
    color: var(--muted);
    border-top: 1px solid var(--border);
    background: var(--surface);
    font-size: var(--font-size-compact);
  }
  footer span:last-child { margin-left: auto; }
  @keyframes pulse { 50% { opacity: .35; transform: scale(.8); } }
  @media (max-width: 760px) {
    .inspectors { grid-template-columns: 1fr; grid-template-rows: repeat(2, minmax(0, 1fr)); }
    .result-row { grid-template-columns: 42px 42px minmax(100px, 1.2fr) 62px 70px 76px; }
    .highlight-key { display: none; }
  }
</style>
