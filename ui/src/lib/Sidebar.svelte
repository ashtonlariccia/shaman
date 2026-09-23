<script module lang="ts">
  /**
   * Width of the collapsed rail.
   *
   * Wide enough for the kind glyph and the toggle to sit centred with room to
   * breathe, narrow enough that it reads as a rail rather than a thin sidebar.
   */
  export const RAIL_WIDTH = 42;
</script>

<script lang="ts">
  import KindIcon from "./KindIcon.svelte";
  import { KIND_TITLE, slotKind } from "./kinds";
  import type { Slot } from "./slots";

  type Props = {
    slots: Slot[];
    activeKey: number | null;
    width: number;
    collapsed: boolean;
    /** A drag is in flight, so the width must track the pointer, not glide. */
    resizing: boolean;
    onselect: (key: number) => void;
    onclose: (key: number) => void;
    ontoggle: () => void;
  };

  let { slots, activeKey, width, collapsed, resizing, onselect, onclose, ontoggle }: Props =
    $props();
</script>

<aside style="width: {width}px" class:collapsed class:resizing>
  <ul>
    {#each slots as slot (slot.key)}
      {@const kind = slotKind(slot)}
      <li>
        <!-- Collapsed, the name is off the screen, so the tooltip has to carry
             it -- otherwise the rail is a column of anonymous glyphs. -->
        <div
          class="row"
          class:active={slot.key === activeKey}
          data-kind={kind}
          role="button"
          tabindex="0"
          title={collapsed ? `${slot.title} — ${KIND_TITLE[kind]}` : KIND_TITLE[kind]}
          onclick={() => onselect(slot.key)}
          onkeydown={(e) => (e.key === "Enter" || e.key === " ") && onselect(slot.key)}
        >
          {#if collapsed}
            <KindIcon {kind} size={13} />
          {:else}
            <span class="dot"></span>
            <span class="label">{slot.title}</span>
            <button
              class="kill"
              title="Kill terminal"
              onclick={(e) => {
                e.stopPropagation();
                onclose(slot.key);
              }}
            >
              ×
            </button>
          {/if}
        </div>
      </li>
    {/each}
  </ul>

  <footer>
    <button
      class="toggle"
      onclick={ontoggle}
      aria-expanded={!collapsed}
      title={collapsed ? "Expand sidebar" : "Collapse sidebar"}
      aria-label={collapsed ? "Expand sidebar" : "Collapse sidebar"}
    >
      <!-- A panel glyph whose left column is filled while the sidebar is open,
           so the icon depicts the current state rather than the action. -->
      <svg width="15" height="15" viewBox="0 0 16 16" aria-hidden="true">
        <rect
          x="1.6"
          y="2.6"
          width="12.8"
          height="10.8"
          rx="2"
          fill="none"
          stroke="currentColor"
          stroke-width="1.2"
        />
        <path d="M6.2 2.6 V13.4" stroke="currentColor" stroke-width="1.2" />
        {#if !collapsed}
          <path d="M3.4 2.6 H4.8 V13.4 H3.4 Z" fill="currentColor" opacity="0.75" />
        {/if}
      </svg>
    </button>
  </footer>
</aside>

<style>
  aside {
    /* Width is driven by the drag handle in App.svelte, or pinned to the rail
       width while collapsed. */
    flex: none;
    display: flex;
    flex-direction: column;
    /* Darkest surface in the app, so it recedes behind the terminal. */
    min-height: 0;
    transition: width 170ms cubic-bezier(0.2, 0.7, 0.3, 1);
    /* NOTE: no overflow here. `overflow` creates a clipping context, which is
       what was cutting off menus that extended past the sidebar. Scrolling
       belongs on the list itself. */
  }

  /* Dragging the handle sets the width every pointer move; easing each one
     would make the sidebar lag behind the cursor. */
  aside.resizing {
    transition: none;
  }

  @media (prefers-reduced-motion: reduce) {
    aside {
      transition: none;
    }
  }

  ul {
    list-style: none;
    margin: 0;
    /* Inset so hover chips never touch the sidebar's edges. */
    padding: 0.3rem 0.35rem;
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }

  li + li {
    margin-top: 1px;
  }

  /* Each row resolves its kind once, into `--kind` and `--kind-soft`; the dot
     and the selected background both read those rather than repeating the
     three-way choice. */
  .row {
    --kind: var(--kind-local);
    --kind-soft: var(--kind-local-soft);

    display: flex;
    align-items: center;
    gap: 0.45rem;
    padding: 0.35rem 0.45rem;
    border-radius: var(--chip-radius);
    cursor: pointer;
    user-select: none;
    color: var(--kind);
  }
  .row[data-kind="admin"] {
    --kind: var(--kind-admin);
    --kind-soft: var(--kind-admin-soft);
  }
  .row[data-kind="remote"] {
    --kind: var(--kind-remote);
    --kind-soft: var(--kind-remote-soft);
  }

  .row:hover {
    background: var(--hover);
  }

  /* Selection is tinted with the row's own kind rather than one shared accent,
     so the colour code survives the state that would otherwise paint over it. */
  .row.active {
    background: var(--kind-soft);
  }

  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--kind);
    flex: none;
  }
  .label {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.82rem;
    color: var(--fg);
  }

  .kill {
    background: transparent;
    border: none;
    color: var(--fg-dim);
    cursor: pointer;
    font-size: 1rem;
    line-height: 1;
    padding: 0 0.15rem;
    border-radius: 3px;
  }
  .kill:hover {
    color: var(--danger);
    background: #f38ba81f;
  }

  /* --- collapsed rail ----------------------------------------------------- */

  .collapsed ul {
    padding: 0.3rem 0;
  }

  /* The glyph is the whole row, so centre it and drop the text-sized padding. */
  .collapsed .row {
    justify-content: center;
    padding: 0.4rem 0;
    margin: 0 0.25rem;
  }

  /* --- footer ------------------------------------------------------------- */

  footer {
    flex: none;
    display: flex;
    /* Bottom left when expanded; the rail is narrow enough that centring is
       the only thing that looks deliberate. */
    justify-content: flex-start;
    padding: 0.25rem 0.35rem;
  }
  .collapsed footer {
    justify-content: center;
    padding: 0.25rem 0;
  }

  .toggle {
    display: grid;
    place-items: center;
    width: 26px;
    height: 22px;
    background: transparent;
    border: none;
    border-radius: var(--chip-radius);
    color: var(--fg-dim);
    cursor: pointer;
  }
  .toggle:hover {
    background: var(--hover);
    color: var(--fg);
  }
  .toggle:focus-visible {
    outline: 1px solid var(--accent);
    outline-offset: -1px;
    color: var(--fg);
  }
</style>
