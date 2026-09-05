<script lang="ts">
  import { onMount } from "svelte";

  export type ContextMenuItem = {
    id: string;
    label?: string;
    disabled?: boolean;
    danger?: boolean;
    separator?: boolean;
    markerColor?: string;
    submenu?: ContextMenuItem[];
  };

  let {
    x,
    y,
    items,
    onAction,
    onClose,
    ariaLabel = "Context menu",
    heading = "",
  }: {
    x: number;
    y: number;
    items: ContextMenuItem[];
    onAction: (id: string) => void;
    onClose: () => void;
    ariaLabel?: string;
    heading?: string;
  } = $props();

  let root: HTMLDivElement;
  let left = $state(0);
  let top = $state(0);
  let openSubmenu = $state<string | null>(null);
  let submenuSide = $state<"left" | "right">("right");
  let submenuVerticalSide = $state<"up" | "down">("down");

  $effect(() => {
    left = x;
    top = y;
  });

  onMount(() => {
    const reposition = () => {
      if (!root) return;
      const bounds = root.getBoundingClientRect();
      left = Math.max(8, Math.min(x, window.innerWidth - bounds.width - 8));
      top = Math.max(8, Math.min(y, window.innerHeight - bounds.height - 8));
    };
    const closeOutside = (event: PointerEvent) => {
      if (!root?.contains(event.target as Node)) onClose();
    };

    document.addEventListener("pointerdown", closeOutside, true);
    window.addEventListener("resize", reposition);
    requestAnimationFrame(() => {
      reposition();
      root?.querySelector<HTMLButtonElement>("button[data-menu-level='top']:not(:disabled)")?.focus();
    });
    return () => {
      document.removeEventListener("pointerdown", closeOutside, true);
      window.removeEventListener("resize", reposition);
    };
  });

  function openNested(item: ContextMenuItem, anchor?: HTMLElement) {
    if (item.disabled || !item.submenu?.length) return;
    openSubmenu = item.id;
    if (anchor) {
      const bounds = anchor.getBoundingClientRect();
      submenuSide = bounds.right + 220 > window.innerWidth - 8 ? "left" : "right";
      const estimatedHeight = (item.submenu.length * 31) + 16;
      submenuVerticalSide = bounds.bottom + estimatedHeight > window.innerHeight - 8 ? "up" : "down";
      requestAnimationFrame(() => {
        const submenu = root?.querySelector<HTMLElement>(".context-submenu");
        if (!submenu) return;
        const submenuBounds = submenu.getBoundingClientRect();
        if (submenuBounds.bottom > window.innerHeight - 8 && submenuBounds.top > 8) submenuVerticalSide = "up";
        else if (submenuBounds.top < 8 && submenuBounds.bottom < window.innerHeight - 8) submenuVerticalSide = "down";
      });
    }
  }

  function closeNested() {
    openSubmenu = null;
  }

  function activate(item: ContextMenuItem) {
    if (item.disabled || item.submenu?.length) return;
    onAction(item.id);
  }

  function topButtons() {
    return [...(root?.querySelectorAll<HTMLButtonElement>("button[data-menu-level='top']") ?? [])]
      .filter((button) => !button.disabled);
  }

  function handleKeydown(event: KeyboardEvent) {
    const target = event.target as HTMLElement | null;
    const submenuTarget = target?.dataset.menuLevel === "submenu";
    if (event.key === "Escape") {
      event.preventDefault();
      if (submenuTarget && openSubmenu) {
        const parentId = openSubmenu;
        closeNested();
        requestAnimationFrame(() => root?.querySelector<HTMLButtonElement>(`button[data-menu-id="${CSS.escape(parentId)}"]`)?.focus());
      } else onClose();
      return;
    }
    if (event.key === "ArrowLeft" && submenuTarget) {
      event.preventDefault();
      const parentId = openSubmenu;
      closeNested();
      root?.querySelector<HTMLButtonElement>(`button[data-menu-id="${CSS.escape(parentId ?? "")}"]`)?.focus();
      return;
    }
    if (event.key === "ArrowRight" && !submenuTarget) {
      const itemId = target?.dataset.menuId;
      const item = items.find((candidate) => candidate.id === itemId);
      if (item?.submenu?.length) {
        event.preventDefault();
        openNested(item, target ?? undefined);
        requestAnimationFrame(() => root?.querySelector<HTMLButtonElement>("button[data-menu-level='submenu']:not(:disabled)")?.focus());
      }
      return;
    }
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      const buttons = submenuTarget
        ? [...(root?.querySelectorAll<HTMLButtonElement>("button[data-menu-level='submenu']") ?? [])].filter((button) => !button.disabled)
        : topButtons();
      const current = buttons.indexOf(target as HTMLButtonElement);
      if (!buttons.length) return;
      event.preventDefault();
      const offset = event.key === "ArrowDown" ? 1 : -1;
      buttons[(current + offset + buttons.length) % buttons.length]?.focus();
      return;
    }
    if ((event.key === "Enter" || event.key === " ") && target?.matches("button")) {
      event.preventDefault();
      target.click();
    }
  }
