<script lang="ts">
import { onMount } from "svelte";
import { showErrorToast } from "$lib/errorToast";
import type { IntruderRange } from "$lib/types";
import { createHttpHighlightPlugin, type HttpMessageKind } from "$lib/http-highlighting";
import { autocompletion, closeBrackets, closeBracketsKeymap, completionKeymap } from "@codemirror/autocomplete";
import { defaultKeymap, history, historyKeymap, redo, undo } from "@codemirror/commands";
import { lintKeymap } from "@codemirror/lint";
import { SearchQuery, searchKeymap } from "@codemirror/search";
import Toggle from "./Toggle.svelte";
import { Compartment, EditorState, RangeSetBuilder, StateEffect } from "@codemirror/state";
import {
  crosshairCursor,
  Decoration,
  dropCursor,
  type DecorationSet,
  EditorView,
  keymap,
  lineNumbers,
  rectangularSelection,
  ViewPlugin,
} from "@codemirror/view";

  const sharedEditorSetup = [
    lineNumbers(),
    dropCursor(),
    EditorState.allowMultipleSelections.of(true),
    rectangularSelection(),
    crosshairCursor(),
  ];

  const editableEditorSetup = [
    ...sharedEditorSetup,
    history(),
    closeBrackets(),
    autocompletion(),
    keymap.of([
      ...closeBracketsKeymap,
      ...defaultKeymap,
      ...searchKeymap,
      ...historyKeymap,
      ...completionKeymap,
      ...lintKeymap,
    ]),
  ];

  const readonlyEditorSetup = [
    ...sharedEditorSetup,
    keymap.of([
      ...searchKeymap,
    ]),
  ];

type EditorCommand = "undo" | "redo" | "cut" | "paste" | "selectAll";
type EditorCommandRequest = { id: number; action: EditorCommand };
type SearchDirection = "next" | "previous";
type SearchConfig = {
  query: string;
  regexp: boolean;
  caseSensitive: boolean;
  activeMatch: number;
};
type SearchMatch = { from: number; to: number };
type SearchUpdate = { config: SearchConfig; matches: SearchMatch[] };

const MAX_SEARCH_MATCHES = 5_000;

function emptySearchConfig(): SearchConfig {
  return { query: "", regexp: false, caseSensitive: false, activeMatch: -1 };
}

function createSearchQuery(config: SearchConfig) {
  return new SearchQuery({
    search: config.query,
    regexp: config.regexp,
    literal: !config.regexp,
    caseSensitive: config.caseSensitive,
  });
}

function findSearchMatches(state: EditorState, config: SearchConfig): SearchMatch[] {
  if (!config.query) return [];
  const query = createSearchQuery(config);
  if (!query.valid) return [];
  const matches: SearchMatch[] = [];
  const cursor = query.getCursor(state);
  let result = cursor.next();
  while (!result.done) {
    const match = result.value;
    if (match.from !== match.to) matches.push(match);
    if (matches.length >= MAX_SEARCH_MATCHES) break;
    result = cursor.next();
  }
  return matches;
}

function buildSearchDecorations(matches: readonly SearchMatch[], config: SearchConfig): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();
  for (const [index, match] of matches.entries()) {
    builder.add(match.from, match.to, index === config.activeMatch ? selectedSearchMatch : searchMatch);
  }
  return builder.finish();
}

const setSearchState = StateEffect.define<SearchUpdate>();
const setActiveSearchMatch = StateEffect.define<number>();
const searchMatch = Decoration.mark({ class: "cm-search-match" });
const selectedSearchMatch = Decoration.mark({ class: "cm-search-match cm-search-match-selected" });

const searchDecorations = ViewPlugin.fromClass(class {
  decorations: DecorationSet;
  config: SearchConfig = emptySearchConfig();
  matches: SearchMatch[] = [];

  constructor(_view: EditorView) {
    this.decorations = Decoration.none;
  }

  update(update: import("@codemirror/view").ViewUpdate) {
    let searchStateChanged = false;
    let activeMatchChanged = false;
    for (const transaction of update.transactions) {
      for (const effect of transaction.effects) {
        if (effect.is(setSearchState)) {
          this.config = effect.value.config;
          this.matches = effect.value.matches;
          searchStateChanged = true;
        } else if (effect.is(setActiveSearchMatch)) {
          this.config = { ...this.config, activeMatch: effect.value };
          activeMatchChanged = true;
        }
      }
    }
    if (searchStateChanged || activeMatchChanged) {
      this.decorations = buildSearchDecorations(this.matches, this.config);
    } else if (update.docChanged) {
      this.matches = [];
      this.decorations = Decoration.none;
    }
  }
}, {
  decorations: (value) => value.decorations,
});

