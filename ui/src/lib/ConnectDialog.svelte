<script lang="ts">
  import Dialog from "./Dialog.svelte";
  import type { DiscoveredKey } from "./types";

  /** Mirrors the tagged `SshAuth` enum in shaman-core. */
  export type SshAuth =
    | { kind: "password"; password: string }
    | { kind: "key"; path: string; passphrase: string };

  export type SshRequest = {
    host: string;
    port: number;
    username: string;
    auth: SshAuth;
    /** Only ever true after the fingerprint has been shown and accepted. */
    trustNewKey?: boolean;
    /** Set when connecting from a saved entry; the password lives in the backend. */
    savedId?: string;
  };

  /** A host key awaiting a decision. */
  export type HostKeyPrompt = {
    changed: boolean;
    fingerprint: string;
    expectedFingerprint?: string;
    message: string;
  };

  type Props = {
    open: boolean;
    /** Private keys found in ~/.ssh, for the picker. */
    keys: DiscoveredKey[];
    connecting: boolean;
    /** Failure text from the last attempt, shown inline. */
    error: string | null;
    /** Set when the server's key needs an explicit decision. */
    hostKey: HostKeyPrompt | null;
    onsubmit: (request: SshRequest) => void;
    oncancel: () => void;
  };

  let { open, keys, connecting, error, hostKey, onsubmit, oncancel }: Props = $props();

  // Every field is on screen at once, and the values survive a failed attempt:
  // a rejected password should cost you the password, not the whole form.
  let host = $state("");
  let port = $state(22);
  let username = $state("");
  let password = $state("");

  // Auth method. Defaults to a discovered key when one exists, since a machine
  // with keys in ~/.ssh almost certainly wants them.
  let method = $state<"password" | "key">("password");
  let keyPath = $state("");
  let passphrase = $state("");

  const OTHER = "__other__";
  let keyChoice = $state(OTHER);

  const selectedKey = $derived(keys.find((k) => k.path === keyChoice));

  function currentAuth(): SshAuth {
    if (method === "key") {
      const path = keyChoice === OTHER ? keyPath.trim() : keyChoice;
      return { kind: "key", path, passphrase };
    }
    return { kind: "password", password };
  }

  // Pick a sensible default the first time the dialog is populated.
  $effect(() => {
    if (keys.length > 0 && keyChoice === OTHER && keyPath === "") {
      keyChoice = keys[0].path;
    }
  });

  // $state because the $effect below reads them after they're bound.
  let hostInput = $state<HTMLInputElement | undefined>();
  let passwordInput = $state<HTMLInputElement | undefined>();

  const hasCredential = $derived(
    method === "password" || (keyChoice !== OTHER ? true : keyPath.trim().length > 0),
  );

  const canSubmit = $derived(
    host.trim().length > 0 && username.trim().length > 0 && hasCredential && !connecting,
  );

  function submit(event: Event) {
    event.preventDefault();
    if (!canSubmit) return;
    onsubmit({
      host: host.trim(),
      port: port || 22,
      username: username.trim(),
      auth: currentAuth(),
    });
  }

  /** Retry the same connection, this time trusting the key we just displayed. */
  function acceptHostKey() {
    onsubmit({
      host: host.trim(),
      port: port || 22,
      username: username.trim(),
      auth: currentAuth(),
      trustNewKey: true,
    });
  }

  // Opening focuses the host field; a failed attempt focuses the password,
  // which is what needs correcting most of the time.
  $effect(() => {
    if (open && !connecting) {
      requestAnimationFrame(() => {
        if (error) passwordInput?.select();
        else hostInput?.focus();
      });
    }
  });
</script>

<!-- Escape is withheld mid-connect: the attempt is already in flight in a tab,
     so dismissing the dialog there would orphan it. -->
