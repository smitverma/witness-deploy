<script lang="ts">
  import {
    formatRawPlusMessage,
    decodeHttpText,
    encodeHttpText,
    isValidHttpMethod,
    normalizeHttpLineEndings,
    readHttpRequestMethod,
    replaceHttpRequestMethod,
    splitHttpLines,
    splitHttpMessage,
  } from "$lib/http-message";
  import CodeEditor from "./CodeEditor.svelte";
  import HexViewer from "./HexViewer.svelte";
  import ContextMenu, { type ContextMenuItem } from "./ContextMenu.svelte";
  import { parseRequestDetails } from "$lib/http-url";
  import { showErrorToast } from "$lib/errorToast";
  import type { IntruderRange } from "$lib/types";

  type Mode = "pretty" | "raw" | "rawPlus" | "hex";
  type MessageKind = "request" | "response";
  type EditorCommand = "undo" | "redo" | "cut" | "paste" | "selectAll";
  type Header = { name: string; value: string; delimiter: string | null };
  const requestMethods = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "CONNECT", "TRACE"];
  const modes: { value: Mode; label: string }[] = [
    { value: "pretty", label: "Pretty" },
    { value: "raw", label: "Raw" },
    { value: "rawPlus", label: "Raw+" },
    { value: "hex", label: "Hex" },
  ];
  let {
    raw,
    title,
    kind = "response",
    editable = false,
    normalizeRequest = false,
    metadata = "",
    search = "",
    searchRegex = false,
    searchCaseSensitive = false,
    highlightRanges = [],
    onChange = (_raw: Uint8Array) => {},
    onTextChange,
    onSendReplay,
    onSendFuzz,
    onSendDecoder,
    onCompareResponse,
    onSaveOrganizer,
    onConfigureIdentities,
    onDuplicate,
    onProxyAction,
    onDirtyChange,
    onMinimize,
    onSelectionChange,
  }: {
    raw: Uint8Array;
    title: string;
    kind?: MessageKind;
    editable?: boolean;
    normalizeRequest?: boolean;
    metadata?: string;
    search?: string;
    searchRegex?: boolean;
    searchCaseSensitive?: boolean;
    highlightRanges?: IntruderRange[];
    onChange?: (raw: Uint8Array) => void;
    onTextChange?: (value: string) => void;
    onSendReplay?: (raw: Uint8Array) => void;
    onSendFuzz?: (raw: Uint8Array) => void;
    onSendDecoder?: (value: string) => void;
    onCompareResponse?: (raw: Uint8Array) => void;
    onSaveOrganizer?: (raw: Uint8Array) => void;
    onConfigureIdentities?: () => void;
    onDuplicate?: () => void;
    onProxyAction?: (action: "forward" | "modify" | "drop") => void;
    onDirtyChange?: (dirty: boolean) => void;
    onMinimize?: () => void;
    onSelectionChange?: (selection: { from: number; to: number }, value: string) => void;
  } = $props();

  let mode = $state<Mode>("rawPlus");
  let wrap = $state(true);
  let text = $state("");
  let rawPlusText = $state("");
  let startLine = $state("");
  let headers = $state<Header[]>([]);
  let body = $state("");
  let messageLineEnding = $state<"\r\n" | "\n" | "\r">("\r\n");
  let messageHasBoundary = $state(false);
  let lastInput = "";
  // Editable viewers keep their active draft as text while the parent keeps
  // the original bytes until an explicit save/send/forward boundary. Track
  // the last raw source separately so a parent refresh cannot reapply that
  // unchanged source over the active draft.
  let lastRawSource = "";
  let localDraftDirty = false;
  let localDraftDirtyState = $state(false);
  let exportDialog = $state(false);
  let selectedText = $state("");
  let contextMenu = $state<{ x: number; y: number; selectionText: string } | null>(null);
  let editorCommand = $state<{ id: number; action: EditorCommand } | null>(null);
  let nextEditorCommandId = 1;
  let customMethodDialog = $state(false);
  let customMethod = $state("");
  let customMethodError = $state("");
  let customMethodInput = $state<HTMLInputElement | undefined>();
  const requestMethod = $derived(kind === "request" ? readHttpRequestMethod(currentText()) : "");
  const canEditRequestMethod = $derived(editable && kind === "request" && (mode === "raw" || mode === "rawPlus"));
  const contextMenuItems = $derived.by(() => buildContextMenuItems());

  // Bodies above this size skip synchronous Raw+ pretty-printing on mount.
  // Raw+ is formatted on demand when the user selects that mode, so opening
  // a large history/proxy message never blocks tab switches.
  const LARGE_MESSAGE_CHARS = 256 * 1024;
  $effect(() => {
    const source = decodeHttpText(raw);
    if (source === lastRawSource && localDraftDirty) return;
    lastRawSource = source;
    if (source === lastInput) return;
    const incoming = normalizeRequest && editable ? normalizeHttpLineEndings(source) : source;
    lastInput = incoming;
    text = incoming;
    if (incoming.length > LARGE_MESSAGE_CHARS) {
      rawPlusText = incoming;
      if (mode === "rawPlus" || mode === "pretty") mode = "raw";
    } else {
      rawPlusText = formatRawPlusMessage(incoming);
    }
    selectedText = "";
    if (mode === "pretty") parse(incoming);
    if (incoming !== source) {
      if (onTextChange && localDraftDirty) {
        localDraftDirty = false;
        localDraftDirtyState = false;
        onDirtyChange?.(false);
      } else notifyChange(incoming, false);
    } else if (localDraftDirty) {
      localDraftDirty = false;
      localDraftDirtyState = false;
      onDirtyChange?.(false);
    }
  });

  $effect(() => {
    if ((search || highlightRanges.length) && !editable && mode !== "raw") mode = "raw";
  });

  $effect(() => {
    if (!customMethodDialog) return;
    requestAnimationFrame(() => customMethodInput?.focus());
  });

  function parse(value: string) {
    const message = splitHttpMessage(value);
    const lines = splitHttpLines(message.head);
    messageLineEnding = message.lineEnding;
    messageHasBoundary = message.complete;
    startLine = lines.shift() ?? "";
    headers = lines.map((line) => {
      const separator = line.indexOf(":");
      if (separator < 0) return { name: line, value: "", delimiter: null };
      const afterSeparator = line.slice(separator + 1);
      const leadingWhitespace = afterSeparator.match(/^[ \t]*/)?.[0] ?? "";
      return {
        name: line.slice(0, separator),
        value: afterSeparator.slice(leadingWhitespace.length),
        delimiter: `:${leadingWhitespace}`,
      };
    });
    body = message.body;
  }

  function emitPretty() {
    const head = [
      startLine,
      ...headers.map((header) =>
        header.delimiter !== null || header.value
          ? `${header.name}${header.delimiter ?? ": "}${header.value}`
          : header.name
      ),
    ].join(messageLineEnding);
    const separator = messageHasBoundary || body
      ? `${messageLineEnding}${messageLineEnding}`
      : "";
    text = `${head}${separator}${body}`;
    lastInput = text;
    rawPlusText = text;
    notifyChange(text);
  }

  function emitRaw(value: string) {
    text = value;
    lastInput = value;
    rawPlusText = value;
    notifyChange(value);
  }

  function emitRawPlus(value: string) {
    rawPlusText = value;
    text = value;
    lastInput = value;
    notifyChange(value);
  }

  function selectMode(next: Mode) {
    if (highlightRanges.length && !editable && next !== "raw") {
      mode = "raw";
      return;
    }
    // Raw+ formatting was skipped at mount for large messages; run it here
    // on explicit user gesture instead of blocking tab switches.
    if (next === "rawPlus") rawPlusText = formatRawPlusMessage(text);
    if (next === "pretty") parse(text);
    mode = next;
  }

  function cycleMode() {
    const index = modes.findIndex((item) => item.value === mode);
    selectMode(modes[(index + 1) % modes.length].value);
  }

  function buildContextMenuItems(): ContextMenuItem[] {
    const hasSelection = Boolean(contextMenu?.selectionText);
    const items: ContextMenuItem[] = [
      { id: hasSelection ? "copy-selection" : "copy", label: hasSelection ? "Copy Selection" : "Copy All" },
    ];

    if (editable) {
      items.push(
        { id: "separator-edit", separator: true },
        { id: "undo", label: "Undo" },
        { id: "redo", label: "Redo" },
        { id: "cut", label: "Cut", disabled: !contextMenu?.selectionText },
        { id: "paste", label: "Paste" },
        { id: "select-all", label: "Select All" },
      );
    }

    if (canEditRequestMethod) {
      items.push(
        { id: "separator-method", separator: true },
        {
          id: "toggle-method",
          label: "Toggle Request Method",
          disabled: !["GET", "POST"].includes(requestMethod),
        },
        {
          id: "change-method",
          label: "Change Request Method",
          submenu: [
            ...requestMethods.map((method): ContextMenuItem => ({
              id: `method:${method}`,
              label: `${method === requestMethod ? "✓ " : ""}${method}`,
            })),
            { id: "separator-method-custom", separator: true },
            { id: "method:custom", label: "Custom…" },
          ],
        },
      );
    }

    const integrations: ContextMenuItem[] = [];
    if (kind === "request" && editable && onConfigureIdentities) {
      integrations.push({ id: "configure-identities", label: "Configure Identities" });
    }
    if (kind === "request" && onSendReplay) integrations.push({ id: "send-replay", label: "Send to Replay" });
    if (kind === "request" && onSendFuzz) integrations.push({ id: "send-fuzz", label: "Send to Fuzz" });
    if (kind === "request" && onSaveOrganizer) integrations.push({ id: "send-organizer", label: "Send to Organizer" });
    if (onSendDecoder) {
      integrations.push({
        id: "send-decoder",
        label: contextMenu?.selectionText ? "Send Selection to Decoder" : "Send to Decoder",
      });
    }
    if (kind === "response" && onCompareResponse) integrations.push({ id: "compare-response", label: "Compare Response", disabled: !currentText().length });
    if (kind === "request" && onDuplicate) integrations.push({ id: "duplicate", label: "Duplicate" });
    if (onProxyAction) {
      integrations.push(
        { id: "proxy-forward", label: "Forward" },
        { id: "proxy-drop", label: "Drop", danger: true },
      );
    }
    if (integrations.length) items.push({ id: "separator-integrations", separator: true }, ...integrations);

    items.push(
      { id: "separator-display", separator: true },
      { id: "export", label: "Export" },
    );
    if (mode === "raw" || mode === "rawPlus") {
      items.push({ id: wrap ? "disable-wrap" : "enable-wrap", label: wrap ? "Disable Wrap" : "Enable Wrap" });
    }
    items.push({
      id: "change-display-mode",
      label: "Change Display Mode",
      submenu: modes.map((displayMode) => ({
        id: `mode:${displayMode.value}`,
        label: `${displayMode.value === mode ? "✓ " : ""}${displayMode.label}`,
      })),
    });
    return items;
  }

  function openContextMenu(event: MouseEvent, selection: { from: number; to: number }, value: string) {
    event.preventDefault();
    event.stopPropagation();
    contextMenu = {
      x: event.clientX,
      y: event.clientY,
      selectionText: value.slice(selection.from, selection.to),
    };
  }

  function openTextSurfaceContextMenu(event: MouseEvent) {
    event.preventDefault();
    event.stopPropagation();
    contextMenu = {
      x: event.clientX,
      y: event.clientY,
      selectionText: window.getSelection()?.toString() ?? "",
    };
  }

  function closeContextMenu() {
    contextMenu = null;
  }

  function copySelection() {
    const value = contextMenu ? contextMenu.selectionText : selectedText;
    if (value) void navigator.clipboard.writeText(value).catch(showErrorToast);
  }

  function issueEditorCommand(action: EditorCommand) {
    editorCommand = { id: nextEditorCommandId++, action };
  }

  function acknowledgeEditorCommand(id: number) {
    if (editorCommand?.id === id) editorCommand = null;
  }

  function emitCurrentText(value: string) {
    if (mode === "rawPlus") emitRawPlus(value);
    else if (mode === "raw") emitRaw(value);
    else {
      text = value;
      lastInput = value;
      rawPlusText = value;
      parse(value);
      notifyChange(value);
    }
  }

  function changeRequestMethod(method: string) {
    if (!canEditRequestMethod || !method || method === requestMethod) return;
    const value = replaceHttpRequestMethod(currentText(), method);
    if (value !== currentText()) emitCurrentText(value);
  }

  function openCustomMethodDialog() {
    customMethod = requestMethod;
    customMethodError = "";
    customMethodDialog = true;
  }

  function submitCustomMethod() {
    const method = customMethod.trim().toUpperCase();
    if (!isValidHttpMethod(method)) {
      customMethodError = "Use a valid HTTP method token, such as PROPFIND.";
      return;
    }
    changeRequestMethod(method);
    customMethodDialog = false;
  }

  function handleContextAction(id: string) {
    if (id === "copy") void copy();
    else if (id === "copy-selection") copySelection();
    else if (id === "undo" || id === "redo" || id === "cut" || id === "paste") issueEditorCommand(id);
    else if (id === "select-all") issueEditorCommand("selectAll");
    else if (id === "toggle-method") changeRequestMethod(requestMethod === "GET" ? "POST" : "GET");
    else if (id === "method:custom") openCustomMethodDialog();
    else if (id.startsWith("method:")) changeRequestMethod(id.slice("method:".length));
    else if (id === "configure-identities") onConfigureIdentities?.();
    else if (id === "send-replay") onSendReplay?.(currentRaw());
    else if (id === "send-fuzz") onSendFuzz?.(currentRaw());
    else if (id === "send-organizer") onSaveOrganizer?.(currentRaw());
    else if (id === "send-decoder") onSendDecoder?.(contextMenu?.selectionText || currentText());
    else if (id === "compare-response") onCompareResponse?.(currentRaw());
    else if (id === "duplicate") onDuplicate?.();
    else if (id === "proxy-forward") onProxyAction?.(localDraftDirtyState ? "modify" : "forward");
    else if (id === "proxy-drop") onProxyAction?.("drop");
    else if (id === "export") exportDialog = true;
    else if (id === "enable-wrap") wrap = true;
    else if (id === "disable-wrap") wrap = false;
    else if (id.startsWith("mode:")) selectMode(id.slice("mode:".length) as Mode);
    closeContextMenu();
  }

  async function copy() {
    try {
      await navigator.clipboard.writeText(currentText());
    } catch (reason) {
      showErrorToast(reason);
    }
  }

  function notifyChange(value: string, dirty = true) {
    localDraftDirty = dirty;
    localDraftDirtyState = dirty;
    onDirtyChange?.(dirty);
    if (onTextChange) onTextChange(value);
    else onChange?.(encodeHttpText(value));
  }

  function currentText() {
    if (mode === "hex" && !editable) return decodeHttpText(raw);
    return mode === "rawPlus" ? rawPlusText : text;
  }

  function currentRaw() {
    if (!editable) return raw;
    return encodeHttpText(currentText());
  }

  function captureSelection(selection: { from: number; to: number }, value: string) {
    selectedText = value.slice(selection.from, selection.to);
    onSelectionChange?.(selection, value);
  }

  function sendToDecoder() {
    onSendDecoder?.(selectedText || (mode === "rawPlus" ? rawPlusText : text));
  }

  function requestDetails() {
    return parseRequestDetails(text);
  }

  function exportHttp() {
    const link = document.createElement("a");
    const url = URL.createObjectURL(new Blob([currentRaw()], { type: "application/octet-stream" }));
    link.href = url;
    link.download = `${title.toLowerCase().replaceAll(" ", "-")}.http`;
    link.click();
    window.setTimeout(() => URL.revokeObjectURL(url), 4000);
    exportDialog = false;
  }

  async function copyUrl() {
    const details = requestDetails();
    if (!details?.url) return;
    try {
      await navigator.clipboard.writeText(details.url);
    } catch (reason) {
      showErrorToast(reason);
      return;
    }
    exportDialog = false;
  }

  async function copyCurl() {
    const details = requestDetails();
    if (!details?.url) return;
    const quote = (value: string) => `'${value.replaceAll("'", "'\\\\''")}'`;
    const parts = ["curl", "-X", quote(details.method), quote(details.url)];
    for (const header of details.headers) {
      if (header && !header.toLowerCase().startsWith("content-length:")) parts.push("-H", quote(header));
    }
    if (details.body) parts.push("--data-binary", quote(details.body));
    try {
      await navigator.clipboard.writeText(parts.join(" "));
    } catch (reason) {
      showErrorToast(reason);
      return;
    }
    exportDialog = false;
  }