const setHighlightRanges = StateEffect.define<IntruderRange[]>();
const modifiedValueHighlight = Decoration.mark({ class: "cm-modified-value" });

function buildHighlightDecorations(ranges: readonly IntruderRange[], documentLength: number): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();
  let previousTo = 0;
  for (const range of [...ranges].sort((left, right) => left.from - right.from || left.to - right.to)) {
    const from = Math.max(previousTo, Math.min(documentLength, Math.floor(range.from)));
    const to = Math.min(documentLength, Math.floor(range.to));
    if (from >= to) continue;
    builder.add(from, to, modifiedValueHighlight);
    previousTo = to;
  }
  return builder.finish();
}

const highlightDecorations = ViewPlugin.fromClass(class {
  decorations: DecorationSet = Decoration.none;
  ranges: IntruderRange[] = [];

  update(update: import("@codemirror/view").ViewUpdate) {
    let rangesChanged = false;
    for (const transaction of update.transactions) {
      for (const effect of transaction.effects) {
        if (effect.is(setHighlightRanges)) {
          this.ranges = effect.value;
          rangesChanged = true;
        }
      }
    }
    if (rangesChanged) {
      this.decorations = buildHighlightDecorations(this.ranges, update.state.doc.length);
    } else if (update.docChanged) {
      this.ranges = [];
      this.decorations = Decoration.none;
    }
  }
}, {
  decorations: (value) => value.decorations,
});

  let {
    value,
    readonly = false,
    wrap = false,
    label = "Raw HTTP editor",
    messageKind = "response",
    onChange = (_value: string) => {},
    onSelectionChange = (_selection: { from: number; to: number }, _value: string) => {},
    onContextMenu,
    command = null,
    onCommandHandled,
    normalizePastedDocument,
    search = "",
    searchRegex = false,
    searchCaseSensitive = false,
    highlightOnMatch = false,
    highlightRanges = [],
  }: {
    value: string;
    readonly?: boolean;
    wrap?: boolean;
    label?: string;
    messageKind?: HttpMessageKind;
    onChange?: (value: string) => void;
    onSelectionChange?: (selection: { from: number; to: number }, value: string) => void;
    onContextMenu?: (event: MouseEvent, selection: { from: number; to: number }, value: string) => void;
    command?: EditorCommandRequest | null;
    onCommandHandled?: (id: number) => void;
    normalizePastedDocument?: (value: string) => string;
    search?: string;
    searchRegex?: boolean;
    searchCaseSensitive?: boolean;
    highlightOnMatch?: boolean;
    highlightRanges?: IntruderRange[];
  } = $props();

  let container: HTMLDivElement;
  let searchRoot: HTMLDivElement;
  let view: EditorView | undefined;
  let currentValue = "";
  let synchronizing = false;
  let handledCommandId = 0;
  let searchQuery = $state("");
  let regexp = $state(false);
  let caseSensitive = $state(false);
  let highlightOnMatchEnabled = $state(false);
  let searchOptionsOpen = $state(false);
  let activeMatch = $state(-1);
  let searchMatches: SearchMatch[] = [];
  let lastSearchStateKey = "";
  let lastScannedSearchKey = "";
  let lastSearchProp: string | undefined;
  let lastSearchOptionsProp: string | undefined;
  const lineSeparator = new Compartment();

  let editorReady = $state(false);
  onMount(() => {
    const closeSearchMenu = (event: PointerEvent) => {
      if (!searchRoot?.contains(event.target as Node)) searchOptionsOpen = false;
    };
    window.addEventListener("pointerdown", closeSearchMenu);
    currentValue = value;
    searchQuery = search;
    regexp = searchRegex;
    caseSensitive = searchCaseSensitive;
    highlightOnMatchEnabled = highlightOnMatch;
    lastSearchProp = search;
    lastSearchOptionsProp = `${searchRegex}:${searchCaseSensitive}:${highlightOnMatch}`;
    // Defer CodeMirror construction past tab-switch paint so switching to
    // History/Proxy/Fuzz feels instant. The empty container paints first;
    // the editor hydrates on the next frame.
    let destroyed = false;
    const rafId = requestAnimationFrame(() => {
      if (destroyed || !container) return;
      // Props may have changed between mount paint and this frame.
      currentValue = value;
    view = new EditorView({
      doc: value,
      parent: container,
      extensions: [
        readonly ? readonlyEditorSetup : editableEditorSetup,
        createHttpHighlightPlugin(messageKind),
        searchDecorations,
        highlightDecorations,
        lineSeparator.of(EditorState.lineSeparator.of(detectLineEnding(value))),
        EditorView.editable.of(!readonly),
        EditorView.domEventHandlers({
          contextmenu: (event, editor) => {
            event.preventDefault();
            const selection = editor.state.selection.main;
            onContextMenu?.(event, { from: selection.from, to: selection.to }, currentValue);
            return true;
          },
          paste: (event, editor) => {
            if (readonly || !normalizePastedDocument) return false;
            const pasted = event.clipboardData?.getData("text/plain");
            if (pasted === undefined) return false;
            const selection = editor.state.selection.main;
            const next = `${editor.state.sliceDoc(0, selection.from)}${pasted}${editor.state.sliceDoc(selection.to)}`;
            const normalized = normalizePastedDocument(next);
            if (normalized === next) return false;
            event.preventDefault();
            const inserted = pasted.replace(/\r\n?|\n/g, "\n");
            editor.dispatch({
              changes: { from: 0, to: editor.state.doc.length, insert: normalized },
              selection: { anchor: Math.min(selection.from + inserted.length, normalized.length) },
              userEvent: "input.paste",
            });
            return true;
          },
        }),
        EditorView.updateListener.of((update) => {
          if (update.docChanged) currentValue = update.state.sliceDoc();
          if (update.docChanged && !synchronizing) {
            searchMatches = [];
            activeMatch = -1;
          }
          if (update.docChanged && !synchronizing) onChange(currentValue);
          if ((update.selectionSet || update.docChanged) && !synchronizing) {
            const selection = update.state.selection.main;
            onSelectionChange(
              {
                from: selection.from,
                to: selection.to,
              },
              currentValue,
            );
          }
        }),
        EditorView.theme({
          "&": { height: "100%", backgroundColor: "var(--editor, #202020)", color: "var(--text, #f4f6fb)", caretColor: "var(--text, #f4f6fb)", fontSize: "var(--font-size-editor, 12px)" },
          ".cm-content": { caretColor: "var(--text, #f4f6fb)" },
          ".cm-cursor, .cm-dropCursor": { borderLeft: "1.2px solid var(--text, #f4f6fb)" },
          ".cm-scroller": { overflow: "auto", fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace" },
          ".cm-gutters": { backgroundColor: "var(--editor, #202020)", color: "var(--muted, #97a1b4)", borderRight: "1px solid var(--border, #292f3e)" },
          ".cm-lineNumbers": { width: "44px" },
          ".cm-lineNumbers .cm-gutterElement": { boxSizing: "border-box", width: "44px", padding: "0 8px 0 4px" },
          ".cm-http-method": { color: "var(--syntax-method, #7dd3fc)", fontWeight: "700" },
          ".cm-http-target": { color: "var(--syntax-target, #c4b5fd)" },
          ".cm-http-protocol": { color: "var(--syntax-protocol, #94a3b8)" },
          ".cm-http-status-success": { color: "var(--syntax-status-success, #86efac)", fontWeight: "700" },
          ".cm-http-status-redirect": { color: "var(--syntax-status-redirect, #fcd34d)", fontWeight: "700" },
          ".cm-http-status-client-error": { color: "var(--syntax-status-client-error, #fb923c)", fontWeight: "700" },
          ".cm-http-status-server-error": { color: "var(--syntax-status-server-error, #fda4af)", fontWeight: "700" },
          ".cm-http-status-reason": { color: "var(--syntax-status-reason, #cbd5e1)" },
          ".cm-http-header-name": { color: "var(--syntax-header-name, #f9a8d4)", fontWeight: "600" },
          ".cm-http-header-important": { color: "var(--syntax-header-name, #f9a8d4)" },
          ".cm-http-header-delimiter": { color: "var(--syntax-header-delimiter, #94a3b8)" },
          ".cm-http-header-value": { color: "var(--text, #f4f6fb)" },
          ".cm-http-request-header-value": { color: "var(--text, #f4f6fb)" },
          ".cm-http-json-key": { color: "var(--syntax-json-key, #93c5fd)" },
          ".cm-http-json-string, .cm-http-json-number, .cm-http-json-boolean, .cm-http-json-null": { color: "var(--syntax-json-value, #a3f705)" },
          ".cm-http-json-punctuation": { color: "var(--syntax-json-punctuation, #cbd5e1)" },
          ".cm-http-multipart-boundary": { color: "var(--syntax-target, #c4b5fd)", fontWeight: "700" },
          ".cm-http-multipart-value": { color: "var(--syntax-json-value, #a3f705)" },
          ".cm-content ::selection": {
            color: "var(--selection-text, #fff) !important",
            backgroundColor: "var(--selection-bg, #5f6878) !important",
          },
          ".cm-search-match": {
            color: "#241b05",
            backgroundColor: "#fbbf24",
            borderRadius: "2px",
          },
          ".cm-search-match-selected": {
            backgroundColor: "#facc15",
            boxShadow: "0 0 0 1px #f59e0b",
          },
          ".cm-modified-value": {
            color: "var(--base, #241b05)",
            backgroundColor: "var(--warning, #fbbf24)",
            borderRadius: "2px",
          },
          "&.cm-focused": { outline: "1px solid var(--accent, #9ca3af)" },
        }),
      ],
    });
    view.contentDOM.classList.toggle("cm-lineWrapping", wrap);
    editorReady = true;
    rescanSearch();
    dispatchHighlightRanges();
    if (highlightOnMatchEnabled && searchQuery) scheduleAutoHighlight();
    }); // end rAF deferred EditorView init
    return () => {
      destroyed = true;
      cancelAnimationFrame(rafId);
      window.removeEventListener("pointerdown", closeSearchMenu);
      view?.destroy();
      view = undefined;
      editorReady = false;
    };
  });

  $effect(() => {
    if (lastSearchProp === undefined || search !== lastSearchProp) {
      lastSearchProp = search;
      if (searchQuery !== search) {
        searchQuery = search;
        activeMatch = -1;
      }
    }
  });

  $effect(() => {
    const nextOptions = `${searchRegex}:${searchCaseSensitive}:${highlightOnMatch}`;
    if (lastSearchOptionsProp !== undefined && nextOptions === lastSearchOptionsProp) return;
    lastSearchOptionsProp = nextOptions;
    const nextHighlightOnMatch = highlightOnMatch;
    regexp = searchRegex;
    caseSensitive = searchCaseSensitive;
    highlightOnMatchEnabled = nextHighlightOnMatch;
    activeMatch = -1;
    if (nextHighlightOnMatch && searchQuery) scheduleAutoHighlight();
  });

  $effect(() => {
    const rangeKey = `${value.length}:${highlightRanges.map((range) => `${range.from}:${range.to}`).join(",")}`;
    if (!view) return;
    void rangeKey;
    dispatchHighlightRanges();
  });

  $effect(() => {
    const config = searchConfig();
    const queryKey = searchQueryKey(config);
    if (!view) return;
    if (queryKey !== lastScannedSearchKey) {
      searchMatches = findSearchMatches(view.state, config);
      lastScannedSearchKey = queryKey;
      dispatchSearchState(config);
    } else {
      applySearchState();
    }
  });

  $effect(() => {
    if (view) {
      view.contentDOM.classList.toggle("cm-lineWrapping", wrap);
      view.requestMeasure();
    }
  });

  $effect(() => {
    if (!view || !command || command.id === handledCommandId) return;
    handledCommandId = command.id;
    void runCommand(command.action)
      .catch(() => {})
      .finally(() => {
        view?.focus();
        onCommandHandled?.(command.id);
      });
  });

  async function runCommand(commandName: EditorCommand) {
    if (!view) return;
    if (commandName === "undo") {
      undo({ state: view.state, dispatch: view.dispatch });
      return;
    }
    if (commandName === "redo") {
      redo({ state: view.state, dispatch: view.dispatch });
      return;
    }
    if (commandName === "selectAll") {
      view.dispatch({
        selection: { anchor: 0, head: view.state.doc.length },
        userEvent: "select",
      });
      return;
    }
    if (commandName === "cut") {
      await cutSelection();
      return;
    }
    await pasteFromClipboard();
  }

  async function cutSelection() {
    if (!view) return;
    const selection = view.state.selection.main;
    if (selection.empty) return;
    try {
      await navigator.clipboard.writeText(view.state.sliceDoc(selection.from, selection.to));
    } catch (reason) {
      showErrorToast(reason);
      return;
    }
    view.dispatch({
      changes: { from: selection.from, to: selection.to, insert: "" },
      selection: { anchor: selection.from },
      userEvent: "delete.cut",
    });
  }

  async function pasteFromClipboard() {
    if (!view) return;
    let pasted: string;
    try {
      pasted = await navigator.clipboard.readText();
    } catch (reason) {
      showErrorToast(reason);
      return;
    }
    const selection = view.state.selection.main;
    const lineSeparatorValue = view.state.facet(EditorState.lineSeparator) ?? "\n";
    const inserted = pasted.replace(/\r\n?|\n/g, lineSeparatorValue);
    const current = view.state.sliceDoc();
    const next = `${current.slice(0, selection.from)}${inserted}${current.slice(selection.to)}`;
    const normalized = normalizePastedDocument?.(next) ?? next;
    if (normalized === next) {
      view.dispatch({
        changes: { from: selection.from, to: selection.to, insert: inserted },
        selection: { anchor: Math.min(selection.from + inserted.length, normalized.length) },
        userEvent: "input.paste",
      });
    } else {
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: normalized },
        selection: { anchor: Math.min(selection.from + inserted.length, normalized.length) },
        userEvent: "input.paste",
      });
    }
  }

  $effect(() => {
    if (view && value !== currentValue) {
      const desiredLineEnding = detectLineEnding(value);
      if (desiredLineEnding !== view.state.facet(EditorState.lineSeparator)) {
        view.dispatch({
          effects: lineSeparator.reconfigure(EditorState.lineSeparator.of(desiredLineEnding)),
        });
      }
      const current = currentValue;
      const externallyChanged = value !== current;
      let prefix = 0;
      while (prefix < current.length && prefix < value.length && current[prefix] === value[prefix]) {
        prefix += 1;
      }
      let suffix = 0;
      while (suffix < current.length - prefix
        && suffix < value.length - prefix
        && current[current.length - suffix - 1] === value[value.length - suffix - 1]) {
        suffix += 1;
      }
      synchronizing = true;
      try {
        view.dispatch({
          changes: {
            from: externalToDocumentOffset(current, prefix, desiredLineEnding),
            to: externalToDocumentOffset(current, current.length - suffix, desiredLineEnding),
            insert: value.slice(prefix, value.length - suffix),
          },
        });
      } finally {
        synchronizing = false;
      }
      if (externallyChanged) {
        rescanSearch();
        dispatchHighlightRanges();
        if (highlightOnMatchEnabled && searchQuery) scheduleAutoHighlight();
      }
    }
  });

  function searchConfig(): SearchConfig {
    return {
      query: searchQuery,
      regexp,
      caseSensitive,
      activeMatch,
    };
  }

  function searchQueryKey(config: SearchConfig) {
    return `${config.query}\u0000${config.regexp ? 1 : 0}\u0000${config.caseSensitive ? 1 : 0}`;
  }

  function searchStateKey(config: SearchConfig) {
    return `${searchQueryKey(config)}\u0000${config.activeMatch}`;
  }

  function dispatchSearchState(config: SearchConfig) {
    if (!view) return;
    const key = searchStateKey(config);
    lastSearchStateKey = key;
    view.dispatch({ effects: setSearchState.of({ config, matches: searchMatches }) });
  }

  function dispatchHighlightRanges(ranges = highlightRanges) {
    if (!view) return;
    view.dispatch({
      effects: setHighlightRanges.of(ranges.map((range) => ({ from: range.from, to: range.to }))),
    });
  }

  function applySearchState() {
    if (!view) return;
    const config = searchConfig();
    const key = searchStateKey(config);
    if (key === lastSearchStateKey) return;
    lastSearchStateKey = key;
    view.dispatch({ effects: setActiveSearchMatch.of(config.activeMatch) });
  }

  function rescanSearch() {
    if (!view) return;
    activeMatch = -1;
    const config = searchConfig();
    searchMatches = findSearchMatches(view.state, config);
    lastScannedSearchKey = searchQueryKey(config);
    dispatchSearchState(config);
  }

  function scheduleAutoHighlight() {
    requestAnimationFrame(() => {
      if (view && highlightOnMatchEnabled && searchQuery) navigateSearch("next", true);
    });
  }

  function navigateSearch(direction: SearchDirection, automatic = false) {
    if (!view) return;
    const matches = searchMatches;
    if (!matches.length) {
      activeMatch = -1;
      applySearchState();
      return;
    }

    let nextIndex = activeMatch;
    if (automatic) {
      nextIndex = direction === "next" ? 0 : matches.length - 1;
    } else if (nextIndex < 0 || nextIndex >= matches.length) {
      const selection = view.state.selection.main;
      if (direction === "next") {
        nextIndex = matches.findIndex((match) => match.from >= selection.to);
        if (nextIndex < 0) nextIndex = 0;
      } else {
        nextIndex = -1;
        for (let index = matches.length - 1; index >= 0; index -= 1) {
          if (matches[index].to <= selection.from) {
            nextIndex = index;
            break;
          }
        }
        if (nextIndex < 0) nextIndex = matches.length - 1;
      }
    } else {
      nextIndex = (nextIndex + (direction === "next" ? 1 : -1) + matches.length) % matches.length;
    }

    const match = matches[nextIndex];
    activeMatch = nextIndex;
    applySearchState();
    view.dispatch({
      effects: EditorView.scrollIntoView(match.from, { y: "center" }),
    });
  }

  function refreshSearch() {
    rescanSearch();
    if (highlightOnMatchEnabled && searchQuery) scheduleAutoHighlight();
  }

  function handleSearchKeydown(event: KeyboardEvent) {
    if (event.key !== "Enter") return;
    event.preventDefault();
    navigateSearch(event.shiftKey ? "previous" : "next");
  }

  function detectLineEnding(input: string) {
    const match = /\r\n|\n|\r/.exec(input);
    return match?.[0] ?? "\n";
  }

  function externalToDocumentOffset(input: string, offset: number, separator: string) {
    if (separator.length === 1) return offset;
    let extraCharacters = 0;
    let cursor = 0;
    while (cursor < offset) {
      const index = input.indexOf(separator, cursor);
      if (index < 0 || index >= offset) break;
      extraCharacters += separator.length - 1;
      cursor = index + separator.length;
    }
    return offset - extraCharacters;
  }
