<script lang="ts">
  type ToggleChangeEvent = Event & { currentTarget: HTMLInputElement };

  let {
    checked = $bindable(false),
    disabled = false,
    ariaLabel,
    onchange,
    onclick,
  }: {
    checked?: boolean;
    disabled?: boolean;
    ariaLabel?: string;
    onchange?: (event: ToggleChangeEvent) => void;
    onclick?: (event: MouseEvent) => void;
  } = $props();
</script>

<span class="toggle-control">
  <input
    type="checkbox"
    bind:checked
    aria-label={ariaLabel}
    disabled={disabled}
    onchange={onchange}
    onclick={onclick}
  />
  <span class="toggle-track" aria-hidden="true"><span></span></span>
</span>

<style>
  .toggle-control {
    position: relative;
    display: inline-flex;
    flex: 0 0 30px;
    width: 30px;
    height: 18px;
    align-items: center;
    vertical-align: middle;
  }
  .toggle-control input {
    position: absolute;
    z-index: 1;
    inset: 0;
    width: 100%;
    height: 100%;
    margin: 0;
    opacity: 0;
    cursor: pointer;
  }
  .toggle-track {
    display: inline-flex;
    width: 100%;
    height: 100%;
    box-sizing: border-box;
    align-items: center;
    padding: 2px;
    border: 1px solid #4b5563;
    border-radius: 999px;
    background: #202630;
    pointer-events: none;
    transition: background 0.15s ease, border-color 0.15s ease;
  }
  .toggle-track > span {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: #858f9d;
    transition: transform 0.15s ease, background 0.15s ease;
  }
  .toggle-control input:checked + .toggle-track {
    border-color: color-mix(in srgb, var(--success) 70%, #4b5563);
    background: color-mix(in srgb, var(--success) 55%, #202630);
  }
  .toggle-control input:checked + .toggle-track > span {
    transform: translateX(12px);
    background: #fff;
  }
  .toggle-control input:disabled + .toggle-track {
    opacity: 0.45;
  }
  .toggle-control input:focus-visible + .toggle-track {
    outline: 2px solid var(--accent, #f59e0b);
    outline-offset: 2px;
  }
</style>
