<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";

  import AppearanceDialog from "./lib/AppearanceDialog.svelte";
  import ConnectDialog, { type SshRequest } from "./lib/ConnectDialog.svelte";
  import ManageConnections from "./lib/ManageConnections.svelte";
  import PinBar from "./lib/PinBar.svelte";
  import SaveToast from "./lib/SaveToast.svelte";
  import Sidebar from "./lib/Sidebar.svelte";
  import SidebarResizer from "./lib/SidebarResizer.svelte";
  import TerminalView from "./lib/TerminalView.svelte";
  import TitleBar from "./lib/TitleBar.svelte";
  import TrustedHosts from "./lib/TrustedHosts.svelte";
  import Watermark from "./lib/Watermark.svelte";

  import { copyFrom, pasteInto } from "./lib/clipboard";
  import { connectionLines } from "./lib/connectionLabel";
  import { clipboardShortcut } from "./lib/keys";
  import { AppearanceStore } from "./lib/state/appearance.svelte";
  import { Connections } from "./lib/state/connections.svelte";
  import { TrustedHosts as TrustedHostStore } from "./lib/state/hosts.svelte";
  import { Pins } from "./lib/state/pins.svelte";
  import { Sessions } from "./lib/state/sessions.svelte";
  import { SshFlow } from "./lib/state/sshFlow.svelte";
  import type { Pin, SavedConnection, ShellProfile } from "./lib/types";

  const sessions = new Sessions();
  const connections = new Connections();
  const pins = new Pins();
  const hosts = new TrustedHostStore();
  const ssh = new SshFlow();
  const appearance = new AppearanceStore();

  let profiles = $state<ShellProfile[]>([]);
  let manageOpen = $state(false);
  let hostKeysOpen = $state(false);
  let appearanceOpen = $state(false);

  let sidebarWidth = $state(230);
  let resizing = $state(false);

  // --- opening terminals ----------------------------------------------------

  /** Terminal → New Saved Connection: connect with the stored password. */
  function openSaved(connection: SavedConnection) {
    sessions.openSsh(
      {
        host: connection.host,
        port: connection.port,
        username: connection.username,
        // Credentials stay in the backend; the tab connects by id.
        auth: { kind: "password", password: "" },
        savedId: connection.id,
      },
      // The name you gave it, falling back to the address when unnamed.
      connection.name ?? connection.host,
    );
  }

  function submitRemote(request: SshRequest) {
    ssh.submitted(sessions.openSsh(request).key);
  }

  // --- pinning --------------------------------------------------------------

  const resolvedPins = $derived(pins.resolved(profiles, connections.list));

  /**
   * What a pin for the active tab would point at.
   *
   * Null for a remote tab that isn't saved yet — there is no id to pin until it
   * has one, which is exactly what [`pinActive`] fixes before pinning.
   */
  const activePin = $derived.by((): Pin | null => {
    const slot = sessions.active;
    if (!slot) return null;

    if (slot.ssh) {
      const connection = connections.find(slot.ssh);
      return connection
        ? {
            kind: "saved",
            target: connection.id,
            label: connectionLines(connection).primary,
          }
        : null;
    }

    if (!slot.profileId) return null;
    const profile = profiles.find((p) => p.id === slot.profileId);
    return {
      kind: "local",
      target: slot.profileId,
      label: profile?.label ?? slot.title,
    };
  });

  const activePinned = $derived(pins.has(activePin));

  /** True when the active tab is a remote session that isn't saved yet. */
  const canSaveRemote = $derived(
    sessions.active?.ssh ? connections.find(sessions.active.ssh) === undefined : false,
  );

  /**
   * File → Pin Connection, acting on the active terminal.
   *
   * A remote tab that isn't saved yet gets saved first: a pin outlives the
   * session it was made from, so it has to point at something that persists.
   */
  async function pinActive() {
    const slot = sessions.active;
    if (!slot) return;

    let pin = activePin;

    if (!pin && slot.ssh) {
      const stored = await connections.save(slot.ssh);
      if (!stored) return;

      sessions.attachSavedId(slot.key, stored.id);
      if (connections.offer === slot.ssh) connections.offer = null;

      pin = { kind: "saved", target: stored.id, label: connectionLines(stored).primary };
    }

    if (pin) await pins.add(pin);
  }

  /** The File menu entry is one item that reads whichever way applies. */
  function togglePin() {
    if (activePinned && activePin) void pins.remove(activePin);
    else void pinActive();
  }

  /** Click on the strip: open a fresh terminal for what the pin points at. */
  function openPin(pin: Pin) {
    if (pin.kind === "local") {
      const profile = profiles.find((p) => p.id === pin.target);
      if (profile) sessions.open(profile);
      return;
    }

    const connection = connections.list.find((c) => c.id === pin.target);
    if (connection) openSaved(connection);
  }

  /** Removing a connection drops its pin in the backend; pick that up here. */
  async function removeConnection(id: string) {
    await connections.remove(id);
    await pins.refresh();
  }

  // --- menu actions ---------------------------------------------------------

  /** File → Save Remote Connection, for when the toast was missed. */
  async function saveActiveRemote() {
    const slot = sessions.active;
    if (slot?.ssh) await connections.save(slot.ssh);
  }

  async function newWindow() {
    try {
      await invoke("new_window");
    } catch (e) {
      console.error("new_window failed", e);
    }
  }

  async function quit() {
    try {
      await invoke("quit_app");
    } catch (e) {
      console.error("quit_app failed", e);
    }
  }

  // The menu advertises these, so they have to actually work. `preventDefault`
  // matters as much as the handler: without it the webview also runs its own
  // paste into xterm's textarea and the text arrives twice.
  function onKeydown(event: KeyboardEvent) {
    const action = clipboardShortcut(event);
    if (action === null) return;
    event.preventDefault();
    if (action === "copy") void copyFrom(sessions.activeTerminal);
    else void pasteInto(sessions.activeTerminal);
  }

  // Drives every surface in app.css at once, so opacity is one number in one
  // place rather than a rule per pane. Set on the root element because the
  // whole cascade reads it, including components this file never touches.
  $effect(() => {
    document.documentElement.style.setProperty("--bg-alpha", String(appearance.alpha));
  });

  onMount(() => {
    (async () => {
      try {
        profiles = await invoke<ShellProfile[]>("list_profiles");
      } catch (e) {
        console.error("list_profiles failed", e);
      }
      await connections.refresh();
      await pins.refresh();
      await connections.refreshKeys();
      await appearance.load();
      // Deliberately opens nothing: the app starts empty, and a terminal is
      // launched from the Terminal menu.

      // Liveness beacon for scripts/verify.sh. Nothing auto-opens, so there is
      // no PTY to serve as proof the frontend came up.
      void invoke("ui_ready", {
        detail: `APP_READY profiles=${profiles.length} sessions=${sessions.slots.length} pins=${pins.list.length}`,
      }).catch(() => {});
    })();

    // `exit` in the shell (or the process dying) closes the tab, exactly as if
    // it had been closed from the sidebar. The session is already gone, so
    // there is nothing to kill -- just drop the row.
    const stop = listen<number>("session-exit", (event) => {
      sessions.dropBySessionId(event.payload);
    });

    return () => {
      void stop.then((unlisten) => unlisten());
    };
  });
