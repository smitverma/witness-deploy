<script lang="ts">
  import { tick } from "svelte";
  import { commands } from "$lib/api";
  import { showErrorToast } from "$lib/errorToast";
  import RecycleBinIcon from "./RecycleBinIcon.svelte";
  import Toggle from "./Toggle.svelte";
  import {
    BUILTIN_PAYLOAD_LISTS,
    generatePayloads,
    payloadLines,
  } from "$lib/intruder";
  import type {
    PayloadProcessingRule,
    PayloadProcessingRuleType,
    PayloadWarehouse,
  } from "$lib/types";

  let {
    warehouse,
    disabled = false,
    positions = [],
    selectedPosition = $bindable(0),
  }: {
    warehouse: PayloadWarehouse;
    disabled?: boolean;
    positions?: { number: number; text: string }[];
    selectedPosition?: number;
  } = $props();

  const payloadTypes = [
    ["list", "List"],
    ["numbers", "Numbers"],
    ["null", "Null payload"],
    ["bruteForce", "Brute forcer"],
    ["dates", "Dates"],
    ["characterSubstitution", "Character substitution"],
  ] as const;
  const processingTypes: { value: PayloadProcessingRuleType; label: string }[] = [
    { value: "addPrefix", label: "Add prefix" },
    { value: "addSuffix", label: "Add suffix" },
    { value: "matchReplace", label: "Match / replace" },
    { value: "substring", label: "Substring" },
    { value: "reverseSubstring", label: "Reverse substring" },
    { value: "modifyCase", label: "Modify case" },
    { value: "encode", label: "Encode" },
    { value: "decode", label: "Decode" },
    { value: "hash", label: "Hash" },
  ];

  let listFileInput = $state<HTMLInputElement>();
  let listTextarea = $state<HTMLTextAreaElement>();
  let substitutionFileInput = $state<HTMLInputElement>();
  let loadingUrl = $state(false);
  let showFetchUrlModal = $state(false);
  let fetchUrlDraft = $state("");
  let fetchUrlError = $state("");
  let fetchedUrlEntries = $state<string[] | null>(null);
  let fetchedUrlExamples = $state<string[]>([]);
  let newRuleType = $state<PayloadProcessingRuleType>("addPrefix");
  let showRuleModal = $state(false);
  let editingRuleId = $state<string | null>(null);
  let reportedPayloadError = $state("");
  let ruleDraft = $state<PayloadProcessingRule>(newRule("addPrefix"));
  const payloadSummary = $derived.by(() => {
    try {
      const generated = generatePayloads(warehouse);
      return {
        count: generated.repeatIndefinitely ? null : generated.payloads.length,
        error: "",
      };
    } catch (reason) {
      return { count: 0, error: reason instanceof Error ? reason.message : String(reason) };
    }
  });

  function reportError(reason: unknown) {
    showErrorToast(reason);
  }

  $effect(() => {
    if (payloadSummary.error && payloadSummary.error !== reportedPayloadError) {
      reportError(payloadSummary.error);
    }
    reportedPayloadError = payloadSummary.error;
  });

  function appendLines(target: "list" | "substitution", values: string[]) {
    const clean = values.filter((line) => line.length > 0);
    if (!clean.length) return;
    const current = target === "list"
      ? warehouse.list.text
      : warehouse.characterSubstitution.itemsText;
    const next = [...payloadLines(current), ...clean].join("\n");
    if (target === "list") warehouse.list.text = next;
    else warehouse.characterSubstitution.itemsText = next;
  }

  async function scrollListToBottom() {
    await tick();
    if (listTextarea) listTextarea.scrollTop = listTextarea.scrollHeight;
  }

  async function loadFile(event: Event, target: "list" | "substitution") {
    const input = event.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;
    try {
      appendLines(target, (await file.text()).split(/\r?\n/));
      if (target === "list") await scrollListToBottom();
    } catch (reason) {
      reportError(reason);
    } finally {
      input.value = "";
    }
  }

  function clearList(target: "list" | "substitution") {
    if (target === "list") warehouse.list.text = "";
    else warehouse.characterSubstitution.itemsText = "";
  }

  function removeLatest(target: "list" | "substitution") {
    const values = payloadLines(
      target === "list" ? warehouse.list.text : warehouse.characterSubstitution.itemsText,
    );
    values.pop();
    if (target === "list") warehouse.list.text = values.join("\n");
    else warehouse.characterSubstitution.itemsText = values.join("\n");
  }

  function deduplicate(target: "list" | "substitution") {
    const value = target === "list"
      ? warehouse.list.text
      : warehouse.characterSubstitution.itemsText;
    const next = [...new Set(payloadLines(value))].join("\n");
    if (target === "list") warehouse.list.text = next;
    else warehouse.characterSubstitution.itemsText = next;
  }

  function addBuiltinList() {
    appendLines("list", BUILTIN_PAYLOAD_LISTS[warehouse.list.builtin] ?? []);
  }

  function addSubstitutionItem() {
    if (!warehouse.characterSubstitution.newItem) return;
    appendLines("substitution", [warehouse.characterSubstitution.newItem]);
    warehouse.characterSubstitution.newItem = "";
  }

  function addSubstitutionBuiltin() {
    appendLines(
      "substitution",
      BUILTIN_PAYLOAD_LISTS[warehouse.characterSubstitution.builtin] ?? [],
    );
  }

  async function pasteSubstitutionItems() {
    try {
      appendLines("substitution", (await navigator.clipboard.readText()).split(/\r?\n/));
    } catch (reason) {
      reportError(`Could not read the clipboard: ${reason instanceof Error ? reason.message : String(reason)}`);
    }
  }

  function openFetchUrlModal() {
    fetchUrlDraft = warehouse.list.url;
    fetchUrlError = "";
    fetchedUrlEntries = null;
    fetchedUrlExamples = [];
    showFetchUrlModal = true;
  }

  function closeFetchUrlModal() {
    if (loadingUrl) return;
    showFetchUrlModal = false;
  }

  function returnToFetchUrlForm() {
    if (loadingUrl) return;
    fetchUrlError = "";
    fetchedUrlEntries = null;
    fetchedUrlExamples = [];
  }

  function importFetchedUrlEntries() {
    if (!fetchedUrlEntries?.length) return;
    appendLines("list", fetchedUrlEntries);
    void scrollListToBottom();
    closeFetchUrlModal();
  }

  function fetchErrorMessage(reason: unknown) {
    const message = reason instanceof Error ? reason.message : String(reason);
    return message || "Unable to fetch payload data. Check the URL and try again.";
  }

  function ensurePayloadUrl(value: string) {
    const url = new URL(value);
    if (url.protocol !== "http:" && url.protocol !== "https:") {
      throw new Error("Payload URL must use HTTP or HTTPS");
    }
    return url;
  }

  function responseHeader(raw: Uint8Array, name: string) {
    const text = new TextDecoder().decode(raw);
    const match = /\r\n\r\n|\n\n|\r\r/.exec(text);
    const head = text.slice(0, match?.index ?? text.length);
    const prefix = `${name.toLowerCase()}:`;
    return head.split(/\r?\n/).find((line) => line.toLowerCase().startsWith(prefix))?.slice(prefix.length).trim() ?? "";
  }

  async function requestPayloadUrl(initialUrl: URL, requestId: string) {
    const redirects = new Set<string>();
    let url = initialUrl;
    for (let hop = 0; hop <= 5; hop += 1) {
      const key = url.toString();
      if (redirects.has(key)) throw new Error("Payload URL redirected in a loop");
      redirects.add(key);

      const path = `${url.pathname || "/"}${url.search}`;
      const request = new TextEncoder().encode(
        `GET ${path} HTTP/1.1\r\nHost: ${url.host}\r\nAccept: text/plain\r\nAccept-Encoding: identity\r\nConnection: close\r\n\r\n`,
      );
      const response = await commands.sendRepeaterRequest(
        `${requestId}-${hop}`,
        request,
        url.protocol === "https:",
      );
      if (![301, 302, 303, 307, 308].includes(response.status)) return { response, url };
      if (hop === 5) throw new Error("Payload URL redirected too many times");

      const location = responseHeader(new Uint8Array(response.raw), "location");
      if (!location) throw new Error(`Payload URL returned HTTP ${response.status} without a Location header`);
      try {
        url = ensurePayloadUrl(new URL(location, url).toString());
      } catch {
        throw new Error("Payload URL redirected to an invalid or unsupported URL");
      }
    }
    throw new Error("Payload URL redirected too many times");
  }

  async function fetchUrlList() {
    fetchUrlError = "";
    fetchedUrlEntries = null;
    fetchedUrlExamples = [];
    let url: URL;
    try {
      if (!fetchUrlDraft.trim()) throw new Error("Enter a URL to fetch payload data");
      url = ensurePayloadUrl(fetchUrlDraft.trim());
    } catch (reason) {
      fetchUrlError = reason instanceof Error ? reason.message : "Enter a valid HTTP or HTTPS URL";
      return;
    }

    loadingUrl = true;
    const requestId = `payload-url-${Date.now()}`;
    try {
      const { response, url: finalUrl } = await requestPayloadUrl(url, requestId);
      if (response.status < 200 || response.status >= 300) {
        throw new Error(`Payload URL returned HTTP ${response.status}`);
      }
      const body = responseBody(new Uint8Array(response.raw));
      const lines = body.replace(/^\uFEFF/, "").split(/\r?\n/).filter((line) => line.length > 0);
      if (!lines.length) throw new Error("The fetched file does not contain any payload entries");
      warehouse.list.url = finalUrl.toString();
      fetchedUrlEntries = lines;
      fetchedUrlExamples = lines.slice(0, 3);
    } catch (reason) {
      fetchUrlError = fetchErrorMessage(reason);
    } finally {
      loadingUrl = false;
    }
  }

  function responseBody(raw: Uint8Array) {
    const text = new TextDecoder().decode(raw);
    const match = /\r\n\r\n|\n\n|\r\r/.exec(text);
    if (!match || match.index === undefined) throw new Error("Payload URL returned an invalid HTTP response");
    const head = text.slice(0, match.index);
    const body = text.slice(match.index + match[0].length);
    if (/^transfer-encoding\s*:\s*.*\bchunked\b/im.test(head)) return decodeChunked(body);
    return body;
  }

  function decodeChunked(value: string) {
    let cursor = 0;
    let output = "";
    while (cursor < value.length) {
      const lineEnd = value.indexOf("\r\n", cursor);
      if (lineEnd < 0) throw new Error("Payload URL returned malformed chunked data");
      const size = Number.parseInt(value.slice(cursor, lineEnd).split(";", 1)[0], 16);
      if (!Number.isFinite(size)) throw new Error("Payload URL returned an invalid chunk size");
      cursor = lineEnd + 2;
      if (size === 0) return output;
      output += value.slice(cursor, cursor + size);
      cursor += size + 2;
    }
    throw new Error("Payload URL ended before the final chunk");
  }

  function newRule(type: PayloadProcessingRuleType): PayloadProcessingRule {
    return {
      id: `${Date.now()}-${Math.random().toString(16).slice(2)}`,
      enabled: true,
      type,
      value: "",
      match: "",
      replacement: "",
      useRegex: false,
      caseSensitive: true,
      start: "0",
      length: "1",
      operation: type === "modifyCase"
        ? "upper"
        : type === "hash"
          ? "sha256"
          : "url",
    };
  }

  function openAddRule() {
    editingRuleId = null;
    ruleDraft = newRule(newRuleType);
    showRuleModal = true;
  }

  function openEditRule(rule: PayloadProcessingRule) {
    editingRuleId = rule.id;
    ruleDraft = { ...rule };
    showRuleModal = true;
  }

  function changeRuleType(type: PayloadProcessingRuleType) {
    const replacement = newRule(type);
    ruleDraft = {
      ...replacement,
      id: ruleDraft.id,
      enabled: ruleDraft.enabled,
    };
  }

  function saveRule() {
    try {
      validateRule(ruleDraft);
    } catch (reason) {
      reportError(reason);
      return;
    }
    if (editingRuleId) {
      const existing = warehouse.processing.find((rule) => rule.id === editingRuleId);
      if (existing) Object.assign(existing, ruleDraft);
    } else {
      warehouse.processing.push({ ...ruleDraft });
    }
    newRuleType = ruleDraft.type;
    showRuleModal = false;
    editingRuleId = null;
  }

  function removeRule(id: string) {
    const index = warehouse.processing.findIndex((rule) => rule.id === id);
    if (index < 0) return;
    warehouse.processing.splice(index, 1);
  }

  function moveRule(id: string, direction: -1 | 1) {
    const index = warehouse.processing.findIndex((rule) => rule.id === id);
    const target = index + direction;
    if (index < 0 || target < 0 || target >= warehouse.processing.length) return;
    const [rule] = warehouse.processing.splice(index, 1);
    warehouse.processing.splice(target, 0, rule);
  }

  function ruleLabel(rule: PayloadProcessingRule) {
    return processingTypes.find((type) => type.value === rule.type)?.label ?? rule.type;
  }

  function ruleValue(rule: PayloadProcessingRule) {
    if (rule.type === "addPrefix" || rule.type === "addSuffix") return rule.value || "(empty)";
    if (rule.type === "matchReplace") return `${rule.match || "(empty)"} → ${rule.replacement || "(empty)"}`;
    if (rule.type === "substring" || rule.type === "reverseSubstring") {
      return `start ${rule.start}, length ${rule.length}`;
    }
    return rule.operation.toUpperCase();
  }

  function validateRule(rule: PayloadProcessingRule) {
    if ((rule.type === "addPrefix" || rule.type === "addSuffix") && !rule.value) {
      throw new Error("Enter the text used by this processing rule");
    }
    if (rule.type === "matchReplace") {
      if (!rule.match) throw new Error("Enter text to match");
      if (rule.useRegex) new RegExp(rule.match, rule.caseSensitive ? "g" : "gi");
    }
    if (rule.type === "substring" || rule.type === "reverseSubstring") {
      const start = Number(rule.start);
      const length = Number(rule.length);
      if (!Number.isInteger(start) || start < 0) throw new Error("Start index must be zero or greater");
      if (!Number.isInteger(length) || length < 1) throw new Error("Length must be a positive integer");
    }
  }
