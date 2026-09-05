import { RangeSetBuilder, type Text } from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  EditorView,
  ViewPlugin,
  type ViewUpdate,
} from "@codemirror/view";
import { HTTP_METADATA_LIMIT, isJsonMessage, splitHttpLines, splitHttpMessage } from "$lib/http-message";

export type HttpMessageKind = "request" | "response";

type HighlightRange = {
  from: number;
  to: number;
  decoration: Decoration;
};

type HighlightInfo = {
  headerEnd: number;
  bodyStart: number | null;
  jsonBody: boolean;
  multipartBoundary: string | null;
};

const MAX_HIGHLIGHT_RANGES = 15_000;
const HEADER_NAME = /^[!#$%&'*+.^_`|~0-9A-Za-z-]+$/;
const JSON_NUMBER = /^-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?/;

const methodDecoration = Decoration.mark({ class: "cm-http-method" });
const targetDecoration = Decoration.mark({ class: "cm-http-target" });
const protocolDecoration = Decoration.mark({ class: "cm-http-protocol" });
const statusSuccessDecoration = Decoration.mark({ class: "cm-http-status-success" });
const statusRedirectDecoration = Decoration.mark({ class: "cm-http-status-redirect" });
const statusClientErrorDecoration = Decoration.mark({ class: "cm-http-status-client-error" });
const statusServerErrorDecoration = Decoration.mark({ class: "cm-http-status-server-error" });
const statusReasonDecoration = Decoration.mark({ class: "cm-http-status-reason" });
const headerNameDecoration = Decoration.mark({ class: "cm-http-header-name" });
const importantHeaderNameDecoration = Decoration.mark({ class: "cm-http-header-name cm-http-header-important" });
const headerDelimiterDecoration = Decoration.mark({ class: "cm-http-header-delimiter" });
const headerValueDecoration = Decoration.mark({ class: "cm-http-header-value" });
const requestHeaderValueDecoration = Decoration.mark({ class: "cm-http-request-header-value" });
const jsonKeyDecoration = Decoration.mark({ class: "cm-http-json-key" });
const jsonStringDecoration = Decoration.mark({ class: "cm-http-json-string" });
const jsonNumberDecoration = Decoration.mark({ class: "cm-http-json-number" });
const jsonBooleanDecoration = Decoration.mark({ class: "cm-http-json-boolean" });
const jsonNullDecoration = Decoration.mark({ class: "cm-http-json-null" });
const jsonPunctuationDecoration = Decoration.mark({ class: "cm-http-json-punctuation" });
const multipartBoundaryDecoration = Decoration.mark({ class: "cm-http-multipart-boundary" });
const multipartValueDecoration = Decoration.mark({ class: "cm-http-multipart-value" });

const importantHeaders = new Set([
  "authorization",
  "content-encoding",
  "content-length",
  "content-type",
  "cookie",
  "location",
  "set-cookie",
  "transfer-encoding",
]);

const multipartHeaderNames = new Set([
  "content-disposition",
  "content-type",
  "content-transfer-encoding",
  "content-id",
  "content-location",
  "content-description",
]);

export function createHttpHighlightPlugin(kind: HttpMessageKind) {
  return ViewPlugin.fromClass(class {
    decorations: DecorationSet;
    info: HighlightInfo;

    constructor(view: EditorView) {
      this.info = analyzeDocument(view.state.doc);
      this.decorations = buildDecorations(view, this.info, kind);
    }

    update(update: ViewUpdate) {
      if (update.docChanged) this.info = analyzeDocument(update.state.doc);
      if (update.docChanged || update.viewportChanged) {
        this.decorations = buildDecorations(update.view, this.info, kind);
      }
    }
  }, {
    decorations: (value) => value.decorations,
  });
}

function analyzeDocument(doc: Text): HighlightInfo {
  const source = doc.sliceString(0, Math.min(doc.length, HTTP_METADATA_LIMIT));
  const message = splitHttpMessage(source);
  const bodyStart = message.complete ? message.head.length + message.separator.length : null;

  return {
    headerEnd: bodyStart ?? source.length,
    bodyStart,
    jsonBody: message.complete && isJsonMessage(message),
    multipartBoundary: message.complete ? findMultipartBoundary(message.head) : null,
  };
}

function buildDecorations(view: EditorView, info: HighlightInfo, kind: HttpMessageKind) {
  const ranges: HighlightRange[] = [];
  const seenLines = new Set<number>();
  const document = view.state.doc;

  for (const viewport of view.visibleRanges) {
    const firstLine = document.lineAt(viewport.from).number;
    const lastPosition = Math.max(viewport.from, viewport.to - (viewport.to > viewport.from ? 1 : 0));
    const lastLine = document.lineAt(lastPosition).number;

    for (let lineNumber = firstLine; lineNumber <= lastLine; lineNumber += 1) {
      if (seenLines.has(lineNumber)) continue;
      seenLines.add(lineNumber);
      addLineDecorations(ranges, document.line(lineNumber), info, kind);
      if (ranges.length >= MAX_HIGHLIGHT_RANGES) break;
    }
    if (ranges.length >= MAX_HIGHLIGHT_RANGES) break;
  }

  ranges.sort((left, right) => left.from - right.from || left.to - right.to);
  const builder = new RangeSetBuilder<Decoration>();
  for (const range of ranges) builder.add(range.from, range.to, range.decoration);
  return builder.finish();
}

function addLineDecorations(
  ranges: HighlightRange[],
  line: { from: number; to: number; text: string; number: number },
  info: HighlightInfo,
  kind: HttpMessageKind,
) {
  const text = line.text.endsWith("\r") ? line.text.slice(0, -1) : line.text;
  if (line.number === 1) addStartLineDecorations(ranges, line.from, text, kind);

  if (line.number > 1 && line.from < info.headerEnd) {
    addHeaderLineDecorations(ranges, line.from, text, kind);
  }

  if (info.jsonBody && info.bodyStart !== null && line.to >= info.bodyStart) {
    const bodyOffset = Math.max(0, info.bodyStart - line.from);
    addJsonDecorations(ranges, line.from + bodyOffset, text.slice(bodyOffset));
  } else if (info.multipartBoundary && info.bodyStart !== null && line.to >= info.bodyStart) {
    const bodyOffset = Math.max(0, info.bodyStart - line.from);
    addMultipartDecorations(ranges, line.from + bodyOffset, text.slice(bodyOffset), info.multipartBoundary, kind);
  }
}

function addStartLineDecorations(
  ranges: HighlightRange[],
  lineFrom: number,
  text: string,
  kind: HttpMessageKind,
) {
  const match = kind === "request"
    ? /^(\s*)([^\s]+)(\s+)(\S+)(\s+)(HTTP\/\S+)(.*)$/.exec(text)
    : /^(\s*)(HTTP\/\S+)(\s+)(\d{3})(\s*)(.*)$/.exec(text);
  if (!match) return;

  const offsets = captureOffsets(match);
  if (kind === "request") {
    addRange(ranges, lineFrom + offsets[2], lineFrom + offsets[2] + match[2].length, methodDecoration);
    addRange(ranges, lineFrom + offsets[4], lineFrom + offsets[4] + match[4].length, targetDecoration);
    addRange(ranges, lineFrom + offsets[6], lineFrom + offsets[6] + match[6].length, protocolDecoration);
    return;
  }

  addRange(ranges, lineFrom + offsets[2], lineFrom + offsets[2] + match[2].length, protocolDecoration);
  const status = Number(match[4]);
  const statusDecoration = status >= 500
    ? statusServerErrorDecoration
    : status >= 400
      ? statusClientErrorDecoration
      : status >= 300
        ? statusRedirectDecoration
        : status >= 200
          ? statusSuccessDecoration
          : protocolDecoration;
  addRange(ranges, lineFrom + offsets[4], lineFrom + offsets[4] + match[4].length, statusDecoration);
  if (match[6]) {
    addRange(ranges, lineFrom + offsets[6], lineFrom + offsets[6] + match[6].length, statusReasonDecoration);
  }
}

function addHeaderLineDecorations(
  ranges: HighlightRange[],
  lineFrom: number,
  text: string,
  kind: HttpMessageKind,
) {
  const match = /^([^\s:][^:]*?)([ \t]*:[ \t]*)(.*)$/.exec(text);
  if (match && HEADER_NAME.test(match[1])) {
    const offsets = captureOffsets(match);
    const nameDecoration = importantHeaders.has(match[1].toLowerCase())
      ? importantHeaderNameDecoration
      : headerNameDecoration;
    addRange(ranges, lineFrom + offsets[1], lineFrom + offsets[1] + match[1].length, nameDecoration);
    addRange(ranges, lineFrom + offsets[2], lineFrom + offsets[2] + match[2].length, headerDelimiterDecoration);
    if (match[3]) {
      const valueDecoration = kind === "request" ? requestHeaderValueDecoration : headerValueDecoration;
      addRange(ranges, lineFrom + offsets[3], lineFrom + offsets[3] + match[3].length, valueDecoration);
    }
    return;
  }

  // HTTP folded headers and incomplete header edits are still useful to read as values.
  const continuation = /^[ \t]+.+$/.test(text);
  if (continuation) {
    const valueDecoration = kind === "request" ? requestHeaderValueDecoration : headerValueDecoration;
    addRange(ranges, lineFrom, lineFrom + text.length, valueDecoration);
  }
}

function addJsonDecorations(ranges: HighlightRange[], lineFrom: number, text: string) {
  let index = 0;
  while (index < text.length && ranges.length < MAX_HIGHLIGHT_RANGES) {
    const character = text[index];
    if (character === '"') {
      let end = index + 1;
      let escaped = false;
      while (end < text.length) {
        const current = text[end];
        if (escaped) escaped = false;
        else if (current === "\\") escaped = true;
        else if (current === '"') {
          end += 1;
          break;
        }
        end += 1;
      }
      const nextSignificant = text.slice(end).search(/\S/);
      const isKey = nextSignificant >= 0 && text[end + nextSignificant] === ":";
      addRange(
        ranges,
        lineFrom + index,
        lineFrom + end,
        isKey ? jsonKeyDecoration : jsonStringDecoration,
      );
      index = end;
      continue;
    }

    if ("{}[],:".includes(character)) {
      addRange(ranges, lineFrom + index, lineFrom + index + 1, jsonPunctuationDecoration);
      index += 1;
      continue;
    }

    if (text.startsWith("true", index) || text.startsWith("false", index)) {
      const value = text.startsWith("true", index) ? "true" : "false";
      addRange(ranges, lineFrom + index, lineFrom + index + value.length, jsonBooleanDecoration);
      index += value.length;
      continue;
    }

    if (text.startsWith("null", index)) {
      addRange(ranges, lineFrom + index, lineFrom + index + 4, jsonNullDecoration);
      index += 4;
      continue;
    }

    if (character === "-" || /\d/.test(character)) {
      const number = JSON_NUMBER.exec(text.slice(index));
      if (number) {
        addRange(ranges, lineFrom + index, lineFrom + index + number[0].length, jsonNumberDecoration);
        index += number[0].length;
        continue;
      }
    }

    index += 1;
  }
}

function captureOffsets(match: RegExpExecArray) {
  const offsets: number[] = [0];
  let cursor = 0;
  for (let index = 1; index < match.length; index += 1) {
    const value = match[index] ?? "";
    const offset = value ? match[0].indexOf(value, cursor) : cursor;
    offsets[index] = offset < 0 ? cursor : offset;
    cursor = offsets[index] + value.length;
  }
  return offsets;
}

function addRange(
  ranges: HighlightRange[],
  from: number,
  to: number,
  decoration: Decoration,
) {
  if (to > from) ranges.push({ from, to, decoration });
}

function findMultipartBoundary(head: string) {
  const contentTypeLine = splitHttpLines(head).find((line) => /^content-type\s*:/i.test(line));
  if (!contentTypeLine) return null;
  const value = contentTypeLine.slice(contentTypeLine.indexOf(":") + 1);
  if (!/^\s*multipart\//i.test(value)) return null;
  const match = /(?:^|;)\s*boundary\s*=\s*(?:"([^"]+)"|([^;\s]+))/i.exec(value);
  return match?.[1] ?? match?.[2] ?? null;
}

function addMultipartDecorations(
  ranges: HighlightRange[],
  lineFrom: number,
  text: string,
  boundary: string,
  kind: HttpMessageKind,
) {
  const boundaryMatch = new RegExp(`^--${escapeRegExp(boundary)}(--)?\\s*$`).exec(text);
  if (boundaryMatch) {
    addRange(ranges, lineFrom, lineFrom + text.length, multipartBoundaryDecoration);
    return;
  }

  const headerMatch = /^([^\s:][^:]*?)([ \t]*:[ \t]*)(.*)$/.exec(text);
  if (headerMatch && multipartHeaderNames.has(headerMatch[1].toLowerCase())) {
    addHeaderLineDecorations(ranges, lineFrom, text, kind);
    return;
  }

  if (text.trim()) addRange(ranges, lineFrom, lineFrom + text.length, multipartValueDecoration);
}

function escapeRegExp(value: string) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
