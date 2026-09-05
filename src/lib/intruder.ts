import {
  normalizeHttpLineEndings,
  splitHttpMessage,
  synchronizeHttpContentLength,
} from "$lib/http-message";
import {
  requestHost as parseRequestHost,
  requestHostText as parseRequestHostText,
} from "$lib/http-url";
import type {
  IntruderMode,
  IntruderRange,
  IntruderResultTab,
  IntruderScan,
  IntruderTab,
  PayloadProcessingRule,
  PayloadWarehouse,
} from "$lib/types";

export const MAX_INTRUDER_REQUESTS = 5_000;

export const BUILTIN_PAYLOAD_LISTS: Record<string, string[]> = {
  "Common usernames": ["admin", "administrator", "root", "guest", "test"],
  "Common passwords": ["password", "123456", "qwerty", "letmein", "admin"],
  "Boolean values": ["true", "false", "1", "0", "null"],
  "SQL probes": ["'", "\"", "' OR '1'='1", "1 OR 1=1", "'--"],
  "Path traversal": ["../", "../../", "../../../etc/passwd", "..\\..\\", "%2e%2e%2f"],
};

export function createPayloadWarehouse(today = new Date()): PayloadWarehouse {
  const date = today.toISOString().slice(0, 10);
  const substitutions = [
    ["a", "4"], ["b", "8"], ["e", "3"], ["g", "6"],
    ["i", "1"], ["o", "0"], ["s", "5"], ["t", "7"],
    ["z", "2"], ["", ""], ["", ""], ["", ""],
    ["", ""], ["", ""], ["", ""], ["", ""],
  ].map(([from, to]) => ({ from, to }));
  return {
    type: "list",
    list: { text: "", builtin: Object.keys(BUILTIN_PAYLOAD_LISTS)[0] ?? "", url: "" },
    numbers: { mode: "sequential", from: "0", to: "10", step: "1", count: "" },
    nullPayload: { mode: "count", count: "10" },
    bruteForce: {
      characterSet: "abcdefghijklmnopqrstuvwxyz0123456789",
      minLength: "1",
      maxLength: "2",
    },
    dates: {
      from: date,
      to: date,
      step: "1",
      unit: "days",
      formatMode: "preset",
      format: "M/D/YY",
      customFormat: "dd.MM.yyyy",
    },
    characterSubstitution: {
      mappings: substitutions,
      caseSensitive: false,
      itemsText: "",
      newItem: "",
      builtin: Object.keys(BUILTIN_PAYLOAD_LISTS)[0] ?? "",
    },
    processing: [],
  };
}

export function createIntruderTab(
  id: number,
  request = new Uint8Array(),
  tls = true,
): IntruderTab {
  return {
    kind: "setup",
    id,
    title: `${id}`,
    groupId: null,
    request: request.slice(),
    tls,
    mode: "single",
    scanName: "",
    warehouse: createPayloadWarehouse(),
    positionWarehouses: [],
    selectedPayloadPosition: 0,
    scans: [],
    activeScanId: null,
    error: "",
  };
}

export function createIntruderResultTab(
  id: number,
  sourceTabId: number,
  scan: Pick<IntruderScan, "name" | "session">,
): IntruderResultTab {
  return {
    kind: "result",
    id,
    title: scan.name,
    groupId: null,
    sourceTabId,
    scanId: scan.session.id,
  };
}

export function clonePayloadWarehouse(warehouse: PayloadWarehouse): PayloadWarehouse {
  return {
    ...warehouse,
    list: { ...warehouse.list },
    numbers: { ...warehouse.numbers },
    nullPayload: { ...warehouse.nullPayload },
    bruteForce: { ...warehouse.bruteForce },
    dates: { ...warehouse.dates },
    characterSubstitution: {
      ...warehouse.characterSubstitution,
      mappings: warehouse.characterSubstitution.mappings.map((mapping) => ({ ...mapping })),
    },
    processing: warehouse.processing.map((rule) => ({ ...rule })),
  };
}

export type GeneratedPayloads = {
  payloads: string[];
  repeatIndefinitely: boolean;
};

