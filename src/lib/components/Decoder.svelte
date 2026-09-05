<script lang="ts">
  import { onDestroy } from "svelte";
import { commands } from "$lib/api";
import { showErrorToast } from "$lib/errorToast";
import type { DecoderWorkspaceState } from "$lib/types";
import Toggle from "./Toggle.svelte";

  type Operation = {
    id: string;
    label: string;
    category: "Encoding" | "Web formats" | "Inspect";
    description: string;
  };
  type RecipeStep = { id: number; operation: string };

  let {
    input = $bindable(""),
    workspace,
    onWorkspaceChange = (_state: DecoderWorkspaceState) => {},
    onStatus = (_message: string) => {},
  }: {
    input?: string;
    workspace?: DecoderWorkspaceState;
    onWorkspaceChange?: (state: DecoderWorkspaceState) => void;
    onStatus?: (message: string) => void;
  } = $props();

  const operations: Operation[] = [
    { id: "urlDecode", label: "URL decode", category: "Encoding", description: "Percent-encoded request values" },
    { id: "urlEncode", label: "URL encode", category: "Encoding", description: "Percent-encode a value" },
    { id: "formDecode", label: "Form decode", category: "Encoding", description: "Decode application/x-www-form-urlencoded" },
    { id: "formEncode", label: "Form encode", category: "Encoding", description: "Encode spaces as + for form bodies" },
    { id: "base64Decode", label: "From Base64", category: "Encoding", description: "Standard or URL-safe input" },
    { id: "base64Encode", label: "To Base64", category: "Encoding", description: "Standard Base64" },
    { id: "base64UrlDecode", label: "From Base64url", category: "Encoding", description: "JWT and URL-safe tokens" },
    { id: "base64UrlEncode", label: "To Base64url", category: "Encoding", description: "Unpadded URL-safe Base64" },
    { id: "hexDecode", label: "From Hex", category: "Encoding", description: "UTF-8 text represented as bytes" },
    { id: "hexEncode", label: "To Hex", category: "Encoding", description: "Encode UTF-8 text as bytes" },
    { id: "htmlDecode", label: "HTML decode", category: "Encoding", description: "Named and numeric HTML entities" },
    { id: "htmlEncode", label: "HTML encode", category: "Encoding", description: "Escape HTML-sensitive characters" },
    { id: "unicodeDecode", label: "Unicode unescape", category: "Encoding", description: "Decode \\uXXXX escapes" },
    { id: "unicodeEncode", label: "Unicode escape", category: "Encoding", description: "Encode as \\uXXXX escapes" },
    { id: "jsonPretty", label: "Format JSON", category: "Web formats", description: "Validate and pretty-print JSON" },
    { id: "jsonMinify", label: "Minify JSON", category: "Web formats", description: "Validate and compact JSON" },
    { id: "queryToJson", label: "Query to JSON", category: "Web formats", description: "Inspect form or query parameters" },
    { id: "jsonToQuery", label: "JSON to query", category: "Web formats", description: "Encode an object as form/query pairs" },
    { id: "jwtDecode", label: "JWT inspect", category: "Inspect", description: "View unverified header and claims" },
    { id: "smartDecode", label: "Smart decode", category: "Inspect", description: "Conservative URL, HTML, Unicode, Hex, and Base64 layers" },
    { id: "sha1", label: "SHA-1", category: "Inspect", description: "Hash text locally" },
    { id: "sha256", label: "SHA-256", category: "Inspect", description: "Hash text locally" },
    { id: "sha384", label: "SHA-384", category: "Inspect", description: "Hash text locally" },
    { id: "sha512", label: "SHA-512", category: "Inspect", description: "Hash text locally" },
  ];
  const categories: Operation["category"][] = ["Encoding", "Web formats", "Inspect"];
  const operationById = new Map(operations.map((operation) => [operation.id, operation]));
  const inverseOperations: Record<string, string> = {
    urlDecode: "urlEncode", urlEncode: "urlDecode",
    formDecode: "formEncode", formEncode: "formDecode",
    base64Decode: "base64Encode", base64Encode: "base64Decode",
    base64UrlDecode: "base64UrlEncode", base64UrlEncode: "base64UrlDecode",
    hexDecode: "hexEncode", hexEncode: "hexDecode",
    htmlDecode: "htmlEncode", htmlEncode: "htmlDecode",
    unicodeDecode: "unicodeEncode", unicodeEncode: "unicodeDecode",
    queryToJson: "jsonToQuery", jsonToQuery: "queryToJson",
    jsonPretty: "jsonMinify", jsonMinify: "jsonPretty",
  };

  let recipe = $state<RecipeStep[]>([]);
  let stageOutputs = $state<string[]>([]);
  let detected = $state("Plain text");
  let running = $state(false);
  let padding = $state(true);
  let filter = $state("");
  let notice = $state("");
  let nextStepId = 1;
  let runTimer: ReturnType<typeof setTimeout> | undefined;
  let runVersion = 0;
  let workspaceInitialized = $state(false);

  const matchingOperations = $derived(
    operations.filter((operation) => {
      const searchable = `${operation.label} ${operation.description} ${operation.category}`.toLowerCase();
      return searchable.includes(filter.trim().toLowerCase());
    }),
  );
  const finalOutput = $derived(recipe.length ? stageOutputs[recipe.length - 1] ?? "" : input);
  $effect(() => {
    if (workspaceInitialized) return;
    if (workspace) {
      input = workspace.input;
      recipe = workspace.recipe.map((step) => ({ ...step }));
      stageOutputs = [...workspace.stageOutputs];
      detected = workspace.detected;
      padding = workspace.padding;
      filter = workspace.filter;
      notice = workspace.notice;
      nextStepId = Math.max(1, workspace.nextStepId);
    }
    workspaceInitialized = true;
    scheduleRun();
  });

  $effect(() => {
    if (!workspaceInitialized) return;
    onWorkspaceChange({
      input,
      recipe: recipe.map((step) => ({ ...step })),
      stageOutputs: [...stageOutputs],
      detected,
      padding,
      filter,
      notice,
      nextStepId,
    });
  });

  $effect(() => {
    if (!workspaceInitialized) return;
    input;
    recipe.length;
    padding;
    scheduleRun();
  });

  function addOperation(operation: string) {
    recipe = [...recipe, { id: nextStepId++, operation }];
    notice = "";
    scheduleRun();
  }

  function removeStep(index: number) {
    recipe = recipe.filter((_, current) => current !== index);
    stageOutputs = [];
    notice = "";
    scheduleRun();
  }

  function moveStep(index: number, direction: -1 | 1) {
    const target = index + direction;
    if (target < 0 || target >= recipe.length) return;
    const next = [...recipe];
    [next[index], next[target]] = [next[target], next[index]];
    recipe = next;
    stageOutputs = [];
    scheduleRun();
  }

  function clearRecipe() {
    recipe = [];
    stageOutputs = [];
    notice = "";
    detected = "Plain text";
  }

  function scheduleRun() {
    clearTimeout(runTimer);
    if (!recipe.length) return;
    runTimer = setTimeout(() => void runRecipe(), 250);
  }

  async function hash(input: string, algorithm: "SHA-1" | "SHA-256" | "SHA-384" | "SHA-512") {
    const digest = await crypto.subtle.digest(algorithm, new TextEncoder().encode(input));
    return Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, "0")).join("");
  }

  async function transform(value: string, operation: string) {
    const hashOperation = operation.match(/^sha(1|256|384|512)$/);
    if (hashOperation) {
      const algorithm = `SHA-${hashOperation[1]}` as "SHA-1" | "SHA-256" | "SHA-384" | "SHA-512";
      return { output: await hash(value, algorithm), detected: `${algorithm} hash` };
    }
    const result = await commands.decoderTransform(value, operation, padding);
    return { output: result.output, detected: result.detected };
  }

  async function runRecipe() {
    const version = ++runVersion;
    clearTimeout(runTimer);
    if (!recipe.length) {
      stageOutputs = [];
      detected = "Plain text";
      return;
    }
    running = true;
    notice = "";
    let value = input;
    const outputs: string[] = [];
    try {
      for (let index = 0; index < recipe.length; index += 1) {
        const result = await transform(value, recipe[index].operation);
        if (version !== runVersion) return;
        value = result.output;
        outputs[index] = value;
        detected = result.detected;
      }
      if (version !== runVersion) return;
      stageOutputs = outputs;
      onStatus(`Decoder recipe applied · ${recipe.length} step${recipe.length === 1 ? "" : "s"}`);
    } catch (reason) {
      if (version === runVersion) {
        notice = "That transformation could not be applied to this value.";
        showErrorToast(reason);
      }
    } finally {
      if (version === runVersion) running = false;
    }
  }

  function updateInput(value: string) {
    input = value;
    scheduleRun();
  }

  function useFinalAsInput() {
    input = finalOutput;
    clearRecipe();
    onStatus("Decoder output moved to input");
  }

  function reverseRecipe() {
    const unsupported = recipe.find((step) => !inverseOperations[step.operation]);
    if (unsupported) {
      notice = `${operationById.get(unsupported.operation)?.label ?? "That step"} cannot be reversed automatically.`;
      return;
    }
    const source = finalOutput;
    recipe = recipe.slice().reverse().map((step) => ({ id: nextStepId++, operation: inverseOperations[step.operation] }));
    input = source;
    stageOutputs = [];
    notice = "Reversed the reversible steps. The recipe will run automatically.";
    scheduleRun();
  }

  async function copyFinalOutput() {
    try {
      await navigator.clipboard.writeText(finalOutput);
    } catch (reason) {
      showErrorToast(reason);
      return;
    }
    onStatus("Decoder output copied");
  }

  export function handleShortcut(action: string): boolean {
    if (action === "decoder.focusFilter") {
      document.querySelector<HTMLInputElement>('.decoder-tool input[placeholder="Search operations"]')?.focus();
      return true;
    }
    if (action === "decoder.run") {
      if (!recipe.length || running) return false;
      void runRecipe();
      return true;
    }
    if (action === "decoder.clear") {
      if (!recipe.length) return false;
      clearRecipe();
      return true;
    }
    if (action === "decoder.reverse") {
      if (!recipe.length) return false;
      reverseRecipe();
      return true;
    }
    if (action === "decoder.useOutput") {
      if (!finalOutput) return false;
      useFinalAsInput();
      return true;
    }
    if (action === "decoder.copyOutput") {
      if (!finalOutput) return false;
      void copyFinalOutput();
      return true;
    }
    return false;
  }

  onDestroy(() => clearTimeout(runTimer));
