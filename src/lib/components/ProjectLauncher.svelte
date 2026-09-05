<script lang="ts">
  import type { RecentProject } from "$lib/types";
  import { formatRelativeTime } from "$lib/format";
  import ConfirmDialog from "./ConfirmDialog.svelte";
  import RecycleBinIcon from "./RecycleBinIcon.svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  let {
    recentProjects,
    ready,
    busy,
    onOpen,
    onCreate,
    onTemporary,
    onBrowse,
    onConfigureAi,
    onTour,
    onDelete,
  }: {
    recentProjects: RecentProject[];
    ready: boolean;
    busy: boolean;
    onOpen: (path?: string) => void;
    onCreate: (name: string, path: string) => void;
    onTemporary: () => void;
    onBrowse: () => Promise<string | null>;
    onConfigureAi: () => void;
    onTour: () => void;
    onDelete: (path: string) => void;
  } = $props();
  let page = $state<"home" | "new">("home");
  let name = $state("");
  let path = $state("");
  let deleteTarget = $state<RecentProject | null>(null);
  async function browse() {
    const selected = await onBrowse();
    if (selected) path = selected;
  }
  function create() {
    if (!name.trim() || !path.trim()) return;
    onCreate(name.trim(), path.trim());
  }
  function requestDelete(project: RecentProject) {
    if (busy) return;
    deleteTarget = project;
  }
  function confirmDelete() {
    const project = deleteTarget;
    deleteTarget = null;
    if (project) onDelete(project.path);
  }
  function relativeDate(value: string) {
    return formatRelativeTime(value);
  }
  async function openExternal(url: string) {
    try {
      await openUrl(url);
    } catch {
      window.open(url, "_blank", "noopener");
    }
  }