export type PayloadRowPlan = {
  rows: string[][];
  repeatIndefinitely: boolean;
};

export function planPayloadRows(
  mode: IntruderMode,
  sets: GeneratedPayloads[],
  positionCount: number,
): PayloadRowPlan {
  if (!sets.length || sets.some((set) => !set.payloads.length)) {
    throw new Error("Each selected position must generate at least one value");
  }

  if (mode === "single") {
    const set = sets[0];
    return {
      rows: set.payloads.map((value) => [value]),
      repeatIndefinitely: set.repeatIndefinitely,
    };
  }

  if (mode === "spread") {
    const set = sets[0];
    return {
      rows: set.payloads.map((value) => Array.from({ length: positionCount }, () => value)),
      repeatIndefinitely: set.repeatIndefinitely,
    };
  }

  if (sets.length !== positionCount) {
    throw new Error("Every marked position needs a configured value set");
  }

  if (mode === "map") {
    const finiteLengths = sets
      .filter((set) => !set.repeatIndefinitely)
      .map((set) => set.payloads.length);
    const repeatIndefinitely = finiteLengths.length === 0;
    const count = repeatIndefinitely ? 1 : Math.min(...finiteLengths);
    const rows = Array.from({ length: count }, (_, index) =>
      sets.map((set) => set.payloads[set.repeatIndefinitely ? 0 : index]),
    );
    return { rows, repeatIndefinitely };
  }

  if (sets.some((set) => set.repeatIndefinitely)) {
    throw new Error("Combine mode cannot use continuous value sets");
  }
  const total = sets.reduce((count, set) => count * set.payloads.length, 1);
  if (total > MAX_INTRUDER_REQUESTS) {
    throw new Error(`The configured Combine attack produces ${total.toLocaleString()} requests; the per-run limit is ${MAX_INTRUDER_REQUESTS.toLocaleString()}`);
  }
  let rows: string[][] = [[]];
  for (const set of sets) {
    rows = rows.flatMap((row) => set.payloads.map((value) => [...row, value]));
  }
  return { rows, repeatIndefinitely: false };
}

export function payloadLines(value: string) {
  return value.split(/\r?\n/).filter((line) => line.length > 0);
}

export function generatePayloads(warehouse: PayloadWarehouse): GeneratedPayloads {
  switch (warehouse.type) {
    case "list":
      return { payloads: payloadLines(warehouse.list.text), repeatIndefinitely: false };
    case "numbers":
      return { payloads: generateNumbers(warehouse.numbers), repeatIndefinitely: false };
    case "null": {
      if (warehouse.nullPayload.mode === "infinite") {
        return { payloads: [""], repeatIndefinitely: true };
      }
      const count = positiveInteger(warehouse.nullPayload.count, "Number of null payloads");
      return { payloads: Array.from({ length: count }, () => ""), repeatIndefinitely: false };
    }
    case "bruteForce":
      return {
        payloads: generateBruteForce(
          warehouse.bruteForce.characterSet,
          warehouse.bruteForce.minLength,
          warehouse.bruteForce.maxLength,
        ),
        repeatIndefinitely: false,
      };
    case "dates":
      return { payloads: generateDates(warehouse.dates), repeatIndefinitely: false };
    case "characterSubstitution":
      return {
        payloads: generateCharacterSubstitutions(warehouse.characterSubstitution),
        repeatIndefinitely: false,
      };
  }
}

function generateNumbers(config: PayloadWarehouse["numbers"]) {
  const from = finiteNumber(config.from, "From");
  const to = finiteNumber(config.to, "To");
  const step = finiteNumber(config.step, "Step");
  if (step <= 0) throw new Error("Step must be greater than zero");
  const requestedCount = config.count.trim()
    ? positiveInteger(config.count, "How many")
    : null;
  if (config.mode === "random") {
    const count = requestedCount ?? 10;
    const direction = from <= to ? 1 : -1;
    const slots = Math.floor(Math.abs(to - from) / step) + 1;
    return Array.from({ length: count }, () => {
      const offset = Math.floor(Math.random() * slots);
      return formatNumber(from + direction * offset * step, step);
    });
  }

  const values: string[] = [];
  const direction = from <= to ? 1 : -1;
  for (
    let value = from;
    direction > 0 ? value <= to + Number.EPSILON : value >= to - Number.EPSILON;
    value += direction * step
  ) {
    values.push(formatNumber(value, step));
    if (requestedCount !== null && values.length >= requestedCount) break;
    assertPayloadLimit(values.length);
  }
  if (!values.length) throw new Error("The number range generated no payloads");
  return values;
}

