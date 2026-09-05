import assert from "node:assert/strict";
import test from "node:test";

import {
  formatRawPlusMessage,
  readHttpRequestMethod,
  replaceHttpRequestMethod,
  splitHttpMessage,
  synchronizeHttpContentLength,
} from "../src/lib/http-message.ts";

test("changes only the request-line method while preserving the request", () => {
  const input = [
    "GET   /items HTTP/1.1",
    "Host: example.test",
    "",
    "body",
  ].join("\r\n");
  const output = replaceHttpRequestMethod(input, "post");
  assert.equal(readHttpRequestMethod(output), "POST");
  assert.equal(output, [
    "POST   /items HTTP/1.1",
    "Host: example.test",
    "",
    "body",
  ].join("\r\n"));
});

test("rejects invalid custom request methods", () => {
  const input = "GET / HTTP/1.1\n\n";
  assert.equal(replaceHttpRequestMethod(input, "bad method"), input);
});

test("leaves form-encoded messages byte-for-byte unchanged", () => {
  const encoded = [
    "POST /submit HTTP/1.1",
    "Host: example.test",
    "Content-Type: application/x-www-form-urlencoded",
    "Content-Length: 31",
    "",
    "data=%7B%22name%22%3A%22A%22%7D",
  ].join("\r\n");
  assert.equal(formatRawPlusMessage(encoded), encoded);

  const jsonShapedFormValue = [
    "POST /submit HTTP/1.1",
    "Content-Type: application/x-www-form-urlencoded",
    "",
    "{\"name\":\"A\"}",
  ].join("\r\n");
  assert.equal(formatRawPlusMessage(jsonShapedFormValue), jsonShapedFormValue);
});

test("formats only declared JSON while preserving headers and the boundary", () => {
  const input = [
    "POST /items HTTP/1.1",
    "host: example.test",
    "content-type: application/json; charset=utf-8",
    "content-length: 7",
    "X-Custom: Keep-Me",
    "",
    "{\"a\":1}",
  ].join("\r\n");
  const output = formatRawPlusMessage(input);
  const message = splitHttpMessage(output);

  assert.equal(
    message.head,
    [
      "POST /items HTTP/1.1",
      "host: example.test",
      "content-type: application/json; charset=utf-8",
      `content-length: ${new TextEncoder().encode(message.body).length}`,
      "X-Custom: Keep-Me",
    ].join("\r\n"),
  );
  assert.equal(message.separator, "\r\n\r\n");
  assert.equal(message.body, "{\r\n  \"a\": 1\r\n}");
});

test("header edits do not add a length header or alter blank lines", () => {
  const input = [
    "POST /submit HTTP/1.1",
    "Host: changed.example",
    "Content-Type: application/x-www-form-urlencoded",
    "",
    "a=1&b=2",
  ].join("\r\n");
  assert.equal(synchronizeHttpContentLength(input), input);
});

test("updates an existing length without changing LF line endings", () => {
  const input = [
    "POST /submit HTTP/1.1",
    "Host: example.test",
    "Content-Length: 1",
    "",
    "name=José",
  ].join("\n");
  const output = synchronizeHttpContentLength(input);
  assert.equal(
    output,
    [
      "POST /submit HTTP/1.1",
      "Host: example.test",
      `Content-Length: ${new TextEncoder().encode("name=José").length}`,
      "",
      "name=José",
    ].join("\n"),
  );
});

test("does not transform chunked, encoded, or malformed JSON bodies", () => {
  const chunked = [
    "POST /items HTTP/1.1",
    "Content-Type: application/json",
    "Transfer-Encoding: chunked",
    "",
    "7",
    "{\"a\":1}",
    "0",
    "",
    "",
  ].join("\r\n");
  assert.equal(formatRawPlusMessage(chunked), chunked);

  const compressed = [
    "HTTP/1.1 200 OK",
    "Content-Type: application/json",
    "Content-Encoding: gzip",
    "",
    "{\"a\":1}",
  ].join("\r\n");
  assert.equal(formatRawPlusMessage(compressed), compressed);

  const malformed = [
    "POST /items HTTP/1.1",
    "Content-Type: application/json",
    "",
    "{\"a\":",
  ].join("\r\n");
  assert.equal(formatRawPlusMessage(malformed), malformed);
});

test("supports vendor JSON types and preserves body-edge whitespace", () => {
  const input = [
    "HTTP/1.1 200 OK",
    "Content-Type: application/problem+json",
    "Content-Length: 9",
    "",
    " \n{\"a\":1}\n",
  ].join("\n");
  const output = formatRawPlusMessage(input);
  const message = splitHttpMessage(output);

  assert.equal(message.body, " \n{\n  \"a\": 1\n}\n");
  assert.match(message.head, new RegExp(`Content-Length: ${new TextEncoder().encode(message.body).length}$`));
});