</script>

<aside class="payload-warehouse" aria-label="Payload Warehouse">
  <div class="warehouse-title">
    <span>Payload Warehouse</span>
    <small>
      {#if payloadSummary.count === null}
        Continuous
      {:else}
        {payloadSummary.count} generated
      {/if}
    </small>
  </div>

  <div class="warehouse-scroll">
    {#if positions.length > 1}
      <label class="position-selector">
        <span>Payload position</span>
        <select bind:value={selectedPosition} disabled={disabled}>
          {#each positions as position, index}
            <option value={index}>{position.number}: {position.text || "(empty selection)"}</option>
          {/each}
        </select>
      </label>
    {/if}
    <section class="warehouse-section">
      <div class="section-heading"><span>A</span><strong>Payload Type</strong></div>
      <label class="type-select">
        <span>Type</span>
        <select bind:value={warehouse.type} disabled={disabled}>
        {#each payloadTypes as type}
            <option value={type[0]}>{type[1]}</option>
        {/each}
        </select>
      </label>
    </section>

    <section class="warehouse-section">
      <div class="section-heading"><span>B</span><strong>Payload Configurator</strong></div>

      {#if warehouse.type === "list"}
        <div class="config-stack">
          <div class="button-row">
            <button class="text-button warehouse-icon-button" type="button" aria-label="Load file" data-tooltip="Load file" onclick={() => listFileInput?.click()} disabled={disabled}>
              <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 3.5h8l4 4V20.5H6z" /><path d="M14 3.5v4h4M12 16V9m-2.5 2.5L12 9l2.5 2.5" /></svg>
            </button>
            <button class="text-button warehouse-icon-button" type="button" aria-label="Fetch URL" data-tooltip="Fetch URL" onclick={openFetchUrlModal} disabled={disabled}>
              <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="8.5" /><path d="M3.5 12h17M12 3.5c2.1 2.3 3.2 5.1 3.2 8.5s-1.1 6.2-3.2 8.5c-2.1-2.3-3.2-5.1-3.2-8.5S9.9 5.8 12 3.5Z" /></svg>
            </button>
            <button class="text-button warehouse-icon-button" type="button" aria-label="Remove duplicates" data-tooltip="Remove duplicates" onclick={() => deduplicate("list")} disabled={disabled}>
              <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 6h10M5 11h10M5 16h7M16 14l4 4m0-4-4 4" /></svg>
            </button>
            <button class="text-button warehouse-icon-button" type="button" aria-label="Remove latest" data-tooltip="Remove latest" onclick={() => removeLatest("list")} disabled={disabled}>
              <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4.5 12h8M15.5 10l2-2v8M15 16h5" /></svg>
            </button>
            <button class="text-button warehouse-icon-button" type="button" aria-label="Clear list" data-tooltip="Clear list" onclick={() => clearList("list")} disabled={disabled}>
              <RecycleBinIcon />
            </button>
            <select class="builtin-select" bind:value={warehouse.list.builtin} disabled={disabled} aria-label="Built-in payload list" data-tooltip="Select from built-in lists">
              {#each Object.keys(BUILTIN_PAYLOAD_LISTS) as name}<option value={name}>{name}</option>{/each}
            </select>
            <button class="text-button warehouse-icon-button" type="button" aria-label="Add built-in" data-tooltip="Add built-in" onclick={addBuiltinList} disabled={disabled}>
              <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
            </button>
            <input class="hidden-file" bind:this={listFileInput} type="file" accept="text/plain,.txt,.lst,.csv" onchange={(event) => void loadFile(event, "list")} />
          </div>
          <textarea bind:this={listTextarea} bind:value={warehouse.list.text} disabled={disabled} aria-label="List payloads" placeholder={"admin\nuser@example.com\nsample"}></textarea>
        </div>
      {:else if warehouse.type === "numbers"}
        <div class="config-stack">
          <p class="description">Generate a sequential or random number range.</p>
          <div class="radio-row">
            <span>Type</span>
            <label><input type="radio" value="sequential" bind:group={warehouse.numbers.mode} disabled={disabled} /> Sequential</label>
            <label><input type="radio" value="random" bind:group={warehouse.numbers.mode} disabled={disabled} /> Random</label>
          </div>
          <div class="field-grid">
            <label><span>From</span><input type="number" bind:value={warehouse.numbers.from} disabled={disabled} /></label>
            <label><span>To</span><input type="number" bind:value={warehouse.numbers.to} disabled={disabled} /></label>
            <label><span>Step</span><input type="number" min="0.0000001" step="any" bind:value={warehouse.numbers.step} disabled={disabled} /></label>
            <label><span>How many</span><input type="number" min="1" max="5000" bind:value={warehouse.numbers.count} disabled={disabled} placeholder={warehouse.numbers.mode === "random" ? "10" : "Until end"} /></label>
          </div>
        </div>
      {:else if warehouse.type === "null"}
        <div class="config-stack">
          <p class="description">Generates empty-string payloads. With no markers, this repeats the unmodified base request.</p>
          <label class="radio-option">
            <input type="radio" value="count" bind:group={warehouse.nullPayload.mode} disabled={disabled} />
            <span>Generate</span>
            <input type="number" min="1" max="5000" bind:value={warehouse.nullPayload.count} disabled={disabled || warehouse.nullPayload.mode !== "count"} />
            <span>payloads</span>
          </label>
          <label class="radio-option">
            <input type="radio" value="infinite" bind:group={warehouse.nullPayload.mode} disabled={disabled} />
            <span>Continue indefinitely, until stopped</span>
          </label>
        </div>
      {:else if warehouse.type === "bruteForce"}
        <div class="config-stack">
          <p class="description">Generate all permutations of the character set for the configured lengths.</p>
          <label><span>Character set</span><input bind:value={warehouse.bruteForce.characterSet} disabled={disabled} /></label>
          <div class="field-grid">
            <label><span>Minimum length</span><input type="number" min="1" bind:value={warehouse.bruteForce.minLength} disabled={disabled} /></label>
            <label><span>Maximum length</span><input type="number" min="1" bind:value={warehouse.bruteForce.maxLength} disabled={disabled} /></label>
          </div>
        </div>
      {:else if warehouse.type === "dates"}
        <div class="config-stack">
          <p class="description">Generate date payloads within a range and format them before use.</p>
          <div class="field-grid">
            <label><span>From</span><input type="date" bind:value={warehouse.dates.from} disabled={disabled} /></label>
            <label><span>To</span><input type="date" bind:value={warehouse.dates.to} disabled={disabled} /></label>
          </div>
          <div class="inline-fields">
            <label><span>Step</span><input type="number" min="1" bind:value={warehouse.dates.step} disabled={disabled} /></label>
            <label><span>Unit</span><select bind:value={warehouse.dates.unit} disabled={disabled}><option value="days">Days</option><option value="weeks">Weeks</option><option value="months">Months</option><option value="years">Years</option></select></label>
          </div>
          <label class="radio-option">
            <input type="radio" value="preset" bind:group={warehouse.dates.formatMode} disabled={disabled} />
            <span>Preset</span>
            <select bind:value={warehouse.dates.format} disabled={disabled || warehouse.dates.formatMode !== "preset"}>
              <option value="M/D/YY">7/28/26</option>
              <option value="MM/DD/YYYY">07/28/2026</option>
              <option value="YYYY-MM-DD">2026-07-28</option>
              <option value="DD/MM/YYYY">28/07/2026</option>
              <option value="dd.MM.yyyy">28.07.2026</option>
            </select>
          </label>
          <label class="radio-option">
            <input type="radio" value="custom" bind:group={warehouse.dates.formatMode} disabled={disabled} />
            <span>Custom</span>
            <input bind:value={warehouse.dates.customFormat} disabled={disabled || warehouse.dates.formatMode !== "custom"} placeholder="dd.MM.yyyy" />
          </label>
        </div>
      {:else}
        <div class="config-stack">
          <p class="description">Apply configured substitutions to every item and generate each resulting combination.</p>
          <div class="substitution-grid">
            {#each warehouse.characterSubstitution.mappings as mapping}
              <div><input bind:value={mapping.from} maxlength="4" disabled={disabled} aria-label="Character to replace" /><span>→</span><input bind:value={mapping.to} maxlength="12" disabled={disabled} aria-label="Replacement character" /></div>
            {/each}
          </div>
          <label class="check-row"><Toggle bind:checked={warehouse.characterSubstitution.caseSensitive} disabled={disabled} ariaLabel="Case-sensitive match" /> Case-sensitive match</label>
          <div class="button-row">
            <button class="text-button" onclick={() => void pasteSubstitutionItems()} disabled={disabled}>Paste</button>
            <button class="text-button warehouse-icon-button" type="button" aria-label="Load file" data-tooltip="Load file" onclick={() => substitutionFileInput?.click()} disabled={disabled}>
              <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 3.5h8l4 4V20.5H6z" /><path d="M14 3.5v4h4M12 16V9m-2.5 2.5L12 9l2.5 2.5" /></svg>
            </button>
            <button class="text-button warehouse-icon-button" type="button" aria-label="Remove duplicates" data-tooltip="Remove duplicates" onclick={() => deduplicate("substitution")} disabled={disabled}>
              <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 6h10M5 11h10M5 16h7M16 14l4 4m0-4-4 4" /></svg>
            </button>
            <button class="text-button warehouse-icon-button" type="button" aria-label="Remove latest" data-tooltip="Remove latest" onclick={() => removeLatest("substitution")} disabled={disabled}>
              <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4.5 12h8M15.5 10l2-2v8M15 16h5" /></svg>
            </button>
            <button class="text-button warehouse-icon-button" type="button" aria-label="Clear list" data-tooltip="Clear list" onclick={() => clearList("substitution")} disabled={disabled}>
              <RecycleBinIcon />
            </button>
            <select class="builtin-select" bind:value={warehouse.characterSubstitution.builtin} disabled={disabled} aria-label="Built-in substitution input list" data-tooltip="Select from built-in lists">
              {#each Object.keys(BUILTIN_PAYLOAD_LISTS) as name}<option value={name}>{name}</option>{/each}
            </select>
            <button class="text-button warehouse-icon-button" type="button" aria-label="Add built-in" data-tooltip="Add built-in" onclick={addSubstitutionBuiltin} disabled={disabled}>
              <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
            </button>
            <input class="hidden-file" bind:this={substitutionFileInput} type="file" accept="text/plain,.txt,.lst" onchange={(event) => void loadFile(event, "substitution")} />
          </div>
          <textarea bind:value={warehouse.characterSubstitution.itemsText} disabled={disabled} aria-label="Character substitution items" placeholder={"password\nadministrator"}></textarea>
          <div class="inline-fields">
            <input bind:value={warehouse.characterSubstitution.newItem} disabled={disabled} placeholder="Enter a new item" onkeydown={(event) => { if (event.key === "Enter") { event.preventDefault(); addSubstitutionItem(); } }} />
            <button class="text-button" onclick={addSubstitutionItem} disabled={disabled}>Add item</button>
          </div>
        </div>
      {/if}

    </section>

    <section class="warehouse-section processing-section">
      <div class="section-heading"><span>C</span><strong>Payload Processing</strong></div>
      <p class="description">Rules run from top to bottom on every generated payload before it is inserted.</p>
      <div class="inline-fields">
        <select bind:value={newRuleType} disabled={disabled} aria-label="Payload processing rule type">
          {#each processingTypes as type}<option value={type.value}>{type.label}</option>{/each}
        </select>
        <button class="text-button" onclick={openAddRule} disabled={disabled}>Add rule</button>
      </div>

      <div class="rule-table">
        <div class="rule-table-header">
          <span>On</span><span>Rule</span><span>Value</span><span>Arrange</span><span>Actions</span>
        </div>
        {#each warehouse.processing as rule, index (rule.id)}
          <div class="processing-rule">
            <Toggle bind:checked={rule.enabled} disabled={disabled} ariaLabel={`Enable ${ruleLabel(rule)}`} />
            <span class="rule-name">{index + 1}. {ruleLabel(rule)}</span>
            <code data-tooltip={ruleValue(rule)}>{ruleValue(rule)}</code>
            <span class="arrange-actions">
              <button data-tooltip="Move up" aria-label={`Move ${ruleLabel(rule)} up`} disabled={disabled || index === 0} onclick={() => moveRule(rule.id, -1)}>↑</button>
              <button data-tooltip="Move down" aria-label={`Move ${ruleLabel(rule)} down`} disabled={disabled || index === warehouse.processing.length - 1} onclick={() => moveRule(rule.id, 1)}>↓</button>
            </span>
            <span class="rule-actions">
              <button class="text-button compact" disabled={disabled} onclick={() => openEditRule(rule)}>Edit</button>
              <button class="text-button compact danger" aria-label={`Delete ${ruleLabel(rule)}`} disabled={disabled} onclick={() => removeRule(rule.id)}>Delete</button>
            </span>
          </div>
        {:else}
          <div class="empty-rules">No processing rules.</div>
        {/each}
      </div>
    </section>
  </div>
</aside>

{#if showRuleModal}
  <div class="rule-modal-backdrop" role="presentation" onclick={(event) => { if (event.target === event.currentTarget) showRuleModal = false; }}>
    <form class="rule-modal" aria-label={editingRuleId ? "Edit processing rule" : "Add processing rule"} onsubmit={(event) => { event.preventDefault(); saveRule(); }}>
      <header>
        <div><strong>{editingRuleId ? "Edit rule" : "Add rule"}</strong><span>Configure the value transformation.</span></div>
        <button type="button" aria-label="Close processing rule dialog" onclick={() => (showRuleModal = false)}>×</button>
      </header>
      <div class="rule-modal-content">
        <label>
          <span>Processing type</span>
          <select value={ruleDraft.type} onchange={(event) => changeRuleType(event.currentTarget.value as PayloadProcessingRuleType)}>
            {#each processingTypes as type}<option value={type.value}>{type.label}</option>{/each}
          </select>
        </label>

        <div class="value-section">
          <strong>Value</strong>
          {#if ruleDraft.type === "addPrefix" || ruleDraft.type === "addSuffix"}
            <label><span>Text</span><input bind:value={ruleDraft.value} /></label>
          {:else if ruleDraft.type === "matchReplace"}
            <label><span>Match</span><input bind:value={ruleDraft.match} /></label>
            <label><span>Replace with</span><input bind:value={ruleDraft.replacement} /></label>
            <div class="button-row">
              <label class="check-row"><Toggle bind:checked={ruleDraft.useRegex} ariaLabel="Regular expression" /> Regular expression</label>
              <label class="check-row"><Toggle bind:checked={ruleDraft.caseSensitive} ariaLabel="Case sensitive" /> Case sensitive</label>
            </div>
          {:else if ruleDraft.type === "substring" || ruleDraft.type === "reverseSubstring"}
            <div class="field-grid">
              <label><span>Start index</span><input type="number" min="0" bind:value={ruleDraft.start} /></label>
              <label><span>Length</span><input type="number" min="1" bind:value={ruleDraft.length} /></label>
            </div>
          {:else if ruleDraft.type === "modifyCase"}
            <label><span>Operation</span><select bind:value={ruleDraft.operation}><option value="upper">Uppercase</option><option value="lower">Lowercase</option><option value="capitalize">Capitalize words</option></select></label>
          {:else if ruleDraft.type === "encode" || ruleDraft.type === "decode"}
            <label><span>Format</span><select bind:value={ruleDraft.operation}><option value="url">URL</option><option value="base64">Base64</option><option value="hex">Hexadecimal</option></select></label>
          {:else}
            <label><span>Algorithm</span><select bind:value={ruleDraft.operation}><option value="sha1">SHA-1</option><option value="sha256">SHA-256</option><option value="sha512">SHA-512</option></select></label>
          {/if}
        </div>
      </div>
      <footer>
        <button class="text-button" type="button" onclick={() => (showRuleModal = false)}>Cancel</button>
        <button class="text-button done" type="submit">Done</button>
      </footer>
    </form>
  </div>
{/if}

{#if showFetchUrlModal}
  <div class="rule-modal-backdrop" role="presentation" onclick={(event) => { if (event.target === event.currentTarget) closeFetchUrlModal(); }}>
    <form class="fetch-url-modal" aria-label="Fetch payload data" novalidate onsubmit={(event) => { event.preventDefault(); void fetchUrlList(); }}>
      <header>
        <div><strong>Fetch payload data</strong><span>Download a newline-separated payload list from a URL.</span></div>
        <button type="button" aria-label="Close fetch payload dialog" disabled={loadingUrl} onclick={closeFetchUrlModal}>×</button>
      </header>
      <div class="fetch-url-content">
        {#if fetchedUrlEntries}
          <div class="fetch-summary">
            <strong>Payload data fetched</strong>
            <span>Entry count: {fetchedUrlEntries.length}</span>
          </div>
          <div class="fetch-examples">
            <strong>Sample entries from the list</strong>
            <ol>
              {#each fetchedUrlExamples as example}<li><code>{example}</code></li>{/each}
            </ol>
          </div>
        {:else}
          <label>
            <span>URL</span>
            <input bind:value={fetchUrlDraft} disabled={loadingUrl} placeholder="https://example.com/payloads.txt" type="url" autocomplete="url" />
          </label>
          {#if fetchUrlError}<p class="fetch-error" role="alert">{fetchUrlError}</p>{/if}
        {/if}
      </div>
      <footer>
        {#if fetchedUrlEntries}
          <button class="text-button" type="button" onclick={returnToFetchUrlForm}>Back</button>
          <button class="text-button done" type="button" onclick={importFetchedUrlEntries}>Import</button>
        {:else}
          <button class="text-button" type="button" disabled={loadingUrl} onclick={closeFetchUrlModal}>Cancel</button>
          <button class="text-button done" type="submit" disabled={loadingUrl || !fetchUrlDraft.trim()}>{loadingUrl ? "Fetching data…" : "Fetch data"}</button>
        {/if}
      </footer>
    </form>
  </div>
{/if}

<style>
  .payload-warehouse { position: relative; display: grid; grid-template-rows: 31px minmax(0, 1fr); width: 100%; height: 100%; min-width: 0; min-height: 0; overflow: hidden; color: var(--text); border: 1px solid var(--border); border-radius: 3px; background: var(--surface); }
  .warehouse-title { display: flex; align-items: center; justify-content: space-between; padding: 0 8px; border-bottom: 1px solid var(--border); font-size: var(--font-size-compact); font-weight: 700; }
  .warehouse-title small { color: var(--muted); font-size: var(--font-size-compact); font-weight: 500; }
  .warehouse-scroll { height: 100%; min-height: 0; overflow: auto; overscroll-behavior: contain; }
  .position-selector { position: sticky; z-index: 2; top: 0; display: grid; grid-template-columns: auto minmax(0, 1fr); align-items: center; gap: 8px; padding: 8px 9px; border-bottom: 1px solid var(--border); background: var(--surface); }
  .position-selector > span { margin: 0; white-space: nowrap; }
  .position-selector select { width: 100%; }
  .warehouse-section { padding: 9px; border-bottom: 1px solid var(--border); }
  .section-heading { display: flex; align-items: center; gap: 7px; margin-bottom: 8px; font-size: var(--font-size-compact); }
  .section-heading > span { display: grid; place-items: center; width: 18px; height: 18px; color: var(--accent); border-radius: 4px; background: var(--accent-soft); font-size: var(--font-size-compact); font-weight: 800; }
  .type-select { display: grid; grid-template-columns: 52px minmax(0, 1fr); align-items: center; gap: 7px; }
  .type-select > span { margin: 0; }
  .config-stack { display: grid; gap: 7px; }
  .description { margin: 0; color: var(--muted); font-size: var(--font-size-compact); line-height: 1.45; }
  button, input, select, textarea { min-width: 0; border: 1px solid var(--border-strong); border-radius: 3px; color: var(--text); background: var(--input); font-size: var(--font-size-compact); }
  button { min-height: 27px; padding: 0 7px; background: var(--surface-2); cursor: pointer; }
  button:disabled, input:disabled, select:disabled, textarea:disabled { opacity: .5; cursor: default; }
  .warehouse-icon-button { position: relative; display: grid !important; width: 30px; min-width: 30px; height: 28px; min-height: 28px; place-items: center; padding: 0 !important; color: var(--muted); border: 0; background: transparent; box-shadow: none; }
  .warehouse-icon-button:hover:not(:disabled), .warehouse-icon-button:focus-visible { color: var(--text); border-color: transparent; background: transparent; box-shadow: none; }
  .warehouse-icon-button svg { width: 15px; height: 15px; fill: none; stroke: currentColor; stroke-linecap: round; stroke-linejoin: round; stroke-width: var(--svgbuttonstrokewidth, 1.5); }
  input, select { height: 28px; padding: 0 7px; }
  textarea { width: 100%; min-height: 126px; padding: 7px; resize: vertical; font: var(--font-size-body)/1.45 ui-monospace, SFMono-Regular, Menlo, monospace; }
  textarea:hover, textarea:focus, textarea:active { border-color: var(--border-strong); box-shadow: none; }
  label { color: var(--muted); font-size: var(--font-size-compact); }
  label > span { display: block; margin-bottom: 3px; }
  label > input:not([type="radio"]), label > select { width: 100%; }
  .button-row, .inline-fields, .radio-row, .radio-option, .check-row { display: flex; align-items: center; gap: 5px; }
  .button-row { flex-wrap: wrap; }
  .button-row .builtin-select { flex: 1; min-width: 140px; }
  .inline-fields > :first-child { flex: 1; }
  .inline-fields label { flex: 1; }
  .radio-row > span { min-width: 45px; color: var(--muted); font-size: var(--font-size-compact); }
  .radio-row label, .radio-option, .check-row { color: var(--text); }
  .radio-option > input[type="number"], .radio-option > input:not([type]), .radio-option > select { flex: 1; }
  .radio-option > span { margin: 0; white-space: nowrap; }
  input[type="radio"] { width: 14px; height: 14px; accent-color: var(--accent); }
  .field-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 6px; }
  .hidden-file { display: none; }
  .substitution-grid { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 4px; }
  .substitution-grid > div { display: grid; grid-template-columns: minmax(0, 1fr) auto minmax(0, 1fr); align-items: center; gap: 2px; color: var(--muted); }
  .substitution-grid input { width: 100%; text-align: center; }
  .processing-section > .inline-fields { margin-bottom: 7px; }
  .rule-table { min-width: 0; overflow: hidden; border: 1px solid var(--border); border-radius: 3px; }
  .rule-table-header, .processing-rule { display: grid; grid-template-columns: 24px minmax(70px, .8fr) minmax(90px, 1.2fr) 54px 82px; align-items: center; gap: 4px; padding: 0 4px; }
  .rule-table-header { min-height: 25px; color: var(--muted); border-bottom: 1px solid var(--border); background: var(--surface-2); font-size: var(--font-size-compact); font-weight: 700; text-transform: uppercase; }
  .processing-rule { min-height: 34px; border-bottom: 1px solid var(--border); }
  .processing-rule:last-child { border-bottom: 0; }
  .processing-rule .rule-name { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .processing-rule code { min-width: 0; overflow: hidden; color: var(--muted); text-overflow: ellipsis; white-space: nowrap; font-size: var(--font-size-compact); }
  .arrange-actions, .rule-actions { display: flex; gap: 2px; }
  .processing-rule button { min-height: 22px; padding: 0 4px; font-size: var(--font-size-compact); }
  .arrange-actions button { width: 24px; padding: 0; }
  .empty-rules { padding: 12px; color: var(--muted); text-align: center; font-size: var(--font-size-compact); }
  .rule-modal-backdrop { position: fixed; z-index: 120; inset: 0; display: grid; place-items: center; padding: 24px; background: color-mix(in srgb, var(--bg) 72%, transparent); backdrop-filter: blur(2px); }
  .rule-modal { display: grid; grid-template-rows: auto minmax(0, 1fr) auto; width: min(480px, 100%); max-height: min(620px, calc(100dvh - 48px)); overflow: hidden; color: var(--text); border: 1px solid var(--border-strong); border-radius: 8px; background: var(--surface); box-shadow: var(--shadow); }
  .rule-modal > header { display: flex; align-items: center; justify-content: space-between; padding: 12px 14px; border-bottom: 1px solid var(--border); }
  .rule-modal > header div { display: grid; gap: 2px; }
  .rule-modal > header strong { font-size: var(--font-size-body); }
  .rule-modal > header span { color: var(--muted); font-size: var(--font-size-compact); }
  .rule-modal > header button { width: 28px; padding: 0; font-size: var(--font-size-heading); }
  .rule-modal-content { display: grid; gap: 12px; padding: 14px; overflow: auto; }
  .value-section { display: grid; gap: 8px; padding: 10px; border: 1px solid var(--border); border-radius: 5px; background: var(--surface-2); }
  .value-section > strong { font-size: var(--font-size-compact); }
  .rule-modal > footer { display: flex; justify-content: flex-end; gap: 6px; padding: 10px 14px; border-top: 1px solid var(--border); }
  .rule-modal .done { color: var(--text); border-color: var(--accent); background: var(--accent); font-weight: 700; }
  .fetch-url-modal { display: grid; grid-template-rows: auto minmax(0, 1fr) auto; width: min(440px, 100%); overflow: hidden; color: var(--text); border: 1px solid var(--border-strong); border-radius: 8px; background: var(--surface); box-shadow: var(--shadow); }
  .fetch-url-modal > header { display: flex; align-items: center; justify-content: space-between; padding: 12px 14px; border-bottom: 1px solid var(--border); }
  .fetch-url-modal > header div { display: grid; gap: 2px; }
  .fetch-url-modal > header strong { font-size: var(--font-size-body); }
  .fetch-url-modal > header span { color: var(--muted); font-size: var(--font-size-compact); }
  .fetch-url-modal > header button { width: 28px; padding: 0; font-size: var(--font-size-heading); }
  .fetch-url-content { display: grid; gap: 12px; padding: 14px; }
  .fetch-error { margin: 0; padding: 8px 9px; color: var(--danger); border: 1px solid color-mix(in srgb, var(--danger) 38%, var(--border)); border-radius: 4px; background: var(--danger-soft); font-size: var(--font-size-compact); line-height: 1.4; }
  .fetch-summary, .fetch-examples { display: grid; gap: 5px; padding: 10px; border: 1px solid var(--border); border-radius: 5px; background: var(--surface-2); }
  .fetch-summary strong, .fetch-examples > strong { font-size: var(--font-size-compact); }
  .fetch-summary span { color: var(--muted); font-size: var(--font-size-compact); }
  .fetch-examples ol { display: grid; gap: 4px; margin: 0; padding: 0; color: var(--muted); list-style: none; }
  .fetch-examples li { min-width: 0; }
  .fetch-examples code { display: block; overflow: hidden; color: var(--text); text-overflow: ellipsis; white-space: nowrap; font: var(--font-size-compact) ui-monospace, SFMono-Regular, Menlo, monospace; }
  .fetch-url-modal > footer { display: flex; justify-content: flex-end; gap: 6px; padding: 10px 14px; }
  .fetch-url-modal .done { color: var(--text); border-color: var(--accent); background: var(--accent); font-weight: 700; }
  @media (max-width: 1050px) {
    .field-grid { grid-template-columns: 1fr; }
    .substitution-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); }
    .rule-table-header, .processing-rule { grid-template-columns: 22px minmax(60px, .8fr) minmax(70px, 1fr) 50px 74px; gap: 2px; }
  }
</style>