function formatNumber(value: number, step: number) {
  const decimals = Math.min(12, (String(step).split(".")[1] ?? "").length);
  return decimals ? value.toFixed(decimals).replace(/\.?0+$/, "") : String(Math.round(value));
}

function generateBruteForce(characterSet: string, minValue: string, maxValue: string) {
  const characters = [...new Set(Array.from(characterSet))];
  if (!characters.length) throw new Error("Character set cannot be empty");
  const minLength = positiveInteger(minValue, "Minimum length");
  const maxLength = positiveInteger(maxValue, "Maximum length");
  if (minLength > maxLength) throw new Error("Minimum length cannot exceed maximum length");
  const estimated = Array.from(
    { length: maxLength - minLength + 1 },
    (_, index) => characters.length ** (minLength + index),
  ).reduce((sum, count) => sum + count, 0);
  if (estimated > MAX_INTRUDER_REQUESTS) {
    throw new Error(`Brute force configuration generates ${estimated.toLocaleString()} payloads; the per-run limit is ${MAX_INTRUDER_REQUESTS.toLocaleString()}`);
  }
  const values: string[] = [];
  const build = (prefix: string, remaining: number) => {
    if (remaining === 0) {
      values.push(prefix);
      return;
    }
    for (const character of characters) build(prefix + character, remaining - 1);
  };
  for (let length = minLength; length <= maxLength; length += 1) build("", length);
  return values;
}

function generateDates(config: PayloadWarehouse["dates"]) {
  const from = parseDate(config.from, "From");
  const to = parseDate(config.to, "To");
  const step = positiveInteger(config.step, "Date step");
  if (from.getTime() > to.getTime()) throw new Error("From date cannot be after To date");
  const format = config.formatMode === "custom" ? config.customFormat.trim() : config.format;
  if (!format) throw new Error("Date format cannot be empty");
  const values: string[] = [];
  let cursor = from;
  while (cursor.getTime() <= to.getTime()) {
    values.push(formatDate(cursor, format));
    assertPayloadLimit(values.length);
    cursor = advanceDate(cursor, step, config.unit);
  }
  return values;
}

function parseDate(value: string, label: string) {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value);
  if (!match) throw new Error(`${label} must be a valid date`);
  const year = Number(match[1]);
  const month = Number(match[2]) - 1;
  const day = Number(match[3]);
  const date = new Date(Date.UTC(year, month, day));
  if (
    Number.isNaN(date.getTime())
    || date.getUTCFullYear() !== year
    || date.getUTCMonth() !== month
    || date.getUTCDate() !== day
  ) {
    throw new Error(`${label} must be a valid date`);
  }
  return date;
}

function advanceDate(value: Date, step: number, unit: PayloadWarehouse["dates"]["unit"]) {
  const date = new Date(value);
  if (unit === "days") date.setUTCDate(date.getUTCDate() + step);
  if (unit === "weeks") date.setUTCDate(date.getUTCDate() + step * 7);
  if (unit === "months") date.setUTCMonth(date.getUTCMonth() + step);
  if (unit === "years") date.setUTCFullYear(date.getUTCFullYear() + step);
  return date;
}