</script>

<section class="decoder-tool recipe-decoder" aria-label="Decoder">

  <div class="recipe-toolbar">
    <label class="padding-toggle">
      <span>Pad Base64</span>
      <Toggle bind:checked={padding} onchange={scheduleRun} ariaLabel="Base64 padding" />
    </label>
    <button class="text-button" disabled={!recipe.length} onclick={reverseRecipe}>Reverse recipe</button>
    <button class="text-button" disabled={!recipe.length} onclick={clearRecipe}>Clear</button>
  </div>

  <div class="workspace">
    <aside class="operation-palette" aria-label="Operations">
      <label class="search"><span>Operations</span><input bind:value={filter} placeholder="Search operations" /></label>
      <div class="operation-list">
        {#each categories as category}
          {@const items = matchingOperations.filter((operation) => operation.category === category)}
          {#if items.length}
            <section class="operation-group">
              <h2>{category}</h2>
              {#each items as operation (operation.id)}
                <button class="operation" data-tooltip={operation.description} onclick={() => addOperation(operation.id)}>
                  <strong>{operation.label}</strong><small>{operation.description}</small>
                </button>
              {/each}
            </section>
          {/if}
        {/each}
        {#if !matchingOperations.length}<p class="empty">No matching operations.</p>{/if}
      </div>
    </aside>

    <section class="recipe-panel" aria-label="Recipe">
      <div class="panel-heading"><div><span>Recipe</span><small>{recipe.length ? `${recipe.length} step${recipe.length === 1 ? "" : "s"}` : "Add an operation to start"}</small></div></div>
      <div class="source-step"><span>Input</span><small>Original value</small></div>
      <div class="recipe-steps">
        {#each recipe as step, index (step.id)}
          {@const operation = operationById.get(step.operation)}
          <div class="recipe-step">
            <div class="step-main"><span>{index + 1}</span><div><strong>{operation?.label ?? step.operation}</strong><small>{operation?.description}</small></div></div>
            <div class="step-actions">
              <button aria-label={`Move ${operation?.label ?? "step"} up`} disabled={index === 0} onclick={() => moveStep(index, -1)}>↑</button>
              <button aria-label={`Move ${operation?.label ?? "step"} down`} disabled={index === recipe.length - 1} onclick={() => moveStep(index, 1)}>↓</button>
              <button aria-label={`Remove ${operation?.label ?? "step"}`} onclick={() => removeStep(index)}>×</button>
            </div>
          </div>
        {/each}
      </div>
    </section>

    <section class="value-panel" aria-label="Input and output">
      <div class="editor-heading"><div><span>Input</span><small>Edit source text · recipe runs automatically</small></div><div class="header-status">
      <span class="detected-badge">Detected: <strong>{detected}</strong></span>
      {#if running}<span class="running">Running…</span>{/if}
    </div></div>
      <textarea class="input-editor" value={input} oninput={(event) => updateInput(event.currentTarget.value)} placeholder="Paste a request value, cookie, token, or JSON body…"></textarea>
      <div class="output-heading"><div><span>Final output</span><small>{recipe.length ? "Output after all recipe steps" : "Input is shown until you add a step"}</small></div><div><button class="text-button" disabled={!finalOutput} onclick={useFinalAsInput}>Use as input</button><button class="text-button" disabled={!finalOutput} onclick={() => void copyFinalOutput()}>Copy</button></div></div>
      <textarea class="final-output" readonly value={finalOutput}></textarea>
      {#if notice}<p class="notice" role="status">{notice}</p>{/if}
    </section>
  </div>
</section>

<style>
  .recipe-decoder { display: grid; grid-template-rows: auto minmax(0, 1fr); height: 100%; min-height: 0; padding: 4px; overflow: hidden; color: var(--text);}
  .header-status { display: flex; align-items: center; gap: 10px; color: var(--muted); font-size: var(--font-size-body); }
  .detected-badge { display: inline-flex; align-items: center; gap: 4px; height: 22px; min-height: 22px; padding: 0 5px; border: 1px solid var(--border-strong); border-radius: 3px; background: var(--surface-2); color: var(--muted); font-size: var(--font-size-compact); font-weight: 400; line-height: 1; white-space: nowrap; }
  .detected-badge strong { color: var(--text); font-weight: 600; }
  .running { color: var(--warning); }
  .recipe-toolbar { display: flex; align-items: center; gap: 7px; min-height: 37px; padding: 4px; border: 1px solid var(--border); border-radius: 3px; background: var(--surface); }
  .recipe-toolbar label { color: var(--muted); font-size: var(--font-size-body); white-space: nowrap; }
  .recipe-toolbar .padding-toggle { display: inline-flex; align-items: center; gap: 6px; height: 22px; margin-right: auto; cursor: pointer;  margin-left: 5px;}
  button, input, textarea { font: inherit; }
  button { min-height: 28px; padding: 0 8px; border: 1px solid var(--border-strong); border-radius: 3px; color: var(--text); background: var(--surface-2); cursor: pointer; }
  button:hover:not(:disabled) { border-color: color-mix(in srgb, var(--accent) 50%, var(--border-strong)); color: var(--text); background: var(--surface-3); }
  button:disabled { opacity: .45; cursor: not-allowed; }
  .workspace { display: grid; grid-template-columns: minmax(180px, .6fr) minmax(210px, .75fr) minmax(280px, 1.2fr); min-height: 0; gap: 4px; padding-top: 4px; }
  .operation-palette, .recipe-panel, .value-panel { min-width: 0; min-height: 0; border: 1px solid var(--border); border-radius: 3px; background: var(--surface); }
  .operation-palette { display: grid; grid-template-rows: auto minmax(0, 1fr); }
  .search { display: grid; gap: 5px; padding: 6px; border-bottom: 1px solid var(--border); color: var(--muted); font-size: var(--font-size-body); }
  .search input { width: 100%; height: 29px; padding: 0 7px; border: 1px solid var(--border-strong); border-radius: 3px; color: var(--text); background: var(--input); }
  .operation-list { padding: 4px; overflow: auto; }
  .operation-group + .operation-group { margin-top: 8px; }
  .operation-group h2 { margin: 0 4px 4px; color: var(--muted); font-size: var(--font-size-compact); letter-spacing: .09em; text-transform: uppercase; }
  button.operation { display: grid; width: 100%; min-height: 0; margin-bottom: 3px; padding: 6px 7px; border-color: transparent; background: transparent; text-align: left; }
  .operation strong { font-size: var(--font-size-body); font-weight: 650; }
  .operation small, .recipe-step small, .source-step small, .panel-heading small, .editor-heading small, .output-heading small { display: block; margin-top: 2px; color: var(--muted); font-size: var(--font-size-compact); line-height: 1.25; }
  .empty { margin: 12px; color: var(--muted); font-size: var(--font-size-body); line-height: 1.5; }
  .recipe-panel { display: grid; grid-template-rows: auto auto minmax(0, 1fr); overflow: hidden; }
  .panel-heading, .editor-heading, .output-heading { display: flex; align-items: center; justify-content: space-between; padding: 7px 8px; border-bottom: 1px solid var(--border); }
  .panel-heading > div > span, .editor-heading > div:first-child > span, .output-heading > div:first-child > span { display: block; color: var(--text); font-size: var(--font-size-body); font-weight: 700; }
  .source-step { display: grid; width: calc(100% - 8px); margin: 4px; padding: 5px 6px; border-color: transparent; background: var(--surface-2); text-align: left; }
  .source-step span { color: var(--text); font-size: var(--font-size-body); font-weight: 700; }
  .recipe-steps { min-height: 0; padding: 0 4px 4px; overflow: auto; }
  .recipe-step { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 4px; margin-top: 3px; padding: 3px; border: 1px solid transparent; border-radius: 3px; background: var(--surface-2); }
  .step-main { display: grid; grid-template-columns: 21px minmax(0, 1fr); min-height: 0; padding: 3px; border: 0; background: transparent; text-align: left; }
  .step-main > span { display: grid; place-items: center; width: 18px; height: 18px; margin-top: 2px; border-radius: 50%; color: var(--accent-contrast); background: var(--accent); font-size: var(--font-size-compact); font-weight: 800; }
  .step-main strong { display: block; font-size: var(--font-size-body); }
  .step-actions { display: flex; align-items: start; gap: 2px; }
  .step-actions button { width: 22px; min-height: 22px; padding: 0; border-color: transparent; background: transparent; }
  .value-panel { display: grid; grid-template-rows: auto minmax(110px, 1fr) auto minmax(110px, 1fr) auto; overflow: hidden; }
  .editor-heading, .output-heading { gap: 8px; }
  .output-heading button { min-height: 25px; padding: 0 7px; font-size: var(--font-size-compact); }
  .output-heading { border-top: 1px solid var(--border); }
  .output-heading > div:last-child { display: flex; gap: 4px; }
  textarea { width: 100%; min-width: 0; height: 100%; padding: 10px; border: 0; color: var(--text); background: var(--input); resize: none; font: var(--font-size-editor, 12px)/1.5 ui-monospace, SFMono-Regular, Menlo, monospace; }
  .input-editor { background: var(--input); }
  .final-output { color: var(--text); }
  .notice { margin: 0; padding: 6px 9px; border-top: 1px solid color-mix(in srgb, var(--warning) 35%, var(--border)); color: var(--warning); background: color-mix(in srgb, var(--warning) 10%, var(--surface)); font-size: var(--font-size-compact); }
  @media (max-width: 900px) { .workspace { grid-template-columns: minmax(160px, .55fr) minmax(210px, .8fr) minmax(250px, 1fr); } }
</style>