</script>
<main class="launcher">
  <section class="launcher-card" aria-labelledby="launcher-title">
    <header class="launcher-brand">
      <div class="launcher-brand-header">
        <div class="launcher-mark" aria-hidden="true">
          <img src="/witness_app_icon.png" alt="" />
        </div>
        <h1 id="launcher-title">Witness</h1>
      </div>
      <div>
        <p>Modern Web Security Testing Toolkit</p>
      </div>
    </header>
    {#if page === "home"}
      <div class="launcher-actions">
        <button
          class="action"
          disabled={!ready || busy}
          onclick={() => (page = "new")}
        >
          <svg viewBox="0 0 24 24" aria-hidden="true"
            ><path d="M12 5v14M5 12h14"></path></svg
          >
          <span
            ><strong>New project</strong><small
              >Start a persistent workspace</small
            ></span
          >
          <b aria-hidden="true">›</b>
        </button>
        <button
          class="action"
          disabled={!ready || busy}
          onclick={() => onOpen()}
        >
          <svg viewBox="0 0 24 24" aria-hidden="true"
            ><path d="M3.5 7.5h6l2-2h9v13h-17Z"></path><path d="M3.5 9.5h17"
            ></path></svg
          >
          <span
            ><strong>Open project</strong><small
              >Open an existing project</small
            ></span
          >
          <b aria-hidden="true">›</b>
        </button>
        <button class="action" disabled={!ready || busy} onclick={onTemporary}>
          <svg viewBox="0 0 24 24" aria-hidden="true"
            ><path
              d="M8 4h8M9 4v5l-4 7.5A2.3 2.3 0 0 0 7 20h10a2.3 2.3 0 0 0 2-3.5L15 9V4"
            ></path><path d="M7.5 15h9"></path></svg
          >
          <span
            ><strong>Temporary session</strong><small
              >Launch a temporary session</small
            ></span
          >
          <b aria-hidden="true">›</b>
        </button>
      </div>
      <button
        class="configure-ai"
        type="button"
        disabled={!ready || busy}
        onclick={onTour}
        style="margin: 22px auto 0; display: block;"
        data-tour="take-tour"
      >
        Take a Quick Tour <span class="configure-ai-arrow" aria-hidden="true">→</span>
      </button>
      <div class="recent">
        <div class="recent-heading">
          <h2>Recent projects</h2>
          <span>{recentProjects.length}</span>
          <div class="recent-heading-line" aria-hidden="true"></div>
        </div>
        {#if recentProjects.length}
          <div class="recent-list" aria-label="Recent projects">
            {#each recentProjects as project (project.path)}
              <div class="recent-project-row">
                <button class="recent-project-open" disabled={busy} onclick={() => onOpen(project.path)}>
                <span class="project-info"
                  ><strong>{project.name}</strong><small data-tooltip={project.path}
                    >{project.path}</small
                  ></span
                ></button>
                <button
                  class="recent-project-delete"
                  type="button"
                  aria-label={`Delete ${project.name}`}
                  data-tooltip="Delete project"
                  disabled={busy}
                  onclick={(event) => { event.stopPropagation(); requestDelete(project); }}
                >
                  <RecycleBinIcon size={14} />
                </button>
                <time datetime={project.lastOpened}>{relativeDate(project.lastOpened)}</time>
              </div>
            {/each}
          </div>
        {:else}
          <div class="recent-empty">
            Projects you create or open will appear here.
          </div>
        {/if}
      </div>
      <div class="launcher-footer">
        <button class="configure-ai" type="button" disabled={!ready || busy} onclick={onConfigureAi}>
          Configure AI Inference <span class="configure-ai-arrow" aria-hidden="true">→</span>
        </button>
        <div class="launcher-links">
        <div class="launcher-link github">
          <a
            href="https://github.com/northcorelabs/witness"
            aria-label="Witness GitHub Link"
            onclick={(event) => {
              event.preventDefault();
              void openExternal("https://github.com/northcorelabs/witness");
            }}
          ><svg
            fill="#FFFFFF"
            width="20px"
            height="20px"
            viewBox="0 -0.5 25 25"
            xmlns="http://www.w3.org/2000/svg"
            style="--darkreader-inline-fill: var(--darkreader-background-000000, #000000);"
            data-darkreader-inline-fill=""
            ><g id="SVGRepo_bgCarrier" stroke-width="0"></g><g
              id="SVGRepo_tracerCarrier"
              stroke-linecap="round"
              stroke-linejoin="round"
            ></g><g id="SVGRepo_iconCarrier"
              ><path
                d="m12.301 0h.093c2.242 0 4.34.613 6.137 1.68l-.055-.031c1.871 1.094 3.386 2.609 4.449 4.422l.031.058c1.04 1.769 1.654 3.896 1.654 6.166 0 5.406-3.483 10-8.327 11.658l-.087.026c-.063.02-.135.031-.209.031-.162 0-.312-.054-.433-.144l.002.001c-.128-.115-.208-.281-.208-.466 0-.005 0-.01 0-.014v.001q0-.048.008-1.226t.008-2.154c.007-.075.011-.161.011-.249 0-.792-.323-1.508-.844-2.025.618-.061 1.176-.163 1.718-.305l-.076.017c.573-.16 1.073-.373 1.537-.642l-.031.017c.508-.28.938-.636 1.292-1.058l.006-.007c.372-.476.663-1.036.84-1.645l.009-.035c.209-.683.329-1.468.329-2.281 0-.045 0-.091-.001-.136v.007c0-.022.001-.047.001-.072 0-1.248-.482-2.383-1.269-3.23l.003.003c.168-.44.265-.948.265-1.479 0-.649-.145-1.263-.404-1.814l.011.026c-.115-.022-.246-.035-.381-.035-.334 0-.649.078-.929.216l.012-.005c-.568.21-1.054.448-1.512.726l.038-.022-.609.384c-.922-.264-1.981-.416-3.075-.416s-2.153.152-3.157.436l.081-.02q-.256-.176-.681-.433c-.373-.214-.814-.421-1.272-.595l-.066-.022c-.293-.154-.64-.244-1.009-.244-.124 0-.246.01-.364.03l.013-.002c-.248.524-.393 1.139-.393 1.788 0 .531.097 1.04.275 1.509l-.01-.029c-.785.844-1.266 1.979-1.266 3.227 0 .025 0 .051.001.076v-.004c-.001.039-.001.084-.001.13 0 .809.12 1.591.344 2.327l-.015-.057c.189.643.476 1.202.85 1.693l-.009-.013c.354.435.782.793 1.267 1.062l.022.011c.432.252.933.465 1.46.614l.046.011c.466.125 1.024.227 1.595.284l.046.004c-.431.428-.718 1-.784 1.638l-.001.012c-.207.101-.448.183-.699.236l-.021.004c-.256.051-.549.08-.85.08-.022 0-.044 0-.066 0h.003c-.394-.008-.756-.136-1.055-.348l.006.004c-.371-.259-.671-.595-.881-.986l-.007-.015c-.198-.336-.459-.614-.768-.827l-.009-.006c-.225-.169-.49-.301-.776-.38l-.016-.004-.32-.048c-.023-.002-.05-.003-.077-.003-.14 0-.273.028-.394.077l.007-.003q-.128.072-.08.184c.039.086.087.16.145.225l-.001-.001c.061.072.13.135.205.19l.003.002.112.08c.283.148.516.354.693.603l.004.006c.191.237.359.505.494.792l.01.024.16.368c.135.402.38.738.7.981l.005.004c.3.234.662.402 1.057.478l.016.002c.33.064.714.104 1.106.112h.007c.045.002.097.002.15.002.261 0 .517-.021.767-.062l-.027.004.368-.064q0 .609.008 1.418t.008.873v.014c0 .185-.08.351-.208.466h-.001c-.119.089-.268.143-.431.143-.075 0-.147-.011-.214-.032l.005.001c-4.929-1.689-8.409-6.283-8.409-11.69 0-2.268.612-4.393 1.681-6.219l-.032.058c1.094-1.871 2.609-3.386 4.422-4.449l.058-.031c1.739-1.034 3.835-1.645 6.073-1.645h.098-.005zm-7.64 17.666q.048-.112-.112-.192-.16-.048-.208.032-.048.112.112.192.144.096.208-.032zm.497.545q.112-.08-.032-.256-.16-.144-.256-.048-.112.08.032.256.159.157.256.047zm.48.72q.144-.112 0-.304-.128-.208-.272-.096-.144.08 0 .288t.272.112zm.672.673q.128-.128-.064-.304-.192-.192-.32-.048-.144.128.064.304.192.192.32.044zm.913.4q.048-.176-.208-.256-.24-.064-.304.112t.208.24q.24.097.304-.096zm1.009.08q0-.208-.272-.176-.256 0-.256.176 0 .208.272.176.256.001.256-.175zm.929-.16q-.032-.176-.288-.144-.256.048-.224.24t.288.128.225-.224z"
              ></path></g
            ></svg></a>
        </div>
        <div class="launcher-link wiki">
          <a
            href="https://witness.northcorelabs.tech/"
            aria-label="Witness Wili"
            onclick={(event) => {
              event.preventDefault();
              void openExternal("https://witness.northcorelabs.tech/wiki");
            }}
          ><svg fill="#ffffff" width="20px" height="20px" viewBox="0 0 512 512" id="Layer_1" enable-background="new 0 0 512 512" xmlns="http://www.w3.org/2000/svg">
<g>
<path d="m437.02 74.98c-48.353-48.352-112.64-74.98-181.02-74.98s-132.667 26.628-181.02 74.98-74.98 112.64-74.98 181.02 26.628 132.667 74.98 181.02 112.64 74.98 181.02 74.98 132.667-26.628 181.02-74.98 74.98-112.64 74.98-181.02-26.628-132.667-74.98-181.02zm-2.132 315.679c-15.31-10.361-31.336-19.314-47.952-26.789 7.339-28.617 11.697-59.688 12.784-91.87h79.702c-3.144 44.336-19.244 85.147-44.534 118.659zm-402.31-118.659h79.702c1.088 32.183 5.446 63.254 12.784 91.87-16.616 7.475-32.642 16.427-47.952 26.789-25.29-33.512-41.39-74.323-44.534-118.659zm44.53-150.654c15.31 10.362 31.336 19.315 47.954 26.79-7.338 28.615-11.695 59.683-12.783 91.864h-79.701c3.144-44.334 19.243-85.142 44.53-118.654zm283.519-42.581c-5.863-10.992-12.198-20.911-18.935-29.713 27.069 11.25 51.473 27.658 71.977 47.997-11.625 7.638-23.702 14.369-36.155 20.185-4.886-13.664-10.528-26.547-16.887-38.469zm-12.965 50.404c-29.211 9.792-60.039 14.831-91.662 14.831s-62.451-5.039-91.662-14.831c20.463-58.253 54.273-97.169 91.662-97.169s71.199 38.916 91.662 97.169zm-203.359 110.831c1.056-28.342 4.885-55.421 10.937-80.116 32.136 10.644 66.018 16.116 100.76 16.116s68.624-5.472 100.76-16.116c6.053 24.695 9.881 51.773 10.937 80.116zm223.394 32c-1.057 28.344-4.885 55.424-10.938 80.12-32.139-10.646-66.02-16.12-100.759-16.12s-68.62 5.474-100.759 16.12c-6.053-24.696-9.882-51.776-10.938-80.12zm-216.324-193.235c-6.358 11.922-12 24.805-16.887 38.468-12.452-5.815-24.53-12.547-36.155-20.185 20.503-20.34 44.907-36.747 71.977-47.997-6.737 8.803-13.073 18.722-18.935 29.714zm-16.886 316.008c4.886 13.661 10.528 26.542 16.885 38.462 5.863 10.992 12.198 20.911 18.935 29.713-27.067-11.25-51.469-27.655-71.971-47.992 11.625-7.637 23.701-14.368 36.151-20.183zm29.853-11.938c29.213-9.794 60.04-14.835 91.66-14.835s62.447 5.041 91.66 14.835c-20.463 58.251-54.272 97.165-91.66 97.165s-71.197-38.914-91.66-97.165zm196.287 50.4c6.357-11.92 11.999-24.801 16.885-38.462 12.451 5.815 24.527 12.547 36.151 20.183-20.502 20.337-44.904 36.743-71.971 47.992 6.737-8.802 13.073-18.721 18.935-29.713zm39.093-193.235c-1.088-32.18-5.445-63.249-12.783-91.864 16.618-7.475 32.645-16.428 47.954-26.79 25.287 33.511 41.386 74.319 44.53 118.654z"/>
</g>
</svg></a>
        </div>
        </div>
      </div>
    {:else}
      <form
        class="new-project"
        onsubmit={(event) => {
          event.preventDefault();
          create();
        }}
      >
        <button
          class="text-button back"
          type="button"
          disabled={busy}
          onclick={() => (page = "home")}
        >
          <span aria-hidden="true">←</span> Back
        </button>
        <div class="page-heading">
          <h2>Create a new project</h2>
          <p>
            Choose a destination ending in .wns to create a portable single-file
            project.
          </p>
        </div>
        <label>
          <span>Project name</span>
          <input
            bind:value={name}
            placeholder="Client Security Review"
            disabled={busy}
          />
        </label>
        <label>
          <span>Project .wns file</span>
          <div class="path-input">
            <input
              bind:value={path}
              placeholder="/path/to/project.wns"
              disabled={busy}
            />
            <button class="text-button" type="button" disabled={busy} onclick={browse}
              >Browse…</button
            >
          </div>
        </label>
        <div class="create-actions">
          <button class="text-button" type="button" disabled={busy} onclick={() => (page = "home")}
            >Cancel</button
          >
          <button
            class="text-button confirm"
            type="submit"
            disabled={busy || !name.trim() || !path.trim()}
          >
            {busy ? "Creating…" : "Create Project"}
          </button>
        </div>
      </form>
    {/if}
    {#if busy}<div class="busy-line" aria-label="Working"></div>{/if}
    <ConfirmDialog
      open={deleteTarget !== null}
      title="Delete project?"
      message={`Permanently delete “${deleteTarget?.name ?? "this project"}” and its .wns file?`}
      confirmLabel="Delete project"
      onConfirm={confirmDelete}
      onCancel={() => (deleteTarget = null)}
    />
  </section>
</main>
<style>
  .launcher {
    --pl-base: #232323;
    --bg: var(--pl-base);
    --surface: color-mix(in srgb, var(--pl-base), white 6%);
    --surface-2: color-mix(in srgb, var(--pl-base), white 12%);
    --border: color-mix(in srgb, var(--pl-base), white 20%);
    --text: color-mix(in srgb, var(--pl-base), white 88%);
    --muted: color-mix(in srgb, var(--pl-base), white 58%);
    --accent: color-mix(in srgb, var(--pl-base), white 72%);
    --accent-hover: color-mix(in srgb, var(--pl-base), white 84%);
    position: fixed;
    inset: 0;
    display: grid;
    place-items: center;
    min-width: 620px;
    min-height: 480px;
    overflow: auto;
    color: var(--text);
    background: radial-gradient(
        circle at 50% -20%,
        rgb(255 255 255 / 8%),
        transparent 47%
      ),
      var(--bg);
    font:
      var(--font-size-body)/1.4 Inter,
      "SF Pro Text",
      ui-sans-serif,
      system-ui,
      sans-serif;
  }
  .launcher-card {
    height: 100%;
    width: 100%;
    padding: 28px 28px 12px;
    border: 1px solid var(--border);
    background: rgb(from var(--surface) r g b / 0.94);
    box-shadow: 0 28px 80px rgb(0 0 0 / 42%);
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }
  .launcher-brand {
    display: flex;
    align-items: center;
    margin-bottom: 16px;
    padding-bottom: 6px;
    justify-content: center;
    flex-direction: column;
  }
  .launcher-brand-header {
    display: flex;
    justify-content: center;
    align-items: center;
    gap: 4px;
  }
  .launcher-mark {
    display: grid;
    place-items: center;
    width: 72px;
    height: 72px;
    overflow: hidden;
  }
  .launcher-mark img {
    display: block;
    width: 56px;
    height: 56px;
    object-fit: contain;
  }
  .launcher-footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    margin-top: auto;
    padding-top: 14px;
  }
  .configure-ai {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 4px 0;
    color: var(--muted);
    border: 0;
    background: transparent;
    font-size: var(--font-size-compact);
    font-weight: 650;
  }
  .configure-ai-arrow { font-size: 14px; line-height: 1; }
  .configure-ai:hover:not(:disabled) { color: var(--text); }
  .launcher-links {
    display: flex;
    gap: 10px;
    margin-right: -6px;
    transform: translateX(6px);
  }
  .launcher-link:hover {
    transform: translateY(-1px);
    cursor: pointer;
  }
  .launcher-link a {
    display: grid;
    place-items: center;
    color: inherit;
    text-decoration: none;
  }
  .launcher-link a:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
    border-radius: 4px;
  }
  h1,
  h2,
  p {
    margin: 0;
  }
  h1 {
    font-size: calc(var(--font-size-title) + 22px);
    letter-spacing: -0.035em;
    line-height: 1;
    font-weight: 600;
  }
  .launcher-brand p {
    margin-top: 6px;
    color: var(--muted);
    font-size: calc(var(--font-size-body) + 2px);
  }
  .page-heading p {
    color: var(--muted);
    font-size: var(--font-size-body);
  }
  .launcher-actions {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 8px;
  }
  button {
    color: inherit;
    font: inherit;
    cursor: pointer;
  }
  button:disabled {
    opacity: 0.5;
    cursor: wait;
  }
  .action {
    display: grid;
    grid-template-columns: 28px minmax(0, 1fr) auto;
    align-items: center;
    gap: 9px;
    min-height: 58px;
    padding: 8px 10px;
    text-align: left;
    border: 1px solid var(--border);
    border-radius: 9px;
    background: var(--surface-2);
  }
  .action:hover:not(:disabled) {
    border-color: color-mix(in srgb, var(--border), white 14%);
  }
  .action svg {
    width: 23px;
    fill: none;
    stroke: var(--accent);
    stroke-width: 1.8;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .action span {
    display: grid;
    gap: 2px;
    min-width: 0;
  }
  .action strong {
    font-size: var(--font-size-body);
  }
  .action small {
    color: var(--muted);
    font-size: var(--font-size-compact);
    line-height: 1.3;
  }
  .action b {
    color: var(--muted);
    font-size: var(--font-size-heading);
    font-weight: 400;
  }
  .recent {
    margin-top: 8px;
    padding-top: 6px;
  }
  .recent-heading {
    display: flex;
    align-items: center;
    gap: 7px;
    margin-bottom: 9px;
  }
  .recent-heading h2 {
    font-size: var(--font-size-body);
    text-transform: uppercase;
    letter-spacing: 0.09em;
  }
  .recent-heading span {
    padding: 1px 6px;
    color: var(--muted);
    border-radius: 999px;
    background: var(--surface-2);
    font-size: var(--font-size-compact);
  }
  .recent-heading-line {
    flex: 1;
    height: 1px;
    margin-left: 8px;
    border-radius: 1px;
    background: var(--border);
    opacity: 0.45;
  }
  .recent-list {
    display: grid;
    gap: 4px;
    max-height: 152px;
    overflow-y: auto;
    overflow-x: hidden;
    scrollbar-width: none;
    -ms-overflow-style: none;
  }
  .recent-list::-webkit-scrollbar {
    display: none;
  }
  .recent-project-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto auto;
    align-items: center;
    gap: 9px;
    width: 100%;
    min-height: 48px;
    padding: 6px 8px;
    border: 1px solid transparent;
    border-radius: 7px;
    background: transparent;
  }
  .recent-project-row:hover {
    border-color: var(--border);
    background: var(--surface-2);
  }
  .recent-project-open {
    display: block;
    min-width: 0;
    padding: 0;
    text-align: left;
    border: 0;
    background: transparent;
  }
  .recent-project-open:hover:not(:disabled) { color: var(--text); }
  .recent-project-delete {
    display: grid;
    width: 24px;
    height: 24px;
    place-items: center;
    padding: 0;
    color: var(--muted);
    border: 0;
    border-radius: 5px;
    background: transparent;
  }
  .recent-project-delete:hover:not(:disabled) {
    color: #f87171;
    background: rgb(248 113 113 / 10%);
  }
  .project-info {
    display: grid;
    min-width: 0;
  }
  .recent-list strong {
    font-size: var(--font-size-body);
  }
  .recent-list small {
    overflow: hidden;
    color: var(--muted);
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--font-size-compact);
  }
  .recent-list time {
    color: var(--muted);
    font-size: var(--font-size-compact);
  }
  .recent-empty {
    display: grid;
    place-items: center;
    height: 78px;
    color: var(--muted);
    border: 1px dashed var(--border);
    border-radius: 8px;
    font-size: var(--font-size-compact);
  }
  .configure-ai:disabled { cursor: wait; }
  .new-project {
    display: grid;
    gap: 17px;
  }
  .back {
    width: fit-content;
    margin-top: 14px;
    padding: 4px 7px;
    color: var(--muted);
    border: 0;
    border-radius: 5px;
    background: transparent;
    font-size: var(--font-size-compact);
  }
  :global(button.back.text-button:hover:not(:disabled)) {
    color: var(--text);
    background: transparent;
  }
  .page-heading {
    display: grid;
    gap: 4px;
    margin-bottom: 4px;
  }
  .page-heading h2 {
    font-size: var(--font-size-title);
    letter-spacing: -0.025em;
  }
  label {
    display: grid;
    gap: 6px;
    color: color-mix(in srgb, var(--pl-base), white 76%);
    font-size: var(--font-size-compact);
    font-weight: 650;
  }
  input {
    width: 100%;
    height: 37px;
    padding: 0 10px;
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: 7px;
    outline: none;
    background: color-mix(in srgb, var(--pl-base), black 18%);
    font:
      var(--font-size-body) ui-monospace,
      SFMono-Regular,
      Menlo,
      monospace;
  }
  .path-input {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 6px;
  }
  .path-input button,
  .create-actions button {
    padding: 0 12px;
    border: 1px solid var(--border);
    border-radius: 7px;
    background: var(--surface-2);
    font-size: var(--font-size-compact);
  }
  .create-actions {
    display: flex;
    justify-content: flex-end;
    gap: 7px;
    margin-top: 8px;
  }
  .create-actions button {
    min-height: 34px;
  }
  .create-actions .confirm {
    color: #fff;
    border-color: var(--success, #279630);
    background: var(--success, #279630);
    font-weight: 700;
  }
  .create-actions .confirm:hover:not(:disabled) {
    color: #fff;
    border-color: color-mix(in srgb, var(--success, #279630) 82%, #000);
    background: color-mix(in srgb, var(--success, #279630) 82%, #000);
  }
  .busy-line {
    position: absolute;
    right: 0;
    bottom: 0;
    left: 0;
    height: 2px;
    background: linear-gradient(90deg, transparent, var(--accent), transparent);
    animation: busy 1s linear infinite;
  }
  @keyframes busy {
    from {
      transform: translateX(-70%);
    }
    to {
      transform: translateX(70%);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .busy-line {
      animation: none;
    }
  }
</style>
