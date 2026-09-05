<script lang="ts">
  import { showErrorToast } from "$lib/errorToast";
  import { moveRule, nextRuleId, removeRule, toggleRule as toggleRuleInList, validateRuleCondition } from "$lib/rule-list";
  import Toggle from "./Toggle.svelte";
  import type { MatchReplaceRule, MatchReplaceRuleType } from "$lib/types";

  let { rules, onChange }: {
    rules: MatchReplaceRule[];
    onChange: (rules: MatchReplaceRule[]) => void;
  } = $props();

  const typeOptions: { value: MatchReplaceRuleType; label: string; detail: string }[] = [
    { value: "requestHost", label: "Request host", detail: "Host in URI + Host header" },
    { value: "requestHeader", label: "Request header", detail: "Header name and value" },
    { value: "requestBody", label: "Request body", detail: "Full request body" },
    { value: "requestParamName", label: "Request param name", detail: "URL ?a= & body form names" },
    { value: "requestParamValue", label: "Request param value", detail: "URL ?a= & body form values" },
    { value: "responseHeader", label: "Response header", detail: "Header name and value" },
    { value: "responseBody", label: "Response body", detail: "Full response body" },
    { value: "responseParamName", label: "Response param name", detail: "Form body names" },
    { value: "responseParamValue", label: "Response param value", detail: "Form body values" },
  ];

  function labelForType(value: string): string {
    return typeOptions.find((o) => o.value === value)?.label ?? value;
  }

  function effectiveType(rule: MatchReplaceRule): MatchReplaceRuleType {
    if (rule.type) return rule.type as MatchReplaceRuleType;
    // legacy migration: location-only
    return rule.location === "response" ? "responseBody" : "requestBody";
  }

  let selectedId = $state<string | null>(null);
  let editor = $state<MatchReplaceRule | null>(null);
  let editingId = $state<string | null>(null);
  const selectedRule = $derived(rules.find((r) => r.id === selectedId) ?? null);

  $effect(() => {
    if (selectedId && rules.some((r) => r.id === selectedId)) return;
    selectedId = rules[0]?.id ?? null;
  });

  function newRule(): MatchReplaceRule {
    return {
      id: nextRuleId(),
      enabled: true,
      location: "request",
      type: "requestHeader",
      match: "",
      replace: "",
      isRegex: false,
    };
  }

  function openAdd() {
    editingId = null;
    editor = newRule();
  }

  function openEdit() {
    if (!selectedRule) return;
    editingId = selectedRule.id;
    editor = { ...selectedRule };
  }

  function saveEditor() {
    if (!editor) return;
    const nextType = (editor.type as string) || effectiveType(editor);
    const loc: "request" | "response" = nextType.startsWith("response") ? "response" : "request";
    const next: MatchReplaceRule = {
      ...editor,
      type: nextType as MatchReplaceRuleType,
      location: loc,
      match: editor.match,
      replace: editor.replace,
    };
    if (!next.match.trim()) {
      showErrorToast("Enter text to match.");
      return;
    }
    if (next.match.length > 2048) {
      showErrorToast("Match cannot exceed 2048 characters.");
      return;
    }
    if (next.replace.length > 4096) {
      showErrorToast("Replacement cannot exceed 4096 characters.");
      return;
    }
    {
      const error = validateRuleCondition(next.match, next.isRegex);
      if (error) {
        showErrorToast(error);
        return;
      }
    }
    const nextRules = editingId
      ? rules.map((r) => (r.id === editingId ? next : r))
      : [...rules, next];
    onChange(nextRules);
    selectedId = next.id;
    editor = null;
  }

  function removeSelected() {
    if (!selectedRule) return;
    const { next, nextSelectedId } = removeRule(rules, selectedRule.id);
    onChange(next);
    selectedId = nextSelectedId;
  }

  function moveSelected(direction: -1 | 1) {
    if (!selectedRule) return;
    onChange(moveRule(rules, selectedRule.id, direction));
  }

  function toggleRuleEntry(rule: MatchReplaceRule) {
    onChange(toggleRuleInList(rules, rule.id));
  }