</script>

<div
  bind:this={root}
  class="context-menu"
  role="menu"
  aria-label={ariaLabel}
  tabindex="-1"
  style={`left:${left}px;top:${top}px`}
  oncontextmenu={(event) => event.preventDefault()}
  onkeydown={handleKeydown}
>
  {#if heading}<strong class="context-menu-heading">{heading}</strong>{/if}
  {#each items as item (item.id)}
    {#if item.separator}
      <div class="context-menu-separator" role="separator"></div>
    {:else if item.submenu?.length}
      <div
        class:open={openSubmenu === item.id}
        class="context-submenu-holder"
        role="presentation"
        onpointerenter={(event) => openNested(item, event.currentTarget as HTMLElement)}
      >
        <button
          class:danger={item.danger}
          data-menu-id={item.id}
          data-menu-level="top"
          role="menuitem"
          aria-haspopup="menu"
          aria-expanded={openSubmenu === item.id}
          disabled={item.disabled}
          onclick={(event) => openNested(item, event.currentTarget as HTMLElement)}
        >
          {#if item.markerColor}<span class="context-menu-marker" style={`--context-menu-marker:${item.markerColor}`} aria-hidden="true"></span>{/if}
          <span>{item.label}</span><span class="submenu-arrow" aria-hidden="true">›</span>
        </button>
        {#if openSubmenu === item.id}
          <div class:open-left={submenuSide === "left"} class:open-up={submenuVerticalSide === "up"} class="context-submenu" role="menu">
            {#each item.submenu as child (child.id)}
              {#if child.separator}
                <div class="context-menu-separator" role="separator"></div>
              {:else}
                <button
                  class:danger={child.danger}
                  data-menu-id={child.id}
                  data-menu-level="submenu"
                  role="menuitem"
                  disabled={child.disabled}
                  onclick={() => activate(child)}
                >
                  {#if child.markerColor}<span class="context-menu-marker" style={`--context-menu-marker:${child.markerColor}`} aria-hidden="true"></span>{/if}
                  <span>{child.label}</span>
                </button>
              {/if}
            {/each}
          </div>
        {/if}
      </div>
    {:else}
      <button
        class:danger={item.danger}
        data-menu-id={item.id}
        data-menu-level="top"
        role="menuitem"
        disabled={item.disabled}
        onclick={() => activate(item)}
      >
        {#if item.markerColor}<span class="context-menu-marker" style={`--context-menu-marker:${item.markerColor}`} aria-hidden="true"></span>{/if}
        <span>{item.label}</span>
      </button>
    {/if}
  {/each}
</div>

<style>
  .context-menu,
  .context-submenu {
    position: fixed;
    z-index: 160;
    display: grid;
    width: max-content;
    min-width: 0;
    padding: 6px;
    border: 1px solid var(--border-strong, #39414c);
    border-radius: 0;
    color: var(--text, #f4f6fb);
    background: var(--surface, #171b21);
    box-shadow: var(--shadow, 0 18px 50px rgb(0 0 0 / 28%));
  }

  .context-menu:focus-visible,
  .context-submenu:focus-visible { outline: none; }

  .context-menu button,
  .context-submenu button {
    display: flex;
    align-items: center;
    justify-content: flex-start;
    min-height: 30px;
    gap: 8px;
    padding: 5px 9px;
    border: 0;
    border-radius: 5px;
    color: inherit;
    background: transparent;
    font: inherit;
    font-size: var(--font-size-body, 13px);
    text-align: left;
    white-space: nowrap;
    cursor: pointer;
  }

  .context-menu button .submenu-arrow { margin-left: auto; }
  .context-menu button:hover:not(:disabled),
  .context-menu button:focus-visible:not(:disabled),
  .context-submenu button:hover:not(:disabled),
  .context-submenu button:focus-visible:not(:disabled) { background: var(--surface-2, #282e36); outline: none; }
  .context-menu button:disabled,
  .context-submenu button:disabled { opacity: .42; cursor: default; }
  .context-menu button.danger,
  .context-submenu button.danger { color: var(--danger, #fca5a5); }
  .context-menu-heading { display: block; padding: 6px 9px; overflow: hidden; color: var(--muted, #8d97a4); text-overflow: ellipsis; white-space: nowrap; font-size: var(--font-size-compact, 10px); }
  .context-menu-marker { width: 8px; height: 8px; flex: 0 0 auto; border-radius: 50%; background: var(--context-menu-marker); }
  .context-menu-separator { height: 1px; margin: 5px 3px; background: var(--border, #343b45); }
  .context-submenu-holder { position: relative; }
  .context-submenu-holder.open > button { background: var(--surface-2, #282e36); }
  .context-submenu { position: absolute; top: -6px; left: calc(100% + 4px); }
  .context-submenu.open-left { right: calc(100% + 4px); left: auto; }
  .context-submenu.open-up { top: auto; bottom: -6px; }
  .submenu-arrow { color: var(--muted, #97a1b4); }
</style>
