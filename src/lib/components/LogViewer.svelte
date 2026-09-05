<script lang="ts">
  import { onMount } from "svelte";
  import { commands, isTauri, onWitnessEvent } from "$lib/api";
  import { formatTime } from "$lib/format";
  import { showErrorToast } from "$lib/errorToast";
  import ConfirmDialog from "./ConfirmDialog.svelte";
  import type { LogEntry, TrafficStats } from "$lib/types";

  const LOG_LIMIT_OPTIONS = [50, 100, 200] as const;
  const MAX_LOGS_IN_MEMORY = 200;

  const emptyTrafficStats = (): TrafficStats => ({
    requestsProcessed: 0,
    totalRequestsSent: 0,
    totalResponsesReceived: 0,
    packetLossPercent: 0,
    bytesSent: 0,
    bytesReceived: 0,
    volumeTransferredBytes: 0,
    uptimeSeconds: 0,
  });

  let logs = $state<LogEntry[]>([]);
  let clearConfirmationOpen = $state(false);
  let trafficStats = $state<TrafficStats>(emptyTrafficStats());
  let level = $state("all");
  let moduleFilter = $state("");
  let logLimit = $state(50);
  const browserStartedAt = Date.now();
  const filtered = $derived(logs.filter((entry) =>
    (level === "all" || entry.level === level)
    && (!moduleFilter || entry.module.toLowerCase().includes(moduleFilter.toLowerCase()))
  ));
  const visibleLogs = $derived(filtered.slice(-logLimit));

  function formatCount(value: number) {
    return new Intl.NumberFormat().format(value);
  }

  function formatBytes(value: number) {
    if (value < 1024) return `${formatCount(value)} B`;
    const units = ["KB", "MB", "GB", "TB"];
    let amount = value;
    let unit = -1;
    while (amount >= 1024 && unit < units.length - 1) {
      amount /= 1024;
      unit += 1;
    }
    return `${amount.toFixed(amount >= 100 ? 0 : amount >= 10 ? 1 : 2)} ${units[unit]}`;
  }

  function formatDuration(totalSeconds: number) {
    const seconds = Math.max(0, Math.floor(totalSeconds));
    const days = Math.floor(seconds / 86400);
    const hours = Math.floor((seconds % 86400) / 3600);
    const minutes = Math.floor((seconds % 3600) / 60);
    const remainder = seconds % 60;
    if (days) return `${days}d ${hours}h ${minutes}m`;
    if (hours) return `${hours}h ${minutes}m ${remainder}s`;
    if (minutes) return `${minutes}m ${remainder}s`;
    return `${remainder}s`;
  }

  function barWidth(value: number) {
    const maximum = Math.max(
      1,
      trafficStats.requestsProcessed,
      trafficStats.totalRequestsSent,
      trafficStats.totalResponsesReceived,
    );
    return Math.min(100, (value / maximum) * 100);
  }

  async function refreshTrafficStats() {
    if (!isTauri()) {
      trafficStats = {
        ...trafficStats,
        uptimeSeconds: Math.floor((Date.now() - browserStartedAt) / 1000),
      };
      return;
    }
    try {
      trafficStats = await commands.getTrafficStats();
    } catch {
      // The Logs tab can still render while the desktop bridge is reconnecting.
    }
  }

  let logLoadRequest = 0;

  async function loadLogs() {
    if (!isTauri()) return;
    const request = ++logLoadRequest;
    try {
      const entries = await commands.getLogs(logLimit);
      if (request === logLoadRequest) logs = entries.slice(-MAX_LOGS_IN_MEMORY);
    } catch {
      // The Logs tab can still render while the desktop bridge is reconnecting.
    }
  }

  onMount(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    const statsTimer = window.setInterval(() => void refreshTrafficStats(), 1000);
    void refreshTrafficStats();
    if (!isTauri()) return () => window.clearInterval(statsTimer);
    void loadLogs();
    void onWitnessEvent((event) => {
      if (event.kind !== "log") return;
      logs = [...logs, event.payload as LogEntry].slice(-MAX_LOGS_IN_MEMORY);
    }).then((value) => {
      if (disposed) value();
      else unlisten = value;
    });
    return () => {
      disposed = true;
      window.clearInterval(statsTimer);
      unlisten?.();
    };
  });

  function exportLogs() {
    if (!visibleLogs.length) return;
    const text = visibleLogs
      .map((entry) => `${entry.timestamp} ${entry.level.toUpperCase().padEnd(5)} [${entry.module}] ${entry.message}`)
      .join("\n");
    const url = URL.createObjectURL(new Blob([text], { type: "text/plain" }));
    const link = document.createElement("a");
    link.href = url;
    link.download = `witness-logs-${new Date().toISOString().replaceAll(":", "-")}.log`;
    link.click();
    window.setTimeout(() => URL.revokeObjectURL(url), 4000);
  }

  async function copyLog(entry: LogEntry) {
    try {
      await navigator.clipboard.writeText(`${entry.timestamp} ${entry.level} [${entry.module}] ${entry.message}`);
    } catch (reason) {
      showErrorToast(reason);
    }
  }

  function clear() {
    clearConfirmationOpen = true;
  }

  async function confirmClear() {
    clearConfirmationOpen = false;
    await commands.clearLogs();
    logs = [];
  }

  export function handleShortcut(action: string): boolean {
    if (action === "logs.focusFilter") {
      document.querySelector<HTMLInputElement>('.logs-tool input[placeholder="Filter module…"]')?.focus();
      return true;
    }
    if (action === "logs.export") {
      if (!visibleLogs.length) return false;
      exportLogs();
      return true;
    }
    if (action === "logs.clear") {
      if (!logs.length) return false;
      void clear();
      return true;
    }
    return false;
  }
