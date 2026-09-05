import type { TabGroup } from "./types";

export type GroupableTab = { id: number; groupId: string | null };

export type TabBarEntry<T extends GroupableTab> =
  | { kind: "tab"; tab: T }
  | { kind: "group"; group: TabGroup; tabs: T[] };

export function buildTabBarEntries<T extends GroupableTab>(
  tabs: readonly T[],
  groups: readonly TabGroup[],
): TabBarEntry<T>[] {
  const knownGroups = new Set(groups.map((group) => group.id));
  const renderedGroups = new Set<string>();
  const entries: TabBarEntry<T>[] = [];
  for (const tab of tabs) {
    if (!tab.groupId || !knownGroups.has(tab.groupId)) {
      entries.push({ kind: "tab", tab });
      continue;
    }
    if (renderedGroups.has(tab.groupId)) continue;
    const group = groups.find((candidate) => candidate.id === tab.groupId);
    if (!group) {
      entries.push({ kind: "tab", tab });
      continue;
    }
    entries.push({ kind: "group", group, tabs: tabs.filter((candidate) => candidate.groupId === group.id) });
    renderedGroups.add(group.id);
  }
  return entries;
}
