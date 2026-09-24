<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import type { SavedConnection, ShellProfile } from "./types";
  import { connectionLines } from "./connectionLabel";
  import KindIcon from "./KindIcon.svelte";
  import { KIND_TITLE, profileKind } from "./kinds";

  type Props = {
    profiles: ShellProfile[];
    saved: SavedConnection[];
    /** Whether a terminal is open, so entries that need one can be disabled. */
    hasSession: boolean;
    /** The active tab is an SSH session that isn't saved yet. */
    canSaveRemote: boolean;
    /** The active tab is already on the quick-open strip, so the entry unpins. */
    activePinned: boolean;
    onnew: (profile: ShellProfile) => void;
    onremote: () => void;
    onsaved: (connection: SavedConnection) => void;
    onsaveremote: () => void;
    onpin: () => void;
    onmanage: () => void;
    onhostkeys: () => void;
    onclose: () => void;
    onrefresh: () => void;
    onnewwindow: () => void;
    oncopy: () => void;
    onpaste: () => void;
    onappearance: () => void;
    /** The window button: this window only. */
    onclosewindow: () => void;
    /** File -> Exit: the whole application. */
    onquit: () => void;
  };

  let {
    profiles,
    saved,
    hasSession,
    canSaveRemote,
    activePinned,
    onnew,
    onremote,
    onsaved,
    onsaveremote,
    onpin,
    onmanage,
    onhostkeys,
    onclose,
    onrefresh,
    onnewwindow,
    oncopy,
    onpaste,
    onappearance,
    onclosewindow,
    onquit,
  }: Props = $props();

  type MenuName = "file" | "edit" | "terminal";

  let openMenu = $state<MenuName | null>(null);
  /** Which submenu of the Terminal menu is showing, if any. */
  let submenu = $state<"local" | "saved" | null>(null);
  let menuWrap: HTMLElement | undefined;
  let maximized = $state(false);

  const appWindow = getCurrentWindow();

  // Hover intent: a submenu that opens the instant the pointer crosses the row
  // flickers when you're only passing over it. Opening is delayed, and closing
  // is delayed slightly longer so a diagonal move into the submenu doesn't
  // dismiss it mid-travel.
  const OPEN_DELAY_MS = 180;
  const CLOSE_DELAY_MS = 240;
  let hoverTimer: ReturnType<typeof setTimeout> | undefined;

  function openSubmenuSoon(which: "local" | "saved") {
    clearTimeout(hoverTimer);
    hoverTimer = setTimeout(() => (submenu = which), OPEN_DELAY_MS);
  }
  function closeSubmenuSoon() {
    clearTimeout(hoverTimer);
    hoverTimer = setTimeout(() => (submenu = null), CLOSE_DELAY_MS);
  }
  function cancelHoverTimer() {
    clearTimeout(hoverTimer);
    hoverTimer = undefined;
  }

  function closeMenus() {
    cancelHoverTimer();
    openMenu = null;
    submenu = null;
  }

  function toggle(menu: MenuName) {
    cancelHoverTimer();
    submenu = null;
    openMenu = openMenu === menu ? null : menu;
  }

  /** Menu bars track the pointer once open: hovering a sibling switches to it. */
  function hover(menu: MenuName) {
    if (openMenu !== null && openMenu !== menu) {
      cancelHoverTimer();
      submenu = null;
      openMenu = menu;
    }
  }

  /** Every entry closes the menu first, so no item has to remember to. */
  function run(action: () => void) {
    closeMenus();
    action();
  }

  function pick(profile: ShellProfile) {
    closeMenus();
    onnew(profile);
  }

  // Close on outside click or Escape, the way a native menu behaves. Testing
  // containment (rather than stopping propagation inside the menu) keeps the
  // markup free of handlers that exist only to block bubbling.
  function onWindowClick(event: MouseEvent) {
    if (menuWrap && event.target instanceof Node && menuWrap.contains(event.target)) return;
    closeMenus();
  }
  function onWindowKey(event: KeyboardEvent) {
    if (event.key === "Escape") closeMenus();
  }

  async function toggleMaximize() {
    await appWindow.toggleMaximize();
    maximized = await appWindow.isMaximized();
  }

  $effect(() => {
    void appWindow.isMaximized().then((v) => (maximized = v));
  });

  // Don't leave a pending open/close firing after the bar is gone.
  $effect(() => () => cancelHoverTimer());
</script>

<svelte:window onclick={onWindowClick} onkeydown={onWindowKey} />

