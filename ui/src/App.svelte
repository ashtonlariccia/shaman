<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import Sidebar from "./lib/Sidebar.svelte";
  import TitleBar from "./lib/TitleBar.svelte";
  import TerminalView from "./lib/TerminalView.svelte";
  import { newSlot, newSshSlot, type Slot } from "./lib/slots";
  import ConnectDialog, {
    type HostKeyPrompt,
    type SshRequest,
  } from "./lib/ConnectDialog.svelte";
  import type {
    DiscoveredKey,
    Pin,
    SavedConnection,
    ShellProfile,
    SshFailureInfo,
    TrustedHost,
  } from "./lib/types";
  import SaveToast from "./lib/SaveToast.svelte";
  import ManageConnections from "./lib/ManageConnections.svelte";
  import PinBar from "./lib/PinBar.svelte";
  import TrustedHosts from "./lib/TrustedHosts.svelte";
  import { connectionLines } from "./lib/connectionLabel";
  import { findSaved, resolvePin, samePin } from "./lib/pins";
  import { clipboardShortcut } from "./lib/keys";
  import type { TerminalApi } from "./lib/terminalApi";
  import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
  // Imported raw rather than inlined: a template literal would need every
  // backslash and backtick escaped, and one missed escape silently corrupts the
  // picture. `?raw` keeps it byte-for-byte.
  import shiv from "./lib/shiv.txt?raw";

  let slots = $state<Slot[]>([]);
  let activeKey = $state<number | null>(null);
  let profiles = $state<ShellProfile[]>([]);
  let saved = $state<SavedConnection[]>([]);
  let pins = $state<Pin[]>([]);
  let manageOpen = $state(false);
  let hostKeysOpen = $state(false);
  let trusted = $state<TrustedHost[]>([]);
  let sshKeys = $state<DiscoveredKey[]>([]);

  async function refreshTrusted() {
    try {
      trusted = await invoke<TrustedHost[]>("trusted_hosts");
    } catch (e) {
      console.error("trusted_hosts failed", e);
    }
  }

  async function forgetTrusted(key: string) {
    try {
      await invoke("forget_trusted_host", { key });
      await refreshTrusted();
    } catch (e) {
      console.error("forget_trusted_host failed", e);
    }
  }

  /** The connection the toast is currently offering to save. */
  let saveOffer = $state<SshRequest | null>(null);

  async function refreshSaved() {
    try {
      saved = await invoke<SavedConnection[]>("saved_connections");
    } catch (e) {
      console.error("saved_connections failed", e);
    }
  }

  const SIDEBAR_MIN = 150;
  const SIDEBAR_MAX = 520;
  let sidebarWidth = $state(230);
  let resizing = $state(false);

  function startResize(event: PointerEvent) {
    resizing = true;
    // Pointer capture keeps events coming even when the cursor outruns the
    // handle -- without it a fast drag detaches and the sidebar sticks.
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    event.preventDefault();
  }

  function onResizeMove(event: PointerEvent) {
    if (!resizing) return;
    // The sidebar starts at x=0, so the pointer's x *is* the desired width.
    sidebarWidth = Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, event.clientX));
  }

  function endResize(event: PointerEvent) {
    if (!resizing) return;
    resizing = false;
    (event.currentTarget as HTMLElement).releasePointerCapture(event.pointerId);
  }

  // Keyboard-accessible resizing, since a drag handle is otherwise mouse-only.
  function onResizeKey(event: KeyboardEvent) {
    const step = event.shiftKey ? 40 : 10;
    if (event.key === "ArrowLeft") {
      sidebarWidth = Math.max(SIDEBAR_MIN, sidebarWidth - step);
      event.preventDefault();
    } else if (event.key === "ArrowRight") {
      sidebarWidth = Math.min(SIDEBAR_MAX, sidebarWidth + step);
      event.preventDefault();
    }
  }

  // Trailing newline trimmed so it doesn't add a blank line and skew the
  // vertical centring.
  const ART = shiv.replace(/\s+$/, "");

  function open(profile: ShellProfile) {
    const slot = newSlot(profile.id, profile.label, profile.elevated);
    slots = [...slots, slot];
    select(slot.key);
  }

  /** Drop a tab from the sidebar and move selection somewhere sensible. */
  function removeSlot(key: number) {
    const index = slots.findIndex((s) => s.key === key);
    if (index === -1) return;

    slots = slots.filter((s) => s.key !== key);
    terminals.delete(key);

    if (activeKey === key) {
      // Prefer the neighbour on the left, which is what tabbed UIs tend to do.
      const next = slots[index - 1] ?? slots[index] ?? null;
      activeKey = next?.key ?? null;
    }
  }

  async function close(key: number) {
    const slot = slots.find((s) => s.key === key);
    if (!slot) return;

    // Unmounting TerminalView also closes the session, but doing it here means
    // the process tree is gone before the row disappears.
    if (slot.sessionId !== null) {
      try {
        await invoke("session_close", { id: slot.sessionId });
      } catch (e) {
        console.error("session_close failed", e);
      }
    }

    removeSlot(key);
  }

  // One entry per live TerminalView, so menu actions can reach the active one.
  const terminals = new Map<number, TerminalApi>();

  /** Terminal -> Close Session, and File -> Close. */
  function closeActive() {
    if (activeKey !== null) void close(activeKey);
  }

  /**
   * Terminal -> Refresh Session: restart the shell in place, for when it hangs.
   *
   * Swapping the slot's key is what does the work: the `{#each}` is keyed, so a
   * new key unmounts the old TerminalView (closing its session and killing the
   * process tree) and mounts a fresh one with the same profile.
   */
  function refreshActive() {
    const current = slots.find((s) => s.key === activeKey);
    if (!current) return;

    const replacement = newSlot(current.profileId, current.title, current.elevated);
    slots = slots.map((s) => (s.key === current.key ? replacement : s));
    terminals.delete(current.key);
    activeKey = replacement.key;
  }

  // Both of these are reachable from the Edit menu, which takes focus to get
  // clicked, so both hand it back when they are done. From the keyboard the
  // terminal already has focus and refocusing is a no-op.
  async function copySelection() {
    if (activeKey !== null) await copyFrom(activeKey);
  }

  /**
   * Copy from one named terminal.
   *
   * `clearAfter` is for the right-click gesture, which otherwise gives no sign
   * it did anything: no menu opens and nothing is printed, so the highlight
   * going out is the acknowledgement. The Edit menu leaves the selection up —
   * clicking a menu item is its own confirmation.
   */
  async function copyFrom(key: number, clearAfter = false) {
    const terminal = terminals.get(key);
    const selection = terminal?.copySelection();
    if (!terminal || !selection) return; // nothing highlighted
    try {
      await writeText(selection);
      if (clearAfter) terminal.clearSelection();
    } catch (e) {
      console.error("copy failed", e);
    }
    terminal.focus();
  }

  async function paste() {
    if (activeKey !== null) await pasteInto(activeKey);
  }

  /**
   * Paste into one named terminal.
   *
   * Right-click names the pane it happened in rather than resolving "the active
   * tab": the two are the same today, since an inactive pane takes no pointer
   * events, but a paste landing in a tab other than the one clicked would be a
   * bad way to find that out.
   */
  async function pasteInto(key: number) {
    const terminal = terminals.get(key);
    if (!terminal) return;
    try {
      const text = await readText();
      if (text) terminal.paste(text);
    } catch (e) {
      console.error("paste failed", e);
    }
    terminal.focus();
  }

  // --- remote connections ---------------------------------------------------
  //
  // The dialog stays open in a "connecting" state while the tab tries to reach
  // the host. On failure the tab is discarded and the error is shown back in the
  // dialog with the fields intact, so a wrong password costs one field rather
  // than the whole form.
  let sshDialogOpen = $state(false);
  let sshConnecting = $state(false);
  let sshError = $state<string | null>(null);
  let sshHostKey = $state<HostKeyPrompt | null>(null);
  let pendingSshKey: number | null = null;

  function openRemote() {
    sshError = null;
    sshHostKey = null;
    sshConnecting = false;
    sshDialogOpen = true;
  }

  function submitRemote(request: SshRequest) {
    sshError = null;
    sshHostKey = null;
    sshConnecting = true;

    const slot = newSshSlot(request);
    pendingSshKey = slot.key;
    slots = [...slots, slot];
    activeKey = slot.key;
  }

  function cancelRemote() {
    sshDialogOpen = false;
    sshConnecting = false;
    sshError = null;
    sshHostKey = null;
    pendingSshKey = null;
  }

  function remoteConnected(key: number) {
    if (key !== pendingSshKey) return;
    pendingSshKey = null;
    sshConnecting = false;
    sshDialogOpen = false;
  }

  /**
   * Landed in the shell of a remote host: offer to remember it, unless it is
   * already saved or the user has turned the prompt off.
   */
  async function maybeOfferToSave(key: number) {
    const slot = slots.find((s) => s.key === key);
    if (!slot?.ssh) return;

    try {
      if (!(await invoke<boolean>("suggest_saving_enabled"))) return;

      const alreadySaved = await invoke<boolean>("connection_is_saved", {
        host: slot.ssh.host,
        port: slot.ssh.port,
        username: slot.ssh.username,
      });
      if (alreadySaved) return;

      saveOffer = slot.ssh;
    } catch (e) {
      console.error("could not check saved connections", e);
    }
  }

  /** Store a remote target. Returns the entry, which carries its new id. */
  async function saveConnection(request: SshRequest): Promise<SavedConnection | null> {
    try {
      const stored = await invoke<SavedConnection>("save_connection", { target: request });
      await refreshSaved();
      try {
        sshKeys = await invoke<DiscoveredKey[]>("ssh_keys");
      } catch (e) {
        console.error("ssh_keys failed", e);
      }
      return stored;
    } catch (e) {
      console.error("save_connection failed", e);
      return null;
    }
  }

  async function acceptSaveOffer() {
    const offer = saveOffer;
    saveOffer = null;
    if (offer) await saveConnection(offer);
  }

  async function neverOfferAgain() {
    saveOffer = null;
    try {
      await invoke("set_suggest_saving", { enabled: false });
    } catch (e) {
      console.error("set_suggest_saving failed", e);
    }
  }

  /** File -> Save Remote Connection, for when the toast was missed. */
  async function saveActiveRemote() {
    const slot = slots.find((s) => s.key === activeKey);
    if (slot?.ssh) await saveConnection(slot.ssh);
  }

  /** Terminal -> New Saved Connection: connect with the stored password. */
  function openSaved(connection: SavedConnection) {
    const slot = newSshSlot(
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
    slots = [...slots, slot];
    activeKey = slot.key;
  }

  async function renameConnection(id: string, name: string) {
    try {
      await invoke("rename_connection", { id, name });
      await refreshSaved();
    } catch (e) {
      console.error("rename_connection failed", e);
    }
  }

  async function removeConnection(id: string) {
    try {
      await invoke("remove_connection", { id });
      await refreshSaved();
      // The backend drops any pin for a deleted connection; pick that up rather
      // than leaving a button on the strip that opens nothing.
      await refreshPins();
      try {
        sshKeys = await invoke<DiscoveredKey[]>("ssh_keys");
      } catch (e) {
        console.error("ssh_keys failed", e);
      }
    } catch (e) {
      console.error("remove_connection failed", e);
    }
  }

  /** The tab every menu action operates on. */
  const activeSlot = $derived(slots.find((s) => s.key === activeKey) ?? null);

  /** True when the active tab is a remote session that isn't saved yet. */
  const canSaveRemote = $derived(
    activeSlot?.ssh ? findSaved(saved, activeSlot.ssh) === undefined : false,
  );

  // --- pinned connections ---------------------------------------------------
  //
  // The strip along the bottom stores ids, never names: a local pin is a shell
  // profile id and a remote one is a saved-connection id. Names are resolved at
  // draw time, so renaming a connection in Manage Saved Connections renames its
  // pin without pins.json ever being touched.

  async function refreshPins() {
    try {
      pins = await invoke<Pin[]>("pinned_connections");
    } catch (e) {
      console.error("pinned_connections failed", e);
    }
  }

  const resolvedPins = $derived(pins.map((p) => resolvePin(p, profiles, saved)));

  /**
   * What a pin for the active tab would point at.
   *
   * Null for a remote tab that isn't saved yet — there is no id to pin until it
   * has one, which is exactly what [`pinActive`] fixes before pinning.
   */
  const activePin = $derived.by((): Pin | null => {
    const slot = activeSlot;
    if (!slot) return null;

    if (slot.ssh) {
      const connection = findSaved(saved, slot.ssh);
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

  const activePinned = $derived(activePin !== null && pins.some((p) => samePin(p, activePin)));

  /**
   * File → Pin Connection, acting on the active terminal.
   *
   * A remote tab that isn't saved yet gets saved first: a pin outlives the
   * session it was made from, so it has to point at something that persists.
   */
  async function pinActive() {
    const slot = activeSlot;
    if (!slot) return;

    let pin = activePin;

    if (!pin && slot.ssh) {
      const stored = await saveConnection(slot.ssh);
      if (!stored) return;

      // Point the tab at its new entry, so it reconnects from the vault and
      // stops being offered for saving.
      slots = slots.map((s) =>
        s.key === slot.key && s.ssh ? { ...s, ssh: { ...s.ssh, savedId: stored.id } } : s,
      );
      if (saveOffer === slot.ssh) saveOffer = null;

      pin = { kind: "saved", target: stored.id, label: connectionLines(stored).primary };
    }

    if (!pin) return;

    try {
      pins = await invoke<Pin[]>("pin_connection", { pin });
    } catch (e) {
      console.error("pin_connection failed", e);
    }
  }

  async function unpin(pin: Pin) {
    try {
      pins = await invoke<Pin[]>("unpin_connection", { kind: pin.kind, target: pin.target });
    } catch (e) {
      console.error("unpin_connection failed", e);
    }
  }

  /** Dragged (or Ctrl+Arrowed) to a new position on the strip. */
  async function movePin(pin: Pin, index: number) {
    // Optimistic: the strip has already shown the drop, and waiting for the
    // round trip would snap it back for a frame first.
    const without = pins.filter((p) => !samePin(p, pin));
    const moved = pins.find((p) => samePin(p, pin));
    if (moved) pins = [...without.slice(0, index), moved, ...without.slice(index)];

    try {
      pins = await invoke<Pin[]>("move_pin", { kind: pin.kind, target: pin.target, index });
    } catch (e) {
      console.error("move_pin failed", e);
      // Whatever the backend actually holds is the truth; put it back.
      await refreshPins();
    }
  }

  /** The File menu entry is one item that reads whichever way applies. */
  function togglePin() {
    if (activePinned && activePin) void unpin(activePin);
    else void pinActive();
  }

  /** Click on the strip: open a fresh terminal for what the pin points at. */
  function openPin(pin: Pin) {
    if (pin.kind === "local") {
      const profile = profiles.find((p) => p.id === pin.target);
      if (profile) open(profile);
      return;
    }

    const connection = saved.find((c) => c.id === pin.target);
    if (connection) openSaved(connection);
  }

  function remoteFailed(key: number, failure: SshFailureInfo) {
    if (key !== pendingSshKey) return;
    pendingSshKey = null;
    sshConnecting = false;

    // An unverified host key isn't an error to retype past -- it's a decision.
    // Show the fingerprint and let the user accept it explicitly.
    if (failure.kind === "unknownHostKey" || failure.kind === "hostKeyChanged") {
      sshError = null;
      sshHostKey = {
        changed: failure.kind === "hostKeyChanged",
        fingerprint: failure.fingerprint ?? "(unknown)",
        expectedFingerprint: failure.expectedFingerprint,
        message: failure.message,
      };
    } else {
      sshHostKey = null;
      sshError = failure.message;
    }

    // Drop the dead tab; the dialog is still up for another attempt.
    removeSlot(key);
  }

  async function newWindow() {
    try {
      await invoke("new_window");
    } catch (e) {
      console.error("new_window failed", e);
    }
  }

  // The menu advertises these, so they have to actually work. `preventDefault`
  // matters as much as the handler: without it the webview also runs its own
  // paste into xterm's textarea and the text arrives twice.
  function onKeydown(event: KeyboardEvent) {
    const action = clipboardShortcut(event);
    if (action === null) return;
    event.preventDefault();
    if (action === "copy") void copySelection();
    else void paste();
  }

  function markOpened(key: number, sessionId: number) {
    slots = slots.map((s) => (s.key === key ? { ...s, sessionId } : s));
  }

  function select(key: number) {
    activeKey = key;
  }

  async function quit() {
    try {
      await invoke("quit_app");
    } catch (e) {
      console.error("quit_app failed", e);
    }
  }

  onMount(() => {
    (async () => {
      try {
        profiles = await invoke<ShellProfile[]>("list_profiles");
      } catch (e) {
        console.error("list_profiles failed", e);
      }
      await refreshSaved();
      await refreshPins();
      try {
        sshKeys = await invoke<DiscoveredKey[]>("ssh_keys");
      } catch (e) {
        console.error("ssh_keys failed", e);
      }
      // Deliberately opens nothing: the app starts empty, and a terminal is
      // launched from File -> New Session.

      // Liveness beacon for scripts/verify.sh. Nothing auto-opens any more, so
      // there is no PTY probe to serve as proof the frontend came up.
      void invoke("ui_ready", {
        detail: `APP_READY profiles=${profiles.length} sessions=${slots.length} pins=${pins.length}`,
      }).catch(() => {});


    })();

    // `exit` in the shell (or the process dying) closes the tab, exactly as if
    // it had been closed from the sidebar. The session is already gone, so
    // there is nothing to kill -- just drop the row.
    const stop = listen<number>("session-exit", (event) => {
      const slot = slots.find((s) => s.sessionId === event.payload);
      if (slot) removeSlot(slot.key);
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
    hasSession={activeKey !== null}
    {saved}
    {canSaveRemote}
    {activePinned}
    onnew={open}
    onremote={openRemote}
    onsaved={openSaved}
    onsaveremote={saveActiveRemote}
    onpin={togglePin}
    onmanage={() => {
      void refreshSaved();
      manageOpen = true;
    }}
    onhostkeys={() => {
      void refreshTrusted();
      hostKeysOpen = true;
    }}
    onclose={closeActive}
    onrefresh={refreshActive}
    onnewwindow={newWindow}
    oncopy={copySelection}
    onpaste={paste}
    onquit={quit}
  />

  <main class:resizing>
    <Sidebar {slots} {activeKey} width={sidebarWidth} onselect={select} onclose={close} />

    <!-- This follows the W3C "Window Splitter" pattern: a focusable
         role="separator" with aria-valuenow IS an interactive widget, but
         svelte-check treats every separator as non-interactive. -->
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class="resizer"
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize sidebar"
      aria-valuenow={sidebarWidth}
      aria-valuemin={SIDEBAR_MIN}
      aria-valuemax={SIDEBAR_MAX}
      tabindex="0"
      onpointerdown={startResize}
      onpointermove={onResizeMove}
      onpointerup={endResize}
      onpointercancel={endResize}
      onkeydown={onResizeKey}
    ></div>

    <section class="stage">
      {#each slots as slot (slot.key)}
        <TerminalView
          profileId={slot.profileId}
          ssh={slot.ssh ?? undefined}
          active={slot.key === activeKey}
          onopened={(id) => {
            markOpened(slot.key, id);
            remoteConnected(slot.key);
            if (slot.ssh) void maybeOfferToSave(slot.key);
          }}
          onfailed={(failure) => remoteFailed(slot.key, failure)}
          onready={(api) => terminals.set(slot.key, api)}
          ongone={() => terminals.delete(slot.key)}
          onpasterequest={() => void pasteInto(slot.key)}
          oncopyrequest={() => void copyFrom(slot.key, true)}
        />
      {/each}

      {#if slots.length === 0}
        <!-- Inside the stage, so it centres on the editor area rather than on
             the whole window. -->
        <div class="blank">
          <pre class="art" aria-hidden="true">{ART}</pre>
        </div>
      {/if}
    </section>
  </main>

  <PinBar pins={resolvedPins} onopen={openPin} onunpin={unpin} onmove={movePin} />

  <SaveToast
    offer={saveOffer
      ? { host: saveOffer.host, username: saveOffer.username, port: saveOffer.port }
      : null}
    onsave={acceptSaveOffer}
    ondismiss={() => (saveOffer = null)}
    onnever={neverOfferAgain}
  />

  <ManageConnections
    open={manageOpen}
    connections={saved}
    onrename={renameConnection}
    onremove={removeConnection}
    onclose={() => (manageOpen = false)}
  />

  <TrustedHosts
    open={hostKeysOpen}
    hosts={trusted}
    onforget={forgetTrusted}
    onclose={() => (hostKeysOpen = false)}
  />

  <ConnectDialog
    open={sshDialogOpen}
    keys={sshKeys}
    connecting={sshConnecting}
    error={sshError}
    hostKey={sshHostKey}
    onsubmit={submitRemote}
    oncancel={cancelRemote}
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

  .stage {
    position: relative;
    flex: 1;
    min-width: 0;
    min-height: 0;
  }

  .blank {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    /* Decorative only — never intercept clicks headed for the sidebar. */
    pointer-events: none;
  }

  .art {
    margin: 0;
    font-family: "Cascadia Mono", Consolas, "Courier New", monospace;
    /* The art is 54 lines tall by 76 wide, so height is the binding constraint
       -- min() takes whichever of height/width runs out first, which keeps it
       fitting whatever shape the window is. */
    font-size: clamp(3px, min(0.95vh, 1.1vw), 14px);
    /* 1.0 keeps the character cell close to a terminal's aspect ratio; anything
       taller stretches the drawing vertically. */
    line-height: 1;
    white-space: pre;
    /* Mocha mauve, dimmed so it reads as a watermark rather than a billboard. */
    color: var(--accent);
    opacity: 0.55;
    user-select: none;
  }
</style>
