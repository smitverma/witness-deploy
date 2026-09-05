import { driver as createDriver, type Config, type DriveStep, type Driver, type Popover, type PopoverDOM } from "driver.js";
import "driver.js/dist/driver.css";
import "./tutorial.css";

const STORAGE_MODE_KEY = "witness:tutorial:v1:mode";

export type TutorialMode = "ask" | "simulate";

export function getTutorialMode(): TutorialMode {
  try {
    const m = localStorage.getItem(STORAGE_MODE_KEY) as TutorialMode | null;
    return m === "simulate" ? "simulate" : "ask";
  } catch {
    return "ask";
  }
}

export function setTutorialMode(mode: TutorialMode): void {
  try {
    localStorage.setItem(STORAGE_MODE_KEY, mode);
  } catch {}
}

// Amazing driver config — dark theme matched to Witness, progress, overlay, skip always
type TutorialHookOpts = {
  driver: Driver;
  state: {
    activeStep?: DriveStep & { data?: { askClick?: unknown } };
    activeElement?: Element;
  };
};

type AskClickData = { askClick?: unknown };

function isAskClickStep(step: DriveStep | undefined): boolean {
  const data = step?.data as AskClickData | undefined;
  return data?.askClick === true;
}

// Tracked once-listeners so destroy() never leaks a pending click handler.
const pendingAskClickListeners: { el: Element; handler: () => void }[] = [];

function trackAskClickListener(el: Element, handler: () => void) {
  pendingAskClickListeners.push({ el, handler });
}

function untrackAskClickListener(el: Element, handler: () => void) {
  const index = pendingAskClickListeners.findIndex((entry) => entry.el === el && entry.handler === handler);
  if (index >= 0) pendingAskClickListeners.splice(index, 1);
}

function clearAskClickListeners() {
  for (const { el, handler } of pendingAskClickListeners.splice(0)) {
    try {
      el.removeEventListener("click", handler);
    } catch {
      // Listener may already be gone (once:true fired).
    }
  }
}

function cleanupTourPulse() {
  document.querySelectorAll(".witness-tour-pulse").forEach((n) => {
    (n as HTMLElement).classList.remove("witness-tour-pulse");
    (n as HTMLElement).style.cursor = "";
  });
}

function injectSkipButton(popover: PopoverDOM, opts: TutorialHookOpts) {
  const footer = popover.footer as HTMLElement | undefined;
  const footerButtons = popover.footerButtons as HTMLElement | undefined;
  const wrapper = popover.wrapper as HTMLElement;
  if (!footer || wrapper.querySelector(".witness-skip-btn")) return;
  const btn = document.createElement("button");
  btn.textContent = "Skip";
  btn.className = "witness-skip-btn";
  btn.type = "button";
  // Style will be in tutorial.css, but inline fallback
  btn.onclick = () => {
    try {
      opts.driver.destroy();
    } catch {}
  };
  // Insert as first button in footer (left side, progress stays right)
  // driver footer has: progress + footerButtons (prev/next/close)
  // We put skip at start of footerButtons or footer
  const target = footerButtons ?? footer;
  target.insertBefore(btn, target.firstChild);
}

/**
 * @deprecated `mode` is kept for API compat but no longer branches behavior.
 * The tour always uses ask-click steps; callers may omit it.
 */
