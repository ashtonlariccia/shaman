<script lang="ts">
  import type { SavedConnection } from "./types";
  import { connectionLines } from "./connectionLabel";
  import KindIcon from "./KindIcon.svelte";

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
      // Handled here so Escape cancels the edit rather than closing the dialog.
      event.preventDefault();
      event.stopPropagation();
      editingId = null;
    }
  }

  // Removal is destructive and the rows sit close together, so it takes two
  // clicks: the second one is the actual confirmation.
  let confirmingId = $state<string | null>(null);

  function onKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      confirmingId = null;
      onclose();
    }
  }

  $effect(() => {
    if (!open) {
      confirmingId = null;
      editingId = null;
    }
  });

</script>

<svelte:window onkeydown={onKeydown} />

{#if open}
  <div class="scrim">
    <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="manage-title">
      <!-- Saved connections are remote by definition, so this dialog reads
           mauve like the rest of the remote surfaces. -->
      <h2 id="manage-title">
        <KindIcon kind="remote" size={13} />
        Saved Connections
      </h2>

      {#if connections.length === 0}
        <p class="empty">
          Nothing saved yet. Connect to a host and choose <strong>Save</strong> when prompted,
          or use File → Save Remote Connection.
        </p>
      {:else}
        <ul>
          {#each connections as connection (connection.id)}
            <li>
              <div class="row">
                {#if editingId === connection.id}
                  <div class="detail">
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
                    <span class="meta">{connection.host} · {connection.username}:{connection.port}</span>
                  </div>
                  <button class="ghost" onclick={() => (editingId = null)}>Cancel</button>
                  <button class="primary" onclick={commitRename}>Save</button>
                {:else}
                  <div class="detail">
                    <span class="label">{connectionLines(connection).primary}</span>
                    <span class="meta">
                      {connectionLines(connection).secondary}{connection.authKind === "key"
                        ? " · key"
                        : ""}
                    </span>
                  </div>

                  {#if confirmingId === connection.id}
                  <button class="ghost" onclick={() => (confirmingId = null)}>Cancel</button>
                  <button
                    class="danger"
                    onclick={() => {
                      onremove(connection.id);
                      confirmingId = null;
                    }}
                  >
                    Remove
                  </button>
                  {:else}
                    <button class="ghost" onclick={() => startRename(connection)}>
                      {connection.name ? "Rename" : "Name"}
                    </button>
                    <button class="ghost" onclick={() => (confirmingId = connection.id)}>
                      Remove
                    </button>
                  {/if}
                {/if}
              </div>
            </li>
          {/each}
        </ul>
      {/if}

      <p class="note">
        Passwords are encrypted with Windows DPAPI under your user account and never leave the
        backend.
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
    width: min(520px, calc(100vw - 3rem));
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
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .rename {
    background: var(--bg-input);
    border: 1px solid var(--accent);
    border-radius: 4px;
    color: var(--fg);
    font-size: 0.83rem;
    padding: 0.2rem 0.4rem;
    width: 100%;
  }
  .rename:focus {
    outline: none;
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