function formatDate(value: Date, format: string) {
  const year = String(value.getUTCFullYear());
  const month = value.getUTCMonth() + 1;
  const day = value.getUTCDate();
  return format
    .replace(/yyyy/g, year)
    .replace(/YYYY/g, year)
    .replace(/yy/g, year.slice(-2))
    .replace(/YY/g, year.slice(-2))
    .replace(/MM/g, String(month).padStart(2, "0"))
    .replace(/dd/g, String(day).padStart(2, "0"))
    .replace(/DD/g, String(day).padStart(2, "0"))
    .replace(/M/g, String(month))
    .replace(/D/g, String(day));
}

function generateCharacterSubstitutions(config: PayloadWarehouse["characterSubstitution"]) {
  const items = payloadLines(config.itemsText);
  const mappings = config.mappings.filter((mapping) => mapping.from && mapping.to);
  if (!items.length) return [];
  if (!mappings.length) return items;
  const values = new Set<string>();
  for (const item of items) {
    let variants = [""];
    for (const character of item) {
      const replacements = mappings
        .filter((mapping) => config.caseSensitive
          ? mapping.from === character
          : mapping.from.toLocaleLowerCase() === character.toLocaleLowerCase())
        .map((mapping) => mapping.to);
      const choices = [...new Set([character, ...replacements])];
      variants = variants.flatMap((prefix) => choices.map((choice) => prefix + choice));
      assertPayloadLimit(values.size + variants.length);
    }
    variants.forEach((variant) => values.add(variant));
    assertPayloadLimit(values.size);
  }
  return [...values];
}

export async function processPayloads(
  payloads: string[],
  rules: PayloadProcessingRule[],
) {
  const enabled = rules.filter((rule) => rule.enabled);
  const processed: string[] = [];
  for (const payload of payloads) {
    let value = payload;
    for (const rule of enabled) value = await applyProcessingRule(value, rule);
    processed.push(value);
  }
  return processed;
}

async function applyProcessingRule(value: string, rule: PayloadProcessingRule) {
  switch (rule.type) {
    case "addPrefix":
      return rule.value + value;
    case "addSuffix":
      return value + rule.value;
    case "matchReplace": {
      if (!rule.match) return value;
      const source = rule.useRegex ? rule.match : escapeRegExp(rule.match);
      const flags = `g${rule.caseSensitive ? "" : "i"}`;
      return value.replace(new RegExp(source, flags), rule.replacement);
    }
    case "substring": {
      const start = nonNegativeInteger(rule.start, "Substring start");
      const length = positiveInteger(rule.length, "Substring length");
      return value.slice(start, start + length);
    }
    case "reverseSubstring": {
      const start = nonNegativeInteger(rule.start, "Reverse-substring start");
      const length = positiveInteger(rule.length, "Reverse-substring length");
      return value.slice(0, start)
        + Array.from(value.slice(start, start + length)).reverse().join("")
        + value.slice(start + length);
    }
    case "modifyCase":
      if (rule.operation === "lower") return value.toLocaleLowerCase();
      if (rule.operation === "capitalize") return value.replace(/\b\p{L}/gu, (letter) => letter.toLocaleUpperCase());
      return value.toLocaleUpperCase();
    case "encode":
      return encodeValue(value, rule.operation);
    case "decode":
      return decodeValue(value, rule.operation);
    case "hash":
      return hashValue(value, rule.operation);
  }
}

function encodeValue(value: string, operation: string) {
  const bytes = new TextEncoder().encode(value);
  if (operation === "hex") return [...bytes].map((byte) => byte.toString(16).padStart(2, "0")).join("");
  if (operation === "base64") return bytesToBase64(bytes);
  return encodeURIComponent(value);
}

function decodeValue(value: string, operation: string) {
  try {
    if (operation === "hex") {
      if (!/^(?:[0-9a-f]{2})*$/i.test(value)) throw new Error("invalid hexadecimal input");
      return new TextDecoder().decode(Uint8Array.from(value.match(/.{2}/g) ?? [], (byte) => parseInt(byte, 16)));
    }
    if (operation === "base64") return new TextDecoder().decode(base64ToBytes(value));
    return decodeURIComponent(value);
  } catch (reason) {
    throw new Error(`Payload decode failed: ${reason instanceof Error ? reason.message : String(reason)}`);
  }
}