<Dialog {open} title="New Remote Connection" width={420} onclose={connecting ? null : oncancel}>
  <form onsubmit={submit}>
    <div class="grid">
      <label for="ssh-host">Host / IP</label>
      <input
        id="ssh-host"
        bind:this={hostInput}
        bind:value={host}
        placeholder="192.168.1.10 or server.local"
        autocomplete="off"
        spellcheck="false"
        disabled={connecting}
      />

      <label for="ssh-port">Port</label>
      <input
        id="ssh-port"
        type="number"
        min="1"
        max="65535"
        bind:value={port}
        disabled={connecting}
      />

      <label for="ssh-user">Username</label>
      <input
        id="ssh-user"
        bind:value={username}
        autocomplete="off"
        spellcheck="false"
        disabled={connecting}
      />

      <span class="label-ish">Sign in with</span>
      <div class="segmented" role="group" aria-label="Authentication method">
        <button
          type="button"
          class:selected={method === "password"}
          disabled={connecting}
          onclick={() => (method = "password")}
        >
          Password
        </button>
        <button
          type="button"
          class:selected={method === "key"}
          disabled={connecting}
          onclick={() => (method = "key")}
        >
          Key file
        </button>
      </div>

      {#if method === "password"}
        <label for="ssh-pass">Password</label>
        <input
          id="ssh-pass"
          type="password"
          bind:this={passwordInput}
          bind:value={password}
          autocomplete="off"
          disabled={connecting}
        />
      {:else}
        <label for="ssh-key">Key</label>
        <select id="ssh-key" bind:value={keyChoice} disabled={connecting}>
          {#each keys as key (key.path)}
            <option value={key.path}>{key.name}{key.encrypted ? " (encrypted)" : ""}</option>
          {/each}
          <option value={OTHER}>Other…</option>
        </select>

        {#if keyChoice === OTHER}
          <label for="ssh-keypath">Key path</label>
          <input
            id="ssh-keypath"
            bind:value={keyPath}
            placeholder="C:\Users\you\.ssh\id_ed25519"
            autocomplete="off"
            spellcheck="false"
            disabled={connecting}
          />
        {/if}

        <label for="ssh-phrase">Passphrase</label>
        <input
          id="ssh-phrase"
          type="password"
          bind:value={passphrase}
          placeholder={selectedKey && !selectedKey.encrypted ? "not required" : ""}
          autocomplete="off"
          disabled={connecting}
        />
      {/if}
    </div>

    {#if hostKey}
      <div class="hostkey" class:changed={hostKey.changed} role="alert">
        <p class="hk-msg">{hostKey.message}</p>
        {#if hostKey.expectedFingerprint}
          <dl>
            <dt>Previously trusted</dt>
            <dd>{hostKey.expectedFingerprint}</dd>
            <dt>Now offering</dt>
            <dd>{hostKey.fingerprint}</dd>
          </dl>
        {:else}
          <dl>
            <dt>Fingerprint</dt>
            <dd>{hostKey.fingerprint}</dd>
          </dl>
        {/if}
        <p class="hk-hint">
          {hostKey.changed
            ? "Only continue if you know the server was rebuilt or its key was rotated."
            : "Check this matches the server before continuing."}
        </p>
      </div>
    {:else if error}
      <p class="error" role="alert">{error}</p>
    {/if}

    <div class="dlg-actions">
      <span class="status">{connecting ? "Connecting…" : ""}</span>
      <button type="button" class="btn ghost" onclick={oncancel} disabled={connecting}>
        Cancel
      </button>
      {#if hostKey}
        <button
          type="button"
          class="btn"
          class:danger={hostKey.changed}
          class:primary={!hostKey.changed}
          disabled={connecting}
          onclick={acceptHostKey}
        >
          {hostKey.changed ? "Accept changed key" : "Trust & Connect"}
        </button>
      {:else}
        <button type="submit" class="btn primary" disabled={!canSubmit}>Connect</button>
      {/if}
    </div>
  </form>

  <p class="dlg-note">Credentials are used for this connection only — nothing is saved yet.</p>
</Dialog>

<style>
  /* The connect form's widgets. Single-caller, so they stay here rather than in
     dialog.css — that file is for what more than one dialog shares. */
  .grid {
    display: grid;
    grid-template-columns: auto 1fr;
    align-items: center;
    gap: 0.5rem 0.75rem;
  }

  label,
  .label-ish {
    font-size: 0.8rem;
    color: var(--fg-dim);
  }

  input,
  select {
    background: var(--bg-input);
    border: 1px solid var(--border-input);
    border-radius: 4px;
    color: var(--fg);
    font-family: inherit;
    font-size: 0.82rem;
    padding: 0.35rem 0.5rem;
    width: 100%;
  }
  input:focus,
  select:focus {
    outline: none;
    border-color: var(--accent);
  }
  input:disabled,
  select:disabled {
    opacity: 0.55;
  }

  /* A two-option toggle reads better than a dropdown here: both choices stay
     visible, so it's obvious key auth exists at all. */
  .segmented {
    display: flex;
    gap: 2px;
    padding: 2px;
    background: var(--bg-input);
    border: 1px solid var(--border-input);
    border-radius: 999px;
    width: fit-content;
  }

  .segmented button {
    background: transparent;
    border: none;
    border-radius: 999px;
    color: var(--fg-dim);
    cursor: pointer;
    font-family: inherit;
    font-size: 0.76rem;
    padding: 0.22rem 0.7rem;
  }
  .segmented button:hover:not(:disabled) {
    color: var(--fg);
  }
  .segmented button.selected {
    background: var(--accent);
    color: var(--accent-ink);
    font-weight: 600;
  }

  .error {
    margin: 0.75rem 0 0;
    padding: 0.45rem 0.55rem;
    border-radius: 4px;
    background: #f38ba81f;
    border: 1px solid #f38ba840;
    color: #f5a7bd;
    font-size: 0.78rem;
    line-height: 1.35;
  }

  /* Takes the slack in the action row so the buttons stay right-aligned. */
  .status {
    flex: 1;
    font-size: 0.78rem;
    color: var(--fg-dim);
  }

  .hostkey {
    margin-top: 0.75rem;
    padding: 0.55rem 0.6rem;
    border-radius: 4px;
    background: #cba6f714;
    border: 1px solid #cba6f740;
    font-size: 0.78rem;
    line-height: 1.35;
  }
  /* A changed key is the dangerous case and must not look routine. */
  .hostkey.changed {
    background: #f38ba81f;
    border-color: #f38ba866;
  }

  .hk-msg {
    margin: 0 0 0.45rem;
  }

  .hostkey dl {
    display: grid;
    grid-template-columns: auto;
    gap: 0.1rem;
    margin: 0 0 0.45rem;
  }
  .hostkey dt {
    color: var(--fg-dim);
    font-size: 0.72rem;
  }
  .hostkey dd {
    margin: 0 0 0.3rem;
    font-family: "Cascadia Mono", Consolas, monospace;
    font-size: 0.72rem;
    word-break: break-all;
  }

  .hk-hint {
    margin: 0;
    color: var(--fg-dim);
    font-size: 0.72rem;
  }
</style>
