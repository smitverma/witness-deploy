<script lang="ts">
  import { showErrorToast } from "$lib/errorToast";
  import { moveRule, nextRuleId, removeRule, toggleRule, validateRuleCondition } from "$lib/rule-list";
  import Toggle from "./Toggle.svelte";
  import type {
    InterceptionRule,
    InterceptionRuleMatchType,
    InterceptionRuleRelationship,
  } from "$lib/types";

  let { kind, rules, onChange }: {
    kind: "request" | "response";
    rules: InterceptionRule[];
    onChange: (rules: InterceptionRule[]) => void;
  } = $props();

  const matchTypes: { value: InterceptionRuleMatchType; label: string }[] = [
    { value: "domain", label: "Domain name" },
    { value: "ipAddress", label: "IP address" },
    { value: "protocol", label: "Protocol" },
    { value: "httpMethod", label: "HTTP method" },
    { value: "url", label: "URL" },
    { value: "fileExtension", label: "File extension" },
    { value: "contentType", label: "Content type" },
    { value: "request", label: "Request" },
    { value: "cookieName", label: "Cookie name" },
    { value: "cookieValue", label: "Cookie value" },
    { value: "anyHeader", label: "Any header" },
    { value: "body", label: "Body" },
    { value: "paramName", label: "Param name" },
    { value: "paramValue", label: "Param value" },
    { value: "listenerPort", label: "Listener port" },
    { value: "inScope", label: "Target scope" },
  ];
  const standardRelationships: { value: InterceptionRuleRelationship; label: string }[] = [
    { value: "matches", label: "Matches regular expression" },
    { value: "doesNotMatch", label: "Does not match regular expression" },
    { value: "contains", label: "Contains" },
    { value: "doesNotContain", label: "Does not contain" },
  ];
  const scopeRelationships: { value: InterceptionRuleRelationship; label: string }[] = [
    { value: "isInScope", label: "Is in target scope" },
    { value: "isNotInScope", label: "Is not in target scope" },
  ];

  let selectedId = $state<string | null>(null);
  let editor = $state<InterceptionRule | null>(null);
  let editingId = $state<string | null>(null);
  const selectedRule = $derived(rules.find((rule) => rule.id === selectedId) ?? null);
  const title = $derived(kind === "request" ? "Request" : "Response");

  $effect(() => {
    if (selectedId && rules.some((rule) => rule.id === selectedId)) return;
    selectedId = rules[0]?.id ?? null;
  });

  function newRule(): InterceptionRule {
    return {
      id: nextRuleId(),
      enabled: true,
      operator: "and",
      matchType: "fileExtension",
      relationship: "doesNotMatch",
      condition: "^(gif|ico|jpg|jpeg|png|svg|css|js|woff2?)$",
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
    const next = {
      ...editor,
      condition: editor.matchType === "inScope" ? "" : editor.condition.trim(),
    };
    if (next.matchType !== "inScope") {
      const error = validateRuleCondition(
        next.condition,
        next.relationship === "matches" || next.relationship === "doesNotMatch",
      );
      if (error) {
        showErrorToast(error);
        return;
      }
    }
    const nextRules = editingId
      ? rules.map((rule) => rule.id === editingId ? next : rule)
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

  function toggleRuleEntry(rule: InterceptionRule) {
    onChange(toggleRule(rules, rule.id));
  }

  function updateMatchType(value: InterceptionRuleMatchType) {
    if (!editor) return;
    editor.matchType = value;
    if (value === "inScope") {
      editor.relationship = "isInScope";
      editor.condition = "";
    } else if (editor.relationship === "isInScope" || editor.relationship === "isNotInScope") {
      editor.relationship = "matches";
    }
  }

  function labelForMatchType(value: InterceptionRuleMatchType) {
    return matchTypes.find((type) => type.value === value)?.label ?? value;
  }

  function labelForRelationship(value: InterceptionRuleRelationship) {
    return [...standardRelationships, ...scopeRelationships].find((relationship) => relationship.value === value)?.label ?? value;
  }
</script>

<section class="rule-editor" aria-label={`${title} interception rules`}>
  <div class="rule-editor-heading">
    <div>
      <strong>{title} rules</strong>
      <small>Enabled rules are evaluated from top to bottom. The first rule starts the expression; later rules use their AND/OR operator.</small>
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
      <thead><tr><th>Enabled</th><th>Operator</th><th>Match type</th><th>Relationship</th><th>Condition</th></tr></thead>
      <tbody>
        {#if rules.length}
          {#each rules as rule, index (rule.id)}
            <tr class:selected={selectedId === rule.id} onclick={() => (selectedId = rule.id)}>
              <td><Toggle ariaLabel={`Enable ${labelForMatchType(rule.matchType)} rule`} checked={rule.enabled} onclick={(event) => { event.stopPropagation(); toggleRuleEntry(rule); }} /></td>
              <td>{index === 0 ? "Start" : rule.operator === "and" ? "And" : "Or"}</td>
              <td>{labelForMatchType(rule.matchType)}</td>
              <td>{labelForRelationship(rule.relationship)}</td>
              <td data-tooltip={rule.condition}>{rule.condition || "—"}</td>
            </tr>
          {/each}
        {:else}
          <tr><td class="empty" colspan="5">No {kind} filters are configured. All {kind}s will match the selected interception mode.</td></tr>
        {/if}
      </tbody>
    </table>
  </div>
</section>

{#if editor}
  <div class="editor-backdrop" role="presentation" onclick={(event) => { if (event.target === event.currentTarget) editor = null; }}>
    <form class="rule-dialog" aria-label={`${editingId ? "Edit" : "Add"} ${kind} interception rule`} onsubmit={(event) => { event.preventDefault(); saveEditor(); }}>
      <header><div><p>{kind.toUpperCase()} INTERCEPTION</p><h2>{editingId ? "Edit" : "Add"} rule</h2></div><button type="button" aria-label="Close rule editor" onclick={() => (editor = null)}>×</button></header>
      <p>Use a regular expression for “matches” rules. “Contains” rules accept one or more comma-separated values.</p>
      <label><span>Boolean operator</span><select value={editor.operator} onchange={(event) => (editor && (editor.operator = event.currentTarget.value as "and" | "or"))}><option value="and">And</option><option value="or">Or</option></select></label>
      <label><span>Match type</span><select value={editor.matchType} onchange={(event) => updateMatchType(event.currentTarget.value as InterceptionRuleMatchType)}>{#each matchTypes as type}<option value={type.value}>{type.label}</option>{/each}</select></label>
      <label><span>Relationship</span><select value={editor.relationship} onchange={(event) => (editor && (editor.relationship = event.currentTarget.value as InterceptionRuleRelationship))}>{#each editor.matchType === "inScope" ? scopeRelationships : standardRelationships as relationship}<option value={relationship.value}>{relationship.label}</option>{/each}</select></label>
      {#if editor.matchType !== "inScope"}
        <label><span>Match condition</span><input bind:value={editor.condition} placeholder={editor.relationship === "contains" || editor.relationship === "doesNotContain" ? "e.g. api, account, token" : "e.g. ^(js|mjs|css)$"} /></label>
      {/if}
      <footer><button class="text-button" type="button" onclick={() => (editor = null)}>Cancel</button><button class="text-button save" type="submit">{editingId ? "Save rule" : "Add rule"}</button></footer>
    </form>
  </div>
{/if}

<style>
  .rule-editor { display: grid; gap: 8px; }
  .rule-editor-heading { display: flex; align-items: flex-start; justify-content: space-between; gap: 12px; }
  .rule-editor-heading strong { display: block; color: var(--text, #dce1e8); font-size: var(--font-size-body); }
  .rule-editor-heading small { display: block; max-width: 560px; margin-top: 3px; color: var(--muted, #929ba8); font-size: var(--font-size-compact); }
  .rule-actions { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: 4px; }
  .rule-actions button, .rule-dialog button { min-height: 27px; padding: 0 8px; border: 1px solid var(--border-strong, #3a424d); border-radius: 4px; color: var(--text, #dbe1e8); background: var(--surface-2, #1b2028); font-size: var(--font-size-compact); cursor: pointer; }
  .rule-actions button:disabled { opacity: .45; cursor: not-allowed; }
  .rules-table-wrap { overflow: auto; border: 1px solid var(--border, #2c333d); border-radius: 4px; }
  table { width: 100%; min-width: 620px; border-collapse: collapse; color: var(--muted, #929ba8); font-size: var(--font-size-compact); }
  th { padding: 6px 8px; color: var(--text, #dbe1e8); background: var(--surface-2, #1b2028); font-weight: 600; text-align: left; }
  td { max-width: 240px; padding: 5px 8px; border-top: 1px solid var(--border, #2c333d); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  tr { cursor: pointer; } tr:hover { background: var(--surface-2, #1b2028); } tr.selected { color: var(--text, #dbe1e8); background: color-mix(in srgb, var(--accent, #9ca3af) 18%, var(--surface, #12161b)); outline: 1px solid var(--accent, #9ca3af); outline-offset: -1px; }
  td.empty { padding: 10px; text-align: center; white-space: normal; cursor: default; }
  .editor-backdrop { position: fixed; inset: 0; z-index: 50; display: grid; place-items: center; padding: 16px; background: rgb(0 0 0 / 55%); }
  .rule-dialog { display: grid; gap: 12px; width: min(520px, 100%); padding: 18px; border: 1px solid var(--border-strong, #3a424d); border-radius: 8px; color: var(--text, #dbe1e8); background: var(--surface, #12161b); box-shadow: 0 20px 55px rgb(0 0 0 / 35%); }
  .rule-dialog header { display: flex; align-items: flex-start; justify-content: space-between; } .rule-dialog header p { margin: 0 0 3px; color: var(--accent, #9ca3af); font-size: var(--font-size-compact); font-weight: 700; letter-spacing: .12em; } .rule-dialog h2 { margin: 0; font-size: var(--font-size-heading); } .rule-dialog header button { width: 27px; padding: 0; font-size: var(--font-size-heading); }
  .rule-dialog > p { margin: 0; color: var(--muted, #929ba8); font-size: var(--font-size-compact); }
  .rule-dialog label { display: grid; gap: 5px; color: var(--muted, #929ba8); font-size: var(--font-size-body); }
  .rule-dialog input, .rule-dialog select { width: 100%; height: 30px; padding: 0 8px; border: 1px solid var(--border-strong, #3a424d); border-radius: 4px; color: var(--text, #dbe1e8); background: var(--input, #0c0f13); }
  .rule-dialog footer { display: flex; justify-content: flex-end; gap: 6px; } .rule-dialog footer .save { color: #fff; border-color: var(--accent, #9ca3af); background: var(--accent, #9ca3af); }
</style>