</script>

<div class="editor-shell" bind:this={searchRoot}>
  <div class="editor" bind:this={container} role="textbox" aria-label={label} aria-busy={!editorReady}></div>
  <div class="search-bar" aria-label={`Search ${label}`}>
    <input
      aria-label={`Search ${label}`}
      placeholder="Search…"
      value={searchQuery}
      spellcheck={false}
      oninput={(event) => { searchQuery = event.currentTarget.value; activeMatch = -1; }}
      onkeydown={handleSearchKeydown}
    />
    <button class="search-button" type="button" aria-label="Previous match" data-tooltip-placement="above" data-tooltip="Previous match" onclick={() => navigateSearch("previous")}>
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m18 15-6-6-6 6" /></svg>
    </button>
    <button class="search-button" type="button" aria-label="Next match" data-tooltip-placement="above" data-tooltip="Next match" onclick={() => navigateSearch("next")}>
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m6 9 6 6 6-6" /></svg>
    </button>
    <button class="search-button refresh-search-button" type="button" aria-label="Refresh search" data-tooltip-placement="above" data-tooltip="Refresh search" onclick={refreshSearch}>
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M20 11a8 8 0 1 0 1 4" /><path d="M20 5v6h-6" /></svg>
    </button>
    <div class="search-options">
      <button
        class:active={searchOptionsOpen}
        class="search-button"
        type="button"
        aria-label="Search options"
        aria-expanded={searchOptionsOpen}
        data-tooltip-placement="above" data-tooltip="Search options"
        onclick={() => (searchOptionsOpen = !searchOptionsOpen)}
      >
        <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="5" r="1" /><circle cx="12" cy="12" r="1" /><circle cx="12" cy="19" r="1" /></svg>
      </button>
      {#if searchOptionsOpen}
        <div class="search-options-menu" role="menu" aria-label="Search options">
          <label>
            <Toggle bind:checked={regexp} onchange={() => (activeMatch = -1)} ariaLabel="Regex" />
            <span>Regex</span>
          </label>
          <label>
            <Toggle bind:checked={caseSensitive} onchange={() => (activeMatch = -1)} ariaLabel="Case sensitive" />
            <span>Case sensitive</span>
          </label>
          <label>
            <Toggle bind:checked={highlightOnMatchEnabled} onchange={() => { activeMatch = -1; if (highlightOnMatchEnabled && searchQuery) scheduleAutoHighlight(); }} ariaLabel="Highlight on match" />
            <span>Highlight on match</span>
          </label>
        </div>
      {/if}
    </div>
  </div>
</div>

<style>
  .editor-shell { position: relative; display: grid; grid-template-rows: minmax(0, 1fr) 29px; min-height: 180px; height: 100%; overflow: visible; }
  .editor { min-height: 0; height: 100%; overflow: hidden; }
  .search-bar { position: relative; z-index: 2; display: flex; align-items: center; gap: 2px; min-width: 0; padding: 2px 4px; border-top: 1px solid var(--border, #292f3e); background: var(--editor, #202020); }
  .search-bar input { min-width: 0; flex: 1; height: 23px; padding: 2px 6px; border: 1px solid var(--border-strong, #3a4353); border-radius: 3px; color: var(--text, #f4f6fb); background: var(--editor, #202020); font: var(--font-size-editor, 12px) ui-monospace, SFMono-Regular, Menlo, monospace; }
  .search-bar input:focus { outline: 1px solid var(--accent, #9ca3af); outline-offset: -1px; }
  .search-button { position: relative; display: grid; place-items: center; width: 25px; height: 23px; padding: 0; border: 1px solid transparent; border-radius: 3px; color: var(--muted, #97a1b4); background: transparent; cursor: pointer; }
  .search-button:hover, .search-button:focus-visible, .search-button.active { color: var(--text, #f4f6fb); border-color: var(--border-strong, #3a4353); background: var(--surface-2, #1a1f2a); }
  .search-button svg { width: 14px; height: 14px; fill: none; stroke: currentColor; stroke-width: var(--svgbuttonstrokewidth, 1.5); stroke-linecap: round; stroke-linejoin: round; }
  .refresh-search-button svg { width: 12px; height: 12px; }
  .search-button svg circle { fill: currentColor; stroke: none; }
  .search-options { position: relative; flex: 0 0 auto; }
  .search-options-menu { position: absolute; right: 0; bottom: calc(100% + 4px); display: grid; min-width: 155px; padding: 4px; border: 1px solid var(--border-strong, #3a4353); border-radius: 4px; color: var(--text, #f4f6fb); background: var(--surface, #11141d); box-shadow: 0 5px 16px rgb(0 0 0 / 35%); }
  .search-options-menu label { display: flex; align-items: center; gap: 7px; min-height: 27px; padding: 3px 6px; border-radius: 3px; color: var(--muted, #97a1b4); cursor: pointer; font-size: var(--font-size-compact, 10px); white-space: nowrap; }
  .search-options-menu label:hover { color: var(--text, #f4f6fb); background: var(--surface-2, #1a1f2a); }
</style>
