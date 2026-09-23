<script lang="ts">
  import Dialog from "./Dialog.svelte";
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

  function close() {
    confirmingKey = null;
    onclose();
  }

  $effect(() => {
    if (!open) confirmingKey = null;
  });
</script>

<Dialog {open} title="Trusted Host Keys" width={560} scrolls onclose={close}>
  {#if hosts.length === 0}
    <p class="dlg-empty">
      No host keys trusted yet. The first time you connect to a server, its fingerprint is
      shown for you to accept.
    </p>
  {:else}
    <ul class="dlg-list">
      {#each hosts as host (host.key)}
        <li>
          <div class="dlg-row">
            <div class="dlg-detail">
              <span class="dlg-label">{host.host}</span>
              <span class="dlg-meta">port {host.port}</span>
              <span class="fingerprint">{host.fingerprint}</span>
            </div>

            {#if confirmingKey === host.key}
              <button class="btn ghost" onclick={() => (confirmingKey = null)}>Cancel</button>
              <button
                class="btn danger"
                onclick={() => {
                  onforget(host.key);
                  confirmingKey = null;
                }}
              >
                Forget
              </button>
            {:else}
              <button class="btn ghost" onclick={() => (confirmingKey = host.key)}>Forget</button>
            {/if}
          </div>
        </li>
      {/each}
    </ul>
  {/if}

  <p class="dlg-note">
    Forgetting a key makes the next connection to that host ask again, as if it were new.
    Remove one if a server was legitimately rebuilt and you want a clean prompt.
  </p>

  <div class="dlg-actions">
    <button class="btn primary" onclick={close}>Done</button>
  </div>
</Dialog>

<style>
  /* The fingerprint is the thing you actually compare, so it gets monospace
     and is allowed to wrap rather than being truncated into uselessness. */
  .fingerprint {
    font-family: "Cascadia Mono", Consolas, monospace;
    font-size: 0.7rem;
    color: var(--fg-dim);
    word-break: break-all;
  }
</style>
