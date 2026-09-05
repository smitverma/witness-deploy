<script lang="ts">
  import { onMount } from "svelte";
  import { commands } from "$lib/api";
  import { deleteAiApiKey, displayAiKey, saveAiApiKey, waitForAiKeyOperation } from "$lib/ai-credentials";
  import { showErrorToast } from "$lib/errorToast";
  import ConfirmDialog from "./ConfirmDialog.svelte";
  import type { SettingsPatch, SettingsState } from "$lib/types";
  import Toggle from "./Toggle.svelte";

  let { settings, onUpdate }: {
    settings: SettingsState;
    onUpdate: (patch: SettingsPatch) => Promise<SettingsState>;
  } = $props();

  let newKey = $state("");
  let keyBusy = $state(false);
  let keyState = $state("");
  let showKeyEntry = $state(false);
  let deleteConfirm = $state(false);
  let connectionState = $state("");
  let connectionTone = $state<"idle" | "busy" | "success" | "error">("idle");

  $effect(() => {
    if (!settings.aiApiKeyConfigured && !keyBusy) showKeyEntry = true;
  });

  onMount(() => {
    let disposed = false;
    let timer: number | undefined;
    const synchronizeKeyState = async () => {
      try {
        const status = await commands.getAiApiKeyStatus();
        if (disposed) return;
        if (status.pending) {
          timer = window.setTimeout(() => void synchronizeKeyState(), 500);
          return;
        }
        if (status.error || status.configured === settings.aiApiKeyConfigured) return;
        await onUpdate({});
      } catch {
        // Settings refresh is auxiliary; the next normal settings interaction
        // will still surface command failures.
      }
    };
    void synchronizeKeyState();
    return () => {
      disposed = true;
      if (timer !== undefined) window.clearTimeout(timer);
    };
  });

  async function update(patch: SettingsPatch) {
    try {
      await onUpdate(patch);
    } catch (reason) {
      showErrorToast(reason);
    }
  }

  async function saveKey() {
    const value = newKey;
    if (!value) return;
    keyBusy = true;
    keyState = "Saving provider key…";
    try {
      const accepted = await saveAiApiKey(value);
      keyState = "Securing provider key…";
      const saved = await waitForAiKeyOperation(accepted);
      await onUpdate({});
      newKey = "";
      showKeyEntry = false;
      deleteConfirm = false;
      keyState = `Connected · ${displayAiKey(saved.prefix, saved.suffix)}`;
    } catch (reason) {
      keyState = "";
      showErrorToast(reason);
    } finally {
      newKey = "";
      keyBusy = false;
    }
  }

  function requestDeleteKey() {
    if (!settings.aiApiKeyConfigured || keyBusy) return;
    keyState = "";
    deleteConfirm = true;
  }

  function cancelDeleteKey() {
    if (keyBusy) return;
    deleteConfirm = false;
  }

  async function deleteKey() {
    if (!settings.aiApiKeyConfigured || !deleteConfirm) return;
    keyBusy = true;
    keyState = "Removing saved key…";
    try {
      const accepted = await deleteAiApiKey();
      await waitForAiKeyOperation(accepted);
      await onUpdate({});
      showKeyEntry = true;
      deleteConfirm = false;
      keyState = "Key removed";
    } catch (reason) {
      keyState = "";
      showErrorToast(reason);
    } finally {
      keyBusy = false;
    }
  }

  function beginReplace() {
    newKey = "";
    keyState = "";
    deleteConfirm = false;
    showKeyEntry = true;
  }

  function cancelReplace() {
    newKey = "";
    keyState = "";
    deleteConfirm = false;
    showKeyEntry = false;
  }

  async function testConnection() {
    connectionState = "Checking provider…";
    connectionTone = "busy";
    try {
      const result = await commands.testAiConnection();
      connectionState = result.message;
      connectionTone = result.ok ? "success" : "error";
    } catch (reason) {
      connectionState = "Connection failed";
      connectionTone = "error";
      showErrorToast(reason);
    }
  }
</script>

