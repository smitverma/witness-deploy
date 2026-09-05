export type HttpMessageParts = {
  head: string;
  body: string;
  separator: string;
  lineEnding: "\r\n" | "\n" | "\r";
  complete: boolean;
};

const HEADER_BODY_SEPARATOR = /\r\n\r\n|\r\n\n|\n\r\n|\n\n|\r\r/;
const HEADER_LINE = /(^|\r\n|\n|\r)([!#$%&'*+.^_`|~0-9A-Za-z-]+)([ \t]*:[ \t]*)([^\r\n]*)/g;
export const HTTP_METADATA_LIMIT = 16_384;
const textDecoder = new TextDecoder();
const textEncoder = new TextEncoder();

export function decodeHttpText(value: Uint8Array) {
  return textDecoder.decode(value);
}

export function encodeHttpText(value: string) {
  return textEncoder.encode(value);
}

/**
 * Returns the request prefix that contains the request line and headers.
 * Callers can compare this prefix before reparsing request metadata, keeping
 * body-only edits out of the metadata hot path.
 */
export function requestHeaderPrefix(value: string) {
  if (!value) return null;
  const limited = value.slice(0, HTTP_METADATA_LIMIT);
  const separator = HEADER_BODY_SEPARATOR.exec(limited);
  return separator
    ? limited.slice(0, separator.index + separator[0].length)
    : limited;
}

const HTTP_METHOD_TOKEN = /^[!#$%&'*+.^_`|~0-9A-Za-z-]+$/;

export function isValidHttpMethod(method: string) {
  return HTTP_METHOD_TOKEN.test(method.trim());
}

export function readHttpRequestMethod(value: string) {
  const firstLine = value.split(/\r\n|\n|\r/, 1)[0] ?? "";
  return firstLine.trim().split(/\s+/, 1)[0]?.toUpperCase() ?? "";
}

export function replaceHttpRequestMethod(value: string, method: string) {
  const normalizedMethod = method.trim().toUpperCase();
  if (!isValidHttpMethod(normalizedMethod)) return value;
  const firstLineEnd = value.search(/\r\n|\n|\r/);
  const firstLine = firstLineEnd < 0 ? value : value.slice(0, firstLineEnd);
  const match = /^(\s*)\S+(\s+)(.*)$/.exec(firstLine);
  if (!match) return value;
  return `${match[1]}${normalizedMethod}${match[2]}${match[3]}${value.slice(firstLine.length)}`;
}

export function splitHttpMessage(value: string): HttpMessageParts {
  const match = HEADER_BODY_SEPARATOR.exec(value);
  if (!match || match.index === undefined) {
    return {
      head: value,
      body: "",
      separator: "",
      lineEnding: detectLineEnding(value),
      complete: false,
    };
  }
  const separator = match[0];
  return {
    head: value.slice(0, match.index),
    body: value.slice(match.index + separator.length),
    separator,
    lineEnding: detectLineEnding(value.slice(0, match.index) || separator),
    complete: true,
  };
}

export function splitHttpLines(value: string) {
  return value.split(/\r\n|\n|\r/);
}

/** Converts CRLF and bare CR line endings to the LF form used by the editor. */
export function normalizeHttpLineEndings(value: string) {
  return value.replace(/\r\n?|\n/g, "\n");
}

export function normalizeHttpLineEndingBytes(value: Uint8Array) {
  const decoded = decodeHttpText(value);
  const normalized = normalizeHttpLineEndings(decoded);
  return normalized === decoded ? value : encodeHttpText(normalized);
}

/**
 * Finalizes an outbound HTTP request. It is intentionally called only at a
 * send or forward boundary, allowing users to edit an incomplete request.
 * Existing Content-Length headers are recalculated from the finalized body.
 */
export function finalizeHttpRequest(value: string) {
  if (!value) return value;
  const normalized = normalizeHttpLineEndings(value);
  const completed = splitHttpMessage(normalized).complete
    ? normalized
    : normalized.endsWith("\n") ? `${normalized}\n` : `${normalized}\n\n`;
  return synchronizeHttpContentLength(completed);
}

export function finalizeHttpRequestBytes(value: Uint8Array) {
  const decoded = decodeHttpText(value);
  const finalized = finalizeHttpRequest(decoded);
  return finalized === decoded ? value : encodeHttpText(finalized);
}

export function synchronizeHttpContentLength(value: string) {
  const message = splitHttpMessage(value);
  if (!message.complete || hasTokenHeader(message.head, "transfer-encoding", "chunked")) {
    return value;
  }

  const bodyLength = encodeHttpText(message.body).length;
  let changed = false;
  const head = message.head.replace(
    HEADER_LINE,
    (line, prefix: string, name: string, delimiter: string, headerValue: string) => {
      if (name.toLowerCase() !== "content-length") return line;
      const trailingWhitespace = headerValue.match(/[ \t]*$/)?.[0] ?? "";
      const nextValue = `${bodyLength}${trailingWhitespace}`;
      if (headerValue === nextValue) return line;
      changed = true;
      return `${prefix}${name}${delimiter}${nextValue}`;
    },
  );

  return changed ? `${head}${message.separator}${message.body}` : value;
}

export function formatRawPlusMessage(value: string) {
  if (!value) return "";
  const message = splitHttpMessage(value);
  if (!message.complete || !isJsonMessage(message)) return value;

  const leadingWhitespace = message.body.match(/^\s*/)?.[0] ?? "";
  const trailingWhitespace = message.body.match(/\s*$/)?.[0] ?? "";
  const candidateEnd = message.body.length - trailingWhitespace.length;
  const candidate = message.body.slice(leadingWhitespace.length, candidateEnd);
  if (!candidate) return value;

  let formatted: string;
  try {
    formatted = prettyPrintJson(candidate, message.lineEnding);
  } catch {
    return value;
  }

  const body = `${leadingWhitespace}${formatted}${trailingWhitespace}`;
  if (body === message.body) return value;
  return synchronizeHttpContentLength(`${message.head}${message.separator}${body}`);
}

export function isJsonMessage(message: HttpMessageParts) {
  if (hasTokenHeader(message.head, "transfer-encoding", "chunked")) return false;
  if (getHeaderValues(message.head, "content-encoding")
    .some((value) => value.trim().toLowerCase() !== "identity")) {
    return false;
  }

  const contentType = getHeaderValues(message.head, "content-type")[0];
  if (!contentType) return false;
  const [mediaType = "", ...parameters] = contentType.split(";");
  const normalizedMediaType = mediaType.trim().toLowerCase();
  const jsonMediaType = normalizedMediaType === "application/json"
    || normalizedMediaType === "text/json"
    || normalizedMediaType.endsWith("+json");
  if (!jsonMediaType) return false;

  const charset = parameters
    .map((parameter) => parameter.trim().split("=", 2))
    .find(([name]) => name?.toLowerCase() === "charset")?.[1]
    ?.trim()
    .replace(/^"|"$/g, "")
    .toLowerCase();
  return !charset || charset === "utf-8" || charset === "utf8";
}

function getHeaderValues(head: string, requestedName: string) {
  const lines = splitHttpLines(head);
  const values: string[] = [];
  let currentName = "";
  for (const line of lines.slice(1)) {
    if (/^[ \t]/.test(line) && values.length && currentName === requestedName) {
      values[values.length - 1] += ` ${line.trim()}`;
      continue;
    }
    const separator = line.indexOf(":");
    if (separator <= 0) {
      currentName = "";
      continue;
    }
    currentName = line.slice(0, separator).trim().toLowerCase();
    if (currentName === requestedName) {
      values.push(line.slice(separator + 1).trim());
    }
  }
  return values;
}

function hasTokenHeader(head: string, name: string, token: string) {
  return getHeaderValues(head, name).some((value) =>
    value.split(",").some((item) => item.trim().toLowerCase() === token)
  );
}

function detectLineEnding(value: string): "\r\n" | "\n" | "\r" {
  const match = /\r\n|\n|\r/.exec(value);
  return match?.[0] === "\n" ? "\n" : match?.[0] === "\r" ? "\r" : "\r\n";
}

function prettyPrintJson(value: string, lineEnding: string) {
  // Validate first, then format lexically so large integers and duplicate keys
  // remain exactly as supplied instead of being coerced through JSON.stringify.
  JSON.parse(value);
  let output = "";
  let depth = 0;
  let inString = false;
  let escaped = false;
  let previousSignificant = "";
  const indentation = () => "  ".repeat(depth);

  for (const character of value) {
    if (inString) {
      output += character;
      if (escaped) escaped = false;
      else if (character === "\\") escaped = true;
      else if (character === "\"") inString = false;
      continue;
    }
    if (character === "\"") {
      inString = true;
      output += character;
      previousSignificant = character;
    } else if (character === "{" || character === "[") {
      output += `${character}${lineEnding}`;
      depth += 1;
      output += indentation();
      previousSignificant = character;
    } else if (character === "}" || character === "]") {
      depth = Math.max(0, depth - 1);
      if (previousSignificant === (character === "}" ? "{" : "[")) {
        output = output.trimEnd();
      } else {
        output = `${output.trimEnd()}${lineEnding}${indentation()}`;
      }
      output += character;
      previousSignificant = character;
    } else if (character === ",") {
      output = `${output.trimEnd()},${lineEnding}${indentation()}`;
      previousSignificant = character;
    } else if (character === ":") {
      output = `${output.trimEnd()}: `;
      previousSignificant = character;
    } else if (!/\s/.test(character)) {
      output += character;
      previousSignificant = character;
    }
  }
  return output;
}
