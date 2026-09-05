<script lang="ts">
  import { showErrorToast } from "$lib/errorToast";
  import type { ToastItem } from "$lib/toast";

  let {
    toasts = [],
    tone = "error",
    bottomPx = 46,
    onDismiss = (_id: number) => {},
  }: {
    toasts: ToastItem[];
    tone?: "error" | "info";
    bottomPx?: number;
    onDismiss?: (id: number) => void;
  } = $props();

  async function copyToast(toast: ToastItem) {
    try {
      await navigator.clipboard.writeText(toast.message);
    } catch (reason) {
      showErrorToast(reason);
      return;
    } finally {
      onDismiss(toast.id);
    }
  }
</script>

{#if toasts.length}
  <div class="toast-layer" style={`bottom:${bottomPx}px`}>
    {#each toasts as toast (toast.id)}
      <div
        class={tone === "error" ? "error-toast" : "info-toast"}
        role={tone === "error" ? "alert" : "status"}
        aria-live={tone === "error" ? "assertive" : "polite"}
      >
        <span class="message">{toast.message}</span>
        <div class="actions">
          <button
            type="button"
            aria-label={tone === "error" ? "Copy error" : "Copy message"}
            data-tooltip={tone === "error" ? "Copy error" : "Copy message"}
            onclick={() => void copyToast(toast)}
          >
            <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="9" y="9" width="11" height="11" rx="1"></rect><path d="M15 9V5a1 1 0 0 0-1-1H5a1 1 0 0 0-1 1v9a1 1 0 0 0 1 1h4"></path></svg>
          </button>
          <button
            type="button"
            aria-label="Dismiss"
            data-tooltip="Dismiss"
            onclick={() => onDismiss(toast.id)}
          ><svg viewBox="0 0 24 24" aria-hidden="true"><path d="m6 6 12 12M18 6 6 18"></path></svg></button>
        </div>
      </div>
    {/each}
  </div>
{/if}

<style>
  .toast-layer {
    position: fixed;
    z-index: 10000;
    inset: auto auto 46px 50%;
    display: grid;
    gap: 8px;
    width: min(560px, calc(100vw - 32px));
    transform: translateX(-50%);
    pointer-events: none;
  }
  .error-toast,
  .info-toast {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 11px 10px 11px 12px;
    color: #fff;
    border: 0;
    border-radius: 8px;
    font: var(--font-size-body, 12px)/1.45 Inter, "SF Pro Text", ui-sans-serif, system-ui, -apple-system, sans-serif;
    pointer-events: auto;
  }
  .error-toast {
    background: #641c25;
    box-shadow: none;
    animation: error-toast-pop 140ms ease-out, error-toast-fade 200ms ease-in 5s forwards;
  }
  .info-toast {
    background: #166534;
    box-shadow: 0 8px 24px rgb(0 0 0 / 35%);
    animation: info-toast-pop 140ms ease-out, info-toast-fade 200ms ease-in 5s forwards;
  }
  .message {
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  button {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    padding: 0;
    color: #fff;
    border: 0;
    background: transparent;
    cursor: pointer;
  }
  button svg {
    width: 17px;
    height: 17px;
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  @keyframes error-toast-pop {
    from { opacity: 0; transform: translateY(8px) scale(0.97); }
    to { opacity: 1; transform: translateY(0) scale(1); }
  }
  @keyframes error-toast-fade {
    to { opacity: 0; }
  }
  @keyframes info-toast-pop {
    from { opacity: 0; transform: translateY(8px) scale(0.97); }
    to { opacity: 1; transform: translateY(0) scale(1); }
  }
  @keyframes info-toast-fade {
    to { opacity: 0; }
  }
</style>
