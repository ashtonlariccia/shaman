<script lang="ts">
  import { onMount } from "svelte";
  import { invoke, Channel } from "@tauri-apps/api/core";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import { WebglAddon } from "@xterm/addon-webgl";
  import "@xterm/xterm/css/xterm.css";

  import type { TerminalApi } from "./terminalApi";
  import { CLEAR_LINE, lineEditorFor } from "./lineEditor";

  import type { SshRequest } from "./ConnectDialog.svelte";

  type Props = {
    /** Exactly one of these: a local shell profile, or an SSH target. */
    profileId?: string;
    ssh?: SshRequest;
    active: boolean;
    onopened?: (sessionId: number) => void;
    onfailed?: (failure: import("./types").SshFailureInfo) => void;
    /** Hands the menu bar a handle on this terminal. */
    onready?: (api: TerminalApi) => void;
    ongone?: () => void;
    /** Right-click with nothing selected: read the clipboard and paste it here. */
    onpasterequest?: () => void;
    /** Right-click with a selection: copy it. */
    oncopyrequest?: () => void;
  };

  let {
    profileId,
    ssh,
    active,
    onopened,
    onfailed,
    onready,
    ongone,
    onpasterequest,
    oncopyrequest,
  }: Props = $props();

  let host: HTMLDivElement;
  let term: Terminal | undefined;
  let fit: FitAddon | undefined;
  let sessionId: number | null = null;

  // Dev-only proof the full loop works: JS -> ConPTY -> channel -> xterm. The
  // marker appears twice (echoed input, then output), so >= 2 means the shell
  // really executed it rather than merely echoing.
  let probeSeen = 0;
  let probeDone = false;
  const PROBE = "shaman-pty-ok";

  function syncSize() {
    if (!term || !fit || sessionId === null) return;
    // A hidden element measures as zero; fitting then would corrupt the size.
    if (host.clientWidth === 0 || host.clientHeight === 0) return;
    fit.fit();
    void invoke("session_resize", { id: sessionId, cols: term.cols, rows: term.rows });
  }

  onMount(() => {
    const t = new Terminal({
      fontFamily: '"Cascadia Mono", Consolas, "Courier New", monospace',
      fontSize: 13,
      scrollback: 5000,
      cursorBlink: true,
      allowProposedApi: true,
      // Catppuccin Charcoal, matching ui/src/app.css. Background tracks the
      // editor surface so the terminal and its padding are seamless.
      theme: {
        background: "#242424",
        foreground: "#d8d8d8",
        cursor: "#f5e0dc",
        cursorAccent: "#242424",
        selectionBackground: "#cba6f733",
        black: "#404040",
        red: "#f38ba8",
        green: "#a6e3a1",
        yellow: "#f9e2af",
        blue: "#89b4fa",
        magenta: "#cba6f7",
        cyan: "#94e2d5",
        white: "#d0d0d0",
        brightBlack: "#5c5c5c",
        brightRed: "#f38ba8",
        brightGreen: "#a6e3a1",
        brightYellow: "#f9e2af",
        brightBlue: "#89b4fa",
        brightMagenta: "#cba6f7",
        brightCyan: "#94e2d5",
        brightWhite: "#f5f5f5",
      },
    });
    const f = new FitAddon();
    t.loadAddon(f);
    t.open(host);

    // GPU rendering keeps a flooding terminal cheap; xterm falls back to the DOM
    // renderer by itself if the driver refuses.
    try {
      const webgl = new WebglAddon();
      // A WebGL context can be taken away after it was granted -- the driver
      // resets, or the browser evicts the oldest context because every open tab
      // holds one and Chromium caps them. A lost context that nobody disposes
      // leaves the terminal painting nothing at all: output arrives, scrollback
      // grows, and the pane stays frozen on whatever it last drew. Disposing the
      // addon hands rendering back to the DOM renderer, which is slower but
      // always works.
      webgl.onContextLoss(() => {
        console.warn("webgl context lost, falling back to the DOM renderer");
        webgl.dispose();
      });
      t.loadAddon(webgl);
    } catch (e) {
      console.warn("webgl renderer unavailable, using fallback", e);
    }

    term = t;
    fit = f;
    f.fit();

    // The menu bar acts on whichever terminal is active, so expose the few
    // operations it needs rather than reaching into xterm from outside.
    const api: TerminalApi = {
      copySelection: () => (t.hasSelection() ? t.getSelection() : null),
      clearSelection: () => t.clearSelection(),
      paste: (text: string) => {
        if (sessionId === null) return;
        // Hand it to xterm rather than writing the string to the PTY ourselves.
        // xterm does two things a raw write does not. It rewrites CRLF and LF
        // to CR, because Enter on a terminal is \r and a stray \n lands in the
        // middle of a line. And when the program on the other end has asked for
        // bracketed paste (DECSET 2004), it wraps the text in \e[200~ ... \e[201~
        // -- the marker that says "this arrived from a clipboard, not a
        // keyboard". Vim uses it to suspend autoindent and abbreviations for
        // the duration; without it every pasted line is indented by the one
        // above and a block stair-steps off the right of the screen.
        t.paste(text);
      },
      focus: () => t.focus(),
    };
    onready?.(api);

    // Right-click copies a selection, and pastes when there isn't one -- the
    // console behaviour Windows has always had.
    //
    // Registered in the capture phase, on the wrapper rather than on xterm's own
    // element, for two reasons. It runs before xterm's mouse handling, so
    // stopping the event there means a program reading the mouse (vim with
    // `set mouse=a`, htop) never sees a right-click it would treat as a click in
    // its own UI. And `preventDefault` suppresses WebView2's own context menu,
    // which would otherwise open over the terminal.
    const onContextMenu = (event: MouseEvent) => {
      event.preventDefault();
      event.stopPropagation();
      if (t.hasSelection()) oncopyrequest?.();
      else onpasterequest?.();
    };
    host.addEventListener("contextmenu", onContextMenu, { capture: true });

    // --- Esc Esc clears the line being typed --------------------------------
    //
    // Only for readline-family shells. `cmd.exe` and PSReadLine already clear on
    // a single Escape (see lineEditor.ts), so there the keystroke is passed
    // straight through and pressing it twice works by itself; intercepting would
    // only put a delay in front of behaviour that is already right.
    const editor = lineEditorFor({ profileId, ssh });

    // How long the first Escape is held while we wait to see whether a second
    // one follows. It is only ever *this* long for a lone Escape, which does
    // nothing in a readline shell anyway -- any other key flushes it at once.
    const DOUBLE_ESC_MS = 400;

    let escapeHeld = false;
    let escapeTimer: ReturnType<typeof setTimeout> | undefined;

    function toShell(data: string) {
      if (sessionId !== null) void invoke("session_write", { id: sessionId, data });
    }

    function releaseEscape(send: boolean) {
      if (!escapeHeld) return;
      escapeHeld = false;
      clearTimeout(escapeTimer);
      if (send) toShell("\x1b");
    }

    if (editor === "readline") {
      t.attachCustomKeyEventHandler((event) => {
        if (event.type !== "keydown") return true;

        // A full-screen program owns Escape outright: in vim it leaves insert
        // mode, and Esc Esc is how you make sure you are in normal mode. Those
        // all run on the alternate screen, which is the signal to stay out of
        // the way entirely.
        if (t.buffer.active.type === "alternate") {
          releaseEscape(true);
          return true;
        }

        // Anything else the user types means the held Escape was a meta prefix
        // after all (`Esc f` is forward-word). Send it first so the shell sees
        // the two in the order they were pressed, with no delay on the second.
        if (event.key !== "Escape" || event.ctrlKey || event.altKey || event.metaKey) {
          releaseEscape(true);
          return true;
        }

        if (escapeHeld) {
          releaseEscape(false); // the pair is the gesture; neither Escape is sent
          toShell(CLEAR_LINE);
          return false;
        }

        escapeHeld = true;
        escapeTimer = setTimeout(() => {
          escapeHeld = false;
          toShell("\x1b");
        }, DOUBLE_ESC_MS);
        return false;
      });
    }

    let disposed = false;

    const channel = new Channel<ArrayBuffer | number[]>();
    channel.onmessage = (message) => {
      const bytes =
        message instanceof ArrayBuffer
          ? new Uint8Array(message)
          : Uint8Array.from(message as number[]);
      t.write(bytes);

      if (import.meta.env.DEV && !probeDone) {
        probeSeen += (new TextDecoder().decode(bytes).match(new RegExp(PROBE, "g")) ?? []).length;
        if (probeSeen >= 2) {
          probeDone = true;
          void invoke("ui_ready", { detail: `PTY_OK marker seen ${probeSeen}x` });
        }
      }
    };

    (async () => {
      try {
        const id = ssh?.savedId
          ? // Saved connection: the backend resolves the password from the
            // vault, so it never crosses the IPC boundary.
            await invoke<number>("ssh_connect_saved", {
              onOutput: channel,
              id: ssh.savedId,
              cols: t.cols,
              rows: t.rows,
            })
          : ssh
          ? await invoke<number>("ssh_connect", {
              onOutput: channel,
              target: ssh,
              cols: t.cols,
              rows: t.rows,
            })
          : await invoke<number>("session_open", {
              onOutput: channel,
              profileId,
              cols: t.cols,
              rows: t.rows,
            });
        if (disposed) {
          void invoke("session_close", { id });
          return;
        }
        sessionId = id;
        onopened?.(id);

        t.onData((data) => {
          if (sessionId !== null) void invoke("session_write", { id: sessionId, data });
        });

        if (active) t.focus();
        if (import.meta.env.DEV && !ssh) {
          void invoke("session_write", { id, data: `echo ${PROBE}\r` });
        }
      } catch (e) {
        // ssh_connect rejects with a structured { kind, message, fingerprint };
        // local shells reject with a plain string.
        const failure: import("./types").SshFailureInfo =
          e && typeof e === "object" && "message" in e
            ? (e as import("./types").SshFailureInfo)
            : { message: String(e) };
        onfailed?.(failure);
        // The tab stays put unless something else removes it (the connect
        // dialog does, so it can offer the fields back). Say so, rather than
        // leaving a dead terminal that looks like it might still be trying.
        t.write(`\r\n\x1b[31m${failure.message}\x1b[0m\r\n`);
        t.write(
          "\x1b[90mNot connected. Close this tab with the × in the sidebar, " +
            "or File → Close.\x1b[0m\r\n",
        );
      }
    })();

    // Debounced: resizing while output streams can corrupt ConPTY's screen.
    let resizeTimer: ReturnType<typeof setTimeout> | undefined;
    const observer = new ResizeObserver(() => {
      clearTimeout(resizeTimer);
      resizeTimer = setTimeout(syncSize, 75);
    });
    observer.observe(host);

    return () => {
      disposed = true;
      clearTimeout(resizeTimer);
      clearTimeout(escapeTimer);
      host.removeEventListener("contextmenu", onContextMenu, { capture: true });
      observer.disconnect();
      ongone?.();
      if (sessionId !== null) void invoke("session_close", { id: sessionId });
      t.dispose();
    };
  });

  // A hidden terminal can't be measured, so re-fit when it comes back into view.
  $effect(() => {
    if (!active) return;
    requestAnimationFrame(() => {
      syncSize();
      term?.focus();
    });
  });
</script>

<div class="pane" class:hidden={!active}>
  <div class="term" bind:this={host}></div>
</div>

<style>
  .pane {
    position: absolute;
    inset: 0;
    padding: 0.5rem;
  }

  /* Kept mounted, not destroyed -- switching tabs must not lose scrollback. */
  .hidden {
    visibility: hidden;
    pointer-events: none;
    z-index: -1;
  }

  .term {
    height: 100%;
    width: 100%;
  }
</style>