<div class="settings-form single-section ai-settings">
  <section class="ai-card ai-controller-card" aria-labelledby="ai-controller-title">
    <div class="card-heading">
      <span class="card-icon controller-icon" aria-hidden="true">
        <svg viewBox="0 0 24 24" fill="none"><path d="M8 4h8M6 8h12M5 12h14M8 16h8M10 20h4" /><circle cx="8" cy="4" r="1" /><circle cx="16" cy="8" r="1" /><circle cx="8" cy="12" r="1" /><circle cx="16" cy="16" r="1" /><circle cx="10" cy="20" r="1" /></svg>
      </span>
      <div>
        <p class="section-kicker">WORKSPACE</p>
        <h3 id="ai-controller-title">AI Controller</h3>
      </div>
    </div>
    <label class="ai-toggle-row">
      <span class="ai-toggle-copy"><strong>Enable AI Controller</strong><small>Applies across all projects and temporary sessions.</small></span>
      <Toggle checked={settings.aiEnabled} ariaLabel="Enable AI Controller" onchange={(event) => void update({ aiEnabled: event.currentTarget.checked })} />
    </label>
    <label class="ai-toggle-row">
      <span class="ai-toggle-copy"><strong>Enter sends messages</strong><small>Press Enter to send. Use Shift+Enter for a new line.</small></span>
      <Toggle checked={settings.aiEnterToSend} ariaLabel="Enter sends messages" onchange={(event) => void update({ aiEnterToSend: event.currentTarget.checked })} />
    </label>
  </section>

  <div class="ai-grid">
    <section class="ai-card" aria-labelledby="ai-provider-title">
      <div class="card-heading">
        <span class="card-icon provider-icon" aria-hidden="true">
          <svg viewBox="0 0 24 24" fill="none"><circle cx="12" cy="12" r="8" /><path d="M4 12h16M12 4c2 2.2 3 4.9 3 8s-1 5.8-3 8c-2-2.2-3-4.9-3-8s1-5.8 3-8Z" /></svg>
        </span>
        <div>
          <p class="section-kicker">PROVIDER</p>
          <h3 id="ai-provider-title">Connect a model</h3>
        </div>
      </div>

      <div class="ai-fields">
        <label class="wide">
          <span>Base URL</span>
          <input type="url" value={settings.aiBaseUrl} onblur={(event) => void update({ aiBaseUrl: event.currentTarget.value })} placeholder="https://provider.example/v1" autocomplete="off" spellcheck="false" />
          <small>Remote providers require HTTPS. Local HTTP endpoints are supported.</small>
        </label>
        <label class="wide">
          <span>Model name</span>
          <input value={settings.aiModelName} onblur={(event) => void update({ aiModelName: event.currentTarget.value })} placeholder="model-name" autocomplete="off" spellcheck="false" />
        </label>
        <div class="ai-number-grid">
          <label>
            <span>Timeout <em>(seconds)</em></span>
            <input type="number" min="1" max="600" value={settings.aiRequestTimeoutSeconds} onblur={(event) => void update({ aiRequestTimeoutSeconds: Number(event.currentTarget.value) })} />
          </label>
          <label>
            <span>Turn limit <em>(steps)</em></span>
            <input type="number" min="1" max="32" value={settings.aiTurnStepLimit} onblur={(event) => void update({ aiTurnStepLimit: Number(event.currentTarget.value) })} />
          </label>
        </div>
      </div>

      <div class="ai-card-footer">
        <div class:success={connectionTone === "success"} class:error={connectionTone === "error"} class="connection-state" aria-live="polite">
          {#if connectionTone === "busy"}<span class="mini-spinner" aria-hidden="true"></span>{:else}<span class="status-dot" aria-hidden="true"></span>{/if}
          <span>{connectionState || "Provider not tested yet"}</span>
        </div>
        <button class="text-button primary-action ai-action" type="button" disabled={keyBusy || connectionTone === "busy" || !settings.aiBaseUrl || !settings.aiModelName} onclick={() => void testConnection()}>Test connection</button>
      </div>
    </section>

    <section class="ai-card key-card" aria-labelledby="ai-key-title">
      <div class="card-heading">
        <span class="card-icon key-icon" aria-hidden="true">
          <svg viewBox="0 0 24 24" fill="none"><circle cx="8.5" cy="15.5" r="3.5" /><path d="m11 13 8-8M16 7l2 2M14 9l2 2" /></svg>
        </span>
        <div>
          <p class="section-kicker">ACCESS</p>
          <h3 id="ai-key-title">Provider key</h3>
        </div>
      </div>

      {#if settings.aiApiKeyConfigured}
        <div class="saved-key-card">
          <div class="saved-key-heading"><span class="saved-key-check" aria-hidden="true">✓</span><span>Connected</span><span class="saved-key-label">Application-wide</span></div>
          <code>{displayAiKey(settings.aiApiKeyPrefix, settings.aiApiKeySuffix)}</code>
          <small>Only the first and last three characters are displayed.</small>
        </div>
        <div class="key-actions">
          <button class="text-button" type="button" disabled={keyBusy || deleteConfirm} onclick={beginReplace}>Replace key</button>
          <button class="text-button danger delete-key-action" type="button" disabled={keyBusy} onclick={requestDeleteKey}>Delete API key</button>
        </div>
      {/if}

      {#if !settings.aiApiKeyConfigured || showKeyEntry}
        {#if settings.aiApiKeyConfigured && showKeyEntry}<div class="replace-note">Enter a new key to replace the current one.</div>{/if}
        <label class="wide key-input-label">
          <span>{settings.aiApiKeyConfigured ? "New provider key" : "Provider key"}</span>
          <input type="password" bind:value={newKey} autocomplete="new-password" placeholder="Enter once; it will not be shown again" disabled={keyBusy} />
        </label>
        <div class="key-entry-actions">
          <button class="text-button primary-action ai-action" type="button" disabled={keyBusy || !newKey} onclick={() => void saveKey()}>{keyBusy ? "Saving…" : "Save key"}</button>
          {#if settings.aiApiKeyConfigured}<button class="text-button" type="button" disabled={keyBusy} onclick={cancelReplace}>Cancel</button>{/if}
        </div>
      {/if}
      {#if keyState}<small class:success={keyState.startsWith("Connected") || keyState === "Key removed"} class="key-state" aria-live="polite">{keyState}</small>{/if}
    </section>
  </div>

  <p class="ai-footnote"><span aria-hidden="true">✦</span> AI settings are application-wide. Project data never contains the saved provider key.</p>

  <ConfirmDialog
    open={deleteConfirm}
    title="Delete provider key?"
    message="Remove the saved API key from this application?"
    confirmLabel="Delete API key"
    busy={keyBusy}
    onConfirm={() => void deleteKey()}
    onCancel={cancelDeleteKey}
  />
</div>

<style>
  /* Keep AI settings in the same compact fieldset language as the other settings pages. */
  .settings-form.ai-settings {
    margin-top: 6px;
    grid-template-columns: minmax(0, 1fr);
    width: min(760px, 100%);
    gap: 6px;
  }

  .ai-settings,
  .ai-settings * { box-sizing: border-box; }

  .ai-settings {
    --ai-toggle-success: var(--success, #279630);
    --ai-test-button-success: var(--success, #279630);
  }

  .ai-card {
    display: grid;
    align-content: start;
    gap: 11px;
    min-width: 0;
    margin: 0;
    padding: 15px;
    border: 1px solid var(--border, #2c333d);
    border-radius: 5px;
    color: var(--text, #d7dde5);
    background: var(--surface, #12161b);
  }

  .ai-controller-card {
    grid-template-columns: repeat(2, minmax(0, 1fr));
    margin-bottom: 14px;
  }

  .ai-controller-card .card-heading { grid-column: 1 / -1; }
  .ai-grid { display: grid; grid-template-columns: 1fr; gap: 14px; margin-bottom: 14px; }

  .card-heading { display: flex; align-items: flex-start; gap: 8px; width: fit-content; max-width: 100%; min-width: 0; margin-top: -24px; padding: 0 6px; background: var(--surface, #12161b); }
  .card-heading > div { min-width: 0; }
  .card-icon { display: none; }
  .section-kicker { display: none; }
  .card-heading h3 { margin: 0; color: var(--text, #d7dde5); font-size: var(--font-size-body, 12px); font-weight: 700; }
  .card-heading p:not(.section-kicker) { margin: 2px 0 0; color: var(--muted, #737e8b); font-size: var(--font-size-compact, 10px); line-height: 1.5; }

  .ai-toggle-row {
    display: flex !important;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    min-height: 30px;
    padding: 0;
    border: 0;
    border-radius: 0;
    cursor: pointer;
  }
  .ai-controller-card .ai-toggle-row + .ai-toggle-row { padding-left: 12px; border-left: 1px solid var(--border, #2c333d); }
  .ai-toggle-row :global(.toggle-control input:checked + .toggle-track) {
    border-color: color-mix(in srgb, var(--ai-toggle-success) 70%, #4b5563);
    background: color-mix(in srgb, var(--ai-toggle-success) 55%, #202630);
  }
  .ai-toggle-copy { display: grid; gap: 2px; }
  .ai-toggle-copy strong { color: var(--text, #d7dde5); font-size: var(--font-size-compact, 10px); font-weight: 650; }
  .ai-toggle-copy small { color: var(--muted, #737e8b); font-size: var(--font-size-compact, 10px); }

  .ai-fields { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 11px; }
  .ai-fields > .wide,
  .ai-number-grid { grid-column: 1 / -1; }
  .ai-number-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 8px; }
  .ai-settings label { display: grid; align-content: start; gap: 5px; min-width: 0; color: var(--muted, #929ba8); font-size: var(--font-size-compact, 10px); }
  .ai-settings label > span { font-weight: 650; }
  .ai-settings label em { color: var(--muted, #737e8b); font-size: var(--font-size-compact, 10px); font-style: normal; font-weight: 500; }
  .ai-settings input { width: 100%; height: 30px; min-height: 30px; padding: 0 8px; border: 1px solid var(--border-strong, #3a424d); border-radius: 4px; color: var(--text, #dbe1e8); background: var(--input, #0c0f13); font-size: var(--font-size-body, 12px); outline: none; }
  .ai-settings input:hover { border-color: color-mix(in srgb, var(--border-strong, #3a424d) 65%, var(--accent, #9ca3af)); }
  .ai-settings input:focus { border-color: var(--accent, #9ca3af); box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent, #9ca3af) 15%, transparent); }
  .ai-settings label small { color: var(--muted, #737e8b); font-size: var(--font-size-compact, 10px); line-height: 1.4; }

  .ai-card-footer,
  .key-actions,
  .key-entry-actions { display: flex; align-items: center; justify-content: space-between; gap: 8px; flex-wrap: wrap; }
  .connection-state { display: inline-flex; align-items: center; gap: 6px; min-width: 0; color: var(--muted, #737e8b); font-size: var(--font-size-compact, 10px); }
  .connection-state.success { color: var(--success, #279630); }
  .connection-state.error { color: var(--danger, #d92d4b); }
  .status-dot { display: inline-block; width: 6px; height: 6px; border-radius: 50%; background: currentColor; }
  .mini-spinner { width: 10px; height: 10px; border: 1.5px solid color-mix(in srgb, var(--accent, #9ca3af) 30%, transparent); border-top-color: var(--accent, #9ca3af); border-radius: 50%; animation: ai-spin .8s linear infinite; }
  .ai-action { min-width: 0; }
  .ai-grid > .ai-card:not(.key-card) .ai-action {
    color: #fff;
    border-color: var(--ai-test-button-success);
    background: var(--ai-test-button-success);
  }
  .ai-grid > .ai-card:not(.key-card) .ai-action:hover:not(:disabled) {
    color: #fff;
    border-color: color-mix(in srgb, var(--ai-test-button-success) 82%, #fff);
    background: color-mix(in srgb, var(--ai-test-button-success) 82%, #000);
  }

  .saved-key-card { display: grid; gap: 6px; padding: 10px; border: 1px solid color-mix(in srgb, var(--success, #279630) 40%, var(--border, #2c333d)); border-radius: 4px; background: color-mix(in srgb, var(--success, #279630) 6%, var(--surface-2, #1b2028)); }
  .saved-key-heading { display: flex; align-items: center; gap: 6px; color: var(--success, #279630); font-size: var(--font-size-compact, 10px); font-weight: 700; }
  .saved-key-check { display: grid; width: 15px; height: 15px; place-items: center; border-radius: 50%; color: var(--surface, #12161b); background: var(--success, #279630); font-size: 10px; }
  .saved-key-label { margin-left: auto; color: var(--muted, #737e8b); font-size: var(--font-size-compact, 10px); font-weight: 500; }
  .saved-key-card code { color: var(--text, #d7dde5); font-size: var(--font-size-body, 12px); letter-spacing: .08em; user-select: text; }
  .saved-key-card small,
  .replace-note,
  .key-state { color: var(--muted, #737e8b); font-size: var(--font-size-compact, 10px); line-height: 1.4; }
  .replace-note { padding: 7px 8px; border-left: 2px solid var(--accent, #9ca3af); background: color-mix(in srgb, var(--accent, #9ca3af) 8%, var(--surface-2, #1b2028)); }
  .key-entry-actions { justify-content: flex-start; }
  :global(.ai-settings .key-actions button.delete-key-action) { color: #fff; border-color: var(--danger, #d92d4b); background: var(--danger, #d92d4b); }
  :global(.ai-settings .key-actions button.delete-key-action:hover:not(:disabled)) { color: #fff; border-color: color-mix(in srgb, var(--danger, #d92d4b) 82%, #fff); background: color-mix(in srgb, var(--danger, #d92d4b) 82%, #000); }
  .key-state.success { color: var(--success, #279630); }
  .danger { color: var(--danger, #d92d4b); }
  .ai-footnote { display: flex; align-items: center; gap: 6px; margin: 0; padding: 0 2px; color: var(--muted, #737e8b); font-size: var(--font-size-compact, 10px); line-height: 1.45; }
  .ai-footnote span { color: var(--accent, #9ca3af); font-size: 12px; }

  @keyframes ai-spin { to { transform: rotate(360deg); } }

  @media (max-width: 460px) {
    .ai-controller-card,
    .ai-fields,
    .ai-number-grid { grid-template-columns: 1fr; }
    .ai-controller-card .ai-toggle-row + .ai-toggle-row { padding-left: 0; border-left: 0; }
    .ai-controller-card .card-heading,
    .ai-fields > .wide,
    .ai-number-grid { grid-column: auto; }
    .ai-card-footer { align-items: stretch; flex-direction: column; }
    .ai-action { width: 100%; }
  }
</style>
