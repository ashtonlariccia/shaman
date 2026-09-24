<script lang="ts">
  /**
   * The app's own right-click menu.
   *
   * WebView2's native one is a browser's menu — Back, Forward, Reload, Save as,
   * Print, Inspect — and none of that describes a terminal. This replaces it
   * wherever the click is not inside a terminal (xterm keeps right-click for
   * copy/paste) with the two or three entries that actually apply here.
   *
   * Positioned in viewport coordinates and rendered at the top level, because
   * every surface that can be right-clicked — the sidebar list especially — is
   * inside something that clips.
   */
  import type { ContextItem } from "./contextMenu";

  type Props = {
    x: number;
    y: number;
    items: ContextItem[];
    onclose: () => void;
  };

  let { x, y, items, onclose }: Props = $props();

  /** Breathing room from the window edge, and how far the menu flips by. */
  const GAP = 4;

  let el = $state<HTMLElement | undefined>();
  // Measured rather than assumed: the menu is two or three entries and its
  // height is whatever the entries come to, which is what the clamp needs.
  let width = $state(0);
  let height = $state(0);

  // Right/bottom edges: a menu opened near them would otherwise hang off the
  // window. Clamping rather than flipping keeps the corner nearest the cursor,
  // which is what makes it feel anchored to the click.
  const left = $derived(Math.max(GAP, Math.min(x, window.innerWidth - width - GAP)));
  const top = $derived(Math.max(GAP, Math.min(y, window.innerHeight - height - GAP)));

  function choose(entry: ContextItem) {
    if (entry.kind !== "item") return;
    // Closed first, so an action that opens a dialog doesn't do it behind a
    // menu that is still up.
    onclose();
    entry.run();
  }

  /**
   * Any press outside dismisses — including the right-press that is about to
   * open a different menu, which is what stops two from being up at once.
   * Capture phase so it still fires over the terminal, whose own handler stops
   * the event before it reaches the window.
   */
  function onPointerDown(event: PointerEvent) {
    if (el && event.target instanceof Node && el.contains(event.target)) return;
    onclose();
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") onclose();
  }
</script>

<svelte:window
  onpointerdowncapture={onPointerDown}
  onkeydown={onKeydown}
  onresize={onclose}
  onblur={onclose}
/>

<div
  class="ctx"
  role="menu"
  tabindex="-1"
  bind:this={el}
  bind:clientWidth={width}
  bind:clientHeight={height}
  style="left: {left}px; top: {top}px"
>
  {#each items as entry, index (index)}
    {#if entry.kind === "sep"}
      <div class="sep"></div>
    {:else}
      <button
        class="ctx-item"
        class:danger={entry.danger}
        role="menuitem"
        onclick={() => choose(entry)}
      >
        {entry.label}
      </button>
    {/if}
  {/each}
</div>

<style>
  /* Same surface as the title-bar menus, so the two read as one menu system. */
  .ctx {
    position: fixed;
    z-index: 1600;
    min-width: 150px;
    padding: 0.2rem;
    background: var(--bg-menu);
    border: 1px solid var(--border);
    border-radius: 5px;
    box-shadow: 0 8px 24px #0009;
    user-select: none;
  }

  .ctx-item {
    display: block;
    width: 100%;
    text-align: left;
    background: transparent;
    border: none;
    border-radius: 3px;
    color: var(--fg);
    cursor: pointer;
    font-family: inherit;
    font-size: 0.78rem;
    padding: 0.32rem 0.5rem;
  }
  .ctx-item:hover {
    background: var(--hover);
    color: var(--accent);
  }

  /* Killing a shell is the one entry here that cannot be undone. */
  .ctx-item.danger:hover {
    background: #f38ba81f;
    color: var(--danger);
  }

  .sep {
    height: 1px;
    margin: 0.2rem 0.3rem;
    background: var(--border);
  }
</style>