function withAmazingDefaults(_mode?: TutorialMode): Partial<Config> {
  return {
    showProgress: true,
    progressText: "{{current}}/{{total}}",
    nextBtnText: "Next →",
    prevBtnText: "← Back",
    doneBtnText: "Finish",
    allowClose: true,
    overlayColor: "black",
    overlayOpacity: 0.72,
    stagePadding: 6,
    stageRadius: 8,
    popoverClass: "witness-driver-popover",
    smoothScroll: false,
    animate: false,
    allowKeyboardControl: true,
    disableActiveInteraction: false,
    skipMissingElement: true,
    waitForElement: 800,
    popoverOffset: 12,
    onPopoverRender: (popover, opts) => {
      injectSkipButton(popover, opts as unknown as TutorialHookOpts);
      // Amazing: Add subtle entrance glow
      popover.wrapper.classList.add("witness-popover-enter");
      // Hook ask-for-clicks: if step wants click, highlight the element's cursor
      const hookOpts = opts as unknown as TutorialHookOpts;
      const step = hookOpts.state.activeStep;
      const el = hookOpts.state.activeElement as HTMLElement | undefined;
      if (el && isAskClickStep(step)) {
        el.style.cursor = "pointer";
        el.classList.add("witness-tour-pulse");
      }
    },
    onDeselected: (el) => {
      if (el) {
        (el as HTMLElement).classList.remove("witness-tour-pulse");
        (el as HTMLElement).style.cursor = "";
      }
    },
    onHighlighted: (el, step, opts) => {
      if (!el || !step) return;
      if (!isAskClickStep(step)) return;
      const hookOpts = opts as unknown as TutorialHookOpts;
      const handler = () => {
        untrackAskClickListener(el, handler);
        window.setTimeout(() => {
          try {
            if (hookOpts.driver.isActive()) hookOpts.driver.moveNext();
          } catch {
            // Tour may have been destroyed mid-click.
          }
        }, 180);
      };
      trackAskClickListener(el, handler);
      el.addEventListener("click", handler, { once: true });
    },
    onNextClick: (el, step, opts) => {
      const hookOpts = opts as unknown as TutorialHookOpts;
      // If ask mode and this step wants a click, simulate the click for them when they press Next
      if (el && isAskClickStep(step)) {
        try {
          (el as HTMLElement).click();
        } catch {
          // Click simulation is best-effort.
        }
        // Let driver moveNext happen after click propagation
        window.setTimeout(() => {
          try {
            hookOpts.driver.moveNext();
          } catch {
            // Tour may have been destroyed.
          }
        }, 200);
        return;
      }
      // Default behavior: move next
      hookOpts.driver.moveNext();
    },
    onDestroyed: () => {
      clearAskClickListeners();
      cleanupTourPulse();
    },
    onDestroyStarted: () => {
    },
  };
}

export type TutorialHandle = {
  driver: Driver;
  destroy: () => void;
};