</script>

<section class="logs-tool">
  <header><div><p>LOGS</p><h1 class="instance-stats-title">Traffic Overview and Application Logs</h1></div><div class="actions"><label class="log-limit">Show <select bind:value={logLimit} onchange={() => void loadLogs()} aria-label="Number of logs to show">{#each LOG_LIMIT_OPTIONS as limit}<option value={limit}>{limit}</option>{/each}</select></label><select bind:value={level} aria-label="Filter logs by level"><option value="all">All levels</option><option value="debug">Debug</option><option value="info">Info</option><option value="warn">Warning</option><option value="error">Error</option></select><input bind:value={moduleFilter} aria-label="Filter logs by module" placeholder="Filter module…" /><button class="text-button" onclick={exportLogs} disabled={!visibleLogs.length} data-tooltip="Export the visible logs">Export logs</button><button class="text-button" onclick={clear} disabled={!logs.length}>Clear</button><span class="stats-live"><i></i>LIVE</span></div></header>
  <section class="stats-section" aria-label="Instance traffic statistics">
    <div class="stats-grid">
      <article class="stat-card"><span>Requests processed</span><strong>{formatCount(trafficStats.requestsProcessed)}</strong><small>Proxy + Replay + Fuzz</small></article>
      <article class="stat-card"><span>Total requests sent</span><strong>{formatCount(trafficStats.totalRequestsSent)}</strong><small>{formatBytes(trafficStats.bytesSent)} outbound</small></article>
      <article class="stat-card"><span>Total responses received</span><strong>{formatCount(trafficStats.totalResponsesReceived)}</strong><small>{formatBytes(trafficStats.bytesReceived)} inbound</small></article>
      <article class="stat-card"><span>Packet loss</span><strong class:loss={trafficStats.packetLossPercent > 0}>{trafficStats.packetLossPercent.toFixed(1)}%</strong><small>Unanswered requests</small></article>
      <article class="stat-card"><span>Volume transferred</span><strong>{formatBytes(trafficStats.volumeTransferredBytes)}</strong><small>Sent + received</small></article>
      <article class="stat-card"><span>Instance uptime</span><strong>{formatDuration(trafficStats.uptimeSeconds)}</strong><small>Since app start</small></article>
    </div>
    <div class="traffic-chart" aria-label="Request flow chart">
      <div class="traffic-chart-heading"><span>Request flow</span><small>Relative volume since app start</small></div>
      <div class="traffic-bar-row"><span>Processed</span><div class="traffic-bar"><i class="processed" style={`width:${barWidth(trafficStats.requestsProcessed)}%`}></i></div><strong>{formatCount(trafficStats.requestsProcessed)}</strong></div>
      <div class="traffic-bar-row"><span>Sent</span><div class="traffic-bar"><i class="sent" style={`width:${barWidth(trafficStats.totalRequestsSent)}%`}></i></div><strong>{formatCount(trafficStats.totalRequestsSent)}</strong></div>
      <div class="traffic-bar-row"><span>Received</span><div class="traffic-bar"><i class="received" style={`width:${barWidth(trafficStats.totalResponsesReceived)}%`}></i></div><strong>{formatCount(trafficStats.totalResponsesReceived)}</strong></div>
    </div>
  </section>
  <div class="log-list" role="log" aria-live="polite">
    {#each visibleLogs as entry, index (`${entry.timestamp}-${index}`)}
      <div class="log-entry" class:error={entry.level === "error"} class:warn={entry.level === "warn"}>
        <time>{formatTime(entry.timestamp)}</time><strong>{entry.level}</strong><code>{entry.module}</code><span>{entry.message}</span><button class="text-button compact" aria-label="Copy log entry" onclick={() => void copyLog(entry)}>Copy</button>
      </div>
    {:else}<div class="empty">No log entries match the current filters.</div>{/each}
  </div>
  <footer>{visibleLogs.length} shown of {filtered.length} matching · latest {logs.length} loaded</footer>
  <ConfirmDialog
    open={clearConfirmationOpen}
    title="Clear instance logs?"
    message="This removes all currently loaded logs for this Witness instance."
    confirmLabel="Clear logs"
    onConfirm={() => void confirmClear()}
    onCancel={() => (clearConfirmationOpen = false)}
  />
</section>

<style>
  .logs-tool { display: grid; grid-template-rows: max-content max-content minmax(0, 1fr) 30px; width: min(1180px, 100%); height: 100%; margin: auto; padding: 0 22px 14px; color: var(--text, #dce1e8); }
  header { display: flex; align-items: center; justify-content: space-between; padding-top: 10px; padding-bottom: 10px; } header p { margin: 0 0 4px; color: #f59e0b; font-size: var(--font-size-compact); font-weight: 800; letter-spacing: .14em; } h1 { margin: 0; font-size: var(--font-size-title); }
  .instance-stats-title { margin: 0 0 3px 4px; color: var(--accent, #f59e0b); font-size: var(--font-size-compact); font-weight: 800; letter-spacing: .14em; }
  .actions { display: flex; gap: 6px; align-items: center; } .log-limit { display: flex; align-items: center; gap: 5px; color: var(--muted, #8993a0); font-size: var(--font-size-compact); } input, select, button { height: 30px; padding: 0 8px; border: 1px solid #343c47; border-radius: 4px; color: var(--text, #cbd2dc); background: var(--surface, #191e25); } input { width: 170px; } button { cursor: pointer; } button:disabled { opacity: .4; }
  .stats-section { display: grid; grid-template-rows: max-content max-content; align-content: start; gap: 8px; margin-bottom: 10px; padding: 10px; border: 1px solid var(--border, #29303a); border-radius: 8px; background: var(--surface, #12161b); box-shadow: var(--shadow-soft, none); }
  .stats-live { display: flex; align-items: center; gap: 5px; margin-left: 2px; color: var(--success, #6ee7b7); font-size: var(--font-size-compact); font-weight: 750; letter-spacing: .1em; white-space: nowrap; }
  .stats-live i { width: 7px; height: 7px; border-radius: 50%; background: currentColor; box-shadow: 0 0 0 3px color-mix(in srgb, currentColor 18%, transparent); }
  .stats-grid { display: grid; grid-template-columns: repeat(6, minmax(0, 1fr)); gap: 6px; }
  .stat-card { display: grid; gap: 4px; min-width: 0; padding: 8px; border: 1px solid var(--border, #29303a); border-radius: 5px; background: var(--surface-2, #191e25); }
  .stat-card > span, .stat-card small { overflow: hidden; color: var(--muted, #8993a0); text-overflow: ellipsis; white-space: nowrap; }
  .stat-card > span { font-size: var(--font-size-compact); }
  .stat-card strong { overflow: hidden; color: var(--text, #f4f6fb); text-overflow: ellipsis; white-space: nowrap; font-size: var(--font-size-heading); }
  .stat-card strong.loss { color: var(--danger, #fca5a5); }
  .stat-card small { font-size: var(--font-size-compact); }
  .traffic-chart { display: grid; gap: 5px; padding: 8px 9px; border: 1px solid var(--border, #29303a); border-radius: 5px; background: var(--editor, #202020); }
  .traffic-chart-heading { display: flex; align-items: center; justify-content: space-between; color: var(--text, #f4f6fb); font-size: var(--font-size-compact); font-weight: 700; }
  .traffic-chart-heading small { color: var(--muted, #8993a0); font-weight: 500; }
  .traffic-bar-row { display: grid; grid-template-columns: 74px minmax(0, 1fr) 58px; align-items: center; gap: 8px; color: var(--muted, #8993a0); font-size: var(--font-size-compact); }
  .traffic-bar-row > strong { color: var(--text, #f4f6fb); text-align: right; font-weight: 650; }
  .traffic-bar { height: 7px; overflow: hidden; border-radius: 999px; background: var(--surface-3, #242b34); }
  .traffic-bar i { display: block; height: 100%; min-width: 2px; border-radius: inherit; transition: width .25s ease; }
  .traffic-bar i.processed { background: var(--accent, #f59e0b); }
  .traffic-bar i.sent { background: #60a5fa; }
  .traffic-bar i.received { background: var(--success, #6ee7b7); }
  .log-list { overflow: auto; border: 1px solid var(--border, #29303a); border-radius: 6px; background: var(--bg, #0c0f13); }
  .log-entry { display: grid; grid-template-columns: 85px 52px 180px 1fr auto; align-items: center; min-height: 34px; padding: 0 8px; border-bottom: 1px solid var(--border, #20262d); color: var(--muted, #aab3bf); font: var(--font-size-body) ui-monospace, SFMono-Regular, Menlo, monospace; }
  .log-entry time { color: var(--muted, #747f8d); } .log-entry strong { color: #93c5fd; text-transform: uppercase; } .log-entry.warn strong { color: #fbbf24; } .log-entry.error strong { color: #fca5a5; } .log-entry code { overflow: hidden; color: var(--muted, #aeb6c1); text-overflow: ellipsis; } .log-entry span { overflow-wrap: anywhere; }
  .log-entry button { height: 24px; font: var(--font-size-compact) system-ui; } .empty { display: grid; place-items: center; height: 180px; color: var(--muted, #747f8d); } footer { display: flex; align-items: center; color: var(--muted, #747f8d); font-size: var(--font-size-compact); }
  @media (max-width: 950px) { .stats-grid { grid-template-columns: repeat(3, minmax(0, 1fr)); } }
  @media (max-width: 640px) { .logs-tool { padding: 0 10px 10px; } .logs-tool > header { align-items: flex-start; flex-direction: column; gap: 8px; padding-top: 8px; } .actions { width: 100%; flex-wrap: wrap; } .actions input { flex: 1; } .stats-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); } .traffic-chart-heading small { display: none; } }
</style>
