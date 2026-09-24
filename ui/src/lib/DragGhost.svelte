<script lang="ts">
  import type { TabDrag } from "./state/tabDrag.svelte";

  type Props = { drag: TabDrag };
  let { drag }: Props = $props();

  /**
   * What releasing here would do, in words.
   *
   * The only feedback a cross-window drag can give. Once the cursor leaves this
   * window there is nothing to highlight — the other window is a separate
   * webview and the desktop is not ours to draw on — so the ghost itself has to
   * say where the terminal is going.
   */
  const hint = $derived(
    drag.kind === "window"
      ? "Move to that window"
      : drag.kind === "new"
        ? "Release for a new window"
        : "",
  );
</script>

{#if drag.active}
  <!-- Fixed, and outside every pane, so it is never clipped by the sidebar's
       scroll area or the terminal stage's rounded corners. -->
  <div class="ghost" class:leaving={drag.kind !== "self"} style="left: {drag.x}px; top: {drag.y}px">
    <span class="name">{drag.title}</span>
    {#if hint}<span class="hint">{hint}</span>{/if}
  </div>
{/if}

<style>
  .ghost {
    position: fixed;
    z-index: 2000;
    /* Offset from the cursor rather than centred on it: the pointer must stay
       able to "see" what is underneath, and a chip sitting under the hotspot
       is exactly what you are trying to look past. */
    transform: translate(14px, 10px);
    pointer-events: none;

    display: flex;
    flex-direction: column;
    gap: 0.1rem;
    max-width: 260px;
    padding: 0.3rem 0.55rem;
    background: var(--bg-menu);
    border: 1px solid var(--border);
    border-radius: var(--chip-radius);
    box-shadow: 0 10px 28px #000a;
    opacity: 0.96;
  }

  /* Dropping outside this window is the consequential half of the gesture, so
     it is the half that gets the accent. */
  .ghost.leaving {
    border-color: var(--accent);
  }

  .name {
    font-size: 0.82rem;
    color: var(--fg);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .hint {
    font-size: 0.7rem;
    color: var(--accent);
  }
</style>
