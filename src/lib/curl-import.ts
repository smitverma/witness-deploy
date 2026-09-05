export type CurlImport = {
  raw: Uint8Array;
  tls: boolean;
};

export function curlToHttpRequest(command: string): CurlImport {
  const tokens = tokenize(command);
  if (tokens.shift() !== "curl") throw new Error("Paste a cURL command starting with curl");

  let url = "";
  let method = "";
  let hasData = false;
  const headers: string[] = [];
  const data: string[] = [];

  for (let index = 0; index < tokens.length; index += 1) {
    const token = tokens[index];
    const takeValue = (option: string) => {
      const value = tokens[++index];
      if (value === undefined) throw new Error(`${option} requires a value`);
      return value;
    };
    const addHeader = (value: string) => {
      if (!value.includes(":")) throw new Error("Each cURL header must include a colon");
      if (/\r|\n/.test(value)) throw new Error("cURL headers cannot contain line breaks");
      headers.push(value);
    };
    const addData = (value: string, fileReference: boolean) => {
      if (fileReference && value.startsWith("@")) {
        throw new Error("File-backed cURL data is not supported; paste the request body instead");
      }
      data.push(value);
      hasData = true;
    };

    if (token === "--") {
      if (!url) url = takeValue("--");
      continue;
    }
    if (token === "-X" || token === "--request") { method = takeValue(token); continue; }
    if (token.startsWith("--request=")) { method = token.slice(10); continue; }
    if (token.startsWith("-X") && token.length > 2) { method = token.slice(2); continue; }
    if (token === "-H" || token === "--header") { addHeader(takeValue(token)); continue; }
    if (token.startsWith("--header=")) { addHeader(token.slice(9)); continue; }
    if (token.startsWith("-H") && token.length > 2) { addHeader(token.slice(2)); continue; }
    if (token === "--url") { url = takeValue(token); continue; }
    if (token.startsWith("--url=")) { url = token.slice(6); continue; }
    if (token === "-d" || token === "--data" || token === "--data-binary") { addData(takeValue(token), true); continue; }
    if (token === "--data-raw" || token === "--data-ascii") { addData(takeValue(token), false); continue; }
    if (token.startsWith("--data=")) { addData(token.slice(7), true); continue; }
    if (token.startsWith("--data-binary=")) { addData(token.slice(14), true); continue; }
    if (token.startsWith("--data-raw=")) { addData(token.slice(11), false); continue; }
    if (token.startsWith("--data-ascii=")) { addData(token.slice(13), false); continue; }
    if (token.startsWith("-d") && token.length > 2) { addData(token.slice(2), true); continue; }
    if (token === "-I" || token === "--head") { method = "HEAD"; continue; }
    if (/^-[sSkLiv]+$/.test(token)
      || ["--silent", "--show-error", "--insecure", "--location", "--include", "--compressed", "--fail", "--verbose", "--globoff", "--http1.1"].includes(token)) continue;
    if (token === "-F" || token === "--form" || token === "--next") {
      throw new Error(`${token} is not supported; import a raw .http request instead`);
    }
    if (token.startsWith("-")) throw new Error(`Unsupported cURL option: ${token}`);
    if (url) throw new Error("A cURL import can contain only one URL");
    url = token;
  }

  if (!url) throw new Error("The cURL command must include an http or https URL");
  let parsedUrl: URL;
  try {
    parsedUrl = new URL(url);
  } catch {
    throw new Error("The cURL URL is invalid");
  }
  if (parsedUrl.protocol !== "http:" && parsedUrl.protocol !== "https:") {
    throw new Error("Only http and https cURL URLs can be imported");
  }
  if (method && !/^[A-Za-z]+$/.test(method)) throw new Error("The cURL request method is invalid");

  const body = data.join("&");
  const hasHeader = (name: string) => headers.some((header) => header.slice(0, header.indexOf(":")).trim().toLowerCase() === name);
  const normalizedHeaders = headers.filter((header) => !header.toLowerCase().startsWith("content-length:"));
  if (!hasHeader("host")) normalizedHeaders.push(`Host: ${parsedUrl.host}`);
  if (hasData && !hasHeader("content-type")) normalizedHeaders.push("Content-Type: application/x-www-form-urlencoded");
  if (hasData && !hasHeader("transfer-encoding")) normalizedHeaders.push(`Content-Length: ${new TextEncoder().encode(body).length}`);

  const target = `${parsedUrl.pathname || "/"}${parsedUrl.search}`;
  const raw = `${method.toUpperCase() || (hasData ? "POST" : "GET")} ${target} HTTP/1.1\r\n${normalizedHeaders.join("\r\n")}\r\n\r\n${body}`;
  return { raw: new TextEncoder().encode(raw), tls: parsedUrl.protocol === "https:" };
}

function tokenize(command: string) {
  const tokens: string[] = [];
  let token = "";
  let quote: "single" | "double" | null = null;
  let started = false;

  const pushToken = () => {
    if (started) tokens.push(token);
    token = "";
    started = false;
  };

  for (let index = 0; index < command.length; index += 1) {
    const character = command[index];
    if (quote === "single") {
      if (character === "'") quote = null;
      else token += character;
      started = true;
      continue;
    }
    if (quote === "double") {
      if (character === '"') {
        quote = null;
      } else if (character === "\\") {
        const next = command[++index];
        if (next === undefined) throw new Error("The cURL command ends with an escape character");
        if (next === "\n") continue;
        token += next === '"' || next === "\\" || next === "$" || next === "`" ? next : `\\${next}`;
      } else {
        token += character;
      }
      started = true;
      continue;
    }
    if (/\s/.test(character)) {
      pushToken();
    } else if (character === "'") {
      quote = "single";
      started = true;
    } else if (character === '"') {
      quote = "double";
      started = true;
    } else if (character === "\\") {
      const next = command[++index];
      if (next === undefined) throw new Error("The cURL command ends with an escape character");
      if (next !== "\n") token += next;
      started = true;
    } else {
      token += character;
      started = true;
    }
  }

  if (quote) throw new Error("The cURL command contains an unterminated quote");
  pushToken();
  return tokens;
}
