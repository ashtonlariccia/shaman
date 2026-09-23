<script module lang="ts">
  export const SIDEBAR_MIN = 150;
  export const SIDEBAR_MAX = 520;
</script>

<script lang="ts">
  type Props = {
    width: number;
    /** Reports a new width. The parent owns the value; this only asks. */
    onresize: (width: number) => void;
    /** Whether a drag is in flight, so the parent can suppress text selection. */
    ondragging: (dragging: boolean) => void;
  };

  let { width, onresize, ondragging }: Props = $props();

  let resizing = false;

  const clamp = (w: number) => Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, w));

  function start(event: PointerEvent) {
    resizing = true;
    ondragging(true);
    // Pointer capture keeps events coming even when the cursor outruns the
    // handle -- without it a fast drag detaches and the sidebar sticks.
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    event.preventDefault();
  }

  function move(event: PointerEvent) {
    if (!resizing) return;
    // The sidebar starts at x=0, so the pointer's x *is* the desired width.
    onresize(clamp(event.clientX));
  }

  function end(event: PointerEvent) {
    if (!resizing) return;
    resizing = false;
    ondragging(false);
    (event.currentTarget as HTMLElement).releasePointerCapture(event.pointerId);
  }

  // Keyboard-accessible resizing, since a drag handle is otherwise mouse-only.
  function onKey(event: KeyboardEvent) {
    const step = event.shiftKey ? 40 : 10;
    if (event.key === "ArrowLeft") {
      onresize(clamp(width - step));
      event.preventDefault();
    } else if (event.key === "ArrowRight") {
      onresize(clamp(width + step));
      event.preventDefault();
    }
  }
</script>

<!-- This follows the W3C "Window Splitter" pattern: a focusable role="separator"
     with aria-valuenow IS an interactive widget, but svelte-check treats every
     separator as non-interactive. -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="resizer"
  role="separator"
  aria-orientation="vertical"
  aria-label="Resize sidebar"
  aria-valuenow={width}
  aria-valuemin={SIDEBAR_MIN}
  aria-valuemax={SIDEBAR_MAX}
  tabindex="0"
  onpointerdown={start}
  onpointermove={move}
  onpointerup={end}
  onpointercancel={end}
  onkeydown={onKey}
></div>

<style>
  /* Doubles as the gap between the chrome and the viewport card, so the grab
     area costs no space of its own. Wide enough to hit; the *visible*
     indicator is only a hairline drawn by the pseudo-element, so hovering
     doesn't paint a slab down the edge. */
  .resizer {
    position: relative;
    flex: none;
    width: var(--viewport-inset);
    z-index: 5;
    cursor: col-resize;
    background: transparent;
  }

  /* The hover indicator lands exactly on the viewport's left border rather
     than floating in the middle of the gap, so dragging looks like taking
     hold of the edge you are actually moving.
     
     `right: -1px` puts it over the card's 1px border, which begins where this
     element ends. The radius is subtracted top and bottom so it covers only
     the straight run between the card's rounded corners -- a straight line
     carried on past them would cut the curve. */
  .resizer::after {
    content: "";
    position: absolute;
    top: calc(var(--viewport-inset) + var(--viewport-radius));
    bottom: calc(var(--viewport-inset) + var(--viewport-radius));
    right: -1px;
    width: 1px;
    background: transparent;
    transition: background 120ms ease;
  }

  .resizer:hover::after,
  .resizer:focus-visible::after {
    background: var(--accent);
  }
  .resizer:focus-visible {
    outline: none;
  }
</style>
