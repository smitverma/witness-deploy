import assert from "node:assert/strict";
import test from "node:test";
import {
  parseHostname,
  parseRequestDetails,
  parseRequestMetadataText,
  requestHostText,
} from "../src/lib/http-url.ts";
import { requestHost, requestHostText as intruderHostText, synchronizeContentLength } from "../src/lib/intruder.ts";
import { synchronizeHttpContentLength } from "../src/lib/http-message.ts";

test("parseHostname handles host, URL, and port", () => {
  assert.equal(parseHostname("example.com"), "example.com");
  assert.equal(parseHostname("example.com:8080"), "example.com");
  assert.equal(parseHostname("https://example.com:8443/path"), "example.com");
  assert.equal(parseHostname(""), "");
});

test("requestHostText uses Host header then target", () => {
  const raw = "GET /path HTTP/1.1\r\nHost: example.test\r\n\r\n";
  assert.equal(requestHostText(raw), "example.test");
  assert.equal(intruderHostText(raw), "example.test");
  assert.equal(requestHost(new TextEncoder().encode(raw)), "example.test");
});

test("parseRequestMetadataText builds url and host", () => {
  const meta = parseRequestMetadataText("GET /a HTTP/1.1\r\nHost: ex.test\r\n\r\n", "");
  assert.equal(meta.method, "GET");
  assert.ok(meta.url.includes("ex.test"));
  assert.equal(meta.host, "ex.test");
});

test("parseRequestDetails parses method/url/headers/body", () => {
  const details = parseRequestDetails("GET /a?b=1 HTTP/1.1\r\nHost: ex.test\r\nX-A: 1\r\n\r\nbody");
  assert.ok(details);
  assert.equal(details.method, "GET");
  assert.ok(details.url.includes("ex.test"));
  assert.equal(details.body, "body");
  assert.equal(parseRequestDetails("not http"), null);
});

test("synchronizeContentLength delegates to canonical", () => {
  const raw = "POST / HTTP/1.1\r\nHost: x\r\nContent-Length: 999\r\n\r\nhello";
  assert.equal(synchronizeContentLength(raw), synchronizeHttpContentLength(raw));
  assert.ok(synchronizeContentLength(raw).includes("Content-Length: 5"));
});
