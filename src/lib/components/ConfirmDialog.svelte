<script lang="ts">
  let {
    open = false,
    title,
    message,
    confirmLabel = "Confirm",
    cancelLabel = "Cancel",
    secondaryLabel,
    busy = false,
    danger = true,
    onConfirm,
    onCancel,
    onSecondary,
  }: {
    open?: boolean;
    title: string;
    message: string;
    confirmLabel?: string;
    cancelLabel?: string;
    secondaryLabel?: string;
    busy?: boolean;
    danger?: boolean;
    onConfirm: () => void | Promise<void>;
    onCancel: () => void;
    onSecondary?: () => void | Promise<void>;
  } = $props();

  let dialogEl: HTMLDivElement | undefined = $state();
  let cancelBtn: HTMLButtonElement | undefined = $state();

  $effect(() => {
    if (open) cancelBtn?.focus();
  });

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === "Escape" && !busy) {
      event.stopPropagation();
      onCancel();
      return;
    }
    if (event.key !== "Tab" || !dialogEl) return;
    const focusables = [...dialogEl.querySelectorAll<HTMLElement>("button:not(:disabled)")];
    if (!focusables.length) return;
    const first = focusables[0];
    const last = focusables[focusables.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  }
</script>

{#if open}
  <div class="confirm-backdrop" role="presentation" onclick={(event) => { if (event.target === event.currentTarget && !busy) onCancel(); }}>
    <div
      class="confirm-dialog"
      role="alertdialog"
      aria-modal="true"
      aria-labelledby="confirm-dialog-title"
      aria-describedby="confirm-dialog-message"
      tabindex="-1"
      bind:this={dialogEl}
      onkeydown={handleKeydown}
      onclick={(event) => event.stopPropagation()}
    >
      <p class="confirm-eyebrow">CONFIRMATION</p>
      <h2 id="confirm-dialog-title">{title}</h2>
      <p id="confirm-dialog-message" class="confirm-message">{message}</p>
      <div class="confirm-actions">
        <button bind:this={cancelBtn} class="text-button" type="button" disabled={busy} onclick={onCancel}>{cancelLabel}</button>
        {#if secondaryLabel && onSecondary}<button class="text-button" type="button" disabled={busy} onclick={() => void onSecondary?.()}>{secondaryLabel}</button>{/if}
        <button class:text-button-danger={danger} class="text-button" type="button" disabled={busy} onclick={() => void onConfirm()}>
          {busy ? "Working…" : confirmLabel}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .confirm-backdrop {
    position: fixed;
    z-index: 120;
    inset: 0;
    display: grid;
    place-items: center;
    padding: 20px;
    background: color-mix(in srgb, var(--bg, #0c0f13) 70%, transparent);
    backdrop-filter: blur(3px);
  }
  .confirm-dialog {
    display: grid;
    gap: 10px;
    width: min(420px, calc(100vw - 32px));
    max-height: calc(100vh - 32px);
    overflow: auto;
    padding: 20px;
    color: var(--text, #dce1e8);
    font-family: Inter, "SF Pro Text", ui-sans-serif, system-ui, -apple-system, sans-serif;
    font-size: var(--font-size-body, 12px);
    line-height: 1.45;
    border: 1px solid var(--border-strong, #3a424d);
    border-radius: 8px;
    background: var(--surface, #12161b);
    box-shadow: var(--shadow, 0 20px 55px #0008);
  }
  .confirm-eyebrow {
    margin: 0;
    color: var(--accent, #9ca3af);
    font-size: var(--font-size-compact, 10px);
    font-weight: 800;
    letter-spacing: .13em;
  }
  .confirm-dialog h2 { margin: 0; font-size: var(--font-size-heading, 14px); }
  .confirm-message { margin: 0; color: var(--muted, #737e8b); font-size: var(--font-size-body, 12px); line-height: 1.5; }
  .confirm-actions { display: flex; justify-content: flex-end; gap: 7px; margin-top: 4px; }
  .confirm-actions .text-button-danger { color: #fff; border-color: var(--danger, #d92d4b); background: var(--danger, #d92d4b); }
  .confirm-actions .text-button-danger:hover:not(:disabled) { color: #fff; border-color: color-mix(in srgb, var(--danger, #d92d4b) 82%, #fff); background: color-mix(in srgb, var(--danger, #d92d4b) 82%, #000); }
</style>
