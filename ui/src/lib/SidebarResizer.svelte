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
  /* The grab area stays 6px wide -- a 1px target is miserable to hit -- but the
     *visible* indicator is a hairline drawn by the pseudo-element, so hovering
     doesn't paint a thick slab down the edge. */
  .resizer {
    position: relative;
    flex: none;
    width: 6px;
    margin-left: -3px;
    margin-right: -3px;
    z-index: 5;
    cursor: col-resize;
    background: transparent;
  }

  .resizer::after {
    content: "";
    position: absolute;
    top: 0;
    bottom: 0;
    left: 50%;
    transform: translateX(-50%);
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
