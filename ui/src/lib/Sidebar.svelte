<script lang="ts">
  import KindIcon from "./KindIcon.svelte";
  import { KIND_TITLE, slotKind, type Kind } from "./kinds";
  import type { Slot } from "./slots";
  import type { TabDrag } from "./state/tabDrag.svelte";

  type Props = {
    slots: Slot[];
    activeKey: number | null;
    width: number;
    collapsed: boolean;
    /** A drag is in flight, so the width must track the pointer, not glide. */
    resizing: boolean;
    /** Dragging a terminal out of the list, possibly into another window. */
    drag: TabDrag;
    onselect: (key: number) => void;
    onclose: (key: number) => void;
    ontoggle: () => void;
    /** The drag was released; the destination is the drag's to report. */
    ondrop: () => void;
    /** Right-click on a row. App owns the menu, so only one is ever open. */
    oncontext: (event: MouseEvent, slot: Slot) => void;
  };

  let {
    slots,
    activeKey,
    width,
    collapsed,
    resizing,
    drag,
    onselect,
    onclose,
    ontoggle,
    ondrop,
    oncontext,
  }: Props = $props();

  // --- hover card ------------------------------------------------------------
  //
  // Not a `title` attribute. The native tooltip is a yellow-white system chip
  // that arrives after a second or so, ignores the theme, and — collapsed —
  // is the *only* thing naming the terminal, which is too important a job for
  // a control the app cannot style.
  //
  // Rendered fixed and outside the `<aside>`, because the list scrolls, and
  // `overflow` on the list clips anything that tries to sit beside a row.

  /** Long enough that sweeping down the list doesn't flash a card per row. */
  const HOVER_DELAY_MS = 260;

  let aside = $state<HTMLElement | undefined>();
  let tip = $state<{ title: string; kind: Kind; x: number; y: number } | null>(null);
  let tipTimer: ReturnType<typeof setTimeout> | undefined;

  function showTipSoon(event: MouseEvent, slot: Slot) {
    const row = event.currentTarget as HTMLElement;
    clearTimeout(tipTimer);
    tipTimer = setTimeout(() => {
      // A drag started during the delay: the ghost is already carrying the
      // terminal's name, and a card as well would be two labels for one thing.
      if (drag.active) return;
      const box = row.getBoundingClientRect();
      const rail = aside?.getBoundingClientRect();
      tip = {
        title: slot.title,
        kind: slotKind(slot),
        // Beside the sidebar rather than beside the row: rows are inset, and a
        // card that tracked them would step in and out as the list scrolled.
        x: (rail?.right ?? box.right) + 6,
        y: box.top + box.height / 2,
      };
    }, HOVER_DELAY_MS);
  }

  function hideTip() {
    clearTimeout(tipTimer);
    tip = null;
  }

  // Don't leave a card scheduled for a sidebar that is being torn down.
  $effect(() => () => clearTimeout(tipTimer));

  // --- dragging a terminal out ----------------------------------------------
  //
  // Pointer events rather than HTML5 drag-and-drop, for the same reason the pin
  // strip uses them -- the window's OS-level drag-drop handler swallows
  // `dragstart` inside the webview -- and for one more: this drag is allowed to
  // leave the window entirely, which `dragstart` could never do.

  /** Set for the duration of one click, so a finished drag doesn't also select. */
  let dragged = false;

  function onPointerDown(event: PointerEvent, slot: Slot) {
    // Any press ends the hover: the card described a resting pointer, and the
    // press means something is about to happen to the row underneath it.
    hideTip();
    // Left button only. A tab still connecting has no session to move.
    if (event.button !== 0 || slot.sessionId === null) return;
    // The kill button lives inside the row, so a press on it reaches here too;
    // it means close, not drag.
    if (event.target instanceof Element && event.target.closest(".kill")) return;
    dragged = false;
    // Capture is what lets the pointer leave the row, the sidebar and the
    // window without the drag ending. Not fatal if it is refused -- the drag
    // just stops early -- so the press still stands.
    try {
      (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    } catch {
      /* no capture available for this pointer */
    }
    drag.begin(slot.key, slot.title, event);
  }

  function onPointerMove(event: PointerEvent) {
    drag.move(event);
  }

  function onPointerUp() {
    if (drag.active) dragged = true;
    ondrop();
  }

  /** A drag that ends on the row it started on must not also count as a click. */
  function onRowClick(key: number) {
    if (dragged) {
      dragged = false;
      return;
    }
    onselect(key);
  }
</script>

<aside style="width: {width}px" class:collapsed class:resizing bind:this={aside}>
  <!-- Scrolling moves every row out from under its card. -->
  <ul onscroll={hideTip}>
    {#each slots as slot (slot.key)}
      {@const kind = slotKind(slot)}
      <li>
        <!-- Collapsed, the name is off the screen, so the hover card has to
             carry it -- otherwise the rail is a column of anonymous glyphs. -->
        <div
          class="row"
          class:active={slot.key === activeKey}
          class:lifted={drag.active && drag.key === slot.key}
          data-kind={kind}
          role="button"
          tabindex="0"
          onclick={() => onRowClick(slot.key)}
          onkeydown={(e) => (e.key === "Enter" || e.key === " ") && onselect(slot.key)}
          oncontextmenu={(e) => {
            // Stopped, not merely defaulted: the window-level handler in App
            // opens the plain menu, and this row wants the one with Close on it.
            e.preventDefault();
            e.stopPropagation();
            hideTip();
            oncontext(e, slot);
          }}
          onmouseenter={(e) => showTipSoon(e, slot)}
          onmouseleave={hideTip}
          onpointerdown={(e) => onPointerDown(e, slot)}
          onpointermove={onPointerMove}
          onpointerup={onPointerUp}
          onpointercancel={() => drag.cancel()}
        >
          {#if collapsed}
            <KindIcon {kind} size={13} />
          {:else}
            <span class="dot"></span>
            <span class="label">{slot.title}</span>
            <!-- aria-label, not `title`: a native tooltip here would fight the
                 hover card the row is already showing. -->
            <button
              class="kill"
              aria-label="Kill terminal"
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

{#if tip}
  <!-- Centred on the row it describes, clamped so a row near the bottom of a
       full list still gets a card that is entirely on screen. -->
  <div
    class="tip"
    data-kind={tip.kind}
    role="tooltip"
    style="left: {tip.x}px; top: {Math.min(Math.max(tip.y, 20), window.innerHeight - 20)}px"
  >
    <span class="tip-dot"></span>
    <span class="tip-text">
      <span class="tip-name">{tip.title}</span>
      <span class="tip-kind">{KIND_TITLE[tip.kind]}</span>
    </span>
  </div>
{/if}

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

  /* The row being dragged stays in place but recedes, so the list keeps its
     shape while the ghost carries the terminal. */
  .row.lifted {
    opacity: 0.35;
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

  /* --- hover card --------------------------------------------------------- */

  /* Same surface as the menus, and opaque for the same reason they are: a card
     you can read the terminal through is not a card. It sits below the context
     menu's layer so the two never stack. */
  .tip {
    --kind: var(--kind-local);
    position: fixed;
    z-index: 1400;
    transform: translateY(-50%);
    display: flex;
    align-items: center;
    gap: 0.45rem;
    max-width: 320px;
    padding: 0.35rem 0.55rem;
    background: var(--bg-menu);
    border: 1px solid var(--border);
    border-radius: var(--chip-radius);
    box-shadow: 0 6px 18px #0009;
    pointer-events: none;
    /* Appears rather than pops. The delay already did the waiting; this is
       just so it doesn't snap into existence at full contrast. */
    animation: tip-in 110ms ease-out;
  }
  .tip[data-kind="admin"] {
    --kind: var(--kind-admin);
  }
  .tip[data-kind="remote"] {
    --kind: var(--kind-remote);
  }

  @keyframes tip-in {
    from {
      opacity: 0;
      transform: translateY(-50%) translateX(-3px);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .tip {
      animation: none;
    }
  }

  /* The same dot the row carries, so the card is visibly *that* row's. */
  .tip-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--kind);
    flex: none;
  }

  .tip-text {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }

  .tip-name {
    font-size: 0.8rem;
    color: var(--fg);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* Second line, because colour alone must never be the only cue for kind. */
  .tip-kind {
    font-size: 0.68rem;
    color: var(--fg-dim);
    white-space: nowrap;
  }
</style>
