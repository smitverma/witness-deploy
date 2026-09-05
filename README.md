# Witness

![Status](https://img.shields.io/badge/status-beta-f59e0b)
![Rust](https://img.shields.io/badge/core-Rust-b7410e)
![Tauri](https://img.shields.io/badge/desktop-Tauri-24c8db)
![License](https://img.shields.io/badge/license-MIT-34d399)

Witness is a local-first desktop toolkit for authorized web-security testing. It combines a native intercepting proxy, HTTPS inspection, persistent projects, searchable traffic history, request replay, request fuzzing, identity rotation, decoding, comparison, site mapping, scope management, organization, live logs, and an optional AI workspace.

Use Witness only against systems you own or are explicitly authorized to test. Scope rules and proxy settings help control traffic; they do not grant permission to test a target.

## What is shipped

Witness runs as a Tauri 2 desktop application with:

- A Svelte 5/SvelteKit frontend in `witness/src`.
- A Rust native core in `witness/src-tauri`.
- SQLite project metadata and file-backed HTTP bodies.
- Native file, folder, and save dialogs.
- Local certificate-authority generation for HTTPS interception.
- A typed Tauri command bridge between the frontend and Rust core.
- Optional encrypted application-wide storage for the Forge provider key.

The main toolbar contains **Proxy**, **History**, **Site Map**, **Replay**, **Fuzz**, **Organizer**, **ID+**, **Decoder**, **Comparer**, **Scope**, and **Forge**. **Logs** is optional and is enabled from Settings. The gear button opens **Settings**.

The browser preview is useful for visual frontend work only. Proxy traffic, SQLite, native dialogs, certificates, project archives, encrypted credentials, and desktop commands require the Tauri application.

## Quick start

### Prerequisites

- macOS, Windows, or Linux.
- Rust stable.
- Node.js 20 or newer.
- npm.
- The [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) for the host operating system.

### Run the desktop application

From the application directory (`witness/` in this repository):

```sh
npm install
npm run tauri dev
```

Tauri starts Vite at `http://localhost:1420`. Vite is configured to use a strict port, so another process using port `1420` must be stopped before starting the desktop app.

On first launch:

1. Create a project, open an existing `.wns` project, or choose a temporary session.
2. Open **Settings → Certificates**, generate the CA certificate, and install `witness-ca.pem` in a dedicated test-browser profile.
3. Configure that browser profile to use Witness's listener, which defaults to `127.0.0.1:8080`.
4. Start the proxy from **Proxy**.
5. Capture traffic in **History**, pause selected traffic in **Proxy**, or send a request to **Replay**, **Fuzz**, **Decoder**, **Comparer**, or **Organizer**.

Use a dedicated browser profile. Installing Witness's CA into a general-purpose profile can expose unrelated traffic to interception and storage.

### Available scripts

```sh
npm run dev                 # Frontend-only Vite development server
npm run build               # Production frontend build
npm run tauri dev           # Tauri desktop development
npm run tauri build         # Tauri desktop build and bundle
npm run check               # SvelteKit sync and svelte-check
npm run test:forge          # Forge lifecycle tests
npm run test:http-message   # HTTP message utility tests
npm run test:intruder       # Fuzz/intruder utility tests
npm run test:keyboard-shortcuts
npm run check:watch         # svelte-check watch mode
```

`npm run dev` does not provide the native proxy or other Tauri commands. Use `npm run tauri dev` for an end-to-end desktop session.

## Project launcher and project lifecycle

The startup launcher provides:

- **New Project** — choose a project name and destination through native dialogs.
- **Open Project** — open an existing `.wns` archive.
- **Temporary Session** — start without immediately choosing a permanent archive; save it later when needed.
- **Recent Projects** — reopen recent projects or remove a recent project through the launcher controls.
- **Quick Tour** — start the button-focused interactive tour.
- Links to the project website, wiki, and source/community destinations shown by the launcher.

A project stores its own history, scope rules, Organizer data, ID+ data, Replay and Fuzz workspaces, Decoder and Comparer state, Site Map selections, Forge chats/draft, and other workspace state. Application-wide settings are kept separately from project data.

### `.wns` archives

An `.wns` project is a portable single-file archive containing project metadata, the SQLite database, file-backed message bodies, and a versioned workspace snapshot. The working copy is materialized while the project is open and removed when it closes. Saves use a temporary archive/replacement flow so an interrupted save does not replace the last complete archive.

Project saves are debounced for workspace changes and validate the workspace snapshot before writing. The workspace validation limit is 512 MiB. The archive is currently unencrypted; protect it like the traffic and credentials it contains.

Temporary sessions can be saved as permanent projects. Closing a project clears the active project from the application without deleting the archive unless the explicit project-delete flow is used.

## Main workspaces

### Proxy / Intercept

The Proxy workspace is the local listener and the live interception queue.

#### Listener and traffic controls

- Start and stop the local proxy.
- See listener address, port, running state, connection count, and TLS/certificate status.
- Toggle interception on or off.
- Toggle request interception and response interception independently in Settings.
- Limit interception to in-scope traffic.
- Choose content-type filters for interception: HTML, JavaScript, CSS, JSON, XML/SVG, images, fonts, media, documents, or other.
- Apply request and response interception rule sets before a message is paused.
- Disable interception to forward all currently paused messages automatically.

#### Pending messages

The queue shows paused requests and responses. Select an item to inspect or edit it, then use:

- **Forward** — continue the selected request or response unchanged, or with the edited message.
- **Drop** — stop the selected message.
- **Forward all** — release the entire pending queue.
- **Drop all** — discard the pending queue after confirmation.
- **Edit** — open the selected message in the shared message editor.
- Request/response direction and target metadata — identify what is paused and why.

A paused message remains pending until it is resolved, the queue is released, the proxy is stopped, or interception is disabled. The native interception manager has no fixed UI timeout.

Proxy request and response events are also published to project History and can be transferred to other workspaces. The proxy supports cleartext HTTP/1.1, HTTPS through CONNECT and a local CA, HTTP/2 client connections negotiated through TLS ALPN, and HTTP/1.1 WebSocket upgrade/tunnel handling for `ws` and `wss` traffic.

### History

History is the searchable project traffic log backed by SQLite and file-backed bodies.

- Filter by HTTP method, host, status, MIME/content type, and free-text search.
- Use in-scope-only filtering.
- Enter custom status ranges in addition to common status filters.
- Search URLs, hosts, methods, statuses, headers, bodies, and other indexed metadata.
- Use case-sensitive and regular-expression search where offered.
- See match snippets and highlighting.
- Sort the visible data and move through paginated/virtualized rows.
- Select a row to inspect its complete request and response.
- Clear project History after confirmation.
- Delete an individual entry after confirmation.
- Copy a request, copy its URL or cURL form, export an `.http` message, or use the context menu.
- Send a request or response to Replay, Fuzz, Decoder, Comparer, or Organizer when the selected operation supports it.

History entries retain request/response bytes rather than only a summary, subject to the native 100 MiB message limit described below.

### Site Map

Site Map derives a host/path/endpoint tree from captured History.

- Search the tree.
- Show only in-scope branches.
- Expand all or collapse all branches.
- Select an endpoint and open its associated History entry.
- Copy the endpoint URL.
- Send the endpoint request to Replay, Fuzz, Decoder, or Organizer.
- Delete the selected endpoint's History entry after confirmation.

Scope decisions are evaluated using the current project scope. An out-of-scope rule overrides an in-scope match.

### Replay

Replay is the manual request sender. It stores multiple request tabs and their response/request history.

#### Tabs and tab groups

- Create a new empty Replay tab.
- Duplicate the active tab.
- Close a tab and reopen the most recently closed tab.
- Search request, response, and identity-response content across Replay tabs.
- Rename tabs.
- Create, rename, recolor, collapse, expand, and reorder tab groups from the tab context menus.
- Move tabs into groups and select tabs from the tab strip.
- Scroll the tab strip when more tabs are present than fit on screen.
- Keep a per-tab request history and restore a previous request version.

#### Request and send controls

- Edit the raw request in the shared message editor.
- Select **HTTP** or **HTTPS** for the target transport.
- Configure one identity group and selected identities for request injection.
- Send the request.
- Cancel an active request.
- Clear the current response.
- Inspect status, timing, size, and response message content.
- Send the request or response to Fuzz, Decoder, Comparer, Organizer, or another Replay tab.

Replay accepts origin-form requests with a Host header and absolute-form requests. A request URL scheme overrides the selected HTTP/HTTPS transport when an absolute URL is supplied. For TLS targets, Witness negotiates HTTP/2 when the origin offers it and otherwise uses HTTP/1.1.

### Fuzz

Fuzz is the tabbed request generator and sequential scanner. It is also referred to as Intruder in implementation and test names.

#### Tabs and scan controls

- Create, duplicate, close, and reopen Fuzz tabs.
- Search request content across Fuzz tabs.
- Group, collapse, expand, recolor, rename, and reorder tabs through tab context menus.
- Choose **Single**, **Spread**, **Map**, or **Combine** mode.
- Choose **HTTP** or **HTTPS** as the target.
- See marked-position count, generated value rows, and planned request count before launch.
- Insert a marker around the current selection with **Add §**.
- Remove all markers with **Clear §**.
- **Launch** a new scan or resume a stopped scan.
- Open **Results** to choose among running and completed scans.
- Stop an active scan; stopped scans retain results and can be resumed.

Markers use the form `§value§`. A marked selection can be in a request line, header, query value, form body, JSON body, or other editable request text. Request finalization synchronizes `Content-Length` where appropriate before each generated request is sent.

#### Attack modes

- **Single** — tests payloads against one marked position at a time; one payload set is reused across positions.
- **Spread** — places the corresponding payload value into all marked positions at once.
- **Map** — advances position-specific payload sets in parallel.
- **Combine** — generates the Cartesian product of position-specific payload sets.

Finite runs are limited to 5,000 generated requests. A null-payload configuration can run continuously until stopped. Requests are sent one at a time through the native repeater path.

#### Payload Warehouse

Each Fuzz tab has a Payload Warehouse. In Map and Combine modes, each marked position can have its own warehouse.

Payload types:

- **List** — edit newline-separated values; load a text file; fetch a newline-separated list from an HTTP/HTTPS URL; remove duplicates; remove the latest value; clear the list; append built-in lists.
- **Numbers** — generate sequential or random values with From, To, Step, and How many settings.
- **Null payload** — generate a chosen number of empty-string values or continue indefinitely. With no marker, repeated requests use the unmodified base request.
- **Brute forcer** — generate permutations from a character set across minimum and maximum lengths.
- **Dates** — generate dates across a range with day/week/month/year steps and preset or custom formats.
- **Character substitution** — define replacement mappings, choose case sensitivity, add or paste items, load a file, append built-in lists, remove duplicates, remove the latest item, and clear the input.

List fetching validates HTTP/HTTPS URLs, follows bounded redirects, rejects redirect loops, displays a sample/count preview, and requires **Import** before adding fetched values to the warehouse.

#### Payload processing rules

Processing rules run top to bottom for each generated value. The rule controls include:

- Enable/disable a rule.
- Add, edit, and delete a rule.
- Move a rule up or down.
- Add prefix or suffix.
- Match/replace with literal or regular-expression matching and case-sensitivity control.
- Take a substring or reverse substring by start index and length.
- Modify case: uppercase, lowercase, or capitalize words.
- Encode/decode URL, Base64, or hexadecimal values.
- Hash with SHA-1, SHA-256, or SHA-512.

#### Fuzz results

The results view provides:

- Running, stopped, complete, and error status.
- Request progress and total/continuous count.
- Payload values used for each request.
- HTTP status, response length, and duration.
- Per-result request and response inspectors.
- Stop and resume controls.
- Transfer of a selected result to Replay, Fuzz, Decoder, or Organizer.

### Organizer

Organizer is the persistent project-local library for saved request/response snapshots. Saving an item creates a separate snapshot; it does not remove the original History entry.

#### Browse and organize

- Search titles, methods, hosts, paths, notes, tags, request text, and response text.
- Browse **All entries** or **Unfiled**.
- Create top-level folders and nested folders up to three levels below a top-level folder (four total levels).
- Rename or delete folders; deleting a folder and its descendants moves saved entries to Unfiled.
- Drag entries into folders or Unfiled.
- Define project tags and colors.
- Filter by a tag and change tag colors.
- Create, recolor, rename, drag-reorder, and delete stages.
- Deleting a stage clears that stage from affected entries.
- Sort by recently updated, recently added, title, or host.
- Import and export Organizer JSON through native file dialogs.

#### Edit and transfer entries

Select an entry to edit:

- Title.
- Folder.
- HTTP/HTTPS target.
- Stage.
- Tags, including comma-separated entry and existing-tag selection.
- Notes.
- Saved request.
- Saved response.

Entry edits save automatically with a short debounce. The detail toolbar provides buttons to send the request to Replay, send it to Fuzz, and delete the entry after confirmation. The request and response viewers expose the shared message controls, including Decoder transfer where applicable. The disk/save action in other viewers creates another Organizer snapshot.

### ID+

ID+ manages reusable identities for Replay injection.

- Create, select, edit, and delete identity groups.
- Create, select, edit, and delete identities in the selected group.
- Set group and identity names, descriptions/notes, colors, authentication/injection values, and injection type.
- Inject an identity as a Cookie, Header, or Query Parameter.
- Replace an existing value for the configured injection key when a request is sent.
- Preview the resolved identity configuration used by Replay.
- Export identity data to JSON through a native save dialog.
- Import identity data from JSON through a native open dialog.
- Confirm destructive group and identity deletion.

Identity values are project data. They can contain credentials, tokens, or other sensitive values; protect project archives and exported JSON.

### Decoder

Decoder is a local recipe workbench. Add operations from the searchable palette; the recipe executes automatically from the current input after edits, and **Enter** can run it immediately.

#### Encoding operations

- URL decode / URL encode.
- Form decode / Form encode.
- From Base64 / To Base64.
- From Base64url / To Base64url.
- From Hex / To Hex.
- HTML decode / HTML encode.
- Unicode unescape / Unicode escape.

#### Web-format operations

- Format JSON.
- Minify JSON.
- Query to JSON.
- JSON to query.

#### Inspection and hashing

- JWT inspect, showing unverified header and claims.
- Smart decode using conservative URL, HTML, Unicode, Hex, and Base64 layers.
- SHA-1, SHA-256, SHA-384, and SHA-512 text hashes computed locally.

#### Recipe controls

- Search and filter operations by name, description, or category.
- Add a step by clicking an operation.
- Move steps up or down.
- Remove individual steps.
- Reverse a recipe when every step has a supported inverse.
- Clear the recipe without clearing the source input.
- Toggle Base64 padding.
- Use final output as the new input.
- Copy final output.
- Send output to Replay or Fuzz when initiated through a supported transfer.
- See detected format, running state, and transformation errors.

Decoder is intentionally a focused local utility, not a general-purpose file forensics or image-analysis engine.

### Comparer

Comparer computes a live diff between two editable inputs.

- Paste or edit the left and right values.
- Compare at character, word, or line granularity.
- Use side-by-side or stacked layout.
- Recompute automatically or force a recomputation.
- View diff statistics and synchronized scrolling.
- Clear both inputs.
- Transfer request or response content from supported workspaces into either side.

### Scope

Scope controls project matching and in-scope filtering.

- Add an in-scope or out-of-scope domain/host pattern.
- Use literal matching or regular expressions.
- Include or exclude subdomains where the rule type supports it.
- Search/filter the scope table.
- Edit or delete entries with confirmation.
- Import scope rules from text.
- Export scope rules to text.
- Use the default allow-all behavior when no restrictive scope entries are configured.

Scope matching is used by interception, Site Map filters, and traffic views. Out-of-scope matches override in-scope matches.

### Logs

Logs is an opt-in toolbar workspace. Enable **Settings → Miscellaneous → Show logs tab** to display it.

- View a live stream of native/frontend diagnostic events.
- See timestamp, level, module, and message fields.
- Filter by level and module.
- Choose the number of visible entries.
- See traffic statistics such as processed, sent, and received counts/bytes.
- Copy available logs.
- Export logs to a file.
- Clear logs after confirmation.

The native log store is a ring buffer retaining the latest 2,000 records. The frontend keeps a smaller working view, so the visible list may be smaller than the native retention limit. Diagnostic sanitization redacts sensitive-looking fields, removes URL query/fragment values, and truncates long values, but logs should still be treated as potentially sensitive.

### Forge / AI Controller

Forge is the optional project assistant in the **Forge** toolbar workspace. It can read project state and, with approval, operate semantic application tools.

#### Setup

Open **Settings → AI** and configure:

- **Enable AI Controller** — application-wide enable/disable.
- **Enter sends messages** — Enter sends; Shift+Enter creates a newline.
- **Base URL** — remote providers require HTTPS; local HTTP endpoints are supported.
- **Model name**.
- **Timeout** — 1–600 seconds.
- **Turn limit** — 1–32 tool/inference steps.
- **Test connection** — check the configured provider.

The provider key controls include **Save key**, **Replace key**, **Delete API key**, and a confirmation dialog for deletion. Only the first and last three characters are displayed after saving. The key is stored in an encrypted application credential store and is not included in a project archive. Forge can be enabled across projects, but the saved key is application-wide.

#### Chats and composer

- Create a new chat or new conversation.
- Select previous/next chats.
- Delete a chat after confirmation.
- Persist chat messages and the current draft with the project workspace.
- Generate a concise title from the first user message.
- Show randomized starter questions when Forge is ready and the chat is empty.
- Copy user and assistant messages.
- Render assistant inline Markdown for paragraphs, lists, code spans, emphasis, and strong text.
- Expand **Attached context** to inspect the project context sent to Forge.
- Stop a running response with the Stop button; Escape requires a second press within the arming window.
- Cancel an outstanding approval and its remaining queued actions.

Forge polls native runtime readiness and displays startup/error status. The composer remains disabled until AI is enabled, the runtime is ready, and no response or approval is pending.

#### Tool permissions and capabilities

Forge read tools can inspect:

- Current context, workspace, project state, recent projects, and navigation state.
- Proxy status/settings and the interception queue.
- Replay tabs, request history, responses, and identities.
- Fuzz tabs, plans, scans, and results.
- History and Site Map.
- Scope rules.
- Organizer folders, tags, stages, entries, and view state.
- ID+ groups, identities, and injection previews.
- Decoder and Comparer state.
- Settings and logs.

Action tools can, subject to approval, navigate and reset workspace state; create/open/save/delete projects; start/stop/configure the proxy; resolve interceptions; create/update/duplicate/delete/select/send/cancel Replay tabs; patch requests and restore request history; configure Replay identities; create/update/delete Fuzz tabs and positions; configure payload warehouses; start/stop/resume scans; open/save Fuzz results; delete/clear History; open or transfer Site Map entries; add/update/delete/import Scope entries; create/update/delete/move Organizer data, tags, folders, and stages; import Organizer data; create/update/delete identities; run Decoder transforms and transfers; set Comparer inputs and run comparisons; update Settings; generate certificates; and clear Logs.

Every action call is approved separately by default. The approval card offers **Cancel**, **Approve**, and **Trust**. **Trust** applies to that tool for the current chat. **Trust Tools** applies to all tool executions for the current Forge session. Trust state is intentionally kept in memory and is not persisted to the project.

Forge is instructed to treat project content as data rather than instructions, read state before editing, use exact IDs returned by read tools, and never claim an action succeeded until its result confirms success. Remote AI providers receive the conversation and any context included in an inference request; configure a provider appropriate for the sensitivity of the project.

## Shared message viewer and editor

The request/response viewer is reused by Proxy, History, Replay, Fuzz, Organizer, ID+ result views, and transfer flows. Depending on context, it provides:

- **Pretty** — formatted message view.
- **Raw** — editable/plain raw HTTP text.
- **Raw+** — enhanced raw message presentation with syntax-aware fields.
- **Hex** — hexadecimal byte inspection.
- Wrap toggle for long lines.
- CodeMirror search with match highlighting, regular-expression mode, and case-sensitive mode.
- Copy message content.
- Export as `.http`.
- Copy URL.
- Copy cURL.
- Edit request method where the view is a request editor.
- Edit headers and body.
- Undo, redo, cut, paste, and select all in editable views.
- Selection-aware transfer to Decoder.
- Send to Replay, Fuzz, Comparer, or Organizer when the parent workspace exposes that action.
- Duplicate the current request into a new Replay/Fuzz tab when supported.
- Forward, drop, and cancel controls in the parent workflow when the message is an interception or active request.

Request editors normalize line endings and synchronize `Content-Length` for non-chunked edited bodies. Hex inspection is not a replacement for a binary-safe request workflow; preserve raw bytes when exporting or moving messages.

## Settings reference

Settings saves fields as they change and shows a save state. The sidebar contains these sections:

### Proxy

- Listener bind address.
- Listener port, from 1–65535.
- Compression behavior: **Decompress all supported**, **Decompress text formats**, or **Pass through unchanged**.
- Upstream timeout, from 1–300 seconds.
- Only intercept in-scope traffic.
- Optional outbound upstream proxy: HTTP or SOCKS5, host, port, optional username, and optional password.
- Match/replace rules.
- Request and response interception direction.
- Interception content-type filters.
- Request and response interception rule sets.

Changing listener or certificate settings while the proxy is running requires a proxy restart.

### Display

- Dark theme.
- Interface font size, 10–24.
- Message editor font size, 9–24.
- History panel split percentage, 20–75.

Light mode is shown as a development placeholder. Selecting it displays an informational notice and keeps the application in dark mode; it should not be treated as a supported light theme.

### Storage

- Autosave interval, 1–3600 seconds.
- History size limit, 100–1,000,000 entries in the settings field.

### Keyboard

- On macOS, choose Command or Control as the primary application modifier.
- On Windows/Linux, application shortcuts use Control.
- The built-in shortcut reference is grouped by workspace and includes availability/destructive-action information.
- Base shortcut keys are fixed; they cannot be remapped from Settings.

### Certificates

- Choose the certificate directory.
- See the expected `witness-ca.pem` path.
- Generate the local CA certificate.
- Install the generated CA in the dedicated browser profile's trusted authorities.
- See certificate generation status.

The CA private key remains in the configured certificate directory and is not placed in project archives or Organizer exports.

### AI

AI Controller enablement, Enter-to-send, provider Base URL, model, timeout, turn limit, connection test, and encrypted provider-key lifecycle are described in [Forge / AI Controller](#forge--ai-controller).

### Miscellaneous

- Show or hide the optional Logs toolbar tab.

### About

The About page includes:

- Witness and North Core Labs branding and project description.
- Wiki, website, GitHub, donation, and sponsorship links.
- **Open Wiki**, **Visit northcorelabs.tech**, **GitHub**, **Donate**, and **Sponsor on GitHub** buttons.
- **Replay tutorial**.
- **Reset auto-start**, which clears the local tutorial-seen marker so the tour can start again on the next launch.
- License, changelog, and privacy links.

The About page also includes a button for an offline wiki download; its current behavior reports that offline download is not yet available.

## Native networking behavior

The native core is deliberately conservative about HTTP framing and message size:

- HTTP/1.1 request and response parsing supports content-length and chunked bodies, including chunk extensions/trailers where applicable.
- HTTP/2 is handled through TLS ALPN for client connections and TLS upstream connections.
- Cleartext HTTP/2 (`h2c`) and an HTTP/2 prior-knowledge preface are rejected by the HTTP/1 parser.
- An origin that advertises HTTP/3 with `Alt-Svc` can be tried opportunistically over QUIC for direct TLS proxy traffic; Witness falls back to the normal path when the attempt fails. HTTP/3 is not used through the configured upstream HTTP/SOCKS5 proxy.
- WebSocket HTTP/1.1 upgrades are forwarded and, after a successful handshake, tunneled bidirectionally. The handshake request/response can still pass through interception rules.
- Supported response decompression formats are gzip, deflate, and Brotli (`br`).
- The HTTP and SOCKS5 upstream settings route Witness's outbound origin connections; they are separate from the local listener that browsers connect to. HTTP upstream uses CONNECT and optional Basic authentication. SOCKS5 supports optional username/password authentication.
- Replay requests can be cancelled. Fuzz requests use the same native repeater path.
- Individual request/response bodies and native HTTP/2 bodies are limited to 100 MiB.
- Upstream failures produce gateway/error responses where the active transport permits it; timeouts use a gateway-timeout response.

### Automatic match and replace

Proxy settings can apply enabled rules to live requests and responses. Supported locations include:

- Request host.
- Request header names and values.
- Request body.
- Request query/form parameter names.
- Request query/form parameter values.
- Response header names and values.
- Response body.
- Response form/query parameter names.
- Response form/query parameter values.

Rules can use literal replacement or regular expressions. Body replacements update `Content-Length` for non-chunked messages. Response rules run after configured decompression and before response interception.

## Data, privacy, and safety

- `.wns` archives are unencrypted and can contain complete requests/responses, cookies, authorization headers, tokens, personal data, notes, and identity values.
- The generated CA private key stays outside project archives in the configured certificate directory.
- Forge's provider key is application-wide and stored separately in the encrypted credential store; it is not stored in project data.
- Organizer and identity exports are explicit user actions and can contain sensitive values.
- The local listener defaults to loopback (`127.0.0.1`) but can be changed in Settings. Do not bind it to a network interface unless that exposure is intentional.
- An upstream proxy may receive all origin traffic that Witness routes through it.
- A configured remote AI provider may receive prompts, project context, message content, and tool-result context sent to Forge. Use a local provider or omit sensitive project content when required by your engagement.
- The application does not require an Witness account. The About screen describes the project as free/open source and without telemetry; origin, upstream, and AI network requests are still made when the user configures or initiates them.
- Keep backups of important archives. Witness is beta software and protocol edge cases can still occur.

## Supported limits

| Area | Current implementation limit or behavior |
| --- | --- |
| Native HTTP request/response body | 100 MiB per message |
| Fuzz finite run | 5,000 generated requests |
| Fuzz continuous/null run | Runs until stopped |
| Native log ring buffer | Latest 2,000 records |
| Frontend log working view | Smaller than native retention; the visible list is bounded |
| Workspace snapshot validation | 512 MiB |
| Upstream timeout setting | 1–300 seconds |
| AI request timeout | 1–600 seconds |
| AI turn step limit | 1–32 steps |
| History setting | 100–1,000,000 entries |
| Organizer folder nesting | Three nested levels below a top-level folder |

## Keyboard shortcuts

The exact shortcut reference is also available in **Settings → Keyboard**. `M` below means the primary application modifier: Command (`⌘`) on macOS by default, Control (`⌃`) on macOS when selected, and Control (`Ctrl`) on Windows/Linux. Shortcuts marked `M+Shift` use both modifiers. Plain arrows, Enter, Escape, and Backspace have no primary modifier unless shown.

### Global

| Shortcut | Action |
| --- | --- |
| `M+S` | Save project and workspace |
| `M+,` | Open Settings at the last section |
| `M+/` | Show or hide the shortcut reference |
| `Escape` | Close/cancel the topmost transient state |

### Proxy / Intercept

| Shortcut | Action |
| --- | --- |
| `M+F` / `M+D` | Forward / drop selected interception |
| `M+Shift+F` / `M+Shift+D` | Forward all / drop all pending interceptions |
| `↑` / `↓` | Select previous / next pending interception |
| `M+R` / `M+I` / `M+U` / `M+O` | Send selected request to Replay / Fuzz / Decoder / Organizer |

### History

| Shortcut | Action |
| --- | --- |
| `↑` / `↓` | Select previous / next visible History entry |
| `M+C` | Copy selected request |
| `M+D` | Delete selected entry after confirmation |
| `M+R` / `M+I` / `M+U` / `M+O` | Send selected request to Replay / Fuzz / Decoder / Organizer |

### Site Map

| Shortcut | Action |
| --- | --- |
| `↑` / `↓` | Select previous / next visible tree row |
| `Enter` | Open selected endpoint in History |
| `M+E` / `M+Shift+E` | Expand all / collapse all branches |
| `M+D` | Delete selected endpoint History after confirmation |
| `M+R` / `M+I` / `M+U` / `M+O` | Send endpoint to Replay / Fuzz / Decoder / Organizer |

### Replay

| Shortcut | Action |
| --- | --- |
| `M+F` | Send active Replay request |
| `M+Shift+F` | Search Replay tabs |
| `M+N` / `M+D` | New tab / duplicate active tab |
| `M+W` / `M+Shift+W` | Close active tab / reopen last closed tab |
| `M+[` / `M+]` | Previous / next request-history version |
| `M+Shift+I` | Configure identities |
| `M+R` / `M+I` / `M+U` / `M+O` | Send active request to Replay / Fuzz / Decoder / Organizer |

### Fuzz

| Shortcut | Action |
| --- | --- |
| `M+Shift+F` | Search Fuzz tabs |
| `M+Enter` | Launch or resume the active Fuzz run |
| `M+.` | Stop the active Fuzz run |
| `M+Shift+R` | Show results or return to setup |
| `M+N` / `M+D` | New tab / duplicate active tab |
| `M+W` / `M+Shift+W` | Close active tab / reopen last closed tab |
| `M+R` / `M+I` / `M+U` / `M+O` | Send selected result to Replay / Fuzz / Decoder / Organizer |

### Organizer

| Shortcut | Action |
| --- | --- |
| `↑` / `↓` | Select previous / next visible entry |
| `Enter` | Open/focus selected entry |
| `M+D` | Delete selected entry after confirmation |
| `M+G` | Create a top-level folder |
| `M+R` / `M+I` / `M+U` / `M+O` | Send selected entry to Replay / Fuzz / Decoder / duplicate it in Organizer |
| `M+Shift+E` / `M+Shift+I` | Export / import Organizer JSON |

### ID+

| Shortcut | Action |
| --- | --- |
| `M+G` / `M+I` | Create identity group / identity |
| `↑` / `↓` | Select previous / next group or identity |
| `M+D` | Delete selected group or identity after confirmation |
| `M+Shift+E` / `M+Shift+I` | Export / import identity JSON |

### Decoder

| Shortcut | Action |
| --- | --- |
| `M+F` | Focus operation filter |
| `M+Enter` | Run recipe now |
| `M+Backspace` | Clear recipe without clearing source input |
| `M+Shift+R` | Reverse a reversible recipe |
| `M+Shift+U` / `M+Shift+C` | Use final output as input / copy final output |

### Comparer

| Shortcut | Action |
| --- | --- |
| `M+L` / `M+R` | Focus left / right editor |
| `M+Enter` | Recompute comparison |
| `M+Backspace` | Clear both inputs |
| `M+\\` | Toggle side-by-side / stacked layout |

### Scope

| Shortcut | Action |
| --- | --- |
| `M+F` / `M+N` | Focus Scope filter / create Scope entry |
| `↑` / `↓` | Select previous / next Scope entry |
| `M+E` / `M+D` | Edit / delete selected entry |
| `M+Enter` | Submit the active Scope form |

### Forge / AI

| Shortcut | Action |
| --- | --- |
| `M+L` | Focus Forge composer |
| `Enter` | Send a focused composer message when Enter-to-send is enabled |
| `Escape` twice | Stop an active Forge reply; the first press arms the stop |
| `M+N` | Create a new Forge chat |
| `M+[` / `M+]` | Previous / next Forge chat |
| `M+Shift+Backspace` | Delete the active Forge chat after confirmation |

### Logs

| Shortcut | Action |
| --- | --- |
| `M+F` | Focus module filter |
| `M+Shift+E` | Export logs |
| `M+Shift+Backspace` | Clear logs after confirmation |

### Settings

| Shortcut | Action |
| --- | --- |
| `M+↑` / `M+↓` | Move to the previous / next Settings section |
| `Enter` | Open the focused Settings section |

Shortcuts that operate on a selected item are ignored when the required selection is absent. Text-editor shortcuts remain available for normal editing, and the application shortcut resolver avoids stealing shortcuts from editable controls unless a shortcut explicitly allows editable targets.

## Documentation

- [Quick start](../docs/quick-start.md)
- [User guide](../docs/user-guide.md)
- [CA certificate installation](../docs/cert-installation.md)
- [Project management](../docs/projmgmt.md)
- [Architecture](../docs/architecture.md)
- [Developer setup](../docs/developer-setup.md)
- [Keyboard-shortcuts design](../docs/keyboard-shortcuts-prd.md)
- [AI Controller design](../docs/ai-controller-prd.md) — design documentation; its proposed items are not automatically evidence of shipped behavior.
- [Product requirements](../docs/prd.md)
- [Implementation checklist](../docs/tasks.md)
- [Contributing](CONTRIBUTING.md)

## Project layout

```text
repository/
├── witness/
│   ├── src/                 SvelteKit/Svelte frontend
│   ├── src-tauri/           Rust desktop core and Tauri commands
│   ├── static/              Application assets
│   ├── tests/               Frontend utility tests
│   ├── CONTRIBUTING.md
│   ├── LICENSE
│   └── README.md
└── docs/                    User, architecture, setup, and design documents
```

Important implementation areas include:

- `src/routes/+page.svelte` — application shell, toolbar, project lifecycle, workspace coordination, and global shortcuts.
- `src/lib/api.ts` — typed frontend command/event bridge.
- `src/lib/components/` — workspaces and reusable viewers/dialogs.
- `src/lib/http-message.ts` and `src/lib/intruder.ts` — frontend message and Fuzz utilities.
- `src-tauri/src/ui_bridge/mod.rs` — native command surface.
- `src-tauri/src/project/mod.rs` — project/archive lifecycle.
- `src-tauri/src/proxy/` — listener, interception, upstream transports, WebSockets, and match/replace.
- `src-tauri/src/repeater/` — native request execution and cancellation.
- `src-tauri/src/http/` — parsing, serialization, chunked bodies, and decompression.
- `src-tauri/src/logging/` — sanitized diagnostics and the native log ring buffer.

## Verification and development checks

Frontend checks:

```sh
npm run check
npm run build
git diff --check
```

Native checks from `witness/src-tauri`:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test -- --test-threads=1
```

The serial Rust test invocation avoids loopback-port contention between networking tests on constrained development machines. Existing frontend tests can be run individually with the scripts listed in [Available scripts](#available-scripts). A full Tauri runtime smoke test should be performed with `npm run tauri dev` because frontend-only Vite cannot exercise the native command bridge.