function buildSteps(): DriveStep[] {
  const s = (element: string | undefined, popover: Popover, askClick = false): DriveStep => {
    const step: DriveStep = element
      ? { element, popover, data: { askClick }, disableActiveInteraction: false }
      : { popover, data: { askClick: false } };
    return step;
  };

  return [
    s(undefined, {
      title: "Welcome to Witness",
      description:
        "Witness is a professional web security testing toolkit by Northcore Labs. This guided tour introduces the primary workspaces, controls, and workflows. Use <b>Next</b> and <b>Back</b> to navigate, or <b>Skip</b> to exit at any time. The interface will be highlighted step by step.",
    }),

    s('[data-tour="toolbar"]', {
      title: "Toolbar — your command bar",
      description:
        "All primary tools live here: <b>Proxy, History, Site Map, Replay, Fuzz, Organizer, ID+, Decoder, Comparer, Scope, Forge</b>. Each button switches the main pane. The tour will auto-traverse — just press <b>Next</b> or click the highlighted button.",
      side: "bottom",
      align: "center",
    }),

    s('[data-tour="tab-Proxy"]', {
      title: "Proxy — the heart",
      description:
        "Start here. The Proxy intercepts live traffic. <b>Click the Proxy tab</b> (or Next) to see its workspace — Intercept toggle, pending queue, and live status.",
      side: "bottom",
    }, true),

    s('[data-tour="proxy-workspace"]', {
      title: "Proxy workspace — Intercept controls",
      description:
        "The live <b>Intercept</b> tab. Top bar has <b>Start/Stop Proxy</b>, <b>Intercept</b> toggle and <b>In-scope only</b>. Below is the pending queue.",
      side: "top",
    }),
    s('[data-tour="proxy-controls"]', {
      title: "Proxy controls — Start & Intercept",
      description:
        "Buttons: <b>Start/Stop Proxy</b> (binds 127.0.0.1:8080), <b>Intercept</b> on/off, <b>In-scope only</b> (keeps queue safe when scope empty), and <b>Proxy settings</b> shortcut.",
      side: "bottom",
    }),
    s('[data-tour="proxy-intercept-table"]', {
      title: "Pending interceptions — the queue",
      description:
        "Paused messages appear here. Columns: <b>Host, Method, URL, Length</b>. Click a row to load it, <b>Forward</b> or <b>Drop</b> single or <b>ALL</b> (with <code>Ctrl+Shift+F/D</code>).",
      side: "top",
    }),
    s('[data-tour="proxy-intercept-actions"]', {
      title: "Forward / Drop — act fast",
      description:
        "Actions for the selected message: <b>Forward</b> (send on), <b>Drop</b> (discard), and <b>ALL</b> variants. Edit the raw below first to tamper.",
      side: "top",
    }),
    s('[data-tour="proxy-message-viewer"]', {
      title: "Message viewer — edit raw",
      description:
        "When a message is selected, edit <b>Request</b> (or <b>Response</b> + original Request) in <b>Pretty/Raw/Hex</b>. Buttons: <b>Send to Replay/Fuzz/Decoder</b>, <b>Save to Organizer</b>, and <b>Forward/Drop</b> after edit.",
      side: "left",
    }),

    s('[data-tour="tab-History"]', {
      title: "History — every request",
      description:
        "Switch to <b>History</b>. Search (⌘K), filter by host/method/status/mime, toggle <b>In Scope Only</b>, sort. Click a row to inspect. <b>Try it: click History tab.</b>",
      side: "bottom",
    }, true),

    s('[data-tour="history-filter"]', {
      title: "Filter bar — slice traffic",
      description:
        "Top bar: <b>Search</b> (text across URL/host), <b>Host/Method/MIME</b> dropdowns, <b>Status min/max</b>, <b>In Scope Only</b> toggle, <b>Clear</b> and <b>Sort</b> (timestamp, host...). All in SQLite.",
      side: "bottom",
    }),

    s('[data-tour="history-table"]', {
      title: "History table — right-click for power",
      description:
        "Columns: <b># URL Method Host Path Status Length MIME Time Scoped</b>. Buttons: sortable headers, <b>Load more</b> pagination. Right-click → <b>Send to Replay/Fuzz/Decoder/Comparer</b>, <b>Copy URL / cURL</b>, <b>Save to Organizer</b>, <b>Delete</b>.",
      side: "top",
    }),

    s('[data-tour="history-inspectors"]', {
      title: "Message inspectors — split view",
      description:
        "Bottom split shows <b>Request</b> (left) and <b>Response</b> (right) for the selected row. Drag the divider, minimize with the handle. Each pane is a full <b>MessageViewer</b>.",
      side: "left",
    }),
    s('[data-tour="history-request-viewer"]', {
      title: "Request viewer — inside History",
      description:
        "Buttons inside: <b>Pretty/Raw/Hex</b> tabs, <b>Send to Replay</b>, <b>Send to Fuzz</b>, <b>Decoder</b>, <b>Save to Organizer</b>, <b>Copy</b>. Edit then resend.",
      side: "right",
    }),
    s('[data-tour="history-response-viewer"]', {
      title: "Response viewer — inside History",
      description:
        "Same viewer for the response: <b>Pretty/Raw/Hex</b>, <b>Decoder</b>, length/status/mime. Search highlights across both panes.",
      side: "left",
    }),

    // Site Map
    s('[data-tour="tab-Site Map"]', {
      title: "Site Map — your target tree",
      description: "Click <b>Site Map</b> to see auto-built host → path tree from History, with in-scope filtering and collapsed groups.",
      side: "bottom",
    }, true),
    s('[data-tour="site-map"]', {
      title: "Site Map workspace",
      description:
        "Search endpoints, expand/collapse hosts, select rows to preview latest entry. Great for scoping and discovering hidden endpoints.",
      side: "right",
    }),

    // Replay
    s('[data-tour="tab-Replay"]', {
      title: "Replay — resend & mutate",
      description:
        "Click <b>Replay</b>. Open any History entry here, edit raw, switch TLS, manage history (undo), duplicate with <code>Ctrl+D</code>, and rotate <b>Identities</b>.",
      side: "bottom",
    }, true),
    s('[data-tour="replay-workspace"]', {
      title: "Replay workspace — resend lab",
      description:
        "Top: <b>Tabs bar</b> with groups (color dots, collapse), <b>New tab +</b>, <b>Search</b>. Toolbar: <b>←/→</b> request history, <b>TLS (https/http)</b> toggle, <b>Duplicate</b>, <b>Cancel</b>, and big <b>Send</b>.",
      side: "top",
    }),
    s('[data-tour="replay-tabs"]', {
      title: "Replay tabs — organize",
      description:
        "Each tab is a request. <b>Groups</b> (color) collapse/expand, right-click for <b>Rename, Close, Add to group</b>. <b>+</b> creates blank tab, search finds across tabs.",
      side: "bottom",
    }),
    s('[data-tour="replay-send"]', {
      title: "Send — fire the request",
      description:
        "Big <b>Send</b> button (primary). While sending shows spinner and <b>Cancel</b>. Response appears below with timing, size, TLS badge.",
      side: "left",
    }),
    s('[data-tour="replay-request-editor"]', {
      title: "Request editor — CodeMirror",
      description:
        "Editable <b>Pretty/Raw/Hex</b> with HTTP-aware CodeMirror: headers + body, <b>Duplicate</b>, <b>Send to Fuzz/Decoder/Organizer</b>, <b>Identities</b> picker (ID+), and <b>Content-Length</b> auto-sync.",
      side: "right",
    }),

    // Fuzz (Intruder)
    s('[data-tour="tab-Fuzz"]', {
      title: "Fuzz — Intruder",
      description:
        "Click <b>Fuzz</b>. Positions with <code>§</code> markers, payload warehouses (list/numbers/brute-force/dates/substitution), processing rules, and live results withComparer diff.",
      side: "bottom",
    }, true),
    s('[data-tour="fuzz-workspace"]', {
      title: "Intruder workspace — Fuzz",
      description:
        "Top: <b>Tabs</b> + <b>New</b>, <b>Positions</b> (<code>§</code> markers, Add/Clear), <b>Payload warehouse</b> (list/numbers/bruteForce/dates/substitution, per-position or shared), <b>Processing rules</b> (addPrefix, encode, hash…), <b>Scans</b> list + <b>Results</b> table (payload/status/length/time, click to view request/response). Modes: single/shared/parallel/combinations.",
      side: "top",
    }),

    // Organizer
    s('[data-tour="tab-Organizer"]', {
      title: "Organizer — your findings notebook",
      description: "Click <b>Organizer</b>. Folders, stages, tags, notes, request/response snapshots — the lab notebook for engagements.",
      side: "bottom",
    }, true),
    s('[data-tour="organizer"]', {
      title: "Organizer workspace — findings",
      description:
        "Left: <b>Folders</b> tree (create/rename/delete), <b>Search</b> + <b>Tag</b> filter, <b>Sort</b> (updated/created/title). Center: items table (title/host/path/status/tags). Right: <b>Detail</b> with Request/Response editors, <b>Notes</b>, <b>Tags</b>, <b>Stages</b> (Todo→Done), <b>Save</b> + <b>Export/Import</b>.",
      side: "top",
    }),

    // ID+
    s('[data-tour="tab-ID+"]', {
      title: "ID+ — Identity rotation",
      description:
        "Click <b>ID+</b>. Groups (cookie/header/query), Identities (color, auth value), and per-request injection for Replay/Fuzz.",
      side: "bottom",
    }, true),
    s('[data-tour="identity"]', {
      title: "Identity workspace — ID+",
      description:
        "Left: <b>Groups</b> (name, description, injection type: cookie/header/query, key). Needle: <b>Identities</b> (name, color, notes, auth value). Buttons: <b>Add Group/Identity</b>, <b>Edit/Delete</b>, and per-request <b>Injection</b> badge in Replay.",
      side: "top",
    }),

    // Decoder
    s('[data-tour="tab-Decoder"]', {
      title: "Decoder — crypto playground",
      description: "Click <b>Decoder</b>. Chain <b>recipes</b> (Base64, URL, Hex, JWT, Gzip, etc.), live stage outputs, detection & padding toggle.",
      side: "bottom",
    }, true),
    s('[data-tour="decoder"]', {
      title: "Decoder workspace — crypto",
      description:
        "Top: <b>Input</b> textarea (with <b>Smart decode</b>). Middle: <b>Recipe</b> pipeline (add steps: Base64, URL, Hex, JWT, Gzip, etc., reorder, delete). Bottom: <b>Stage outputs</b> (click to copy), <b>Detected</b> badge, <b>Padding</b> toggle, <b>Filter</b>.",
      side: "top",
    }),

    // Comparer
    s('[data-tour="tab-Comparer"]', {
      title: "Comparer — diff engine",
      description: "Click <b>Comparer</b>. Side-by-side or stacked, char/line/word granularity, diff stats.",
      side: "bottom",
    }, true),
    s('[data-tour="comparer"]', {
      title: "Comparer workspace — diff",
      description:
        "Two <b>CodeEditors</b> left/right, top bar: <b>Granularity</b> (char/line/word), <b>Layout</b> (side/stacked), <b>Compare</b> button, stats: <b>+additions / -deletions / ~unchanged</b>. Diff chunks colored.",
      side: "top",
    }),

    // Scope
    s('[data-tour="tab-Scope"]', {
      title: "Scope — define your target",
      description:
        "Click <b>Scope</b>. Allow-list (in-scope) + deny-list (out-of-scope) with subdomain/regex. Affects History, Site Map, and <b>Only intercept in-scope</b>.",
      side: "bottom",
    }, true),
    s('[data-tour="scope"]', {
      title: "Scope manager — target",
      description:
        "Table: <b>Pattern, Regex, Subdomains, In-scope</b> toggle. Buttons: <b>Add</b> (pattern, regex, subdomains), <b>Edit/Remove</b>, <b>Include subdomains</b> checkbox, <b>Regex</b> badge. Affects History & Site Map & Intercept in-scope.",
      side: "top",
    }),

    // Forge AI
    s('[data-tour="tab-Forge"]', {
      title: "Forge — AI assistant",
      description:
        "Click <b>Forge</b>. Chat with tool-calling (read history, open replay, run decoder). Needs provider config in Settings → AI.",
      side: "bottom",
    }, true),
    s('[data-tour="forge"]', {
      title: "Forge workspace — AI",
      description:
        "Left: <b>Chats</b> list (new, rename, delete), <b>Draft</b> input. Center: messages (user/assistant/tool), <b>Tool approvals</b> (Approve/Trust/Reject), <b>Usage</b> tokens. Settings → AI needed for provider.",
      side: "top",
    }),

    // Settings
    s('[data-tour="tab-Settings"]', {
      title: "Settings — make it yours",
      description: "Click <b>Settings</b> (gear icon also works). Proxy, Display, Storage, Keyboard, Certificates, AI, Misc, About.",
      side: "bottom",
    }, true),
    s('[data-tour="settings-panel"]', {
      title: "Settings panel",
      description:
        "The left sidebar switches sections: Proxy, Display, Storage, Keyboard, Certificates, AI, Misc, About. Your <b>Proxy settings</b> (listener & match/replace) are here, not in the Intercept tab.",
      side: "left",
    }),

    s('[data-tour="settings-section-proxy"]', {
      title: "Settings → Proxy section",
      description:
        "Click <b>Proxy</b> in the left sidebar to open all proxy settings. <b>Try it: click Proxy.</b>",
      side: "right",
    }, true),

    s('[data-tour="proxy-listener"]', {
      title: "Listener — where browsers connect",
      description:
        "<b>Bind address + Port</b> (default 127.0.0.1:8080). Change requires proxy restart if running. This is what you set as your browser’s HTTP proxy.",
      side: "right",
    }),

    s('[data-tour="proxy-traffic"]', {
      title: "Traffic handling",
      description:
        "Control <b>Compression</b> (decompress all/text/pass-through), <b>Upstream timeout</b>, and <b>Only intercept in-scope</b> toggle. That toggle now safely keeps pending queue if scope is empty.",
      side: "right",
    }),

    s('[data-tour="proxy-upstream"]', {
      title: "Upstream proxy — proxy your proxy",
      description:
        "Route Witness’s outbound traffic through another <b>HTTP/SOCKS5</b> proxy (separate from the listener). Set host/port/creds if you’re chaining through corporate proxy.",
      side: "left",
    }),

    s('[data-tour="proxy-match-replace"]', {
      title: "Match and replace — automatic rewrites",
      description:
        "Auto-replace text on the fly. Rules run top-to-bottom. Choose precise type: <b>Request host, header, body, param name/value</b> and <b>Response header, body, param name/value</b>. Supports literal or regex with <code>$1</code> captures. Rewrites happen <b>before</b> upstream (request) and after decompression (response).",
      side: "top",
    }),

    s('[data-tour="proxy-interception"]', {
      title: "Interception — pause & edit",
      description:
        "Enable <b>Intercept requests / responses</b>, filter by <b>Data-type</b> (HTML/JS/JSON/Images…), and build <b>Request/Response rules</b> (domain, method, content-type, scope). Pending interceptions appear in the Intercept table above.",
      side: "top",
    }),

    // Logs (if enabled) — hidden by default, but we still cover
    s('[data-tour="tab-Logs"]', {
      title: "Logs — when you need it",
      description:
        "Enable in Settings → Misc to show <b>Logs</b> tab. In-memory ring buffer with `witness-event` bridge. Useful for debugging.",
      side: "bottom",
    }),

    // Project / titlebar
    s('[data-tour="project-save"]', {
      title: "Save your work",
      description:
        "Project is a portable <code>.wns</code> archive (SQLite + bodies + workspace). Save often; autosave checkpoints every 30s. Temporary sessions can be promoted.",
      side: "bottom",
      align: "end",
    }),

    // Final
    s(undefined, {
      title: "You are ready!",
      description:
        "That was the full app tour — <b>Proxy, History, Site Map, Replay, Fuzz, Organizer, ID+, Decoder, Comparer, Scope, Forge, Settings</b>. <br><br>• Replay the tour anytime via <b>Settings → About → Replay tutorial</b>.<br>• Click the highlighted buttons to traverse, or press <b>Next</b> and we will click for you.<br><br>Witness is MIT, no telemetry. If it saves you hours, consider <b>Donate</b> in Settings → About. Happy hacking!",
    }),
  ];
}

export function createTutorial(
  mode: TutorialMode = getTutorialMode(),
  onFinish?: () => void,
): Driver {
  const steps = buildSteps();
  let driver: Driver;
  let finished = false;
  const finishOnce = () => {
    if (finished) return;
    finished = true;
    try {
      onFinish?.();
    } catch {
      // onFinish is best-effort.
    }
  };
  driver = createDriver({
    ...withAmazingDefaults(mode),
    steps,
    onDestroyed: () => {
      clearAskClickListeners();
      cleanupTourPulse();
      finishOnce();
    },
    onDestroyStarted: () => {
      // also ensure finish on destroy started (covers skip via overlay/Esc)
      // delay to avoid double call with onDestroyed
      window.setTimeout(finishOnce, 50);
    },
  } satisfies Config);
  return driver;
}

export function startTutorial(
  mode: TutorialMode = getTutorialMode(),
  onFinish?: () => void,
): Driver | null {
  if (typeof window === "undefined" || typeof document === "undefined") return null;
  const driver = createTutorial(mode, onFinish);
  try {
    driver.drive();
  } catch (e) {
    console.warn("[tutorial] failed to start", e);
  }
  return driver;
}
