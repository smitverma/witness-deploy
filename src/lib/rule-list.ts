export const MAX_RULE_CONDITION_LEN = 512;

export function nextRuleId(): string {
  return (
    globalThis.crypto?.randomUUID?.() ??
    `${Date.now()}-${Math.random().toString(36).slice(2)}`
  );
}

export function validateRuleCondition(condition: string, isRegex: boolean): string | null {
  const trimmed = condition.trim();
  if (!trimmed) return "Enter a condition for this rule.";
  if (trimmed.length > MAX_RULE_CONDITION_LEN)
    return `Condition must be at most ${MAX_RULE_CONDITION_LEN} characters.`;
  if (isRegex) {
    try {
      new RegExp(trimmed, "i");
    } catch {
      return "Enter a valid regular expression.";
    }
  }
  return null;
}

export function moveRule<T extends { id: string }>(rules: T[], id: string, direction: -1 | 1): T[] {
  const index = rules.findIndex((r) => r.id === id);
  const target = index + direction;
  if (index < 0 || target < 0 || target >= rules.length) return rules;
  const next = [...rules];
  [next[index], next[target]] = [next[target], next[index]];
  return next;
}

export function removeRule<T extends { id: string }>(rules: T[], id: string): { next: T[]; nextSelectedId: string | null } {
  const index = rules.findIndex((r) => r.id === id);
  const next = rules.filter((r) => r.id !== id);
  return { next, nextSelectedId: next[Math.min(Math.max(index, 0), next.length - 1)]?.id ?? null };
}

export function toggleRule<T extends { id: string; enabled: boolean }>(rules: T[], id: string): T[] {
  return rules.map((r) => (r.id === id ? { ...r, enabled: !r.enabled } : r));
}