async function hashValue(value: string, operation: string) {
  const algorithm = operation === "sha1" ? "SHA-1" : operation === "sha512" ? "SHA-512" : "SHA-256";
  const digest = await crypto.subtle.digest(algorithm, new TextEncoder().encode(value));
  return [...new Uint8Array(digest)].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

function bytesToBase64(bytes: Uint8Array) {
  let binary = "";
  bytes.forEach((byte) => (binary += String.fromCharCode(byte)));
  return btoa(binary);
}

function base64ToBytes(value: string) {
  return Uint8Array.from(atob(value), (character) => character.charCodeAt(0));
}

function finiteNumber(value: string, label: string) {
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) throw new Error(`${label} must be a number`);
  return parsed;
}

function positiveInteger(value: string, label: string) {
  const parsed = Number(value);
  if (!Number.isInteger(parsed) || parsed <= 0) throw new Error(`${label} must be a positive integer`);
  if (parsed > MAX_INTRUDER_REQUESTS) throw new Error(`${label} cannot exceed ${MAX_INTRUDER_REQUESTS.toLocaleString()}`);
  return parsed;
}

function nonNegativeInteger(value: string, label: string) {
  const parsed = Number(value);
  if (!Number.isInteger(parsed) || parsed < 0) throw new Error(`${label} must be zero or greater`);
  return parsed;
}

function assertPayloadLimit(count: number) {
  if (count > MAX_INTRUDER_REQUESTS) {
    throw new Error(`Payload generation exceeds the ${MAX_INTRUDER_REQUESTS.toLocaleString()} payload limit`);
  }
}

function escapeRegExp(value: string) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

export type TestPosition = {
  open: number;
  close: number;
  original: string;
};

export function findTestPositions(template: string): TestPosition[] {
  const positions: TestPosition[] = [];
  let cursor = 0;
  while (cursor < template.length) {
    const open = template.indexOf("§", cursor);
    if (open < 0) break;
    const close = template.indexOf("§", open + 1);
    if (close < 0) throw new Error("Unmatched § value marker");
    positions.push({
      open,
      close,
      original: template.slice(open + 1, close),
    });
    cursor = close + 1;
  }
  return positions;
}

export function requestHost(raw: Uint8Array) {
  return parseRequestHost(raw);
}

export function requestHostText(value: string) {
  return parseRequestHostText(value);
}

export function renderTestRequest(
  template: string,
  positions: TestPosition[],
  targetPosition: number,
  value: string,
) {
  let rendered = "";
  let cursor = 0;
  positions.forEach((position, index) => {
    rendered += template.slice(cursor, position.open);
    rendered += index === targetPosition ? value : position.original;
    cursor = position.close + 1;
  });
  rendered += template.slice(cursor);
  return synchronizeContentLength(rendered);
}

type RangeBuilder = {
  source: string;
  parts: string[];
  length: number;
  fromMap: (number | undefined)[];
  toMap: (number | undefined)[];
};

export type RenderedTestRequest = {
  value: string;
  ranges: IntruderRange[];
};

function createRangeBuilder(source: string): RangeBuilder {
  return {
    source,
    parts: [],
    length: 0,
    fromMap: Array.from({ length: source.length + 1 }),
    toMap: Array.from({ length: source.length + 1 }),
  };
}

