<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";

  import AppearanceDialog from "./lib/AppearanceDialog.svelte";
  import ConnectDialog, { type SshRequest } from "./lib/ConnectDialog.svelte";
  import ContextMenu from "./lib/ContextMenu.svelte";
  import DragGhost from "./lib/DragGhost.svelte";
  import ManageConnections from "./lib/ManageConnections.svelte";
  import PinBar from "./lib/PinBar.svelte";
  import SaveToast from "./lib/SaveToast.svelte";
  import Sidebar from "./lib/Sidebar.svelte";
  import SidebarResizer from "./lib/SidebarResizer.svelte";
  import TitleBar from "./lib/TitleBar.svelte";
  import TrustedHosts from "./lib/TrustedHosts.svelte";

  import { copyFrom, pasteInto } from "./lib/clipboard";
  import { connectionLines } from "./lib/connectionLabel";
  import { item, SEP, type ContextMenuState } from "./lib/contextMenu";
  import { clipboardShortcut } from "./lib/keys";
  import { RAIL_WIDTH } from "./lib/layout";
  import { AppearanceStore } from "./lib/state/appearance.svelte";
  import { Connections } from "./lib/state/connections.svelte";
  import { TrustedHosts as TrustedHostStore } from "./lib/state/hosts.svelte";
  import { Pins } from "./lib/state/pins.svelte";
  import { Sessions } from "./lib/state/sessions.svelte";
  import { SshFlow } from "./lib/state/sshFlow.svelte";
  import { TabDrag } from "./lib/state/tabDrag.svelte";
  import type { Handoff, Slot } from "./lib/slots";
  import type { Pin, SavedConnection, ShellProfile } from "./lib/types";

  const sessions = new Sessions();
  const connections = new Connections();
  const pins = new Pins();
  const hosts = new TrustedHostStore();
  const ssh = new SshFlow();
  const appearance = new AppearanceStore();

  const appWindow = getCurrentWindow();
  const drag = new TabDrag(appWindow.label);

  /**
   * xterm and its WebGL renderer are most of the frontend bundle, and the
   * window opens with no terminal in it — so they are kept out of the startup
   * payload entirely and fetched afterwards.
   *
   * Two things trigger the fetch, whichever comes first: the browser going
   * idle once the window is up, or a terminal actually being opened. The idle
   * pass is what keeps the first terminal instant despite the split; the
   * effect below is the guarantee, for the case where someone opens one faster
   * than the idle callback fires.
   */
  let TerminalView = $state<typeof import("./lib/TerminalView.svelte").default | null>(null);
  let terminalViewLoad: Promise<unknown> | null = null;

  function loadTerminalView() {
    terminalViewLoad ??= import("./lib/TerminalView.svelte")
      .then((m) => {
        TerminalView = m.default;
        // The one failure mode a split introduces is a chunk that never
        // arrives -- a bad path, or a CSP that refuses it -- and it is
        // invisible until someone opens a terminal. Say so on the same beacon
        // scripts/verify.sh already reads, so it is caught headlessly.
        void invoke("ui_ready", { detail: "TERMINAL_READY" }).catch(() => {});
      })
      .catch((e) => {
        // Let the next attempt retry rather than wedging on a failed fetch.
        terminalViewLoad = null;
        console.error("loading the terminal view failed", e);
      });
    return terminalViewLoad;
  }

  $effect(() => {
    if (sessions.slots.length > 0) void loadTerminalView();
  });

  let profiles = $state<ShellProfile[]>([]);
  let manageOpen = $state(false);
  let hostKeysOpen = $state(false);
  let appearanceOpen = $state(false);

  // `sidebarWidth` stays the *expanded* width while collapsed, so expanding
  // returns to the width you dragged rather than a default.
  let sidebarWidth = $state(230);
  // Starts as the rail: the window opens small, and 230px of empty list is a
  // quarter of it spent on nothing until a terminal exists.
  let sidebarCollapsed = $state(true);
  let resizing = $state(false);

  const effectiveSidebarWidth = $derived(sidebarCollapsed ? RAIL_WIDTH : sidebarWidth);

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

  /**
   * Close this window and nothing else.
   *
   * The × used to call `quit_app`, which exits the process — so closing one of
   * two windows took the other one with it. Only File → Exit means the whole
   * application now.
   *
   * The terminals go first: they belong to this window, and a shell left
   * running with no window attached is a process nobody can see or stop.
   * `destroy` rather than `close`, because `close` would come straight back
   * round through the close handler below.
   */
  let closing = false;

  async function closeWindow() {
    if (closing) return;
    closing = true;
    try {
      await sessions.closeAll();
    } finally {
      await appWindow.destroy();
    }
  }

  // --- right-click menu -------------------------------------------------------
  //
  // WebView2 supplies its own, and it is a *browser's* menu: a dozen entries
  // about pages, history and printing, none of which mean anything in a
  // terminal. It is suppressed everywhere the app has something better to say
  // -- which is everywhere except a text field, where Cut/Copy/Paste is the
  // native menu earning its keep, and inside a terminal, where xterm already
  // owns right-click for copy-and-paste.
  //
  // One menu, owned here, so a right-click on a sidebar row and a right-click
  // on the window cannot both leave a popover up.

  let ctx = $state<ContextMenuState | null>(null);

  /**
   * Reload the frontend.
   *
   * The shells are closed first. The slot list lives only in this page, so a
   * reload loses the tabs either way; without this the PTYs behind them would
   * keep running with nothing left that can see or stop them.
   */
  async function refreshPage() {
    await sessions.closeAll();
    location.reload();
  }

  /** True for anything where the browser's own Cut/Copy/Paste menu is right. */
  function isEditable(target: EventTarget | null): boolean {
    return (
      target instanceof HTMLElement &&
      (target.isContentEditable || !!target.closest("input, textarea"))
    );
  }

  function onWindowContextMenu(event: MouseEvent) {
    // Something nearer the click already answered it -- the pinned strip has
    // its own menu, and the terminal has right-click copy/paste.
    if (event.defaultPrevented || isEditable(event.target)) return;
    event.preventDefault();
    ctx = { x: event.clientX, y: event.clientY, items: [item("Refresh Page", refreshPage)] };
  }

  /** A row in the sidebar: the same menu, plus the one thing a row can do. */
  function onSlotContextMenu(event: MouseEvent, slot: Slot) {
    ctx = {
      x: event.clientX,
      y: event.clientY,
      items: [
        item("Close Terminal", () => void sessions.close(slot.key), true),
        SEP,
        item("Refresh Page", refreshPage),
      ],
    };
  }

  // --- moving terminals between windows --------------------------------------

  /** Take in every terminal handed to this window while it wasn't looking. */
  async function claimHandoffs() {
    try {
      const waiting = await invoke<Handoff[]>("claim_handoffs");
      for (const handoff of waiting) sessions.adopt(handoff);
    } catch (e) {
      console.error("claim_handoffs failed", e);
    }
  }

  /** A drag was released. Where it landed is the OS's answer, not the page's. */
  async function dropDraggedTab() {
    const drop = await drag.finish();
    if (!drop) return;
    await sessions.handoff(drop.key, drop.target, drop.x, drop.y);
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
      // Nothing here depends on anything else here: the pinned strip resolves
      // itself reactively once both the pins and the connections have landed,
      // so awaiting each in turn only serialised five independent round trips
      // -- five disk reads among them -- that can all be in flight at once.
      const [listed] = await Promise.all([
        invoke<ShellProfile[]>("list_profiles").catch((e) => {
          console.error("list_profiles failed", e);
          return [] as ShellProfile[];
        }),
        connections.refresh(),
        pins.refresh(),
        connections.refreshKeys(),
        appearance.load(),
      ]);
      profiles = listed;
      // Deliberately opens nothing: the app starts empty, and a terminal is
      // launched from the Terminal menu.

      // Liveness beacon for scripts/verify.sh. Nothing auto-opens, so there is
      // no PTY to serve as proof the frontend came up.
      void invoke("ui_ready", {
        detail: `APP_READY profiles=${profiles.length} sessions=${sessions.slots.length} pins=${pins.list.length}`,
      }).catch(() => {});

      // Warm the terminal chunk now that the window is up and idle, so the
      // first terminal does not pay for the split second time.
      const idle = window.requestIdleCallback ?? ((fn: () => void) => setTimeout(fn, 200));
      idle(() => void loadTerminalView());
    })();

    // A window opened *for* a dragged terminal has one waiting before its first
    // frame; an existing window is told when one arrives. Both end up here.
    void claimHandoffs();

    // `exit` in the shell (or the process dying) closes the tab, exactly as if
    // it had been closed from the sidebar. The session is already gone, so
    // there is nothing to kill -- just drop the row.
    const stop = listen<number>("session-exit", (event) => {
      sessions.dropBySessionId(event.payload);
    });

    const stopAdopt = listen("session-adopt", () => void claimHandoffs());

    // Alt+F4 and the taskbar's Close both arrive here, so the terminals are
    // cleaned up however the window is shut -- not only from the × in the bar.
    const stopClose = appWindow.onCloseRequested((event) => {
      event.preventDefault();
      void closeWindow();
    });

    return () => {
      void stop.then((unlisten) => unlisten());
      void stopAdopt.then((unlisten) => unlisten());
      void stopClose.then((unlisten) => unlisten());
    };
  });
