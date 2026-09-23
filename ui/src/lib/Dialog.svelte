<script module lang="ts">
  /** Per-instance id for `aria-labelledby`, so every dialog names its own title. */
  let nextId = 1;
</script>

<script lang="ts">
  import type { Snippet } from "svelte";
  import KindIcon from "./KindIcon.svelte";
  import type { Kind } from "./kinds";

  type Props = {
    open: boolean;
    title: string;
    /** Panel width in px, before the viewport clamp. */
    width?: number;
    /** Colours the title and its icon, per the app-wide colour code. */
    kind?: Kind;
    /** List bodies scroll inside a height budget; forms size to their content. */
    scrolls?: boolean;
    /**
     * Dismiss on Escape. Pass `null` while the dialog must not be dismissed —
     * ConnectDialog does that mid-connect, so Escape can't abandon a tab that
     * is already being opened.
     */
    onclose: (() => void) | null;
    children: Snippet;
  };

  let {
    open,
    title,
    width = 520,
    kind = "remote",
    scrolls = false,
    onclose,
    children,
  }: Props = $props();

  const titleId = `dlg-title-${nextId++}`;

  // Guarded on `open`. The three dialogs each used to keep a window-level
  // Escape handler alive whether or not they were showing, so one keypress in a
  // terminal ran three handlers and three preventDefaults for nothing.
  function onWindowKey(event: KeyboardEvent) {
    if (!open || !onclose || event.key !== "Escape") return;
    event.preventDefault();
    onclose();
  }
</script>

<svelte:window onkeydown={onWindowKey} />

{#if open}
  <div class="dlg-scrim">
    <div
      class="dlg-panel"
      class:scrolls
      role="dialog"
      aria-modal="true"
      aria-labelledby={titleId}
      style="--dlg-width: {width}px; --dlg-kind: var(--kind-{kind})"
    >
      <h2 class="dlg-title" id={titleId}>
        <KindIcon {kind} size={13} />
        {title}
      </h2>

      {@render children()}
    </div>
  </div>
{/if}