function appendMappedSegment(
  builder: RangeBuilder,
  sourceStart: number,
  sourceEnd: number,
  replacement: string,
) {
  const sourcePart = builder.source.slice(sourceStart, sourceEnd);
  const outputStart = builder.length;
  let prefix = 0;
  while (
    prefix < sourcePart.length
    && prefix < replacement.length
    && sourcePart[prefix] === replacement[prefix]
  ) {
    prefix += 1;
  }
  let suffix = 0;
  while (
    suffix < sourcePart.length - prefix
    && suffix < replacement.length - prefix
    && sourcePart[sourcePart.length - suffix - 1] === replacement[replacement.length - suffix - 1]
  ) {
    suffix += 1;
  }
  const sourceChangeEnd = sourcePart.length - suffix;
  const replacementChangeEnd = replacement.length - suffix;
  const mapOffset = (offset: number, fromBoundary: boolean) => {
    if (offset <= prefix) return outputStart + offset;
    if (offset >= sourceChangeEnd) {
      return outputStart + replacement.length - (sourcePart.length - offset);
    }
    return outputStart + (fromBoundary ? prefix : replacementChangeEnd);
  };

  builder.parts.push(replacement);
  builder.length += replacement.length;
  for (let offset = 0; offset <= sourcePart.length; offset += 1) {
    const sourceOffset = sourceStart + offset;
    if (builder.fromMap[sourceOffset] === undefined) {
      builder.fromMap[sourceOffset] = mapOffset(offset, true);
    }
    if (builder.toMap[sourceOffset] === undefined) {
      builder.toMap[sourceOffset] = mapOffset(offset, false);
    }
  }
}

function appendInsertion(builder: RangeBuilder, sourceOffset: number, value: string) {
  const outputStart = builder.length;
  builder.parts.push(value);
  builder.length += value.length;
  builder.toMap[sourceOffset] ??= outputStart;
  builder.fromMap[sourceOffset] = builder.length;
}

function mapBuiltRanges(
  ranges: IntruderRange[],
  builder: RangeBuilder,
): IntruderRange[] {
  return ranges.flatMap((range) => {
    const from = Math.max(0, Math.min(builder.length, builder.fromMap[range.from] ?? 0));
    const to = Math.max(0, Math.min(builder.length, builder.toMap[range.to] ?? builder.length));
    return from < to ? [{ from, to }] : [];
  });
}

function normalizeHttpLineEndingsWithRanges(
  value: string,
  ranges: IntruderRange[],
): RenderedTestRequest {
  const builder = createRangeBuilder(value);
  let cursor = 0;
  while (cursor < value.length) {
    if (value[cursor] !== "\r" && value[cursor] !== "\n") {
      const carriageReturn = value.indexOf("\r", cursor);
      const lineFeed = value.indexOf("\n", cursor);
      const nextBreak = carriageReturn < 0
        ? lineFeed
        : lineFeed < 0
          ? carriageReturn
          : Math.min(carriageReturn, lineFeed);
      const end = nextBreak < 0 ? value.length : nextBreak;
      appendMappedSegment(builder, cursor, end, value.slice(cursor, end));
      cursor = end;
      continue;
    }
    const end = value[cursor] === "\r" && value[cursor + 1] === "\n"
      ? cursor + 2
      : cursor + 1;
    appendMappedSegment(builder, cursor, end, "\n");
    cursor = end;
  }
  return {
    value: normalizeHttpLineEndings(value),
    ranges: mapBuiltRanges(ranges, builder),
  };
}

function mapRangesThroughTextEdit(
  source: string,
  target: string,
  ranges: IntruderRange[],
): IntruderRange[] {
  if (source === target) return ranges.map((range) => ({ ...range }));
  let prefix = 0;
  while (prefix < source.length && prefix < target.length && source[prefix] === target[prefix]) {
    prefix += 1;
  }
  let suffix = 0;
  while (
    suffix < source.length - prefix
    && suffix < target.length - prefix
    && source[source.length - suffix - 1] === target[target.length - suffix - 1]
  ) {
    suffix += 1;
  }
  const sourceChangeEnd = source.length - suffix;
  const targetChangeEnd = target.length - suffix;
  const mapOffset = (offset: number, fromBoundary: boolean) => {
    if (offset <= prefix) return offset;
    if (offset >= sourceChangeEnd) return target.length - (source.length - offset);
    return fromBoundary ? prefix : targetChangeEnd;
  };
  return ranges.flatMap((range) => {
    const from = Math.max(0, Math.min(target.length, mapOffset(range.from, true)));
    const to = Math.max(0, Math.min(target.length, mapOffset(range.to, false)));
    return from < to ? [{ from, to }] : [];
  });
}