<!-- data-tauri-drag-region makes the empty areas behave like a real titlebar. -->
<header class="titlebar" data-tauri-drag-region>
  <nav class="menus" bind:this={menuWrap}>
    <!-- File -->
    <div class="menu-host">
      <button
        class="menu-trigger"
        class:open={openMenu === "file"}
        onclick={() => toggle("file")}
        onmouseenter={() => hover("file")}
      >
        File
      </button>

      {#if openMenu === "file"}
        <div class="menu" role="menu">
          <button class="menu-item" role="menuitem" onclick={() => run(onnewwindow)}>
            New Window
          </button>

          <div class="sep"></div>

          <!-- Acts on the active terminal, and toggles: an already-pinned tab
               offers the way back out rather than a dead entry. -->
          <button
            class="menu-item"
            role="menuitem"
            disabled={!hasSession}
            onclick={() => run(onpin)}
          >
            {activePinned ? "Unpin Connection" : "Pin Connection"}
          </button>

          <button
            class="menu-item"
            role="menuitem"
            disabled={!canSaveRemote}
            onclick={() => run(onsaveremote)}
          >
            Save Remote Connection
          </button>
          <button class="menu-item" role="menuitem" onclick={() => run(onmanage)}>
            Manage Saved Connections…
          </button>
          <button class="menu-item" role="menuitem" onclick={() => run(onhostkeys)}>
            Trusted Host Keys…
          </button>

          <div class="sep"></div>
          <button
            class="menu-item"
            role="menuitem"
            disabled={!hasSession}
            onclick={() => run(onclose)}
          >
            Close
          </button>
          <button class="menu-item" role="menuitem" onclick={() => run(onclosewindow)}>
            Close Window
          </button>
          <button class="menu-item" role="menuitem" onclick={() => run(onquit)}>Exit</button>
        </div>
      {/if}
    </div>

    <!-- Edit -->
    <div class="menu-host">
      <button
        class="menu-trigger"
        class:open={openMenu === "edit"}
        onclick={() => toggle("edit")}
        onmouseenter={() => hover("edit")}
      >
        Edit
      </button>

      {#if openMenu === "edit"}
        <div class="menu" role="menu">
          <button
            class="menu-item"
            role="menuitem"
            disabled={!hasSession}
            onclick={() => run(oncopy)}
          >
            <span>Copy Selection</span>
            <span class="hint">Ctrl+Shift+C</span>
          </button>
          <button
            class="menu-item"
            role="menuitem"
            disabled={!hasSession}
            onclick={() => run(onpaste)}
          >
            <span>Paste</span>
            <span class="hint">Ctrl+Shift+V</span>
          </button>

          <div class="sep"></div>

          <button class="menu-item" role="menuitem" onclick={() => run(onappearance)}>
            Appearance…
          </button>
        </div>
      {/if}
    </div>

    <!-- Terminal -->
    <div class="menu-host">
      <button
        class="menu-trigger"
        class:open={openMenu === "terminal"}
        onclick={() => toggle("terminal")}
        onmouseenter={() => hover("terminal")}
      >
        Terminal
      </button>

      {#if openMenu === "terminal"}
        <div class="menu" role="menu">
          <div
            class="submenu-host"
            role="none"
            onmouseenter={() => openSubmenuSoon("local")}
            onmouseleave={closeSubmenuSoon}
          >
            <button
              class="menu-item"
              role="menuitem"
              aria-haspopup="true"
              aria-expanded={submenu === "local"}
              onclick={() => {
                // Clicking is an explicit request: skip the hover delay.
                cancelHoverTimer();
                submenu = submenu === "local" ? null : "local";
              }}
            >
              <span>New Local Terminal</span>
              <span class="chev">›</span>
            </button>

            {#if submenu === "local"}
              <div class="submenu" role="menu">
                {#if profiles.length === 0}
                  <div class="menu-empty">No shells detected</div>
                {:else}
                  {#each profiles as profile (profile.id)}
                    {@const kind = profileKind(profile)}
                    <button
                      class="menu-item kinded"
                      data-kind={kind}
                      role="menuitem"
                      title={KIND_TITLE[kind]}
                      onclick={() => pick(profile)}
                    >
                      <KindIcon {kind} size={10} />
                      <span class="grow">{profile.label}</span>
                    </button>
                  {/each}
                {/if}
              </div>
            {/if}
          </div>

          <div
            class="submenu-host"
            role="none"
            onmouseenter={() => openSubmenuSoon("saved")}
            onmouseleave={closeSubmenuSoon}
          >
            <button
              class="menu-item"
              role="menuitem"
              aria-haspopup="true"
              aria-expanded={submenu === "saved"}
              onclick={() => {
                cancelHoverTimer();
                submenu = submenu === "saved" ? null : "saved";
              }}
            >
              <span>New Saved Connection</span>
              <span class="chev">›</span>
            </button>

            {#if submenu === "saved"}
              <div class="submenu wide" role="menu">
                {#if saved.length === 0}
                  <div class="menu-empty">Nothing saved yet</div>
                {:else}
                  {#each saved as connection (connection.id)}
                    <button
                      class="menu-item kinded saved-item"
                      data-kind="remote"
                      role="menuitem"
                      title={KIND_TITLE.remote}
                      onclick={() => {
                        closeMenus();
                        onsaved(connection);
                      }}
                    >
                      <KindIcon kind="remote" size={10} />
                      <span class="grow">{connectionLines(connection).primary}</span>
                      <span class="hint">{connectionLines(connection).secondary}</span>
                    </button>
                  {/each}
                {/if}
              </div>
            {/if}
          </div>

          <button
            class="menu-item kinded"
            data-kind="remote"
            role="menuitem"
            title={KIND_TITLE.remote}
            onclick={() => run(onremote)}
          >
            <KindIcon kind="remote" size={10} />
            <span class="grow">New Remote Connection…</span>
          </button>

          <div class="sep"></div>

          <button
            class="menu-item"
            role="menuitem"
            disabled={!hasSession}
            onclick={() => run(onrefresh)}
          >
            Refresh Session
          </button>
        </div>
      {/if}
    </div>
  </nav>

  <!-- Grows to fill the middle, so most of the bar stays draggable. -->
  <div class="drag" data-tauri-drag-region></div>

  <div class="controls">
    <button class="ctl" title="Minimize" onclick={() => appWindow.minimize()}>
      <svg width="8" height="8" viewBox="0 0 10 10" aria-hidden="true">
        <path d="M0 5 h10" stroke="currentColor" stroke-width="1.2" />
      </svg>
    </button>

    <button class="ctl" title={maximized ? "Restore" : "Maximize"} onclick={toggleMaximize}>
      {#if maximized}
        <svg width="8" height="8" viewBox="0 0 10 10" aria-hidden="true">
          <rect x="0.5" y="2.5" width="7" height="7" rx="1.2" fill="none" stroke="currentColor" stroke-width="1.1" />
          <path d="M2.6 2.4 V1.6 A1 1 0 0 1 3.6 0.6 H8.5 A1 1 0 0 1 9.5 1.6 V6.5 A1 1 0 0 1 8.5 7.5 H7.6" fill="none" stroke="currentColor" stroke-width="1.1" />
        </svg>
      {:else}
        <svg width="8" height="8" viewBox="0 0 10 10" aria-hidden="true">
          <rect x="0.6" y="0.6" width="8.8" height="8.8" rx="1.4" fill="none" stroke="currentColor" stroke-width="1.1" />
        </svg>
      {/if}
    </button>

    <!-- Closes this window, not the application: with two windows open, the
         other one carries on. File -> Exit is the one that ends everything. -->
    <button class="ctl close" title="Close window" onclick={onclosewindow}>
      <svg width="8" height="8" viewBox="0 0 10 10" aria-hidden="true">
        <path d="M0.6 0.6 L9.4 9.4 M9.4 0.6 L0.6 9.4" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" />
      </svg>
    </button>
  </div>
</header>

<style>
  .titlebar {
    /* Every chip in the bar is sized from this rather than from a repeated
       literal, so the menus and the window buttons stay the same height and
       centre on the same line. */
    --titlebar-height: 28px;

    display: flex;
    align-items: stretch;
    height: var(--titlebar-height);
    flex: none;
    /* The eye does not centre the menus in the 28px bar -- it centres them in
       the whole band between the window's top edge and the viewport below,
       which is the bar plus the viewport's inset. Padding the top by that inset
       hands the extra space to the top, so the chips land on the band's centre
       line instead of riding high above it. */
    padding-top: var(--viewport-inset);
    /* Less the pill's own padding and margin, so the label -- not the invisible
       pill -- starts on the shared text line. */
    padding-left: calc(var(--bar-text-inset) - 0.7rem - 1px);
    user-select: none;
    /* Above the terminal stage, so menus are never painted over. */
    position: relative;
    z-index: 100;
  }

  .menus {
    display: flex;
    align-items: center;
  }

  .menu-host {
    position: relative;
    display: flex;
    align-items: center;
  }

  /* Hover chips float inside the bar instead of spanning its full height, and
     are fully rounded into pills. Height is the bar less the chip inset top and
     bottom -- stated rather than left to the text's own line box, so the pill
     is centred on the bar exactly and matches the window buttons beside it. The
     label is then centred inside the pill by the flexbox, not by padding. */
  .menu-trigger {
    display: flex;
    align-items: center;
    height: calc(var(--titlebar-height) - 2 * var(--chip-inset));
    background: transparent;
    border: none;
    border-radius: 999px;
    color: var(--fg);
    cursor: pointer;
    font-size: 0.75rem;
    line-height: 1;
    /* Horizontal room the pill needs on its ends, or the rounded caps crowd the
       text -- plus a hair at the top. `align-items: center` centres the line
       box, and that box reserves descender depth these three labels never use,
       so the letters sit about 0.7px high; top padding shifts content by half
       its value, hence 1.4px. Measured off a screenshot, not guessed. */
    padding: 1.4px 0.7rem 0;
    margin: 0 1px;
    transition: background 90ms ease;
  }
  .menu-trigger:hover {
    background: var(--hover);
  }
  .menu-trigger.open {
    background: var(--hover-strong);
  }

  .drag {
    flex: 1;
    min-width: 0;
  }

  .controls {
    display: flex;
    align-items: center;
    flex: none;
    padding-right: 2px;
  }

  /* Rounded and inset, so they read as buttons rather than as slabs welded to
     the window edge. Same height as the menu pills, from the same measurement. */
  .ctl {
    width: 30px;
    height: calc(var(--titlebar-height) - 2 * var(--chip-inset));
    background: transparent;
    border: none;
    border-radius: var(--chip-radius);
    color: var(--fg-dim);
    cursor: pointer;
    display: grid;
    place-items: center;
    margin: 0 1px;
  }
  .ctl:hover {
    background: var(--hover);
    color: var(--fg);
  }
  .ctl.close:hover {
    background: #f38ba8;
    color: var(--accent-ink);
  }

  /* Menus float above everything, including the terminal. */
  .menu {
    position: absolute;
    top: 100%;
    left: 0;
    z-index: 1000;
    min-width: 190px;
    padding: 0.2rem;
    background: var(--bg-menu);
    border: 1px solid var(--border);
    border-radius: 5px;
    box-shadow: 0 8px 24px #0009;
  }

  .submenu-host {
    position: relative;
  }

  .submenu {
    position: absolute;
    top: -0.2rem;
    left: 100%;
    z-index: 1001;
    min-width: 190px;
    padding: 0.2rem;
    margin-left: 2px;
    background: var(--bg-menu);
    border: 1px solid var(--border);
    border-radius: 5px;
    box-shadow: 0 8px 24px #0009;
  }

  .menu-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1.25rem;
    width: 100%;
    text-align: left;
    background: transparent;
    border: none;
    border-radius: 3px;
    color: var(--fg);
    cursor: pointer;
    font-size: 0.8rem;
    padding: 0.35rem 0.5rem;
  }
  .menu-item:hover:not(:disabled) {
    background: var(--hover);
    color: var(--accent);
  }
  .menu-item:disabled {
    color: var(--fg-faint);
    cursor: default;
  }

  /* Anything you can *open* is colour-coded by kind — blue local, red admin,
     mauve remote — so an entry reads as privileged, or as remote, before you
     click it. Commands about those things (Pin, Save, Manage…) stay neutral:
     the colour marks a destination, not an action. */
  .menu-item.kinded {
    --kind: var(--kind-local);
    --kind-soft: var(--kind-local-soft);

    gap: 0.5rem;
    color: var(--kind);
  }
  .menu-item.kinded[data-kind="admin"] {
    --kind: var(--kind-admin);
    --kind-soft: var(--kind-admin-soft);
  }
  .menu-item.kinded[data-kind="remote"] {
    --kind: var(--kind-remote);
    --kind-soft: var(--kind-remote-soft);
  }
  .menu-item.kinded:hover:not(:disabled) {
    background: var(--kind-soft);
    color: var(--kind);
    filter: brightness(1.12);
  }

  /* Takes the slack so the trailing detail stays right-aligned. */
  .grow {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .chev,
  .hint {
    color: var(--fg-dim);
    font-size: 0.72rem;
  }
  .menu-item:disabled .hint {
    color: var(--fg-faint);
  }

  .sep {
    height: 1px;
    margin: 0.2rem 0.3rem;
    background: var(--border);
  }

  .submenu.wide {
    min-width: 230px;
  }

  .menu-empty {
    color: var(--fg-dim);
    font-size: 0.78rem;
    padding: 0.35rem 0.5rem;
  }
</style>
