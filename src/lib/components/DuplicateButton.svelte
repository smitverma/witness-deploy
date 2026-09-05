<script lang="ts">
  let {
    label = "Duplicate tab",
    onclick,
  }: {
    label?: string;
    onclick: () => void;
  } = $props();
</script>

<button class="duplicate-button" type="button" aria-label={label} data-tooltip={label} onclick={onclick}>
  <svg viewBox="0 0 24 24" aria-hidden="true">
    <rect x="9" y="9" width="10" height="11" rx="1"></rect>
    <path d="M15 9V5H5v11h4"></path>
  </svg>
</button>

<style>
  .duplicate-button {
    display: grid;
    place-items: center;
    width: 28px;
    min-width: 28px;
    height: 28px;
    min-height: 28px;
    padding: 0;
    border: 1px solid var(--border-strong);
    border-radius: 4px;
    color: var(--muted);
    background: var(--surface-2);
    cursor: pointer;
  }
  .duplicate-button:hover:not(:disabled) {
    color: var(--text);
    border-color: color-mix(in srgb, var(--accent) 42%, var(--border-strong));
    background: var(--surface-3);
  }
  .duplicate-button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  .duplicate-button:disabled {
    opacity: .45;
    cursor: not-allowed;
  }
  .duplicate-button svg {
    width: 15px;
    height: 15px;
    fill: none;
    stroke: currentColor;
    stroke-width: var(--svgbuttonstrokewidth, 1.5);
    stroke-linecap: round;
    stroke-linejoin: round;
  }
</style>
