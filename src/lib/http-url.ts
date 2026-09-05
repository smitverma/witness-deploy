export const HTTP_METADATA_LIMIT = 16_384;

const textDecoder = new TextDecoder();

export type RequestMetadata = { method: string; url: string; host: string };

export type RequestDetails = {
  method: string;
  url: string;
  headers: string[];
  body: string;
};

/** Extract hostname from a Host header value, URL, or host[:port]. */
export function parseHostname(value: string): string {
  if (!value) return "";
  try {
    return new URL(value.includes("://") ? value : `http://${value}`).hostname;
  } catch {
    return value.replace(/:\d+$/, "");
  }
}

/** Single host extractor from raw request text (header prefix only). */
export function requestHostText(value: string): string {
  const head = value.slice(0, HTTP_METADATA_LIMIT).split(/\r?\n\r?\n/, 1)[0] ?? "";
  const lines = head.split(/\r?\n/);
  const target = (lines[0] ?? "").split(/\s+/, 3)[1] ?? "";
  const host =
    lines.find((line) => line.toLowerCase().startsWith("host:"))?.slice(5).trim() || target;
  if (!host) return "";
  return parseHostname(host);
}

/** Host extractor from raw request bytes (header prefix only). */
export function requestHost(raw: Uint8Array): string {
  return requestHostText(textDecoder.decode(raw.slice(0, HTTP_METADATA_LIMIT)));
}

/** Parse request line + Host into {method, url, host}. */
export function parseRequestMetadataText(value: string, suppliedUrl = ""): RequestMetadata {
  const head = value.slice(0, HTTP_METADATA_LIMIT).split(/\r?\n\r?\n/, 1)[0] ?? "";
  const lines = head.split(/\r?\n/);
  const [method = "—", target = "/"] = (lines[0] ?? "").split(/\s+/, 3);
  const hostHeader =
    lines.find((line) => line.toLowerCase().startsWith("host:"))?.slice(5).trim() ?? "";
  const url =
    suppliedUrl ||
    (target.startsWith("http://") || target.startsWith("https://")
      ? target
      : `${hostHeader ? `https://${hostHeader}` : ""}${target}`);
  return { method, url, host: parseHostname(hostHeader || url) };
}

/** Parse request metadata from raw bytes. */
export function parseRequestMetadata(raw: Uint8Array, suppliedUrl = ""): RequestMetadata {
  return parseRequestMetadataText(
    textDecoder.decode(raw.slice(0, HTTP_METADATA_LIMIT)),
    suppliedUrl,
  );
}

/**
 * Parse editable request text into details for Copy URL / cURL.
 * Returns null for non-request payloads.
 */
export function parseRequestDetails(text: string): RequestDetails | null {
  const [head, ...bodyParts] = text.split(/\r?\n\r?\n/);
  const lines = head.split(/\r?\n/);
  const [method = "", target = ""] = (lines.shift() ?? "").trim().split(/\s+/, 3);
  if (!/^[A-Z]+$/.test(method) || !target) return null;

  const host = lines.find((line) => line.toLowerCase().startsWith("host:"))?.slice(5).trim() ?? "";
  const forwardedProtocol = lines
    .find((line) => line.toLowerCase().startsWith("x-forwarded-proto:"))
    ?.slice(18)
    .trim();
  let url = "";
  if (target.startsWith("http://") || target.startsWith("https://")) {
    try {
      url = new URL(target).toString();
    } catch {
      return null;
    }
  } else if (host) {
    const path = target.startsWith("/") ? target : `/${target}`;
    url = `${forwardedProtocol === "http" ? "http" : "https"}://${host}${path}`;
  }
  return { method, url, headers: lines, body: bodyParts.join("\r\n\r\n") };
}

/** @deprecated Use parseHostname instead. */
export function hostname(value: string): string {
  return parseHostname(value);
}
