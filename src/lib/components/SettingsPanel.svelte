<script lang="ts">
  import { onMount } from "svelte";
  import { getVersion } from "@tauri-apps/api/app";
  import { isTauri } from "$lib/api";
  import InterceptionRules from "$lib/components/InterceptionRules.svelte";
  import MatchReplaceRules from "$lib/components/MatchReplaceRules.svelte";
  import AiSettings from "$lib/components/AiSettings.svelte";
  import Toggle from "$lib/components/Toggle.svelte";
  import { showErrorToast } from "$lib/errorToast";
  import { showInfoToast } from "$lib/infoToast";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import {
    checkForUpdate,
    closeUpdate,
    getUpdateMode,
    installUpdateAndRelaunch,
    setUpdateMode,
    type UpdateMode,
  } from "$lib/updater";
  import type { Update } from "@tauri-apps/plugin-updater";
  import {
    SHORTCUTS,
    SHORTCUT_GROUP_LABELS,
    SHORTCUT_GROUP_ORDER,
    detectShortcutPlatform,
    formatShortcut,
    formatShortcutParts,
    normalizeShortcutModifier,
    type ShortcutPlatform,
    type ShortcutScope,
  } from "$lib/keyboard-shortcuts";
  import type {
    CertificateInfo,
    InterceptionContentType,
    InterceptionMode,
    InterceptionRule,
    SettingsPatch,
    SettingsSection,
    SettingsState,
  } from "$lib/types";

  let {
    settings,
    proxyRunning,
    selectedSection,
    onSectionChange,
    onUpdate,
    onGenerateCertificate,
    onReplayTutorial,
    onBeforeInstall,
  }: {
    settings: SettingsState;
    proxyRunning: boolean;
    selectedSection: SettingsSection;
    onSectionChange: (section: SettingsSection) => void;
    onUpdate: (patch: SettingsPatch) => Promise<SettingsState>;
    onGenerateCertificate: () => Promise<CertificateInfo>;
    onReplayTutorial?: () => void;
    onBeforeInstall?: (version: string | null) => Promise<boolean>;
  } = $props();

  const shortcutPlatform: ShortcutPlatform = detectShortcutPlatform();
  const macOS = shortcutPlatform === "macos";
  const shortcutGroups = SHORTCUT_GROUP_ORDER.map((scope) => ({
    scope,
    label: SHORTCUT_GROUP_LABELS[scope],
    definitions: SHORTCUTS.filter((definition) => definition.scope === scope),
  }));
  let selectedShortcutScope = $state<ShortcutScope>(SHORTCUT_GROUP_ORDER[0]);
  const activeShortcutGroup = $derived(
    shortcutGroups.find((group) => group.scope === selectedShortcutScope) ??
      shortcutGroups[0],
  );

  function shortcutScopeTabId(scope: ShortcutScope) {
    return `keyboard-shortcut-tab-${scope.toLowerCase().replace(/[^a-z0-9]+/g, "-")}`;
  }

  function shortcutScopePanelId(scope: ShortcutScope) {
    return `keyboard-shortcut-panel-${scope.toLowerCase().replace(/[^a-z0-9]+/g, "-")}`;
  }

  function focusShortcutScope(scope: ShortcutScope) {
    selectedShortcutScope = scope;
    window.requestAnimationFrame(() =>
      document.getElementById(shortcutScopeTabId(scope))?.focus(),
    );
  }

  function handleShortcutScopeKeydown(
    event: KeyboardEvent,
    scope: ShortcutScope,
  ) {
    const currentIndex = SHORTCUT_GROUP_ORDER.indexOf(scope);
    if (currentIndex < 0) return;
    let nextIndex: number | null = null;
    if (event.key === "ArrowRight")
      nextIndex = (currentIndex + 1) % SHORTCUT_GROUP_ORDER.length;
    else if (event.key === "ArrowLeft")
      nextIndex =
        (currentIndex - 1 + SHORTCUT_GROUP_ORDER.length) %
        SHORTCUT_GROUP_ORDER.length;
    else if (event.key === "Home") nextIndex = 0;
    else if (event.key === "End") nextIndex = SHORTCUT_GROUP_ORDER.length - 1;
    if (nextIndex === null) return;
    event.preventDefault();
    focusShortcutScope(SHORTCUT_GROUP_ORDER[nextIndex]);
  }

  function settingsDraft(): SettingsState {
    return {
      ...settings,
      theme: "dark",
      proxyInterceptMode:
        settings.proxyInterceptMode ??
        (settings.proxyIntercepting ? "allRequests" : "none"),
      interceptContentTypes: settings.interceptContentTypes ?? [],
      requestInterceptionRules: settings.requestInterceptionRules ?? [],
      responseInterceptionRules: settings.responseInterceptionRules ?? [],
      matchReplaceRules: settings.matchReplaceRules ?? [],
      upstreamProxy: settings.upstreamProxy ?? {
        enabled: false,
        kind: "http",
        host: "",
        port: 8080,
        username: "",
        password: "",
      },
      shortcutModifier: normalizeShortcutModifier(
        settings.shortcutModifier,
        shortcutPlatform,
      ),
    };
  }

  let draft = $state<SettingsState>(settingsDraft());
  let appVersion = $state<string | null>(null);
  let saving = $state(false);
  let saveState = $state("");
  let certificateState = $state("");
  let saveTimer: ReturnType<typeof setTimeout> | undefined;
  let ruleSet = $state<"request" | "response">("request");
  let updateMode = $state<UpdateMode>("auto-check");
  let updaterBusy = $state(false);
  let updaterStatus = $state("");
  let updaterProgress = $state<number | null>(null);
  let pendingUpdate: Update | null = $state(null);
  let pendingVersion = $state<string | null>(null);
  const interceptRequests = $derived(
    draft.proxyInterceptMode === "allRequests" ||
      draft.proxyInterceptMode === "requestsAndResponses",
  );
  const interceptResponses = $derived(
    draft.proxyInterceptMode === "allResponses" ||
      draft.proxyInterceptMode === "requestsAndResponses",
  );
  onMount(() => {
    updateMode = getUpdateMode();
    if (!isTauri()) return;
    void getVersion()
      .then((version) => (appVersion = version))
      .catch(() => {});
  });

  function setUpdateModeChoice(mode: UpdateMode) {
    updateMode = mode;
    setUpdateMode(mode);
  }

  async function discardPendingUpdate() {
    const previous = pendingUpdate;
    pendingUpdate = null;
    pendingVersion = null;
    updaterProgress = null;
    await closeUpdate(previous);
  }

  async function manualCheckForUpdates() {
    if (updaterBusy) return;
    if (!isTauri()) {
      updaterStatus = "Update checks are only available in the desktop app.";
      return;
    }
    updaterBusy = true;
    updaterStatus = "Checking for updates…";
    updaterProgress = null;
    await discardPendingUpdate();
    try {
      const update = await checkForUpdate();
      if (!update) {
        updaterStatus = "You're up to date.";
        return;
      }
      pendingUpdate = update;
      pendingVersion = update.version;
      updaterStatus = `Version ${update.version} is available.`;
    } catch (reason) {
      updaterStatus = `Check failed: ${reason instanceof Error ? reason.message : String(reason)}`;
      showErrorToast(reason);
    } finally {
      updaterBusy = false;
    }
  }

  async function installPendingUpdate() {
    const update = pendingUpdate;
    if (!update || updaterBusy) return;
    if (onBeforeInstall) {
      const allowed = await onBeforeInstall(pendingVersion);
      if (!allowed) {
        updaterStatus = `Update to version ${update.version} cancelled — staying on this version.`;
        return;
      }
    }
    updaterBusy = true;
    updaterProgress = 0;
    updaterStatus = `Downloading version ${update.version}…`;
    try {
      await installUpdateAndRelaunch(update, (percent) => {
        if (percent !== null) updaterProgress = percent;
        updaterStatus =
          percent === 100
            ? "Installing… the app will restart."
            : `Downloading version ${update.version}… ${percent ?? 0}%`;
      });
      updaterStatus = "Restarting into the new version…";
    } catch (reason) {
      updaterStatus = `Update failed: ${reason instanceof Error ? reason.message : String(reason)}. You can keep using this version.`;
      showErrorToast(reason);
      updaterBusy = false;
    }
  }

  const sections: {
    id: SettingsSection;
    label: string;
    description: string;
  }[] = [
    {
      id: "proxy",
      label: "Proxy",
      description: "Listener, traffic and interception",
    },
    {
      id: "display",
      label: "Display",
      description: "Theme and workspace layout",
    },
    { id: "storage", label: "Storage", description: "Autosave and retention" },
    {
      id: "keyboard",
      label: "Keyboard",
      description:
        "Fixed application shortcuts and the macOS modifier preference.",
    },
    {
      id: "certificates",
      label: "Certificates",
      description: "HTTPS interception authority",
    },
    { id: "ai", label: "AI Inference", description: "Assistant workspace and provider" },
    {
      id: "miscellaneous",
      label: "Miscellaneous",
      description: "Optional tools and preferences",
    },
    {
      id: "updates",
      label: "Updates",
      description: "Automatic checks and installation",
    },
    { id: "about", label: "About", description: "Witness and North Core Labs" },
  ];
  const activeSection = $derived(
    sections.find((section) => section.id === selectedSection) ?? sections[0],
  );
  const contentTypeFilters: {
    value: InterceptionContentType;
    label: string;
    detail: string;
  }[] = [
    { value: "html", label: "HTML", detail: ".html" },
    { value: "javascript", label: "JavaScript", detail: ".js, .mjs" },
    { value: "css", label: "CSS", detail: ".css" },
    { value: "json", label: "JSON", detail: ".json" },
    { value: "xml", label: "XML / SVG", detail: ".xml, .svg" },
    { value: "images", label: "Images", detail: "image/*" },
    { value: "fonts", label: "Fonts", detail: "woff, ttf" },
    { value: "media", label: "Media", detail: "audio/video" },
    { value: "documents", label: "Documents", detail: "pdf, office" },
    { value: "other", label: "Other", detail: "uncategorised" },
  ];

  $effect(() => {
    if (!draft.theme) Object.assign(draft, settingsDraft());
    else if (draft.theme === "light") draft.theme = "dark";
  });

  function updateField<K extends keyof SettingsState>(
    field: K,
    value: SettingsState[K],
    delayed = false,
  ) {
    const previous = draft[field];
    draft[field] = value;
    saveState = "Saving…";
    if (saveTimer) clearTimeout(saveTimer);
    if (delayed) {
      saveTimer = setTimeout(
        () => void save({ [field]: value }, () => (draft[field] = previous)),
        350,
      );
    } else {
      void save({ [field]: value }, () => (draft[field] = previous));
    }
  }

  function setShortcutModifier(value: "command" | "control") {
    if (!macOS) return;
    updateField("shortcutModifier", value);
  }

  function updateUpstreamProxy<K extends keyof SettingsState["upstreamProxy"]>(
    field: K,
    value: SettingsState["upstreamProxy"][K],
    delayed = false,
  ) {
    const upstreamProxy = { ...draft.upstreamProxy, [field]: value };
    updateField("upstreamProxy", upstreamProxy, delayed);
  }

  async function save(patch: SettingsPatch, rollback?: () => void) {
    saving = true;
    try {
      await onUpdate(patch);
      saveState = "Saved";
    } catch {
      rollback?.();
      saveState = "";
    } finally {
      saving = false;
    }
  }

  async function generateCertificate() {
    certificateState = "Generating…";
    try {
      const certificate = await onGenerateCertificate();
      certificateState = certificate.generated
        ? `Generated: ${certificate.certificatePath}`
        : `Ready: ${certificate.certificatePath}`;
    } catch (reason) {
      certificateState = "";
      showErrorToast(reason);
    }
  }

  function setInterceptionDirection(
    direction: "request" | "response",
    enabled: boolean,
  ) {
    const requests = direction === "request" ? enabled : interceptRequests;
    const responses = direction === "response" ? enabled : interceptResponses;
    const mode: InterceptionMode =
      requests && responses
        ? "requestsAndResponses"
        : requests
          ? "allRequests"
          : responses
            ? "allResponses"
            : "none";
    const masterEnabled = mode !== "none";
    draft.proxyInterceptMode = mode;
    draft.proxyIntercepting = masterEnabled;
    saveState = "Saving…";
    void save({ proxyInterceptMode: mode, proxyIntercepting: masterEnabled });
  }

  function toggleContentType(contentType: InterceptionContentType) {
    const filters = draft.interceptContentTypes.includes(contentType)
      ? draft.interceptContentTypes.filter((value) => value !== contentType)
      : [...draft.interceptContentTypes, contentType];
    updateField("interceptContentTypes", filters);
  }

  function updateRules(
    kind: "request" | "response",
    rules: InterceptionRule[],
  ) {
    if (kind === "request") updateField("requestInterceptionRules", rules);
    else updateField("responseInterceptionRules", rules);
  }

  function updateMatchReplaceRules(
    rules: import("$lib/types").MatchReplaceRule[],
  ) {
    updateField("matchReplaceRules", rules);
  }

  async function openExternal(url: string) {
    try {
      await openUrl(url);
    } catch {
      window.open(url, "_blank", "noopener");
    }
  }