</script>

<section class="message-viewer" aria-label={title}>
  <header>
    <strong>{title}</strong>
    {#if metadata}<span class="metadata-separator" aria-hidden="true">•</span><span class="metadata">{metadata}</span>{/if}
    <span class="spacer"></span>
    {#if onMinimize}
      <button class="icon-action minimize-action" aria-label={`Minimize ${title.toLowerCase()} inspector`} data-tooltip="Minimize inspector" onclick={() => onMinimize?.()}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m6 9 6 6 6-6"/></svg>
      </button>
    {/if}
    {#if mode === "raw" || mode === "rawPlus"}
      <button
        class="icon-action wrap-icon-action"
        aria-label={wrap ? "Disable Wrap" : "Enable Wrap"}
        aria-pressed={wrap}
        data-tooltip={wrap ? "Disable Wrap" : "Enable Wrap"}
        onclick={() => (wrap = !wrap)}
      >
        {#if wrap}
          <svg viewBox="0 0 24 24" fill="none" aria-hidden="true">
            <path stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 7h14M5 12h11c1 0 3 .5 3 2.5S17.333 17 16.5 17H12m-7 0h4m3 0 2-2m-2 2 2 2" />
          </svg>
        {:else}
          <svg viewBox="0 0 48 48" fill="none" aria-hidden="true">
            <path d="M8 10V38" stroke="currentColor" stroke-width="4" stroke-linecap="round" />
            <path d="M24 4V16" stroke="currentColor" stroke-width="4" stroke-linecap="round" />
            <path d="M16 24H42" stroke="currentColor" stroke-width="4" stroke-linecap="round" />
            <path d="M37.0561 19.0113L42.0929 24.0255L37.0561 29.123" stroke="currentColor" stroke-width="4" stroke-linecap="round" stroke-linejoin="round" />
            <path d="M24 32V44" stroke="currentColor" stroke-width="4" stroke-linecap="round" />
          </svg>
        {/if}
      </button>
    {/if}
    <button class="text-button compact mode-cycle" aria-label={`Display mode: ${modes.find((item) => item.value === mode)?.label}. Click to change.`} data-tooltip="Change Display Mode" onclick={cycleMode}>{modes.find((item) => item.value === mode)?.label}</button>
    <button class="icon-action" aria-label="Copy message" data-tooltip="Copy" onclick={copy}>
      <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="9" y="9" width="10" height="11" rx="1"/><path d="M15 9V5H5v11h4"/></svg>
    </button>
    {#if onSendDecoder}
      <button class="icon-action" aria-label="Send selected text to Decoder" data-tooltip={selectedText ? "Selection To Decoder" : "To Decoder"} onclick={sendToDecoder}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 4h14v16H5z"/><path d="M8 8h8M8 12h5M14 15l2 2 3-4"/></svg>
      </button>
    {/if}
    <button
      class="icon-action export-action"
      aria-label="Export message"
      data-tooltip="Export request"
      onclick={() => (exportDialog = true)}
    >
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 15V3"/><path d="m8 7 4-4 4 4"/><path d="M5 13v7h14v-7"/></svg>
    </button>
    {#if onCompareResponse}
      <button
        class="icon-action"
        aria-label="Compare response"
        data-tooltip="Compare response"
        disabled={!currentText().length}
        onclick={() => onCompareResponse?.(currentRaw())}
      >
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 5h6v14H4zM14 5h6v14h-6z"/><path d="M10 9h4M10 15h4"/></svg>
      </button>
    {/if}
    {#if editable && onConfigureIdentities}
      <button
        class="icon-action"
        aria-label="Configure identities"
        data-tooltip="Configure Identities"
        onclick={() => onConfigureIdentities?.()}
      >
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2"/><circle cx="9" cy="7" r="4"/><path d="M22 21v-2a4 4 0 0 0-3-3.87M16 3.13a4 4 0 0 1 0 7.75"/></svg>
      </button>
    {/if}
    {#if onSaveOrganizer}
      <button
        class="icon-action organizer-action"
        aria-label="Send current request to Organizer"
        data-tooltip="To Organizer"
        onclick={() => onSaveOrganizer?.(currentRaw())}
      >
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 3h12l2 2v16H5zM8 3v6h8V3M8 21v-7h8v7M9 6h4"/></svg>
      </button>
    {/if}
    {#if onSendReplay}
      <button class="icon-action" aria-label="Send current request to Replay" data-tooltip="To Replay" onclick={() => onSendReplay?.(currentRaw())}>
        <svg viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg"><g id="SVGRepo_bgCarrier" stroke-width="0"></g><g id="SVGRepo_tracerCarrier" stroke-linecap="round" stroke-linejoin="round"></g><g id="SVGRepo_iconCarrier"> <path d="M18.364 8.05026L17.6569 7.34315C14.5327 4.21896 9.46734 4.21896 6.34315 7.34315C3.21895 10.4673 3.21895 15.5327 6.34315 18.6569C9.46734 21.7811 14.5327 21.7811 17.6569 18.6569C19.4737 16.84 20.234 14.3668 19.9377 12.0005M18.364 8.05026H14.1213M18.364 8.05026V3.80762" stroke="#ffffff" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"></path> </g></svg>
      </button>
    {/if}
    {#if onSendFuzz}
      <button class="icon-action" aria-label="Send current request to Fuzz" data-tooltip="To Fuzz" onclick={() => onSendFuzz?.(currentRaw())}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="6"/><path d="M12 3v3M12 18v3M3 12h3M18 12h3M12 10v4M10 12h4"/></svg>
      </button>
    {/if}
  </header>

  <div class="content">
      {#if mode === "raw" || mode === "rawPlus"}
        <CodeEditor
          value={mode === "rawPlus" ? rawPlusText : text}
          readonly={!editable}
          {wrap}
          {search}
          searchRegex={searchRegex}
          searchCaseSensitive={searchCaseSensitive}
          {highlightRanges}
          label={`${title} ${mode === "rawPlus" ? "Raw+" : "raw"} HTTP`}
          messageKind={kind}
          normalizePastedDocument={normalizeRequest ? normalizeHttpLineEndings : undefined}
          onChange={mode === "rawPlus" ? emitRawPlus : emitRaw}
          onSelectionChange={captureSelection}
          onContextMenu={openContextMenu}
          command={editorCommand}
          onCommandHandled={acknowledgeEditorCommand}
        />
      {:else if mode === "hex"}
      <HexViewer bytes={currentRaw()} />
    {:else}
      <div class="pretty">
        <label class="start-line">
          <span>Start line</span>
          <input bind:value={startLine} disabled={!editable} oninput={emitPretty} />
        </label>
        <div class="headers">
          <div class="header-label">Headers</div>
          {#each headers as header, index}
            <div class="header-row">
              <input aria-label={`Header ${index + 1} name`} bind:value={header.name} disabled={!editable} oninput={emitPretty} />
              <input aria-label={`Header ${index + 1} value`} bind:value={header.value} disabled={!editable} oninput={emitPretty} />
              {#if editable}<button aria-label="Remove header" onclick={() => { headers.splice(index, 1); emitPretty(); }}>×</button>{/if}
            </div>
          {/each}
          {#if editable}<button class="text-button compact add-header" onclick={() => { headers.push({ name: "", value: "", delimiter: ": " }); emitPretty(); }}>+ Header</button>{/if}
        </div>
        <label class="body">
          <span>Body</span>
          <textarea bind:value={body} disabled={!editable} oninput={emitPretty}></textarea>
        </label>
      </div>
    {/if}
  </div>
</section>

{#if contextMenu}
  <ContextMenu
    x={contextMenu.x}
    y={contextMenu.y}
    items={contextMenuItems}
    onAction={handleContextAction}
    onClose={closeContextMenu}
  />
{/if}

{#if customMethodDialog}
  <div class="export-backdrop" role="presentation" onclick={(event) => { if (event.target === event.currentTarget) customMethodDialog = false; }} onkeydown={(event) => { if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); customMethodDialog = false; } }}>
    <div class="export-dialog method-dialog" role="dialog" aria-modal="true" aria-labelledby="custom-method-title">
      <form class="method-form" onsubmit={(event) => { event.preventDefault(); submitCustomMethod(); }}>
        <h2 id="custom-method-title">Change Request Method</h2>
        <label>
          <span>HTTP method</span>
          <input bind:this={customMethodInput} bind:value={customMethod} maxlength="32" autocomplete="off" onkeydown={(event) => { if (event.key === "Escape") customMethodDialog = false; }} />
        </label>
        {#if customMethodError}<p class="method-error" role="alert">{customMethodError}</p>{/if}
        <footer>
          <button class="text-button" type="button" onclick={() => (customMethodDialog = false)}>Cancel</button>
          <button class="text-button primary-action" type="submit">Apply</button>
        </footer>
      </form>
    </div>
  </div>
{/if}

{#if exportDialog}
  {@const details = requestDetails()}
  <div class="export-backdrop" role="presentation" onclick={(event) => { if (event.target === event.currentTarget) exportDialog = false; }} onkeydown={(event) => { if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); exportDialog = false; } }}>
    <div class="export-dialog" role="dialog" aria-modal="true" aria-labelledby="message-export-title">
      <div>
        <h2 id="message-export-title">Export request</h2>
      </div>
      <div class="export-options">
        <button class="text-button" onclick={exportHttp} style="text-transform: unset;">Export request as .http</button>
        <button class="text-button" disabled={!details?.url} onclick={() => void copyUrl()}>Copy URL</button>
        <button class="text-button" disabled={!details?.url} onclick={() => void copyCurl()}>Copy as cURL command</button>
      </div>
      <footer><button class="text-button" onclick={() => (exportDialog = false)}>Cancel</button></footer>
    </div>
  </div>
{/if}

<style>
  .message-viewer { display: grid; grid-template-rows: 38px minmax(0, 1fr); min-height: 0; border: 1px solid var(--border, #282e36); border-radius: 6px; overflow: hidden; background: var(--editor, #202020); }
  header { display: flex; align-items: center; gap: 3px; padding: 0 9px; border-bottom: 1px solid var(--border, #282e36); background: var(--editor, #202020); font-size: var(--font-size-body); }
  header > button { margin: 0; }
  .metadata-separator { color: var(--muted, #8d96a3); }
  .metadata { min-width: 0; max-width: 160px; overflow: hidden; color: var(--muted, #8d96a3); text-overflow: ellipsis; white-space: nowrap; }
  .spacer { flex: 1; }
  button { padding: 4px 8px; border: 1px solid #343b45; border-radius: 4px; color: #b9c0ca; background: #171b21; cursor: pointer; text-transform: capitalize; }
  button.icon-action { position: relative; display: grid; place-items: center; width: 28px; padding: 0; color: #fbbf24; border-color: #6b4b16; }
  button.icon-action.minimize-action { color: #b9c0ca; border-color: #343b45; }
  button.icon-action:not(.wrap-icon-action) svg { width: 14px; height: 14px; fill: none; stroke: currentColor; stroke-width: var(--svgbuttonstrokewidth, 1.5); stroke-linecap: round; stroke-linejoin: round; }
  button.icon-action.organizer-action::after { right: -6px; left: auto; transform: translateY(-2px); }
  button.icon-action.organizer-action:hover::after, button.icon-action.organizer-action:focus-visible::after { transform: translateY(0); }
  button.icon-action:last-of-type::after { right: -6px; left: auto; transform: translateY(-2px); }
  button.icon-action:last-of-type:hover::after, button.icon-action:last-of-type:focus-visible::after { transform: translateY(0); }
  button.icon-action.wrap-icon-action {
    display: inline-grid;
    place-items: center;
    width: var(--svgbuttonsize, 24px);
    min-width: var(--svgbuttonsize, 24px);
    height: var(--svgbuttonsize, 24px);
    min-height: var(--svgbuttonsize, 24px);
    padding: 0;
    border: 0;
    border-radius: 3px;
    color: var(--text, #dbe1e8);
    background: transparent;
    cursor: pointer;
  }
  button.icon-action.wrap-icon-action:hover:not(:disabled) {
    border-color: transparent;
    color: var(--text, #dbe1e8);
    background: transparent;
  }
  button.icon-action.wrap-icon-action svg {
    width: calc(var(--svgbuttonsize, 24px) - 6px);
    height: calc(var(--svgbuttonsize, 24px) - 6px);
    fill: none;
    stroke: currentColor;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  button.icon-action.wrap-icon-action svg path {
    stroke-width: var(--svgbuttonstrokewidth, 1.5);
  }
  button.icon-action.wrap-icon-action svg[viewBox="0 0 48 48"] path {
    stroke-width: calc(var(--svgbuttonstrokewidth, 1.5) * 2);
  }
  .mode-cycle { width: 52px; padding: 4px 5px; text-align: center; }
  .content { min-height: 0; overflow: hidden; }
  .pretty { height: 100%; padding: 10px; overflow: auto; color: var(--text, #f4f6fb); background: var(--editor, #202020); font-size: var(--font-size-body); }
  label { display: grid; gap: 5px; }
  .start-line { margin-bottom: 12px; }
  input, textarea { min-width: 0; padding: 6px 8px; border: 1px solid var(--border-strong, #343b45); border-radius: 4px; color: var(--text, #f4f6fb); background: var(--editor, #202020); font: var(--font-size-editor, 12px) ui-monospace, SFMono-Regular, Menlo, monospace; }
  input:disabled, textarea:disabled { opacity: 1; }
  .header-label { margin-bottom: 5px; }
  .header-row { display: grid; grid-template-columns: minmax(100px, .7fr) 1.5fr auto; gap: 5px; margin-bottom: 4px; }
  .add-header { margin: 3px 0 11px; }
  .body textarea { min-height: 110px; resize: vertical; }
  .method-dialog { width: min(360px, calc(100vw - 32px)); }
  .method-form { display: grid; gap: 15px; }
  .method-error { margin: -5px 0 0; color: var(--danger); font-size: var(--font-size-compact); }
  .export-backdrop { position: fixed; z-index: 110; inset: 0; display: grid; place-items: center; padding: 20px; background: rgb(5 7 12 / 24%); backdrop-filter: blur(2px); }
  .export-dialog { display: grid; gap: 15px; width: min(350px, 100%); padding: 20px; border: 1px solid var(--border-strong); border-radius: 7px; color: var(--text); background: var(--surface); box-shadow: var(--shadow); }
  .export-dialog h2 { margin: 0; font-size: var(--font-size-heading); }
  .export-options { display: grid; gap: 5px; }
  .export-options button, .export-dialog footer button { min-height: 31px; border: 1px solid var(--border-strong); border-radius: 4px; color: var(--text); background: var(--surface-2); text-align: left; }
  .export-options button { transition: none; }
  .export-options button:not(:disabled):hover { border-color: color-mix(in srgb, var(--text) 45%, var(--border-strong)); }
  .export-options button:disabled { opacity: .45; cursor: not-allowed; }
  .export-dialog footer { display: flex; justify-content: flex-end; }
  .export-dialog footer button { padding: 0 10px; }
</style>
