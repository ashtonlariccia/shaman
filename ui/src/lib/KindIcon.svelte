<script lang="ts">
  import type { Kind } from "./kinds";

  type Props = {
    kind: Kind;
    /** Edge length in px. The glyphs are drawn on a 12x12 grid. */
    size?: number;
  };

  let { kind, size = 11 }: Props = $props();
</script>

<!-- Drawn in currentColor so the caller's kind colour flows straight through,
     rather than every call site having to state the colour twice. -->
{#if kind === "remote"}
  <!-- Facing chevrons: the usual shorthand for "somewhere else". -->
  <svg class="icon" width={size} height={size} viewBox="0 0 12 12" aria-hidden="true">
    <path
      d="M4.5 3 L1.5 6 L4.5 9 M7.5 3 L10.5 6 L7.5 9"
      fill="none"
      stroke="currentColor"
      stroke-width="1.4"
      stroke-linecap="round"
      stroke-linejoin="round"
    />
  </svg>
{:else}
  <!-- A shell prompt: chevron over an input line. Admin shares it with local,
       because an admin tab *is* a local shell — the colour is what differs. -->
  <svg class="icon" width={size} height={size} viewBox="0 0 12 12" aria-hidden="true">
    <path
      d="M2 3 L5 6 L2 9"
      fill="none"
      stroke="currentColor"
      stroke-width="1.4"
      stroke-linecap="round"
      stroke-linejoin="round"
    />
    <path d="M6.5 9 H10" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
  </svg>
{/if}

<style>
  .icon {
    flex: none;
    display: block;
  }
</style>
