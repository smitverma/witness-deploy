import type { HistoryEntry, ScopeEntry } from "$lib/types";

export type SiteMapBranch = {
  kind: "host" | "directory";
  key: string;
  label: string;
  host: string;
  path: string;
  url: string;
  entries: HistoryEntry[];
  children: SiteMapBranch[];
  endpoints: HistoryEntry[];
};

export type SiteMapRow =
  | {
      kind: "branch";
      key: string;
      label: string;
      depth: number;
      node: SiteMapBranch;
    }
  | {
      kind: "endpoint";
      key: string;
      label: string;
      depth: number;
      entry: HistoryEntry;
    };

type MutableBranch = Omit<SiteMapBranch, "children"> & {
  children: Map<string, MutableBranch>;
};

function normalizePath(path: string) {
  if (!path) return "/";
  if (path.startsWith("/")) return path;
  try {
    const url = new URL(path);
    return (url.pathname || "/") + url.search;
  } catch {
    return "/" + path;
  }
}

function originFor(entry: HistoryEntry) {
  try {
    const url = new URL(entry.url);
    return url.protocol + "//" + url.host;
  } catch {
    return "https://" + entry.host;
  }
}

function entryOrder(left: HistoryEntry, right: HistoryEntry) {
  return left.path.localeCompare(right.path, undefined, { numeric: true })
    || left.method.localeCompare(right.method)
    || right.timestamp.localeCompare(left.timestamp)
    || left.id.localeCompare(right.id);
}

function branchOrder(left: MutableBranch, right: MutableBranch) {
  return left.label.localeCompare(right.label, undefined, { numeric: true });
}

function branchLabel(path: string) {
  if (path === "/") return "/";
  return path.replace(/^\/+/, "").replace(/\/?$/, "/");
}

function matchesScopeEntry(entry: ScopeEntry, host: string) {
  if (entry.isRegex) {
    try {
      return new RegExp(entry.pattern).test(host);
    } catch {
      return false;
    }
  }

  const normalizedHost = host.trimEnd().replace(/\.+$/, "").toLowerCase();
  const normalizedPattern = entry.pattern.trim().replace(/\.+$/, "").toLowerCase();
  return normalizedHost === normalizedPattern
    || (entry.includeSubdomains
      && normalizedHost.length > normalizedPattern.length
      && normalizedHost.endsWith(normalizedPattern)
      && normalizedHost[normalizedHost.length - normalizedPattern.length - 1] === ".");
}

export function isHostInScope(host: string, entries: readonly ScopeEntry[]) {
  const hasInScopeRules = entries.some((entry) => entry.isInScope);
  const inScope = entries.some((entry) => entry.isInScope && matchesScopeEntry(entry, host));
  const outOfScope = entries.some((entry) => !entry.isInScope && matchesScopeEntry(entry, host));
  return (!hasInScopeRules || inScope) && !outOfScope;
}

function createBranch(
  kind: MutableBranch["kind"],
  key: string,
  label: string,
  host: string,
  path: string,
  url: string,
): MutableBranch {
  return { kind, key, label, host, path, url, entries: [], children: new Map(), endpoints: [] };
}

function materialize(branch: MutableBranch): SiteMapBranch {
  return {
    ...branch,
    entries: [...branch.entries].sort(entryOrder),
    children: [...branch.children.values()].sort(branchOrder).map(materialize),
    endpoints: [...branch.endpoints].sort(entryOrder),
  };
}

function compressChildren(parent: MutableBranch, source: MutableBranch) {
  for (const child of source.children.values()) {
    let current = child;

    // A path with one continuation and no endpoint of its own carries no
    // useful branching information, so fold it into the eventual branch or
    // endpoint below it.
    while (current.endpoints.length === 0 && current.children.size === 1) {
      current = current.children.values().next().value!;
    }

    // A leaf is rendered as an endpoint under the nearest meaningful branch.
    // This is what keeps a host with one request on one path line.
    if (current.children.size === 0) {
      parent.endpoints.push(...current.endpoints);
      continue;
    }

    const branch = createBranch(
      "directory",
      "directory:" + JSON.stringify([current.host, current.path]),
      branchLabel(current.path),
      current.host,
      current.path,
      current.url,
    );
    branch.entries = [...current.entries];
    branch.endpoints = [...current.endpoints];
    compressChildren(branch, current);
    parent.children.set(current.path, branch);
  }
}

export function buildSiteMap(entries: readonly HistoryEntry[], search = "", inScopeOnly = false) {
  const query = search.trim().toLowerCase();
  const hosts = new Map<string, MutableBranch>();

  for (const entry of entries) {
    if (inScopeOnly && !entry.scoped) continue;
    if (query && !(entry.host + entry.path).toLowerCase().includes(query)) continue;

    const hostKey = entry.host;
    const hostLabel = entry.host || "(unknown host)";
    const origin = originFor(entry);
    let host = hosts.get(hostKey);
    if (!host) {
      host = createBranch(
        "host",
        "host:" + JSON.stringify(hostKey),
        hostLabel,
        hostKey,
        "/",
        origin + "/",
      );
      hosts.set(hostKey, host);
    }

    const normalizedPath = normalizePath(entry.path);
    const segments = normalizedPath.split("/").filter(Boolean);
    let branch = host;
    branch.entries.push(entry);

    let path = "";
    for (const segment of segments) {
      path += "/" + segment;
      let directory = branch.children.get(segment);
      if (!directory) {
        directory = createBranch(
          "directory",
          "path:" + JSON.stringify([hostKey, path]),
          branchLabel(path),
          hostKey,
          path,
          origin + path,
        );
        branch.children.set(segment, directory);
      }
      directory.entries.push(entry);
      branch = directory;
    }

    branch.endpoints.push(entry);
  }

  return [...hosts.values()].sort(branchOrder).map((host) => {
    const compressed = createBranch("host", host.key, host.label, host.host, host.path, host.url);
    compressed.entries = [...host.entries];
    compressed.endpoints = [...host.endpoints];
    compressChildren(compressed, host);
    return materialize(compressed);
  });
}

export function endpointLabel(entry: HistoryEntry, directoryPath: string) {
  const path = normalizePath(entry.path);
  if (directoryPath === "/") return path;
  const prefix = directoryPath.endsWith("/") ? directoryPath : directoryPath + "/";
  if (path === directoryPath || path === prefix) return "/";
  return path.startsWith(prefix) ? path.slice(prefix.length) || "/" : path;
}

export function flattenSiteMap(
  hosts: readonly SiteMapBranch[],
  collapsed: ReadonlySet<string>,
  forceExpanded = false,
) {
  const rows: SiteMapRow[] = [];

  function visit(branch: SiteMapBranch, depth: number) {
    rows.push({ kind: "branch", key: branch.key, label: branch.label, depth, node: branch });
    if (!forceExpanded && collapsed.has(branch.key)) return;

    for (const child of branch.children) visit(child, depth + 1);
    for (const entry of branch.endpoints) {
      rows.push({
        kind: "endpoint",
        key: "endpoint:" + entry.id,
        label: endpointLabel(entry, branch.path),
        depth: depth + 1,
        entry,
      });
    }
  }

  for (const host of hosts) visit(host, 0);
  return rows;
}
