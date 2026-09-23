<script lang="ts">
  import KindIcon from "./KindIcon.svelte";
  import { KIND_TITLE } from "./kinds";
  import { dropGap, dropIndex, type ResolvedPin } from "./pins";
  import type { Pin } from "./types";

  type Props = {
    pins: ResolvedPin[];
    onopen: (pin: Pin) => void;
    onunpin: (pin: Pin) => void;
    /** Drop a pin at a new position. `index` counts the reordered strip. */
    onmove: (pin: Pin, index: number) => void;
  };

  let { pins, onopen, onunpin, onmove }: Props = $props();

  // The context menu is positioned in *viewport* coordinates rather than inside
  // the row, because the strip scrolls horizontally -- and `overflow` clips any
  // popover that tries to escape it.
  let menu = $state<{ pin: ResolvedPin; x: number; y: number } | null>(null);

  function openMenu(event: MouseEvent, pin: ResolvedPin) {
    event.preventDefault();
    menu = { pin, x: event.clientX, y: event.clientY };
  }

  function activate(pin: ResolvedPin) {
    menu = null;
    if (pin.available) onopen(pin.pin);
  }

  /** A drag finishes with a click on the same chip; that click is not an open. */
  function onClick(pin: ResolvedPin) {
    if (dragged) {
      dragged = false;
      return;
    }
    activate(pin);
  }

  function unpin(pin: ResolvedPin) {
    menu = null;
    onunpin(pin.pin);
  }

  // Middle-click closes tabs everywhere else; here it removes the pin.
  function onAuxClick(event: MouseEvent, pin: ResolvedPin) {
    if (event.button !== 1) return;
    event.preventDefault();
    unpin(pin);
  }

  // --- drag to reorder ------------------------------------------------------
  //
  // Pointer events rather than HTML5 drag-and-drop: the window has the OS-level
  // drag-drop handler enabled, which swallows `dragstart` inside the webview, and
  // pointer capture gives a drag that keeps tracking even when the cursor
  // outruns the 24px-tall strip -- the same reason the sidebar resizer uses it.

  /** Pixels of travel before a press becomes a drag rather than a click. */
  const DRAG_SLOP = 4;

  let strip = $state<HTMLElement | undefined>();
  let drag = $state<{ from: number; started: boolean; gap: number } | null>(null);

  /** Set for the duration of one click, so a finished drag doesn't also open. */
  let dragged = false;

  /** Horizontal midpoint of every chip, in viewport coordinates. */
  function midpoints(): number[] {
    if (!strip) return [];
    return [...strip.querySelectorAll<HTMLElement>("li")].map((li) => {
      const box = li.getBoundingClientRect();
      return box.left + box.width / 2;
    });
  }

  function onPointerDown(event: PointerEvent, index: number) {
    // Left button only: right opens the menu, middle unpins.
    if (event.button !== 0) return;
    // Cleared here rather than only in the click handler: pointer capture does
    // not guarantee a click follows, and a stale flag would eat the next open.
    dragged = false;
    // Capture keeps the drag alive when the cursor outruns a 24px-tall strip.
    // Not fatal if the pointer can't be captured — the drag still tracks, it
    // just stops early if the cursor leaves the chip — so don't fail the press.
    try {
      (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    } catch {
      /* no capture available for this pointer */
    }
    drag = { from: index, started: false, gap: index };
  }

  function onPointerMove(event: PointerEvent) {
    if (!drag) return;

    const mids = midpoints();
    if (!drag.started) {
      // A press that never really moves is a click; only commit to a drag once
      // the pointer has left the chip it started on.
      const own = mids[drag.from];
      if (own === undefined || Math.abs(event.clientX - own) < DRAG_SLOP) return;
      if (dropGap(mids, event.clientX) === drag.gap) return;
      drag.started = true;
    }

    drag.gap = dropGap(mids, event.clientX);
  }

  function onPointerUp(event: PointerEvent) {
    const current = drag;
    drag = null;
    if (!current?.started) return;

    dragged = true;
    const index = dropIndex(midpoints(), event.clientX, current.from);
    if (index !== current.from) onmove(pins[current.from].pin, index);
  }

  function onPointerCancel() {
    drag = null;
  }

  /**
   * Where to paint the insertion line: before the chip at this index, or past
   * the last one. Null while the gap is one the dragged chip already occupies,
   * so a drag that would change nothing shows nothing.
   */
  const marker = $derived.by(() => {
    if (!drag?.started) return null;
    if (drag.gap === drag.from || drag.gap === drag.from + 1) return null;
    return drag.gap;
  });

  function onKeydown(event: KeyboardEvent, pin: ResolvedPin, index: number) {
    if (event.key === "Delete") {
      event.preventDefault();
      unpin(pin);
      return;
    }

    // Reordering without a mouse. Ctrl, because bare arrows belong to moving
    // between chips.
    if (event.ctrlKey && (event.key === "ArrowLeft" || event.key === "ArrowRight")) {
      event.preventDefault();
      const to = index + (event.key === "ArrowLeft" ? -1 : 1);
      if (to >= 0 && to < pins.length) onmove(pin.pin, to);
    }
  }

  function onWindowKey(event: KeyboardEvent) {
    if (event.key === "Escape") menu = null;
  }

  // A trackpad or wheel over a one-line horizontal strip produces deltaY, which
  // would otherwise do nothing at all.
  function onWheel(event: WheelEvent) {
    if (event.deltaY === 0) return;
    const strip = event.currentTarget as HTMLElement;
    if (strip.scrollWidth <= strip.clientWidth) return;
    event.preventDefault();
    strip.scrollLeft += event.deltaY;
  }
</script>

<svelte:window onkeydown={onWindowKey} onclick={() => (menu = null)} />

<footer class="pinbar">
  {#if pins.length === 0}
    <span class="hint">Pin a terminal with File → Pin Connection</span>
  {:else}
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <ul class="strip" class:dragging={drag?.started} bind:this={strip} onwheel={onWheel}>
      {#each pins as entry, index (entry.pin.kind + ":" + entry.pin.target)}
        <li
          class:drop-before={marker === index}
          class:drop-after={marker === pins.length && index === pins.length - 1}
        >
          <button
            class="pin"
            class:missing={!entry.available}
            class:lifted={drag?.started && drag.from === index}
            data-kind={entry.kind}
            title="{KIND_TITLE[entry.kind]} · {entry.label} — {entry.detail}"
            onclick={() => onClick(entry)}
            onauxclick={(e) => onAuxClick(e, entry)}
            oncontextmenu={(e) => openMenu(e, entry)}
            onkeydown={(e) => onKeydown(e, entry, index)}
            onpointerdown={(e) => onPointerDown(e, index)}
            onpointermove={onPointerMove}
            onpointerup={onPointerUp}
            onpointercancel={onPointerCancel}
          >
            <KindIcon kind={entry.kind} />
            <span class="label">{entry.label}</span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</footer>

{#if menu}
  {@const entry = menu.pin}
  <!-- Anchored above the cursor: the strip sits on the bottom edge, so a menu
       hanging below it would open off-screen. -->
  <div
    class="ctx"
    role="menu"
    tabindex="-1"
    style="left: {menu.x}px; bottom: {Math.max(0, window.innerHeight - menu.y + 4)}px"
  >
    {#if entry.available}
      <button class="ctx-item" role="menuitem" onclick={() => activate(entry)}>Open</button>
    {/if}
    <button class="ctx-item" role="menuitem" onclick={() => unpin(entry)}>Unpin</button>
  </div>
{/if}

<style>
  /* Deliberately thin: this is a strip of shortcuts, not a panel. It brackets
     the window against the 28px title bar without eating terminal rows. */
  .pinbar {
    display: flex;
    align-items: center;
    height: 24px;
    flex: none;
    background: var(--bg-chrome);
    padding: 0 0.2rem;
    user-select: none;
    overflow: hidden;
  }

  .hint {
    color: var(--fg-faint);
    font-size: 0.7rem;
    padding: 0 0.4rem;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .strip {
    display: flex;
    align-items: center;
    gap: 1px;
    list-style: none;
    margin: 0;
    padding: 0;
    flex: 1;
    min-width: 0;
    overflow-x: auto;
    overflow-y: hidden;
  }

  /* The default 10px scrollbar would take almost half this bar's height. */
  .strip::-webkit-scrollbar {
    height: 3px;
  }

  /* While a chip is in flight the cursor says so everywhere on the strip, not
     just over the chip it started on. */
  .strip.dragging,
  .strip.dragging .pin {
    cursor: grabbing;
  }

  li {
    position: relative;
  }

  /* Where it will land. A line in the gap rather than chips shuffling live:
     shifting the row under the pointer makes the drop target move as you chase
     it, and re-measuring a moving layout is what makes such drags jitter. */
  .drop-before::before,
  .drop-after::after {
    content: "";
    position: absolute;
    top: 2px;
    bottom: 2px;
    width: 2px;
    border-radius: 1px;
    background: var(--accent);
  }
  .drop-before::before {
    left: -1px;
  }
  .drop-after::after {
    right: -1px;
  }


  /* Same three-way choice as the sidebar, resolved once per chip. The strip is
     the one place all three kinds sit side by side, so the colour is doing real
     work here: it says what a button will open before you read it. */
  .pin {
    --kind: var(--kind-local);
    --kind-soft: var(--kind-local-soft);

    display: flex;
    align-items: center;
    gap: 0.3rem;
    max-width: 200px;
    background: transparent;
    border: none;
    border-radius: var(--chip-radius);
    color: var(--kind);
    cursor: pointer;
    font-family: inherit;
    font-size: 0.71rem;
    line-height: 1;
    padding: 0.25rem 0.45rem;
    white-space: nowrap;
    /* Held slightly back so a row of pins reads as a strip of shortcuts rather
       than a row of alerts; hover brings the colour up to full. */
    opacity: 0.82;
    transition:
      background 90ms ease,
      opacity 90ms ease;
  }
  .pin[data-kind="admin"] {
    --kind: var(--kind-admin);
    --kind-soft: var(--kind-admin-soft);
  }
  .pin[data-kind="remote"] {
    --kind: var(--kind-remote);
    --kind-soft: var(--kind-remote-soft);
  }

  .pin:hover {
    background: var(--kind-soft);
    opacity: 1;
  }
  .pin:focus-visible {
    outline: 1px solid var(--kind);
    outline-offset: -1px;
    opacity: 1;
  }

  /* The shell was uninstalled, or the connection deleted. Kept visible rather
     than silently dropped, so a vanished pin is explained rather than mysterious.
     It drops out of the colour code entirely: it opens nothing, so claiming a
     kind would be a promise it can't keep. */
  /* The chip being carried: dimmed, so the insertion line rather than the chip
     reads as the thing being positioned. After the hover rules on purpose — the
     pointer is captured on this chip, so it is hovered for the whole drag. */
  .pin.lifted,
  .pin.lifted:hover {
    opacity: 0.4;
  }

  .pin.missing,
  .pin.missing:hover {
    color: var(--fg-faint);
    background: transparent;
    opacity: 1;
    cursor: default;
    text-decoration: line-through;
  }

  .label {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .ctx {
    position: fixed;
    z-index: 1500;
    min-width: 120px;
    padding: 0.2rem;
    background: var(--bg-menu);
    border: 1px solid var(--border);
    border-radius: 5px;
    box-shadow: 0 8px 24px #0009;
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
    padding: 0.3rem 0.5rem;
  }
  .ctx-item:hover {
    background: var(--hover);
    color: var(--accent);
  }
</style>