</script>

<svelte:window onkeydown={onKeydown} oncontextmenu={onWindowContextMenu} />

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
    onclosewindow={() => void closeWindow()}
    onquit={quit}
  />

  <main class:resizing>
    <Sidebar
      slots={sessions.slots}
      activeKey={sessions.activeKey}
      width={effectiveSidebarWidth}
      collapsed={sidebarCollapsed}
      {resizing}
      {drag}
      onselect={(key) => sessions.select(key)}
      onclose={(key) => void sessions.close(key)}
      ontoggle={() => (sidebarCollapsed = !sidebarCollapsed)}
      ondrop={() => void dropDraggedTab()}
      oncontext={onSlotContextMenu}
    />

    <!-- No handle while collapsed: the rail has one width, and a drag that
         silently expanded it would fight the toggle. -->
    {#if !sidebarCollapsed}
      <SidebarResizer
        width={sidebarWidth}
        onresize={(w) => (sidebarWidth = w)}
        ondragging={(d) => (resizing = d)}
      />
    {/if}

    <!-- The resizer normally provides the gap on this side; collapsed, it is
         not rendered, so the stage supplies its own. -->
    <section class="stage" class:railed={sidebarCollapsed}>
      {#if TerminalView}
        {#each sessions.slots as slot (slot.key)}
          <TerminalView
            profileId={slot.profileId}
            ssh={slot.ssh ?? undefined}
            adopt={slot.adopt ?? undefined}
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
      {/if}
    </section>
  </main>

  <DragGhost {drag} />

  {#if ctx}
    <ContextMenu x={ctx.x} y={ctx.y} items={ctx.items} onclose={() => (ctx = null)} />
  {/if}

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
    /* The whole window's surface, including the inset around the viewport. */
    background: var(--bg-chrome);
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

  /* The one bordered thing in the window. The chrome around it is seamless, so
     this line is what separates "the app" from "what the app is showing" --
     the same trick a browser plays with its content area.
     
     It is also the painted surface for the terminal area, rather than each
     TerminalView: it covers the pane's padding too, so a translucent window
     has no see-through gutter framing every terminal. */
  .stage {
    position: relative;
    flex: 1;
    min-width: 0;
    min-height: 0;
    margin: var(--viewport-inset) var(--viewport-inset) var(--viewport-inset) 0;
    background: var(--bg-viewport-wash);
    border: 1px solid var(--border);
    border-radius: var(--viewport-radius);
    transition: margin-left 170ms cubic-bezier(0.2, 0.7, 0.3, 1);
    /* Keeps the terminal inside the rounded corners. Safe here, unlike on the
       sidebar: nothing in the stage needs to escape it, and the menus that
       once got clipped live in the title bar. */
    overflow: hidden;
  }

  .stage.railed {
    margin-left: var(--viewport-inset);
  }

  @media (prefers-reduced-motion: reduce) {
    .stage {
      transition: none;
    }
  }
</style>
