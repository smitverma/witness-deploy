import assert from "node:assert/strict";
import test from "node:test";

import { buildSiteMap, flattenSiteMap, isHostInScope } from "../src/lib/siteMap.ts";

function entry(id, path, host = "example.test", method = "GET", scoped = true) {
  return {
    id,
    url: "https://" + host + path,
    method,
    host,
    path,
    status: 200,
    length: 100,
    mimeType: "text/html",
    durationMs: 10,
    timestamp: "2026-08-08T00:00:0" + id + "Z",
    scoped,
  };
}

test("keeps a single path on one endpoint line", () => {
  const hosts = buildSiteMap([entry("1", "/api/query/dancing/trust")]);

  assert.deepEqual(hosts[0].children, []);
  assert.deepEqual(flattenSiteMap(hosts, new Set()).map((row) => row.label), [
    "example.test",
    "/api/query/dancing/trust",
  ]);
});

test("creates branches only at path divergence", () => {
  const hosts = buildSiteMap([
    entry("1", "/api/query/dancing/trust"),
    entry("2", "/api/query/dancing/false", "example.test", "POST"),
    entry("3", "/api/query/singing/song"),
  ]);

  assert.equal(hosts.length, 1);
  assert.deepEqual(hosts[0].children.map((node) => node.label), ["api/query/"]);
  assert.deepEqual(hosts[0].children[0].children.map((node) => node.label), ["api/query/dancing/"]);
  assert.deepEqual(hosts[0].children[0].children[0].endpoints.map((item) => item.path), ["/api/query/dancing/false", "/api/query/dancing/trust"]);
  assert.deepEqual(hosts[0].children[0].endpoints.map((item) => item.path), ["/api/query/singing/song"]);
});

test("flattening preserves hierarchy and collapsed branches", () => {
  const hosts = buildSiteMap([entry("1", "/api/v1/users"), entry("2", "/api/v1/projects"), entry("3", "/health")]);
  const rows = flattenSiteMap(hosts, new Set([hosts[0].children[0].key]));

  assert.deepEqual(rows.map((row) => row.kind + ":" + row.label + ":" + row.depth), [
    "branch:example.test:0",
    "branch:api/v1/:1",
    "endpoint:/health:1",
  ]);
});

test("search filtering keeps matching entries under their full ancestor path", () => {
  const hosts = buildSiteMap([entry("1", "/api/v1/users"), entry("2", "/api/v1/projects")], "users");
  const rows = flattenSiteMap(hosts, new Set(), true);

  assert.deepEqual(rows.map((row) => row.label), ["example.test", "/api/v1/users"]);
});

test("scope filtering excludes out-of-scope URLs", () => {
  const hosts = buildSiteMap([
    entry("1", "/in", "example.test", "GET", true),
    entry("2", "/out", "example.test", "GET", false),
  ], "", true);

  assert.deepEqual(flattenSiteMap(hosts, new Set()).map((row) => row.label), ["example.test", "/in"]);
});

test("scope matching uses the current rules instead of capture-time flags", () => {
  const rules = [
    { id: 1, pattern: "example.test", isRegex: false, includeSubdomains: true, isInScope: true },
    { id: 2, pattern: "telemetry.example.test", isRegex: false, includeSubdomains: true, isInScope: false },
  ];

  assert.equal(isHostInScope("api.example.test", rules), true);
  assert.equal(isHostInScope("telemetry.example.test", rules), false);
  assert.equal(isHostInScope("outside.test", rules), false);
  assert.equal(isHostInScope("outside.test", []), true);
});