</script>

<svelte:window onkeydown={onKeydown} />

<div class="app">
  <TitleBar
    {profiles}
    hasSession={sessions.activeKey !== null}
    saved={connections.list}
    {canSaveRemote}
    {activePinned}
    onnew={(profile) => sessions.open(profile)}
    onremote={() => ssh.begin()}
    onsaved={openSaved}
    onsaveremote={saveActiveRemote}
    onpin={togglePin}
    onmanage={() => {
      void connections.refresh();
      manageOpen = true;
    }}
    onhostkeys={() => {
      void hosts.refresh();
      hostKeysOpen = true;
    }}
    onclose={() => sessions.closeActive()}
    onrefresh={() => sessions.refreshActive()}
    onnewwindow={newWindow}
    onappearance={() => (appearanceOpen = true)}
    oncopy={() => void copyFrom(sessions.activeTerminal)}
    onpaste={() => void pasteInto(sessions.activeTerminal)}
    onquit={quit}
  />

  <main class:resizing>
    <Sidebar
      slots={sessions.slots}
      activeKey={sessions.activeKey}
      width={sidebarWidth}
      onselect={(key) => sessions.select(key)}
      onclose={(key) => void sessions.close(key)}
    />

    <SidebarResizer
      width={sidebarWidth}
      onresize={(w) => (sidebarWidth = w)}
      ondragging={(d) => (resizing = d)}
    />

    <section class="stage">
      {#each sessions.slots as slot (slot.key)}
        <TerminalView
          profileId={slot.profileId}
          ssh={slot.ssh ?? undefined}
          active={slot.key === sessions.activeKey}
          {appearance}
          onopened={(id) => {
            sessions.markOpened(slot.key, id);
            ssh.connected(slot.key);
            if (slot.ssh) void connections.maybeOffer(slot.ssh);
          }}
          onfailed={(failure) => {
            // Drop the dead tab; the dialog stays up for another attempt.
            if (ssh.failed(slot.key, failure)) sessions.drop(slot.key);
          }}
          onready={(api) => sessions.terminals.set(slot.key, api)}
          ongone={() => sessions.terminals.delete(slot.key)}
          onpasterequest={() => void pasteInto(sessions.terminals.get(slot.key))}
          oncopyrequest={() => void copyFrom(sessions.terminals.get(slot.key), true)}
        />
      {/each}

      {#if sessions.slots.length === 0}
        <!-- Inside the stage, so it centres on the editor area rather than on
             the whole window. -->
        <Watermark />
      {/if}
    </section>
  </main>

  <PinBar
    pins={resolvedPins}
    onopen={openPin}
    onunpin={(pin) => void pins.remove(pin)}
    onmove={(pin, index) => void pins.move(pin, index)}
  />

  <SaveToast
    offer={connections.offer
      ? {
          host: connections.offer.host,
          username: connections.offer.username,
          port: connections.offer.port,
        }
      : null}
    onsave={() => void connections.acceptOffer()}
    ondismiss={() => (connections.offer = null)}
    onnever={() => void connections.neverOfferAgain()}
  />

  <ManageConnections
    open={manageOpen}
    connections={connections.list}
    onrename={(id, name) => void connections.rename(id, name)}
    onremove={(id) => void removeConnection(id)}
    onclose={() => (manageOpen = false)}
  />

  <TrustedHosts
    open={hostKeysOpen}
    hosts={hosts.list}
    onforget={(key) => void hosts.forget(key)}
    onclose={() => (hostKeysOpen = false)}
  />

  <AppearanceDialog
    open={appearanceOpen}
    {appearance}
    onclose={() => (appearanceOpen = false)}
  />

  <ConnectDialog
    open={ssh.open}
    keys={connections.keys}
    connecting={ssh.connecting}
    error={ssh.error}
    hostKey={ssh.hostKey}
    onsubmit={submitRemote}
    oncancel={() => ssh.cancel()}
  />
</div>

<style>
  .app {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }

  main {
    display: flex;
    flex: 1;
    min-height: 0;
  }

  /* While dragging, stop the terminal from selecting text under the cursor and
     keep the resize cursor even when the pointer strays off the handle. */
  main.resizing {
    cursor: col-resize;
    user-select: none;
  }

  /* The painted surface for the terminal area, rather than each TerminalView:
     it covers the pane's padding too, so a translucent window has no
     see-through gutter framing every terminal. */
  .stage {
    position: relative;
    flex: 1;
    min-width: 0;
    min-height: 0;
    background: var(--bg);
  }
</style>
