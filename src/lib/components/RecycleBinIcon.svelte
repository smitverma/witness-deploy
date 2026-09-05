<script lang="ts">
  let { size = 15 }: { size?: number } = $props();
</script>

<svg
  viewBox="0 0 24 24"
  width={size}
  height={size}
  fill="none"
  aria-hidden="true"
  focusable="false"
>
  <path d="M5 7h14M10 7V5h4v2M8 10v8m4-8v8m4-8v8M7 7l1 13h8l1-13" />
</svg>

<style>
  svg {
    display: block;
    flex: 0 0 auto;
    stroke: var(--recycle-bin-stroke, currentColor);
    stroke-linecap: round;
    stroke-linejoin: round;
    stroke-width: var(--svgbuttonstrokewidth, 1.5);
  }
</style>