function synchronizeContentLengthWithRanges(
  value: string,
  ranges: IntruderRange[],
): RenderedTestRequest {
  const separator = /\r?\n\r?\n/.exec(value);
  if (!separator || separator.index === undefined) {
    return { value, ranges: ranges.map((range) => ({ ...range })) };
  }
  const separatorStart = separator.index;
  const separatorEnd = separatorStart + separator[0].length;
  const head = value.slice(0, separatorStart);
  const body = value.slice(separatorEnd);
  const lines = head.split(/\r?\n/);
  const chunked = lines.some((line) => {
    const colon = line.indexOf(":");
    return colon > 0
      && line.slice(0, colon).trim().toLowerCase() === "transfer-encoding"
      && line.slice(colon + 1).split(",").some((item) => item.trim().toLowerCase() === "chunked");
  });
  if (chunked) throw new Error("Chunked request bodies are not supported here yet");

  const bodyLength = new TextEncoder().encode(body).length;
  let hasLength = false;
  const builder = createRangeBuilder(value);
  let cursor = 0;
  while (cursor < separatorStart) {
    const lineBreak = /\r\n|\n|\r/.exec(value.slice(cursor, separatorStart));
    const lineEnd = lineBreak ? cursor + lineBreak.index : separatorStart;
    const line = value.slice(cursor, lineEnd);
    const colon = line.indexOf(":");
    let outputLine = line;
    if (colon > 0 && line.slice(0, colon).trim().toLowerCase() === "content-length") {
      hasLength = true;
      outputLine = `${line.slice(0, colon)}: ${bodyLength}`;
    }
    appendMappedSegment(builder, cursor, lineEnd, outputLine);
    cursor = lineEnd;
    if (!lineBreak) break;
    const breakEnd = lineEnd + lineBreak[0].length;
    appendMappedSegment(builder, lineEnd, breakEnd, "\r\n");
    cursor = breakEnd;
  }
  if (!hasLength && body.length > 0) {
    appendInsertion(builder, separatorStart, `Content-Length: ${bodyLength}\r\n`);
  }
  appendMappedSegment(builder, separatorStart, separatorEnd, "\r\n\r\n");
  appendMappedSegment(builder, separatorEnd, value.length, body);
  return {
    value: builder.parts.join(""),
    ranges: mapBuiltRanges(ranges, builder),
  };
}

export function renderTestRequestValuesWithRanges(
  template: string,
  positions: TestPosition[],
  values: string[],
): RenderedTestRequest {
  if (positions.length !== values.length) {
    throw new Error("The number of supplied values does not match the marked positions");
  }
  let rendered = "";
  let cursor = 0;
  const ranges: IntruderRange[] = [];
  positions.forEach((position, index) => {
    rendered += template.slice(cursor, position.open);
    const from = rendered.length;
    rendered += values[index];
    ranges.push({ from, to: rendered.length });
    cursor = position.close + 1;
  });
  rendered += template.slice(cursor);
  return synchronizeContentLengthWithRanges(rendered, ranges);
}

export function finalizeRenderedTestRequest(rendered: RenderedTestRequest): RenderedTestRequest {
  if (!rendered.value) return { value: rendered.value, ranges: [] };
  const normalized = normalizeHttpLineEndingsWithRanges(rendered.value, rendered.ranges);
  const completed = splitHttpMessage(normalized.value).complete
    ? normalized.value
    : normalized.value.endsWith("\n") ? `${normalized.value}\n` : `${normalized.value}\n\n`;
  const completedRanges = mapRangesThroughTextEdit(normalized.value, completed, normalized.ranges);
  const finalized = synchronizeHttpContentLength(completed);
  return {
    value: finalized,
    ranges: mapRangesThroughTextEdit(completed, finalized, completedRanges),
  };
}

export function renderTestRequestValues(
  template: string,
  positions: TestPosition[],
  values: string[],
) {
  return renderTestRequestValuesWithRanges(template, positions, values).value;
}

export function synchronizeContentLength(value: string) {
  // Canonical Content-Length sync lives in $lib/http-message.
  // The range-preserving variant (synchronizeContentLengthWithRanges) stays here.
  return synchronizeHttpContentLength(value);
}