</script>

<section class="rule-editor" aria-label="Match and replace rules">
  <div class="rule-editor-heading">
    <div>
      <strong>Match and replace</strong>
      <small>Automatically replace text in proxied traffic. Rules run top-to-bottom. Choose the precise input type: Request host, header (name & value), body, param name/value, and the same for Response. Literal replaces every occurrence; Regex supports $1 captures.</small>
    </div>
    <div class="rule-actions">
      <button class="text-button" type="button" onclick={openAdd}>Add rule</button>
      <button class="text-button" type="button" onclick={openEdit} disabled={!selectedRule}>Edit</button>
      <button class="text-button danger" type="button" onclick={removeSelected} disabled={!selectedRule}>Remove</button>
      <button type="button" aria-label="Move selected rule up" onclick={() => moveSelected(-1)} disabled={!selectedRule || rules[0]?.id === selectedRule?.id}>↑</button>
      <button type="button" aria-label="Move selected rule down" onclick={() => moveSelected(1)} disabled={!selectedRule || rules[rules.length - 1]?.id === selectedRule?.id}>↓</button>
    </div>
  </div>

  <div class="rules-table-wrap">
    <table>
      <thead><tr><th>Enabled</th><th>Type</th><th>Match</th><th>Replace</th><th>Regex</th></tr></thead>
      <tbody>
        {#if rules.length}
          {#each rules as rule (rule.id)}
            {@const t = effectiveType(rule)}
            <tr class:selected={selectedId === rule.id} onclick={() => (selectedId = rule.id)}>
              <td><Toggle ariaLabel={`Enable ${labelForType(t)} rule`} checked={rule.enabled} onclick={(event) => { event.stopPropagation(); toggleRuleEntry(rule); }} /></td>
              <td data-tooltip={typeOptions.find(o=>o.value===t)?.detail}>{labelForType(t)}</td>
              <td data-tooltip={rule.match}>{rule.match || "—"}</td>
              <td data-tooltip={rule.replace}>{rule.replace || "—"}</td>
              <td>{rule.isRegex ? "Yes" : "No"}</td>
            </tr>
          {/each}
        {:else}
          <tr><td class="empty" colspan="5">No match/replace rules. All proxied traffic passes through unchanged.</td></tr>
        {/if}
      </tbody>
    </table>
  </div>
</section>

{#if editor}
  <div class="editor-backdrop" role="presentation" onclick={(event) => { if (event.target === event.currentTarget) editor = null; }}>
    <form class="rule-dialog" aria-label={`${editingId ? "Edit" : "Add"} match/replace rule`} onsubmit={(event) => { event.preventDefault(); saveEditor(); }}>
      <header><div><p>MATCH AND REPLACE</p><h2>{editingId ? "Edit" : "Add"} rule</h2></div><button type="button" aria-label="Close editor" onclick={() => (editor = null)}>×</button></header>
      <p>Choose the exact part of the message to rewrite. <strong>Host</strong> is URI + Host header, <strong>Header</strong> covers name & value, <strong>Body</strong> is full body, <strong>Param name/value</strong> target query + form params. Literal replaces every occurrence; Regex supports <code>$1</code> captures.</p>
      <label><span>Type</span><select value={(editor as MatchReplaceRule).type ?? effectiveType(editor as MatchReplaceRule)} onchange={(event) => editor && (editor.type = event.currentTarget.value as MatchReplaceRuleType)}>{#each typeOptions as opt}<option value={opt.value}>{opt.label}</option>{/each}</select></label>
      {#if (editor as MatchReplaceRule).type}
        <small style="color:var(--muted);font-size:var(--font-size-compact)">{typeOptions.find(o=>o.value===(editor as MatchReplaceRule).type)?.detail}</small>
      {/if}
      <label class="check"><Toggle checked={editor.isRegex} ariaLabel="Use regular expression" onchange={(e) => editor && (editor.isRegex = e.currentTarget.checked)} /><span>Use regular expression</span></label>
      <label><span>Match</span><input bind:value={editor.match} placeholder={editor.isRegex ? "e.g. Bearer\\s+\\S+" : "e.g. foo"} /></label>
      <label><span>Replace</span><input bind:value={editor.replace} placeholder={editor.isRegex ? "e.g. Bearer $1" : "e.g. bar"} /></label>
      <footer><button class="text-button" type="button" onclick={() => (editor = null)}>Cancel</button><button class="text-button save" type="submit">{editingId ? "Save rule" : "Add rule"}</button></footer>
    </form>
  </div>
{/if}

<style>
  .rule-editor { display: grid; gap: 8px; }
  .rule-editor-heading { display: flex; align-items: flex-start; justify-content: space-between; gap: 12px; }
  .rule-editor-heading strong { display: block; color: var(--text, #dce1e8); font-size: var(--font-size-body); }
  .rule-editor-heading small { display: block; max-width: 620px; margin-top: 3px; color: var(--muted, #929ba8); font-size: var(--font-size-compact); }
  .rule-actions { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: 4px; }
  .rule-actions button, .rule-dialog button { min-height: 27px; padding: 0 8px; border: 1px solid var(--border-strong, #3a424d); border-radius: 4px; color: var(--text, #dbe1e8); background: var(--surface-2, #1b2028); font-size: var(--font-size-compact); cursor: pointer; }
  .rule-actions button:disabled { opacity: .45; cursor: not-allowed; }
  .rules-table-wrap { overflow: auto; border: 1px solid var(--border, #2c333d); border-radius: 4px; }
  table { width: 100%; min-width: 620px; border-collapse: collapse; color: var(--muted, #929ba8); font-size: var(--font-size-compact); }
  th { padding: 6px 8px; color: var(--text, #dbe1e8); background: var(--surface-2, #1b2028); font-weight: 600; text-align: left; }
  td { max-width: 260px; padding: 5px 8px; border-top: 1px solid var(--border, #2c333d); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  tr { cursor: pointer; } tr:hover { background: var(--surface-2, #1b2028); } tr.selected { color: var(--text, #dbe1e8); background: color-mix(in srgb, var(--accent, #9ca3af) 18%, var(--surface, #12161b)); outline: 1px solid var(--accent, #9ca3af); outline-offset: -1px; }
  td.empty { padding: 10px; text-align: center; white-space: normal; cursor: default; }
  .editor-backdrop { position: fixed; inset: 0; z-index: 50; display: grid; place-items: center; padding: 16px; background: rgb(0 0 0 / 55%); }
  .rule-dialog { display: grid; gap: 12px; width: min(520px, 100%); padding: 18px; border: 1px solid var(--border-strong, #3a424d); border-radius: 8px; color: var(--text, #dbe1e8); background: var(--surface, #12161b); box-shadow: 0 20px 55px rgb(0 0 0 / 35%); }
  .rule-dialog header { display: flex; align-items: flex-start; justify-content: space-between; } .rule-dialog header p { margin: 0 0 3px; color: var(--accent, #9ca3af); font-size: var(--font-size-compact); font-weight: 700; letter-spacing: .12em; } .rule-dialog h2 { margin: 0; font-size: var(--font-size-heading); } .rule-dialog header button { width: 27px; padding: 0; font-size: var(--font-size-heading); }
  .rule-dialog > p { margin: 0; color: var(--muted, #929ba8); font-size: var(--font-size-compact); }
  .rule-dialog label { display: grid; gap: 5px; color: var(--muted, #929ba8); font-size: var(--font-size-body); }
  .rule-dialog input, .rule-dialog select { width: 100%; height: 30px; padding: 0 8px; border: 1px solid var(--border-strong, #3a424d); border-radius: 4px; color: var(--text, #dbe1e8); background: var(--input, #0c0f13); }
  .rule-dialog label.check { display: flex; flex-direction: row; align-items: center; gap: 7px; min-height: 30px; }
  .rule-dialog footer { display: flex; justify-content: flex-end; gap: 6px; } .rule-dialog footer .save { color: #fff; border-color: var(--accent, #9ca3af); background: var(--accent, #9ca3af); }
</style>