</script>

<section class="settings-tool">
  <header class="settings-header">
    <h1>Settings</h1>
    <div class="save-state" aria-live="polite">
      {saving ? "Saving…" : saveState}
    </div>
  </header>

  <div class="settings-layout">
    <aside class="settings-sidebar" aria-label="Settings categories">
      <div class="sidebar-title">
        <strong>Application settings</strong><small
          >Select a category to configure Witness.</small
        >
      </div>
      <nav>
        {#each sections as section}
          <button
            type="button"
            class:active={selectedSection === section.id}
            aria-current={selectedSection === section.id ? "page" : undefined}
            data-section={section.id}
            data-tour={`settings-section-${section.id}`}
            onclick={() => onSectionChange(section.id)}
          >
            <span>{section.label}</span>
            <small>{section.description}</small>
          </button>
        {/each}
      </nav>
    </aside>

    <div class="settings-pane">
      <div class="pane-heading">
        <h2>{activeSection.label}</h2>
        <p>{activeSection.description}</p>
      </div>

      <div class="pane-body">
        {#if selectedSection === "proxy"}
          <div class="settings-form">
            <fieldset class="listener-settings" data-tour="proxy-listener">
              <legend>Listener</legend>
              <label
                ><span>Bind address</span><input
                  value={draft.proxyBindAddress}
                  oninput={(event) =>
                    updateField(
                      "proxyBindAddress",
                      event.currentTarget.value,
                      true,
                    )}
                  required
                /></label
              >
              <label
                ><span>Port</span><input
                  type="number"
                  min="1"
                  max="65535"
                  value={draft.proxyPort}
                  oninput={(event) =>
                    updateField(
                      "proxyPort",
                      Number(event.currentTarget.value),
                      true,
                    )}
                  required
                /></label
              >
              {#if proxyRunning}<p class="notice">
                  Changing listener settings requires the running proxy to
                  restart.
                </p>{/if}
            </fieldset>

            <fieldset class="traffic-settings" data-tour="proxy-traffic">
              <legend>Traffic handling</legend>
              <label
                ><span>Compression behaviour</span><select
                  value={draft.compressionMode}
                  onchange={(event) =>
                    updateField(
                      "compressionMode",
                      event.currentTarget
                        .value as SettingsState["compressionMode"],
                    )}
                  ><option value="decompressAll"
                    >Decompress all supported</option
                  ><option value="decompressText"
                    >Decompress text formats</option
                  ><option value="passThrough">Pass through unchanged</option
                  ></select
                ></label
              >
              <label
                ><span>Upstream timeout (seconds)</span><input
                  type="number"
                  min="1"
                  max="300"
                  value={draft.upstreamTimeoutSeconds}
                  oninput={(event) =>
                    updateField(
                      "upstreamTimeoutSeconds",
                      Number(event.currentTarget.value),
                      true,
                    )}
                /></label
              >
              <label class="check"
                ><Toggle
                  checked={draft.interceptInScopeOnly}
                  ariaLabel="Only intercept in-scope traffic"
                  onchange={(event) =>
                    updateField(
                      "interceptInScopeOnly",
                      event.currentTarget.checked,
                    )}
                /><span>Only intercept in-scope traffic</span></label
              >
            </fieldset>

            <fieldset class="upstream-settings" data-tour="proxy-upstream">
              <legend>Upstream proxy</legend>
              <p class="notice">
                Route Witness's own outbound traffic (to origin servers) through
                another HTTP or SOCKS5 proxy. This is separate from the listener
                above, which is what browsers connect to.
              </p>
              <label class="check"
                ><Toggle
                  checked={draft.upstreamProxy.enabled}
                  ariaLabel="Forward outbound traffic through an upstream proxy"
                  onchange={(event) =>
                    updateUpstreamProxy("enabled", event.currentTarget.checked)}
                /><span>Forward outbound traffic through an upstream proxy</span
                ></label
              >
              <label
                ><span>Proxy type</span><select
                  value={draft.upstreamProxy.kind}
                  onchange={(event) =>
                    updateUpstreamProxy(
                      "kind",
                      event.currentTarget
                        .value as SettingsState["upstreamProxy"]["kind"],
                    )}
                  ><option value="http">HTTP</option><option value="socks5"
                    >SOCKS5</option
                  ></select
                ></label
              >
              <label
                ><span>Host</span><input
                  value={draft.upstreamProxy.host}
                  oninput={(event) =>
                    updateUpstreamProxy(
                      "host",
                      event.currentTarget.value,
                      true,
                    )}
                  placeholder="e.g. 127.0.0.1"
                /></label
              >
              <label
                ><span>Port</span><input
                  type="number"
                  min="1"
                  max="65535"
                  value={draft.upstreamProxy.port}
                  oninput={(event) =>
                    updateUpstreamProxy(
                      "port",
                      Number(event.currentTarget.value),
                      true,
                    )}
                /></label
              >
              <label
                ><span>Username <em>(optional)</em></span><input
                  value={draft.upstreamProxy.username}
                  oninput={(event) =>
                    updateUpstreamProxy(
                      "username",
                      event.currentTarget.value,
                      true,
                    )}
                  autocomplete="off"
                /></label
              >
              <label
                ><span>Password <em>(optional)</em></span><input
                  type="password"
                  value={draft.upstreamProxy.password}
                  oninput={(event) =>
                    updateUpstreamProxy(
                      "password",
                      event.currentTarget.value,
                      true,
                    )}
                  autocomplete="off"
                /></label
              >
            </fieldset>

            <fieldset
              class="match-replace-settings"
              data-tour="proxy-match-replace"
            >
              <legend>Match and replace</legend>
              <MatchReplaceRules
                rules={draft.matchReplaceRules}
                onChange={updateMatchReplaceRules}
              />
            </fieldset>

            <fieldset
              class="interception-settings"
              data-tour="proxy-interception"
            >
              <legend>Interception</legend>
              <div class="interception-intro">
                <strong>Interception behavior</strong>
                <small
                  >Control which traffic is paused for viewing and editing in
                  the Intercept tab. Switching modes immediately forwards any
                  currently paused traffic.</small
                >
              </div>
              <div
                class="interception-modes"
                role="group"
                aria-label="Interception directions"
              >
                <label class:active={interceptRequests}>
                  <Toggle
                    checked={interceptRequests}
                    ariaLabel="Intercept requests"
                    onchange={(event) =>
                      setInterceptionDirection(
                        "request",
                        event.currentTarget.checked,
                      )}
                  />
                  <span
                    ><strong>Intercept requests</strong><small
                      >Pause matching traffic before it reaches the server.</small
                    ></span
                  >
                </label>
                <label class:active={interceptResponses}>
                  <Toggle
                    checked={interceptResponses}
                    ariaLabel="Intercept responses"
                    onchange={(event) =>
                      setInterceptionDirection(
                        "response",
                        event.currentTarget.checked,
                      )}
                  />
                  <span
                    ><strong>Intercept responses</strong><small
                      >Pause matching traffic after it returns from the server.</small
                    ></span
                  >
                </label>
              </div>

              <div class="content-filters">
                <div>
                  <strong>Data-type filter</strong><small
                    >Optional. Leave every type unselected to match all traffic;
                    select one or more types to intercept only those types.</small
                  >
                </div>
                <div class="filter-list">
                  {#each contentTypeFilters as filter}
                    <label
                      class:active={draft.interceptContentTypes.includes(
                        filter.value,
                      )}
                      data-tooltip={filter.detail}
                    >
                      <Toggle
                        checked={draft.interceptContentTypes.includes(
                          filter.value,
                        )}
                        ariaLabel={filter.label}
                        onchange={() => toggleContentType(filter.value)}
                      />
                      <span>{filter.label}</span>
                    </label>
                  {/each}
                </div>
              </div>

              <div
                class="rule-tabs"
                role="tablist"
                aria-label="Interception rule sets"
              >
                <button
                  type="button"
                  class:active={ruleSet === "request"}
                  role="tab"
                  aria-selected={ruleSet === "request"}
                  onclick={() => (ruleSet = "request")}>Request rules</button
                >
                <button
                  type="button"
                  class:active={ruleSet === "response"}
                  role="tab"
                  aria-selected={ruleSet === "response"}
                  onclick={() => (ruleSet = "response")}>Response rules</button
                >
              </div>
              {#if ruleSet === "request"}
                <InterceptionRules
                  kind="request"
                  rules={draft.requestInterceptionRules}
                  onChange={(rules) => updateRules("request", rules)}
                />
              {:else}
                <InterceptionRules
                  kind="response"
                  rules={draft.responseInterceptionRules}
                  onChange={(rules) => updateRules("response", rules)}
                />
              {/if}
            </fieldset>
          </div>
        {:else if selectedSection === "display"}
          <div class="settings-form single-section">
            <fieldset>
              <legend>Appearance & layout</legend>
              <label
                ><span>Theme</span><select
                  value={draft.theme}
                  onchange={(event) => {
                    const value = event.currentTarget
                      .value as SettingsState["theme"];
                    if (value === "light") {
                      showInfoToast("Light mode is under development!");
                      event.currentTarget.value = "dark";
                      draft.theme = "dark";
                      return;
                    }
                    updateField("theme", value);
                  }}
                  ><option value="dark">Dark</option><option value="light"
                    >Light</option
                  ></select
                ></label
              >
              <label>
                <span>Interface font size</span>
                <input
                  type="number"
                  min="10"
                  max="24"
                  value={draft.fontSize}
                  oninput={(event) =>
                    updateField(
                      "fontSize",
                      Number(event.currentTarget.value),
                      true,
                    )}
                />
                <small
                  >Scales labels, controls, headings, and titles consistently.</small
                >
              </label>
              <label>
                <span>Message editor font size</span>
                <input
                  type="number"
                  min="9"
                  max="24"
                  value={draft.messageEditorFontSize}
                  oninput={(event) =>
                    updateField(
                      "messageEditorFontSize",
                      Number(event.currentTarget.value),
                      true,
                    )}
                />
                <small
                  >Only changes raw, pretty, and hexadecimal message content.</small
                >
              </label>
              <label
                ><span>History panel size (%)</span><input
                  type="number"
                  min="20"
                  max="75"
                  value={draft.layoutSplitPercent}
                  oninput={(event) =>
                    updateField(
                      "layoutSplitPercent",
                      Number(event.currentTarget.value),
                      true,
                    )}
                /></label
              >
            </fieldset>
          </div>
        {:else if selectedSection === "storage"}
          <div class="settings-form single-section">
            <fieldset>
              <legend>Project storage & retention</legend>
              <label
                ><span>Autosave interval (seconds)</span><input
                  type="number"
                  min="1"
                  max="3600"
                  value={draft.autosaveIntervalSeconds}
                  oninput={(event) =>
                    updateField(
                      "autosaveIntervalSeconds",
                      Number(event.currentTarget.value),
                      true,
                    )}
                /></label
              >
              <label
                ><span>History size limit</span><input
                  type="number"
                  min="100"
                  max="1000000"
                  step="100"
                  value={draft.historyLimit}
                  oninput={(event) =>
                    updateField(
                      "historyLimit",
                      Number(event.currentTarget.value),
                      true,
                    )}
                /></label
              >
            </fieldset>
          </div>
        {:else if selectedSection === "keyboard"}
          <div class="keyboard-settings">
            <fieldset class="modifier-settings">
              <legend>Primary modifier</legend>
              {#if macOS}
                <div
                  class="modifier-options"
                  role="radiogroup"
                  aria-label="Primary modifier"
                >
                  <label class:active={draft.shortcutModifier === "command"}>
                    <input
                      type="radio"
                      name="shortcut-modifier"
                      value="command"
                      checked={draft.shortcutModifier === "command"}
                      onchange={() => setShortcutModifier("command")}
                    />
                    <span
                      ><strong>Command (⌘)</strong><small
                        >Default on macOS</small
                      ></span
                    >
                  </label>
                  <label class:active={draft.shortcutModifier === "control"}>
                    <input
                      type="radio"
                      name="shortcut-modifier"
                      value="control"
                      checked={draft.shortcutModifier === "control"}
                      onchange={() => setShortcutModifier("control")}
                    />
                    <span
                      ><strong>Control (⌃)</strong><small
                        >Alternative application modifier</small
                      ></span
                    >
                  </label>
                </div>
                <p class="keyboard-note">
                  This changes the modifier Witness uses for its application
                  shortcuts. It does not change macOS or editor shortcuts such
                  as copy, paste, undo, or text navigation.
                </p>
              {:else}
                <p class="keyboard-note">
                  Witness uses Control (Ctrl) for application shortcuts on this
                  platform.
                </p>
              {/if}
            </fieldset>

            <div
              class="shortcut-reference"
              aria-label="Keyboard shortcut reference"
            >
              <div class="reference-heading">
                <div>
                  <h3>Shortcut reference</h3>
                  <p>Base keys are fixed and cannot be remapped.</p>
                </div>
              </div>
              <div
                class="shortcut-category-tabs"
                role="tablist"
                aria-label="Shortcut categories"
              >
                {#each shortcutGroups as group}
                  <button
                    id={shortcutScopeTabId(group.scope)}
                    class:active={selectedShortcutScope === group.scope}
                    type="button"
                    role="tab"
                    aria-selected={selectedShortcutScope === group.scope}
                    aria-controls={selectedShortcutScope === group.scope
                      ? shortcutScopePanelId(group.scope)
                      : undefined}
                    tabindex={selectedShortcutScope === group.scope ? 0 : -1}
                    onclick={() => (selectedShortcutScope = group.scope)}
                    onkeydown={(event) =>
                      handleShortcutScopeKeydown(event, group.scope)}
                    >{group.label}</button
                  >
                {/each}
              </div>
              {#if activeShortcutGroup}
                <div
                  id={shortcutScopePanelId(activeShortcutGroup.scope)}
                  class="shortcut-group"
                  role="tabpanel"
                  aria-labelledby={shortcutScopeTabId(
                    activeShortcutGroup.scope,
                  )}
                  tabindex="0"
                >
                  <h4>{activeShortcutGroup.label}</h4>
                  <div
                    class="shortcut-table"
                    role="table"
                    aria-label={`${activeShortcutGroup.label} shortcuts`}
                  >
                    <div
                      class="shortcut-table-row shortcut-table-header"
                      role="row"
                    >
                      <span role="columnheader">Action</span><span
                        role="columnheader">Shortcut</span
                      ><span role="columnheader">Scope</span><span
                        role="columnheader">Availability</span
                      >
                    </div>
                    {#each activeShortcutGroup.definitions as definition}
                      <div class="shortcut-table-row" role="row">
                        <span role="cell"
                          ><strong>{definition.label}</strong><small
                            >{definition.description}</small
                          ></span
                        >
                        <kbd
                          role="cell"
                          aria-label={formatShortcut(
                            definition,
                            shortcutPlatform,
                            draft.shortcutModifier,
                          )}
                        >
                          {#each formatShortcutParts(definition, shortcutPlatform, draft.shortcutModifier) as part, index}
                            {#if index > 0 && shortcutPlatform !== "macos"}<span
                                class="shortcut-key-separator"
                                aria-hidden="true">+</span
                              >{/if}
                            <span class="shortcut-key-part">{part}</span>
                          {/each}
                        </kbd>
                        <span role="cell">{activeShortcutGroup.label}</span>
                        <span
                          role="cell"
                          class:destructive={definition.destructive}
                          >{definition.availability}</span
                        >
                      </div>
                    {/each}
                  </div>
                </div>
              {/if}
            </div>
          </div>
        {:else if selectedSection === "ai"}
          <AiSettings
            settings={draft}
            onUpdate={async (patch) => {
              const updated = await onUpdate(patch);
              Object.assign(draft, updated);
              return updated;
            }}
          />
        {:else if selectedSection === "miscellaneous"}
          <div class="settings-form single-section">
            <fieldset>
              <legend>Optional tools</legend>
              <label class="check wide"
                ><Toggle
                  checked={draft.showLogsTab}
                  ariaLabel="Show logs tab"
                  onchange={(event) =>
                    updateField("showLogsTab", event.currentTarget.checked)}
                /><span>Show logs tab</span></label
              >
              <small class="wide"
                >The Logs tab is hidden by default. Turn it on to add it to the
                main toolbar.</small
              >
            </fieldset>
            <fieldset>
                <legend>Interactive tour</legend>
                <small class="wide"
                  >Replay the interactive button-focused tour that introduces you to the entirety of Witness.</small
                >
                <div class="miscellaneous-actions wide">
                  <button
                    class="text-button"
                    type="button"
                    onclick={() => onReplayTutorial?.()}>Restart Interactive Tour</button
                  >
                </div>
              </fieldset>
            </div>
        {:else if selectedSection === "updates"}
          <div class="settings-form single-section">
            <fieldset>
              <legend>Update mode</legend>
              <div
                class="modifier-options"
                role="radiogroup"
                aria-label="Update mode"
              >
                <label class:active={updateMode === "auto-update"}>
                  <input
                    type="radio"
                    name="update-mode"
                    value="auto-update"
                    checked={updateMode === "auto-update"}
                    onchange={() => setUpdateModeChoice("auto-update")}
                  />
                  <span
                    ><strong>Auto update</strong><small
                      >Check, download and install automatically</small
                    ></span
                  >
                </label>
                <label class:active={updateMode === "auto-check"}>
                  <input
                    type="radio"
                    name="update-mode"
                    value="auto-check"
                    checked={updateMode === "auto-check"}
                    onchange={() => setUpdateModeChoice("auto-check")}
                  />
                  <span
                    ><strong>Auto check</strong><small
                      >Check automatically, install manually (default)</small
                    ></span
                  >
                </label>
                <label class:active={updateMode === "manual"}>
                  <input
                    type="radio"
                    name="update-mode"
                    value="manual"
                    checked={updateMode === "manual"}
                    onchange={() => setUpdateModeChoice("manual")}
                  />
                  <span
                    ><strong>Manual only</strong><small
                      >Check and install manually</small
                    ></span
                  >
                </label>
              </div>
              <small class="wide"
                >Change applies to the next check. Manual Check for Updates always
                works.</small
              >
            </fieldset>
            <fieldset>
              <legend>Current version</legend>
              <small class="wide">
                {#if appVersion}v{appVersion}{:else}Version unavailable in browser preview.{/if}
              </small>
              <div class="miscellaneous-actions wide">
                <button
                  class="text-button"
                  type="button"
                  disabled={updaterBusy}
                  onclick={() => void manualCheckForUpdates()}
                  >{updaterBusy && !pendingUpdate ? "Checking…" : "Check for Updates"}</button
                >
                {#if pendingUpdate && pendingVersion}
                  <button
                    class="text-button"
                    type="button"
                    disabled={updaterBusy}
                    onclick={() => void installPendingUpdate()}
                    >{updaterBusy ? "Updating…" : `Update Now (v${pendingVersion})`}</button
                  >
                  <button
                    class="text-button"
                    type="button"
                    disabled={updaterBusy}
                    onclick={() => void discardPendingUpdate().then(() => (updaterStatus = "Update dismissed. You can check again later."))}
                    >Later</button
                  >
                {/if}
              </div>
              {#if updaterStatus}<small class="wide" aria-live="polite">{updaterStatus}</small>{/if}
              {#if updaterProgress !== null}
                <small class="wide" aria-live="polite">Downloading… {updaterProgress}%</small>
              {/if}
            </fieldset>
          </div>
        {:else if selectedSection === "about"}
          <div class="about-page">
            <div class="about-hero">
              <div class="about-hero-logos">
                <img
                  src="/witness_app_icon.png"
                  alt="Witness"
                  class="about-logo"
                />
                <div class="about-hero-text">
                  <div class="about-title-row">
                    <h2>Witness</h2>
                    {#if appVersion}
                      <span class="about-version-badge" aria-label={`Version ${appVersion}`}>v{appVersion}</span>
                    {/if}
                  </div>
                  <p class="about-hero-tagline">
                    Modern Web Security Testing Toolkit
                  </p>
                  <p class="about-hero-tagline"><button
                  class="about-link"
                  type="button"
                  style="text-decoration: none;"
                  onclick={() =>
                    void openExternal("https://witness.northcorelabs.org")}
                  >witness.northcorelabs.org</button
                ></p>
                </div>
              </div>

            </div>

            <section class="about-section">
              <h3>About Witness</h3>
              <p class="about-hero-desc">
                Witness is an open-source, cross-platform, modern desktop web security testing suite.
                No subscriptions, no sign-up, no telemetry.</p>
            </section>

            <section class="about-section">
              <h3>Documentation</h3>
              <p>
                You can access the full Witness docs at <button
                  class="about-link"
                  type="button"
                  style="text-decoration: none;"
                  onclick={() =>
                    void openExternal("https://witness.northcorelabs.org/docs")}
                  >witness.northcorelabs.org/docs</button
                >
                — quick start, proxy &amp; CA setup, and all the other information you need. The wiki is updated with every
                release.
              </p>
              <div class="about-actions">
                <button
                  class="text-button"
                  type="button"
                  onclick={() =>
                    void openExternal("https://witness.northcorelabs.tech/docs")}
                  >Open Witness Docs</button
                >
                <button
                  class="text-button"
                  type="button"
                  onclick={() =>
                    showInfoToast("Offline wiki download coming soon")}
                  >Download Docs Offline</button
                >
              </div>
            </section>

            <section class="about-section">
              <h3>Northcore Labs</h3>
              <p>
                Northcore Labs is a venture to build
                open, practical security tooling for the community. We believe essential security
                testing infrastructure should be free, auditable, and free of vendor lock-in.
              </p>
              <div class="about-actions">
                <button
                  class="text-button"
                  type="button"
                  onclick={() =>
                    void openExternal("https://northcorelabs.tech")}
                  >Visit northcorelabs.tech</button
                >
                <button
                  class="text-button"
                  type="button"
                  onclick={() =>
                    void openExternal(
                      "https://github.com/northcorelabs/",
                    )}>Visit North Core Labs GitHub</button
                >
              </div>
            </section>

            <section class="about-section last-about">
              <h3>Donate &amp; Support</h3>
              <p>
                Witness is made possible by the support of the community. Donations help keep it free and enable the continued development and maintenance of the Witness project and various similar endeavors by North Core Labs.<br>If Witness has enabled simpler, cheaper and more productive workflows for you, please consider supporting.
              </p>
              <div class="about-actions">
                <button
                  class="text-button donate"
                  type="button"
                  onclick={() =>
                    void openExternal("https://northcorelabs.tech/donate")}
                  >Donate</button
                >
              </div>
            </section>

            <footer class="about-footer">
              <span
                >Witness by North Core Labs · AGPLv3 licensed</span
              >
              <span class="about-footer-links">
                <button
                  class="about-link"
                  type="button"
                  onclick={() =>
                    void openExternal(
                      "https://github.com/northcorelabs/witness/blob/main/LICENSE",
                    )}>License</button
                >
                <button
                  class="about-link"
                  type="button"
                  onclick={() =>
                    void openExternal(
                      "https://witness.northcorelabs.tech/changelog",
                    )}>Changelog</button
                >
                <button
                  class="about-link"
                  type="button"
                  onclick={() =>
                    void openExternal(
                      "https://witness.northcorelabs.tech/privacy",
                    )}>Privacy</button
                >
              </span>
            </footer>
          </div>
        {:else}
          <div class="settings-form single-section">
            <fieldset>
              <legend>Certificate authority</legend>
              <label class="wide"
                ><span>Certificate directory</span><input
                  value={draft.certificateDirectory}
                  oninput={(event) =>
                    updateField(
                      "certificateDirectory",
                      event.currentTarget.value,
                      true,
                    )}
                  required
                /><small
                  >CA certificate: {draft.certificateDirectory}/witness-ca.pem</small
                ></label
              >
              <div class="certificate-actions wide">
                <button
                  class="text-button"
                  type="button"
                  onclick={() => void generateCertificate()}
                  >Generate CA certificate</button
                >
                <small
                  >Install the generated <code>witness-ca.pem</code> in your browser’s
                  trusted authorities.</small
                >
                {#if certificateState}<small
                    class="certificate-state"
                    aria-live="polite">{certificateState}</small
                  >{/if}
              </div>
              {#if proxyRunning}<p class="notice">
                  Certificate changes require the running proxy to restart.
                </p>{/if}
            </fieldset>
          </div>
        {/if}
      </div>
    </div>
  </div>
</section>

<style>
  .settings-tool {
    display: grid;
    grid-template-rows: 52px minmax(0, 1fr);
    width: 100%;
    height: 100%;
    min-height: 0;
    margin: 0;
    padding: 0;
    overflow: hidden;
    color: var(--text, #dce1e8);
    background: var(--bg, #0c0f13);
  }
  .settings-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 16px;
    border-bottom: 1px solid var(--border, #2c333d);
    background: var(--surface, #12161b);
  }
  .settings-header h1 {
    margin: 0;
    font-size: var(--font-size-heading);
  }
  .save-state {
    color: var(--success, #34d399);
    font-size: var(--font-size-compact);
  }
  .settings-layout {
    display: grid;
    grid-template-columns: 224px minmax(0, 1fr);
    min-height: 0;
  }
  .settings-sidebar {
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    min-width: 0;
    padding: 14px 9px 9px;
    border-right: 1px solid var(--border, #2c333d);
    background: var(--surface, #12161b);
  }
  .sidebar-title {
    display: grid;
    gap: 3px;
    padding: 0 8px 12px;
    border-bottom: 1px solid
      color-mix(in srgb, var(--border, #2c333d) 92%, transparent);
  }
  .sidebar-title strong {
    font-size: var(--font-size-body);
  }
  .sidebar-title small {
    color: var(--muted, #737e8b);
    font-size: var(--font-size-compact);
  }
  .settings-sidebar nav {
    display: grid;
    align-content: start;
    gap: 3px;
  }
  .settings-sidebar nav button {
    position: relative;
    display: grid;
    gap: 2px;
    width: 100%;
    min-height: 48px;
    padding: 7px 9px 7px 12px;
    border: 1px solid transparent;
    border-radius: 4px;
    color: var(--text, #dce1e8);
    background: transparent;
    text-align: left;
    cursor: pointer;
  }
  .settings-sidebar nav button:hover {
    color: var(--text, #dce1e8);
    border-color: var(--border, #2c333d);
    background: var(--surface-2, #1b2028);
  }
  .settings-sidebar nav button.active {
    color: var(--text, #dce1e8);
    border-color: color-mix(
      in srgb,
      var(--accent, #9ca3af) 35%,
      var(--border, #2c333d)
    );
    background: color-mix(
      in srgb,
      var(--accent, #9ca3af) 12%,
      var(--surface, #12161b)
    );
  }
  .settings-sidebar nav button.active::before {
    position: absolute;
    top: 7px;
    bottom: 7px;
    left: 3px;
    width: 2px;
    border-radius: 2px;
    background: var(--accent, #9ca3af);
    content: "";
  }
  .settings-sidebar nav button span {
    color: inherit;
    font-size: var(--font-size-body);
    font-weight: 650;
  }
  .settings-sidebar nav button small {
    overflow: hidden;
    color: inherit;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--font-size-compact);
    opacity: 1;
  }
  .settings-pane {
    display: grid;
    grid-template-rows: 58px minmax(0, 1fr);
    min-width: 0;
    min-height: 0;
  }
  .pane-heading {
    display: flex;
    flex-direction: column;
    justify-content: center;
    padding: 0 18px;
    border-bottom: 1px solid var(--border, #2c333d);
    background: var(--surface-2, #1b2028);
  }
  .pane-heading h2 {
    margin: 0;
    font-size: var(--font-size-heading);
  }
  .pane-heading p {
    margin: 2px 0 0;
    color: var(--muted, #737e8b);
    font-size: var(--font-size-compact);
  }
  .pane-body {
    min-height: 0;
    padding: 14px 18px 18px;
    overflow: auto;
  }
  .settings-form {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 10px;
    width: min(1080px, 100%);
  }
  .settings-form.single-section {
    grid-template-columns: minmax(0, 760px);
  }
  .keyboard-settings {
    display: grid;
    gap: 14px;
    width: min(1180px, 100%);
  }
  .modifier-settings {
    grid-template-columns: 1fr;
    width: min(760px, 100%);
  }
  .modifier-options {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 8px;
  }
  .modifier-options label {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 10px;
    border: 1px solid var(--border-strong, #3a424d);
    border-radius: 5px;
    color: var(--muted, #929ba8);
    background: var(--surface-2, #1b2028);
    cursor: pointer;
  }
  .modifier-options label.active {
    color: var(--text, #d7dde5);
    border-color: var(--accent, #9ca3af);
    background: color-mix(
      in srgb,
      var(--accent, #9ca3af) 10%,
      var(--surface, #12161b)
    );
  }
  .modifier-options label > span {
    display: grid;
    gap: 3px;
  }
  .modifier-options strong {
    color: inherit;
    font-size: var(--font-size-body);
  }
  .modifier-options small {
    overflow: visible;
    white-space: normal;
  }
  .modifier-options input {
    width: auto;
    height: auto;
    margin-top: 2px;
    accent-color: var(--accent, #9ca3af);
  }
  .keyboard-note {
    margin: 0;
    color: var(--muted, #737e8b);
    font-size: var(--font-size-compact);
    line-height: 1.5;
  }
  .shortcut-reference {
    display: grid;
    gap: 14px;
  }
  .reference-heading h3 {
    margin: 0;
    color: var(--text, #d7dde5);
    font-size: var(--font-size-body);
  }
  .reference-heading p {
    margin: 3px 0 0;
    color: var(--muted, #737e8b);
    font-size: var(--font-size-compact);
  }
  .shortcut-category-tabs {
    display: flex;
    gap: 4px;
    overflow-x: auto;
    padding: 4px;
    border: 1px solid var(--border, #2c333d);
    border-radius: 5px;
    background: var(--surface-2, #1b2028);
    scrollbar-width: thin;
  }
  .shortcut-category-tabs button {
    flex: 0 0 auto;
    min-height: 31px;
    padding: 0 10px;
    border: 1px solid transparent;
    border-radius: 4px;
    color: var(--muted, #929ba8);
    background: transparent;
    font-size: var(--font-size-compact);
    white-space: nowrap;
    cursor: pointer;
  }
  .shortcut-category-tabs button:hover {
    color: var(--text, #dce1e8);
    border-color: var(--border, #2c333d);
    background: var(--surface, #12161b);
  }
  .shortcut-category-tabs button.active {
    color: var(--text, #d7dde5);
    border-color: color-mix(
      in srgb,
      var(--accent, #9ca3af) 55%,
      var(--border, #2c333d)
    );
    background: color-mix(
      in srgb,
      var(--accent, #9ca3af) 14%,
      var(--surface, #12161b)
    );
  }
  .shortcut-category-tabs button:focus-visible {
    outline: 2px solid var(--accent, #9ca3af);
    outline-offset: 1px;
  }
  .shortcut-group {
    display: grid;
    gap: 6px;
  }
  .shortcut-group h4 {
    margin: 0;
    padding: 0 2px;
    color: var(--text, #d7dde5);
    font-size: var(--font-size-body);
  }
  .shortcut-table {
    overflow: hidden;
    border: 1px solid var(--border, #2c333d);
    border-radius: 5px;
    background: var(--surface, #12161b);
  }
  .shortcut-table-row {
    display: grid;
    grid-template-columns: minmax(220px, 1.2fr) minmax(100px, 0.35fr) minmax(
        120px,
        0.5fr
      ) minmax(240px, 1fr);
    gap: 12px;
    align-items: center;
    min-height: 44px;
    padding: 7px 10px;
    border-top: 1px solid var(--border, #2c333d);
    color: var(--muted, #929ba8);
    font-size: var(--font-size-compact);
  }
  .shortcut-table-row:first-child {
    border-top: 0;
  }
  .shortcut-table-row > span:first-child {
    display: grid;
    gap: 2px;
    min-width: 0;
  }
  .shortcut-table-row strong {
    color: var(--text, #d7dde5);
    font-size: var(--font-size-body);
    font-weight: 650;
  }
  .shortcut-table-row small {
    overflow: visible;
    white-space: normal;
  }
  .shortcut-table-header {
    min-height: 30px;
    color: var(--muted, #737e8b);
    background: var(--surface-2, #1b2028);
    font-size: var(--font-size-compact);
    font-weight: 700;
  }
  .shortcut-table kbd {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 0.3em;
    box-sizing: border-box;
    width: fit-content;
    min-width: 72px;
    min-height: 34px;
    padding: 6px 10px;
    border: 1px solid var(--border-strong, #3a424d);
    border-radius: 6px;
    color: var(--text, #d7dde5);
    background: var(--input, #0c0f13);
    box-shadow:
      inset 0 -1px 0 color-mix(in srgb, var(--border-strong, #3a424d) 70%, transparent),
      0 1px 1px #0005;
    font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
    font-size: var(--font-size-body);
    font-weight: 800;
    letter-spacing: 0.035em;
    line-height: 1;
    white-space: nowrap;
  }
  .shortcut-key-part,
  .shortcut-key-separator {
    display: inline-flex;
    align-items: center;
  }
  .shortcut-key-separator {
    color: var(--muted, #929ba8);
    font-weight: 650;
  }
  .shortcut-table-row > span:last-child.destructive {
    color: var(--warning, #f7b955);
  }
  fieldset {
    display: grid;
    grid-template-columns: 1fr 1fr;
    align-content: start;
    gap: 11px;
    min-width: 0;
    margin: 0;
    padding: 15px;
    border: 1px solid var(--border, #2c333d);
    border-radius: 5px;
    background: var(--surface, #12161b);
  }
  fieldset.interception-settings,
  fieldset.match-replace-settings {
    grid-column: 1 / -1;
    grid-template-columns: 1fr;
  }
  fieldset.listener-settings {
    grid-column: 1;
    grid-row: 1;
  }
  fieldset.traffic-settings {
    grid-column: 1;
    grid-row: 2;
  }
  fieldset.upstream-settings {
    grid-column: 2;
    grid-row: 1 / span 2;
    align-self: start;
  }
  legend {
    padding: 0 6px;
    color: var(--text, #d7dde5);
    font-size: var(--font-size-body);
    font-weight: 700;
  }
  label {
    display: grid;
    align-content: start;
    gap: 5px;
    color: var(--muted, #929ba8);
    font-size: var(--font-size-compact);
  }
  label.wide,
  .notice,
  .certificate-actions,
  .miscellaneous-actions {
    grid-column: 1 / -1;
  }
  fieldset > small.wide {
    grid-column: 1 / -1;
    overflow: visible;
    white-space: normal;
  }
  input,
  select {
    width: 100%;
    height: 30px;
    padding: 0 8px;
    border: 1px solid var(--border-strong, #3a424d);
    border-radius: 4px;
    color: var(--text, #dbe1e8);
    background: var(--input, #0c0f13);
  }
  label.check {
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: 7px;
    min-height: 30px;
  }
  small {
    overflow: hidden;
    color: var(--muted, #737e8b);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .notice {
    margin: 0;
    color: var(--warning, #fbbf24);
    font-size: var(--font-size-compact);
  }
  .certificate-actions {
    display: grid;
    grid-template-columns: auto 1fr;
    align-items: center;
    gap: 7px;
  }
  .certificate-actions button {
    width: fit-content;
    min-height: 29px;
    padding: 0 10px;
    border: 1px solid var(--accent, #9ca3af);
    border-radius: 4px;
    color: var(--accent-contrast, #fff);
    background: var(--accent, #9ca3af);
    font-weight: 700;
    cursor: pointer;
  }
  .certificate-actions code {
    color: inherit;
  }
  .miscellaneous-actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .certificate-state {
    grid-column: 1 / -1;
    color: var(--success, #34d399);
  }
  .interception-intro {
    display: grid;
    gap: 3px;
  }
  .interception-intro strong,
  .content-filters strong {
    color: var(--text, #d7dde5);
    font-size: var(--font-size-body);
  }
  .interception-intro small,
  .content-filters small {
    overflow: visible;
    white-space: normal;
    font-size: var(--font-size-compact);
  }
  .interception-modes {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 7px;
  }
  .interception-modes label {
    display: flex;
    flex-direction: row;
    align-items: flex-start;
    gap: 7px;
    padding: 8px;
    border: 1px solid var(--border-strong, #3a424d);
    border-radius: 5px;
    background: var(--surface-2, #1b2028);
    cursor: pointer;
  }
  .interception-modes label.active {
    border-color: var(--border-strong, #3a424d);
    background: var(--surface-2, #1b2028);
  }
  .interception-modes span {
    display: grid;
    gap: 3px;
  }
  .interception-modes strong {
    color: var(--text, #d7dde5);
    font-size: var(--font-size-compact);
  }
  .interception-modes small {
    overflow: visible;
    white-space: normal;
    font-size: var(--font-size-compact);
  }
  .content-filters {
    display: grid;
    gap: 7px;
    padding-top: 2px;
  }
  .content-filters > div:first-child {
    display: grid;
    gap: 3px;
  }
  .filter-list {
    display: flex;
    flex-wrap: wrap;
    gap: 5px;
  }
  .filter-list label {
    display: flex;
    flex-direction: row;
    align-items: center;
    gap: 5px;
    min-height: 27px;
    padding: 0 8px;
    border: 1px solid var(--border-strong, #3a424d);
    border-radius: 999px;
    color: var(--muted, #929ba8);
    background: var(--surface-2, #1b2028);
    cursor: pointer;
  }
  .filter-list label.active {
    color: var(--text, #d7dde5);
    border-color: var(--accent, #9ca3af);
    background: color-mix(
      in srgb,
      var(--accent, #9ca3af) 14%,
      var(--surface, #12161b)
    );
  }
  .rule-tabs {
    display: flex;
    gap: 4px;
    padding-top: 4px;
    border-bottom: 1px solid var(--border, #2c333d);
  }
  .rule-tabs button {
    min-height: 28px;
    padding: 0 9px;
    border: 1px solid transparent;
    border-bottom: 0;
    border-radius: 4px 4px 0 0;
    color: var(--muted, #929ba8);
    background: transparent;
    font-size: var(--font-size-compact);
    cursor: pointer;
  }
  .rule-tabs button.active {
    color: var(--text, #d7dde5);
    border-color: var(--border, #2c333d);
    background: var(--surface-2, #1b2028);
  }
  .about-page {
    display: flex;
    flex-direction: column;
    width: 103%;
    max-width: none;
    margin: -14px -18px -18px;
  }
  .about-hero {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    gap: 14px;
    padding: 36px 24px 28px;
    background: var(#afb7c2);
  }
  .about-hero-logos {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 16px;
  }
  .about-logo {
    width: 72px;
    height: 72px;
    object-fit: contain;
  }
  .about-title-row {
    display: flex;
    align-items: baseline;
    justify-content: flex-start;
    gap: 8px;
    text-align: left;
  }
  .about-version-badge {
    display: inline-flex;
    align-items: center;
    padding: 4px 8px;
    border-radius: 4px;
    color: #111827;
    background: #fff;
    font-size: var(--font-size-compact);
    font-weight: 800;
    line-height: 1;
    transform: translateY(-3px);
    white-space: nowrap;
  }
  .about-hero h2 {
    margin: 0;
    font-size: 28px;
    font-weight: 700;
    letter-spacing: -0.03em;
  }
  .about-hero-tagline {
    margin: 0;
    color: var(--muted, #737e8b);
    font-size: var(--font-size-body);
    font-weight: 650;
    text-align: left;
  }
  .about-hero-desc {
    max-width: 720px;
    margin: 0;
    color: var(--text, #d7dde5);
    font-size: var(--font-size-body);
    line-height: 1.6;
  }
  .about-section {
    display: grid;
    gap: 10px;
    padding: 22px 24px;
    padding-bottom: 0px;
  }
  .about-section:last-of-type {
    border-bottom: none;
  }
  .about-section h3 {
    margin: 0;
    font-size: 15px;
    font-weight: 700;
    letter-spacing: -0.01em;
  }
  .about-section p {
    margin: 0;
    max-width: 760px;
    color: var(--muted, #929ba8);
    font-size: var(--font-size-body);
    line-height: 1.6;
  }
  .about-actions {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    margin-top: 4px;
  }
  .about-actions .donate {
    background: var(--success);
    color: white;
    font-weight: 700;
  }
  .about-link {
    padding: 0;
    color: var(--accent, #9ca3af);
    border: 0;
    background: transparent;
    font: inherit;
    text-decoration: underline;
    cursor: pointer;
    text-align: left;
    font-weight: 400;
    margin-top: 4px;
  }
  .about-link:hover {
    color: var(--text, #d7dde5);
  }
  .about-footer {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    gap: 12px;
    padding: 14px 24px;
    border-top: 1px solid var(--border, #2c333d);
    background: var(--surface-2, #1b2028);
    color: var(--muted, #737e8b);
    font-size: var(--font-size-compact);
  }
  .about-footer-links {
    display: flex;
    gap: 10px;
  }
  .last-about {
    padding: 20px;
  }
  @media (max-width: 780px) {
    .settings-layout {
      grid-template-columns: 176px minmax(0, 1fr);
    }
    .settings-form,
    .interception-modes,
    .modifier-options {
      grid-template-columns: 1fr;
    }
    fieldset.listener-settings,
    fieldset.traffic-settings,
    fieldset.upstream-settings {
      grid-column: auto;
      grid-row: auto;
    }
    .shortcut-table-row {
      grid-template-columns: 1fr;
      gap: 4px;
    }
    .shortcut-table-header {
      display: none;
    }
  }
</style>
