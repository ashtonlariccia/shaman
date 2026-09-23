<script lang="ts">
  import KindIcon from "./KindIcon.svelte";

  type Props = {
    /** The connection being offered, or null when there is nothing to offer. */
    offer: { host: string; username: string; port: number } | null;
    onsave: () => void;
    ondismiss: () => void;
    onnever: () => void;
  };

  let { offer, onsave, ondismiss, onnever }: Props = $props();

  const DISMISS_MS = 4000;

  // Auto-dismisses after 4s. Hovering *pauses* it rather than restarting it, so
  // the drain bar can pause in place — a bar that jumps back to full when the
  // pointer leaves would be lying about how long is left.
  let hovering = $state(false);

  // Plain locals, not $state: these are bookkeeping for the timer and must not
  // re-trigger the effects that write them.
  let timer: ReturnType<typeof setTimeout> | undefined;
  let remaining = DISMISS_MS;
  let startedAt = 0;

  function stopTimer() {
    clearTimeout(timer);
    timer = undefined;
  }

  // A new offer restarts the countdown from full.
  $effect(() => {
    void offer;
    stopTimer();
    remaining = DISMISS_MS;
    return stopTimer;
  });

  // Pause on hover, resume on leave, carrying the remaining time across.
  $effect(() => {
    if (!offer) return;

    if (hovering) {
      if (timer !== undefined) {
        stopTimer();
        remaining = Math.max(0, remaining - (Date.now() - startedAt));
      }
    } else if (timer === undefined) {
      startedAt = Date.now();
      timer = setTimeout(ondismiss, remaining);
    }
  });
</script>

{#if offer}
  <div
    class="toast"
    role="status"
    aria-live="polite"
    onmouseenter={() => (hovering = true)}
    onmouseleave={() => (hovering = false)}
  >
    <!-- Keyed so a fresh offer restarts the drain rather than resuming it. -->
    {#key offer}
      <div class="timer" aria-hidden="true"></div>
    {/key}

    <p class="title">
      <KindIcon kind="remote" size={12} />
      Save this connection?
    </p>
    <p class="target">{offer.host}</p>
    <p class="meta">{offer.username}:{offer.port}</p>
    <p class="hint">Saved connections appear under Terminal → New Saved Connection.</p>

    <div class="actions">
      <button class="never" onclick={onnever}>Never</button>
      <span class="spacer"></span>
      <button class="ghost" onclick={ondismiss}>No</button>
      <button class="primary" onclick={onsave}>Save</button>
    </div>
  </div>
{/if}

<style>
  .toast {
    position: fixed;
    right: 16px;
    bottom: 16px;
    z-index: 1800;
    width: 300px;
    /* No top padding: the timer bar sits flush against the top edge. */
    padding: 0 0.8rem 0.6rem;
    background: var(--bg-menu);
    border: 1px solid var(--border);
    border-radius: 6px;
    /* Clips the bar to the rounded corners. */
    overflow: hidden;
    box-shadow: 0 10px 30px #000a;
    animation: slide-in 160ms ease-out;
  }

  /* Drains left-to-right over the dismiss window. scaleX keeps it on the
     compositor rather than relayouting 60 times a second. */
  .timer {
    height: 2px;
    margin: 0 -0.8rem 0.7rem;
    background: var(--accent);
    transform-origin: left;
    animation: drain 4s linear forwards;
  }

  /* Pauses in place, matching the JS timer exactly. */
  .toast:hover .timer {
    animation-play-state: paused;
  }

  @keyframes drain {
    from {
      transform: scaleX(1);
    }
    to {
      transform: scaleX(0);
    }
  }

  @keyframes slide-in {
    from {
      opacity: 0;
      transform: translateY(8px);
    }
  }

  /* Respect users who don't want motion. */
  @media (prefers-reduced-motion: reduce) {
    .toast {
      animation: none;
    }
  }

  /* Only remote connections are ever offered for saving, so the toast is mauve
     like the rest of the remote surfaces. */
  .title {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    margin: 0 0 0.2rem;
    font-size: 0.82rem;
    font-weight: 600;
    color: var(--kind-remote);
  }

  .target {
    margin: 0;
    font-size: 0.82rem;
    color: var(--accent);
    word-break: break-all;
  }

  .meta {
    margin: 0 0 0.35rem;
    font-size: 0.71rem;
    color: var(--fg-dim);
  }

  .hint {
    margin: 0 0 0.6rem;
    font-size: 0.71rem;
    color: var(--fg-dim);
    line-height: 1.3;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 0.4rem;
  }

  .spacer {
    flex: 1;
  }

  button {
    border-radius: 4px;
    cursor: pointer;
    font-size: 0.76rem;
    padding: 0.28rem 0.7rem;
  }

  .ghost {
    background: transparent;
    border: 1px solid var(--border);
    color: var(--fg);
  }
  .ghost:hover {
    border-color: var(--fg-dim);
  }

  /* Deliberately quiet: "never" is the destructive-ish choice and shouldn't
     compete with Save. */
  .never {
    background: transparent;
    border: none;
    color: var(--fg-dim);
    padding-left: 0;
  }
  .never:hover {
    color: var(--danger);
  }

  .primary {
    background: var(--accent);
    border: 1px solid var(--accent);
    color: var(--accent-ink);
    font-weight: 600;
  }
  .primary:hover {
    filter: brightness(1.08);
  }
</style>
