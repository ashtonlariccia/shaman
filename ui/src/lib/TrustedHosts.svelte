<script lang="ts">
  import KindIcon from "./KindIcon.svelte";
  import type { TrustedHost } from "./types";

  type Props = {
    open: boolean;
    hosts: TrustedHost[];
    onforget: (key: string) => void;
    onclose: () => void;
  };

  let { open, hosts, onforget, onclose }: Props = $props();

  // Forgetting a key means the next connection to that host will prompt again,
  // so it takes a second click rather than firing on the first.
  let confirmingKey = $state<string | null>(null);

  function onKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      confirmingKey = null;
      onclose();
    }
  }

  $effect(() => {
    if (!open) confirmingKey = null;
  });
</script>

<svelte:window onkeydown={onKeydown} />

{#if open}
  <div class="scrim">
    <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="hosts-title">
      <!-- Host keys belong to remote servers; mauve, like every other remote
           surface. -->
      <h2 id="hosts-title">
        <KindIcon kind="remote" size={13} />
        Trusted Host Keys
      </h2>

      {#if hosts.length === 0}
        <p class="empty">
          No host keys trusted yet. The first time you connect to a server, its fingerprint is
          shown for you to accept.
        </p>
      {:else}
        <ul>
          {#each hosts as host (host.key)}
            <li>
              <div class="row">
                <div class="detail">
                  <span class="label">{host.host}</span>
                  <span class="meta">port {host.port}</span>
                  <span class="fingerprint">{host.fingerprint}</span>
                </div>

                {#if confirmingKey === host.key}
                  <button class="ghost" onclick={() => (confirmingKey = null)}>Cancel</button>
                  <button
                    class="danger"
                    onclick={() => {
                      onforget(host.key);
                      confirmingKey = null;
                    }}
                  >
                    Forget
                  </button>
                {:else}
                  <button class="ghost" onclick={() => (confirmingKey = host.key)}>Forget</button>
                {/if}
              </div>
            </li>
          {/each}
        </ul>
      {/if}

      <p class="note">
        Forgetting a key makes the next connection to that host ask again, as if it were new.
        Remove one if a server was legitimately rebuilt and you want a clean prompt.
      </p>

      <div class="actions">
        <button class="primary" onclick={onclose}>Done</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 2000;
    display: grid;
    place-items: center;
    background: #000000a8;
  }

  .dialog {
    width: min(560px, calc(100vw - 3rem));
    max-height: min(70vh, 640px);
    display: flex;
    flex-direction: column;
    background: var(--bg-menu);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: 0 18px 48px #000c;
    padding: 1rem 1.1rem 0.9rem;
  }

  h2 {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    margin: 0 0 0.75rem;
    font-size: 0.95rem;
    font-weight: 600;
    color: var(--kind-remote);
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    overflow-y: auto;
    flex: 1;
    min-height: 0;
  }

  li + li {
    margin-top: 2px;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.45rem 0.5rem;
    border-radius: var(--chip-radius);
  }
  .row:hover {
    background: var(--hover);
  }

  .detail {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
  }

  .label {
    font-size: 0.83rem;
  }

  .meta {
    font-size: 0.71rem;
    color: var(--fg-dim);
  }

  /* The fingerprint is the thing you actually compare, so it gets monospace
     and is allowed to wrap rather than being truncated into uselessness. */
  .fingerprint {
    font-family: "Cascadia Mono", Consolas, monospace;
    font-size: 0.7rem;
    color: var(--fg-dim);
    word-break: break-all;
  }

  .empty {
    margin: 0.25rem 0 0;
    font-size: 0.8rem;
    color: var(--fg-dim);
    line-height: 1.45;
  }

  .note {
    margin: 0.8rem 0 0;
    font-size: 0.71rem;
    color: var(--fg-dim);
    line-height: 1.35;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    margin-top: 0.7rem;
  }

  button {
    border-radius: 4px;
    cursor: pointer;
    font-size: 0.78rem;
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

  .danger {
    background: var(--danger);
    border: 1px solid var(--danger);
    color: var(--accent-ink);
    font-weight: 600;
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
