<script lang="ts">
  import Dialog from "./Dialog.svelte";
  import type { SavedConnection } from "./types";
  import { connectionLines } from "./connectionLabel";

  type Props = {
    open: boolean;
    connections: SavedConnection[];
    onrename: (id: string, name: string) => void;
    onremove: (id: string) => void;
    onclose: () => void;
  };

  let { open, connections, onrename, onremove, onclose }: Props = $props();

  // Inline rename: the row turns into an input rather than opening a second
  // dialog on top of this one.
  let editingId = $state<string | null>(null);
  let draftName = $state("");

  // Removal is destructive and the rows sit close together, so it takes two
  // clicks: the second one is the actual confirmation.
  let confirmingId = $state<string | null>(null);

  function startRename(connection: SavedConnection) {
    confirmingId = null;
    editingId = connection.id;
    draftName = connection.name ?? "";
  }

  function commitRename() {
    if (editingId) onrename(editingId, draftName);
    editingId = null;
  }

  function onEditKey(event: KeyboardEvent) {
    if (event.key === "Enter") {
      event.preventDefault();
      commitRename();
    } else if (event.key === "Escape") {
      // Stopped here so Escape cancels the edit rather than reaching Dialog's
      // window handler and closing the whole thing.
      event.preventDefault();
      event.stopPropagation();
      editingId = null;
    }
  }

  function close() {
    confirmingId = null;
    onclose();
  }

  $effect(() => {
    if (!open) {
      confirmingId = null;
      editingId = null;
    }
  });
</script>

<Dialog {open} title="Saved Connections" scrolls onclose={close}>
  {#if connections.length === 0}
    <p class="dlg-empty">
      Nothing saved yet. Connect to a host and choose <strong>Save</strong> when prompted,
      or use File → Save Remote Connection.
    </p>
  {:else}
    <ul class="dlg-list">
      {#each connections as connection (connection.id)}
        <li>
          <div class="dlg-row">
            {#if editingId === connection.id}
              <div class="dlg-detail">
                <!-- svelte-ignore a11y_autofocus -->
                <input
                  class="rename"
                  bind:value={draftName}
                  placeholder="Name (leave blank to clear)"
                  autocomplete="off"
                  spellcheck="false"
                  autofocus
                  onkeydown={onEditKey}
                />
                <span class="dlg-meta">
                  {connection.host} · {connection.username}:{connection.port}
                </span>
              </div>
              <button class="btn ghost" onclick={() => (editingId = null)}>Cancel</button>
              <button class="btn primary" onclick={commitRename}>Save</button>
            {:else}
              <div class="dlg-detail">
                <span class="dlg-label">{connectionLines(connection).primary}</span>
                <span class="dlg-meta">
                  {connectionLines(connection).secondary}{connection.authKind === "key"
                    ? " · key"
                    : ""}
                </span>
              </div>

              {#if confirmingId === connection.id}
                <button class="btn ghost" onclick={() => (confirmingId = null)}>Cancel</button>
                <button
                  class="btn danger"
                  onclick={() => {
                    onremove(connection.id);
                    confirmingId = null;
                  }}
                >
                  Remove
                </button>
              {:else}
                <button class="btn ghost" onclick={() => startRename(connection)}>
                  {connection.name ? "Rename" : "Name"}
                </button>
                <button class="btn ghost" onclick={() => (confirmingId = connection.id)}>
                  Remove
                </button>
              {/if}
            {/if}
          </div>
        </li>
      {/each}
    </ul>
  {/if}

  <p class="dlg-note">
    Passwords are encrypted with Windows DPAPI under your user account and never leave the
    backend.
  </p>

  <div class="dlg-actions">
    <button class="btn primary" onclick={close}>Done</button>
  </div>
</Dialog>

<style>
  .rename {
    background: var(--bg-input);
    border: 1px solid var(--accent);
    border-radius: 4px;
    color: var(--fg);
    font-family: inherit;
    font-size: 0.83rem;
    padding: 0.2rem 0.4rem;
    width: 100%;
  }
  .rename:focus {
    outline: none;
  }
</style>
