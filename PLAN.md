# Shaman — Terminal Manager for Windows

A tabbed terminal application for Windows that manages `cmd`, PowerShell, elevated
shells, WSL distros, and SSH connections from a single window, with a left-hand
session pane for switching, launching, and killing terminals.

---

## 1. Verified environment

All checked on this machine (2026-08-08). The dev loop works end-to-end:
I edit files from WSL at `/mnt/c/...` and drive the **Windows** toolchain over
WSL interop, so builds are native NTFS + native MSVC (no cross-compilation).

| Component | Status |
|---|---|
| Windows 11 Home | build `10.0.26200` |
| WSL interop | enabled (`/proc/sys/fs/binfmt_misc/WSLInterop`) |
| Rust (Windows) | `1.96.0`, target `x86_64-pc-windows-msvc` |
| MSVC linker | VS Build Tools 2026, MSVC `14.51.36231` |
| WebView2 runtime | `151.0.4129.72` (pre-installed) |
| Node (Windows) | `v24.14.1`, npm `11.11.0` |
| OpenSSH client | `C:\windows\System32\OpenSSH\ssh.exe` |
| WSL distros | `main` |

**Smoke-tested:** created, compiled, and *executed* a Windows release binary
from the WSL shell. The development loop is proven.

### Build invocation rules (important)

- Use `cargo.exe` / `node.exe`, **never** the Linux `cargo` or `node` — the
  target is Windows.
- `npm` on the WSL `PATH` resolves to the Windows shell script and fails under
  bash. Always call it as `cmd.exe /c "npm ..."`.
- Keep the project on the Windows filesystem (`C:\Users\LaRiccia\Desktop\shaman`)
  so the Windows toolchain gets native I/O speed.
- I can verify the GUI myself by screenshotting the window via PowerShell +
  `System.Drawing`, so UI work is not blind.

---

## 2. Stack decision

**Tauri v2 + Rust backend + Svelte/TypeScript frontend + xterm.js.**

RAM footprint was the stated top priority, so the reasoning:

| Option | Baseline RAM | Verdict |
|---|---|---|
| Electron | ~150–250 MB | Rejected — heaviest |
| **Tauri v2 (WebView2)** | **~60–120 MB** | **Chosen** |
| Pure-Rust GPU GUI (`egui`/`wgpu` + `alacritty_terminal`) | ~50–90 MB | Rejected for v1 — scope |

Tauri wins on effort-per-megabyte. The pure-native option saves maybe 30 MB but
requires writing a terminal renderer from scratch: font shaping, ligatures,
wide/CJK glyphs, selection, scrollback, reflow. That is a project unto itself.
xterm.js already solves all of it correctly, and WebView2 is a shared OS
component that is already installed here.

**Hedge against that trade-off:** `shaman-core` is a standalone crate with
**zero Tauri dependencies**. All the genuinely hard logic — ConPTY, SSH,
elevation, the session model, the credential vault — lives there. If the
WebView2 footprint ever becomes unacceptable, the UI layer can be replaced
without touching the core.

**RAM mitigations baked in from day one:** WebGL renderer addon, backend-side
scrollback caps, output coalescing, and virtualizing terminals that are not
visible.

---

## 3. Architecture

```
shaman/
├── crates/
│   ├── shaman-core/        # UI-agnostic. No Tauri. The real logic.
│   │   ├── session/        # Session trait, registry, lifecycle
│   │   ├── pty/            # ConPTY local shells (portable-pty)
│   │   ├── ssh/            # SSH transport (russh)
│   │   ├── elevate/        # UAC broker client + named-pipe protocol
│   │   ├── profiles/       # Shell detection + saved connection profiles
│   │   └── vault/          # DPAPI-encrypted credential store
│   ├── shaman-helper/      # Small elevated broker .exe (see §4)
│   └── shaman-app/         # Tauri shell: commands, channels, window
├── ui/                     # Svelte + Vite + xterm.js
└── PLAN.md
```

### The session abstraction

Everything the UI touches is one uniform thing — a bidirectional byte stream
with a size and a lifecycle:

```rust
pub trait Session: Send {
    fn id(&self) -> SessionId;
    fn write(&mut self, data: &[u8]) -> Result<()>;
    fn resize(&mut self, cols: u16, rows: u16) -> Result<()>;
    fn kill(&mut self) -> Result<()>;
    // output is pushed to a subscriber channel, not polled
}
```

A local `cmd.exe`, an elevated PowerShell, a WSL shell, and an SSH connection
are then *the same object* to the entire UI layer. Adding a session type later
means implementing one trait, not touching the frontend.

### Crate choices

| Need | Crate | Why |
|---|---|---|
| Local PTY | `portable-pty` | WezTerm's ConPTY layer; the most battle-tested Windows PTY in Rust |
| SSH | `russh` | Pure Rust (no C build step), async, supports password / pubkey / keyboard-interactive |
| Windows APIs | `windows` | DPAPI, named pipes, `ShellExecuteW`, job objects |
| Terminal UI | `xterm.js` + `addon-webgl`, `addon-fit`, `addon-search`, `addon-serialize` | Correct VT emulation, GPU-rendered |

### IPC performance (a real trap)

Tauri's event system is too slow for terminal throughput — `cat` a large file
and an event-per-chunk design will lock the UI. Instead:

- One Tauri v2 `ipc::Channel<Vec<u8>>` **per session** (a direct, much cheaper path).
- **Coalesce output in Rust** (`shaman-core/src/pump.rs`): buffer PTY bytes and
  flush when the stream goes quiet for ~3 ms, when a byte has waited ~16 ms (one
  60Hz frame), or at ~64 KB — whichever comes first. This turns thousands of tiny
  writes into a handful of batched ones and is the single biggest perf decision in
  the app. Flushing on *quiet* is what lets the delay cap be generous without
  making an echoed keystroke feel late.
- **Windows' default timer granularity is 15.6 ms, and it is per-process since
  Windows 11.** Every wait above rounds up to it unless the process asks for
  better, so the pump's 3 ms and 16 ms deadlines were both silently landing at
  ~15 ms — measured: a 3 ms wait took 14.6 ms on average, 16.8 ms at worst. That
  is a pump delivering output on a 15 ms grid regardless of what it was
  configured to do, which reads as choppy, lumpy scrolling. `win::TimerResolution`
  raises the resolution to 1 ms for as long as any pump thread is alive
  (refcounted, released when the last session closes); the same 3 ms wait then
  measures 3.8 ms. WebView2 renders in its own process, so its own timer requests
  do nothing for ours.
- **Input is queued too** (`pty::spawn_input_writer`, `helper::spawn_frame_writer`).
  Keystrokes arrive on synchronous Tauri commands, which run on the **main
  thread**, and writing to a ConPTY or a named pipe blocks once the reader stops
  draining it. Written inline, one program that has stopped reading its input
  freezes the whole window — a paste into a busy program is enough. Both write
  paths now hand bytes to a dedicated thread and return immediately, and each
  drains whatever else is already queued into one write.

---

## 4. Elevation model — app-wide admin, de-elevated children

**Decision:** the entire Shaman window runs elevated. One UAC prompt at launch
(via an app manifest with `requestedExecutionLevel = requireAdministrator`), and
admin terminals then cost nothing extra. Admin and non-admin sessions coexist in
the same sidebar, distinguished by a badge.

### The problem this inverts

Running elevated makes admin tabs free, but it flips the hard part: child
processes **inherit the elevated token**, so now *non-admin* terminals are the
ones requiring explicit work. Without de-elevation, every terminal would
silently be admin — the worst possible default.

So: **new terminals are non-admin by default**; elevation is opt-in per session.

### How de-elevation works

An elevated token carries a linked "filtered" standard-user token, reachable via
`GetTokenInformation(TokenLinkedToken)`. That token is what we launch normal
sessions with.

**The trap:** we cannot simply call `CreateProcessWithTokenW` on the shell
itself. ConPTY requires a `STARTUPINFOEX` carrying
`PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE`, and `CreateProcessWithTokenW` ignores
extended attribute lists. The pseudoconsole would be silently dropped.

**The fix — invert the broker.** The helper process moves to the *other* side of
the boundary:

1. The elevated app spawns one medium-integrity `shaman-helper.exe` using the
   linked token. No attribute list is needed at that moment, so the API
   limitation does not apply.
2. That helper, now running at normal integrity, creates ConPTYs natively.
3. Bytes proxy back over a named pipe (same protocol as originally designed,
   simply pointing the other direction).

Admin sessions skip the helper entirely and spawn ConPTYs in-process.

### Spike results (run 2026-08-08, elevated)

`crates/shaman-core/examples/spike_deelevate.rs` tested three strategies. The
textbook answer lost:

| Strategy | Result |
|---|---|
| Linked token → `DuplicateTokenEx` → `CreateProcessWithTokenW` | **FAIL** `0x80070542` |
| **Shell (explorer.exe) token → duplicate → `CreateProcessWithTokenW`** | **PASS** |
| SAFER `NORMALUSER` + medium integrity → `CreateProcessAsUserW` | **FAIL** — child still elevated |

**Why the linked token fails:** `GetTokenInformation(TokenLinkedToken)` only
returns a primary-capable token to a caller holding `SeTcbPrivilege`.
Administrators do not have it — SYSTEM does. What we get is an
identification-level token, and duplicating that to a primary token fails with
`ERROR_BAD_IMPERSONATION_LEVEL`. Nearly every write-up of "de-elevate on
Windows" recommends this route; it does not work from an ordinary elevated app.

**Why SAFER fails:** it spawns fine and the integrity stamp applies, but the
child still reports elevated — `TokenIsElevated` follows the enabled admin
group membership, which `SAFER_LEVELID_NORMALUSER` did not demote to deny-only
here. Dropping integrity alone is not de-elevation.

**Chosen: the shell token.** `GetShellWindow()` → owning process → duplicate its
token → `CreateProcessWithTokenW`. Explorer already runs as the user at medium
integrity, so its token is exactly the context we want to hand normal tabs.

**New risk this introduces:** it depends on Explorer running. With no shell
(killed explorer, some kiosk/server configurations) there is no token to borrow,
and normal tabs cannot start. The app must detect this and say so plainly
rather than silently opening an elevated shell where the user expected a normal
one — silently handing someone admin is the one outcome worse than failing.

### The honest cost of running elevated

- The WebView2 frontend now runs at **high integrity** — any frontend RCE is
  admin-level. Mitigations: strict CSP, zero remote content, devtools disabled
  in release builds.
- UIPI blocks drag-and-drop from Explorer into the window.
- The vault file is written by an elevated process, so its ACL must be pinned to
  the user explicitly rather than inherited.
- Every launch shows a UAC prompt. **Dev builds ship without the manifest**, and
  elevation gets tested in deliberate, batched runs.

We still never suppress UAC. Scheduled-task-with-highest-privileges and similar
tricks exist, but they are UAC *bypass* techniques — AV flags them, and they
lie to the user about privilege escalation.

---

## 5. SSH and credential autofill

Per the decisions made: a **real SSH library** (`russh`), not a wrapper around
`ssh.exe`.

This matters — the alternative is watching a PTY for the text `password:` and
injecting keystrokes, which breaks on non-English servers, custom prompts,
banners, MFA, and timing. With `russh` the credentials are passed
*programmatically* through the actual auth protocol. Autofill becomes a real
feature rather than a screen-scraping hack.

Supported auth: password, public key (with encrypted-key passphrase),
keyboard-interactive, and `ssh-agent` / Pageant.

### Crypto backend: `ring`, not `aws-lc-rs`

`russh` defaults to `aws-lc-rs`, whose `aws-lc-sys` build compiles C sources and
needs NASM + CMake — it fails on this toolchain. We use the `ring` backend
instead.

This is not a downgrade, and it was checked rather than assumed: every
backend-gated site in russh is a paired either/or supplying the *same*
primitives (AES-128/256-GCM, ChaCha20-Poly1305), e.g.

```rust
#[cfg(all(not(feature = "aws-lc-rs"), feature = "ring"))]
use ring::aead::{AES_128_GCM, AES_256_GCM};
```

No cipher, KEX or key type exists under one backend and not the other, so server
compatibility is identical. `ring` additionally needs no C toolchain, which
keeps builds reproducible for anyone who clones this.

**Switch to `aws-lc-rs` only if FIPS validation is ever required** — that is the
one thing it offers that `ring` does not, and it is worth installing NASM and
CMake for at that point.

### Credential vault

Per the decision to keep it self-contained: an **encrypted local file** using
**Windows DPAPI** (`CryptProtectData`).

Deliberately **not** Windows Credential Manager: the machines behind these
hostnames and IPs change, and pinning credentials to host strings in the OS
credential store means constantly orphaned and stale entries.

- Secrets are encrypted against the **Windows user account**, so no master
  password is needed and the ciphertext is useless if copied to another machine
  or read by another user.
- Split storage: profile metadata (host, port, username) in readable JSON;
  secrets only ever as DPAPI blobs. Nothing sensitive is in plaintext at rest.
- **Credentials are keyed by a stable profile UUID, never by hostname or IP.**
  This is the detail that makes the above work: edit a profile's address and its
  saved credentials follow it, instead of being orphaned.
- Vault file lives under `%APPDATA%\shaman\`, ACL-pinned to the user (the app
  runs elevated, so this cannot be left to inheritance).

### Autofill authoring

Adding credentials must be a first-class part of connecting, not a separate
settings chore:

- The connect dialog offers **"save these credentials"** inline, so a new
  autofill entry is created as a side effect of a successful connection.
- Profiles are fully editable in-app — add, edit, duplicate, delete — with
  multiple credential sets storable per profile (e.g. a password *and* a key).
- Ad-hoc connections that were never saved can be promoted to a saved profile
  afterwards from the sidebar.

Trade-off worth knowing: DPAPI protects against *other users and stolen files*,
not against malware already running as you. An optional master-password mode
(Argon2id + AES-GCM) is a natural later addition — noted, not built in v1.

### Colour code for session kind

Three kinds, one colour each, applied on every surface: **local blue**
(`#89b4fa`), **admin red** (`#f38ba8`), **remote mauve** (`#cba6f7`) — Catppuccin
Mocha, the same values xterm gets for blue/red/magenta, so the chrome and the
terminal contents agree.

- **Kind is derived once**, in `ui/src/lib/kinds.ts`. Components ask for a kind;
  none of them re-test `elevated` and `ssh` themselves. That is what stops the
  scheme drifting to "blue means local *almost* everywhere".
- **Remote beats elevated.** `elevated` describes local privilege, which an SSH
  tab does not have, so a remote session is never red.
- **Colour marks a destination, not an action.** Things you can open are
  coloured; commands about them (Pin, Save Remote Connection, Manage…) stay
  neutral, or the menus become a wall of colour that says nothing.
- **Remote gets mauve deliberately**, because mauve is also `--accent`: a remote
  surface reading like the app's own highlight is harmless, whereas a local one
  wearing the accent would be a false signal.
- Every colour is paired with a glyph (`>_` local, `><` remote) and a `title`,
  so it never carries meaning alone.

Each surface resolves `--kind` / `--kind-soft` once per row or chip and styles
from those, rather than repeating the three-way choice per property.

### Pinned connections (the bottom strip)

A thin VSCode-style status bar along the bottom edge, holding quick-open buttons
for the terminals you actually reach for. `File → Pin Connection` pins the
active tab; the same entry reads `Unpin Connection` once it is pinned.

Three rules, in `crates/shaman-core/src/pins.rs` and `ui/src/lib/pins.ts`:

- **A pin stores an id, never a name.** Local pins hold a `ShellProfile` id,
  remote pins a saved-connection id, and the label is resolved from the profile
  list or the connection store every time the strip draws. That is what makes a
  rename in *Manage Saved Connections* rename its pin for free. The `label`
  field in `pins.json` is only a fallback for a target that has gone away.
- **One button per target.** Pinning something already pinned refreshes its
  stored label instead of adding a second identical button; identity is
  `(kind, target)`, so a local pin and a saved pin can share an id string.
- **Pinning a remote tab saves it first.** A pin outlives the session it was
  made from, so an unsaved connection is upserted into the vault and the tab is
  re-pointed at its new id. This is why the pin action is in File beside the
  saved-connection entries rather than in Terminal.

Reordering is a drag with **pointer events, not HTML5 drag-and-drop**: the
window has the OS-level drag-drop handler enabled, which swallows `dragstart`
inside the webview, and pointer capture is what keeps a drag tracking when the
cursor leaves a 24px-tall strip. The strip does not shuffle live under the
pointer — it draws an insertion line instead, because re-measuring a layout that
moves as you chase it is what makes such drags jitter. `move_pin` sends the
destination index and the *backend* re-reads the file and moves that one entry,
rather than the frontend overwriting the whole order, so a second window that
pinned something meanwhile does not lose it to a stale list.

Deleting a saved connection drops its pin in the same command — a strip button
that opens nothing is worse than no button. A pin whose *shell* disappeared
(uninstalled pwsh, removed WSL distro) is kept, struck through and inert, so the
absence is explained rather than mysterious.

### Copy and paste

Clipboard text is handed to **`term.paste()`, never written to the PTY
directly**. xterm normalises CRLF and LF to `\r` and, when the program has set
DECSET 2004, wraps the text in `\e[200~ … \e[201~`. Bracketed paste is not
cosmetic: it is the only signal vim has that a run of characters came from a
clipboard rather than a keyboard, and without it autoindent applies to every
pasted line and a block stair-steps off the screen. Writing the raw string to
the PTY skips both transforms, which is what made pasting into vim useless.

`Ctrl+Shift+C`/`V` rather than plain `Ctrl+C`/`V`, which a terminal owes to the
program running in it. The chord is matched by keycap where the layout produces
a Latin letter and by physical position otherwise (`ui/src/lib/keys.ts`), so
Dvorak fires on the key labelled C and Cyrillic still has a reachable chord.
Handling the chord ourselves means `preventDefault()` is load-bearing: without
it the webview also performs its own paste and the text arrives twice.

**Right-click copies a selection and pastes when there is none**, as console
windows on Windows always have. Copying clears the highlight, because the gesture
is otherwise silent — no menu, no output — and an unchanged screen is
indistinguishable from a right-click that did nothing. The handler sits on the
pane wrapper in the **capture** phase, which does three things at once: it
suppresses WebView2's own context menu, it stops xterm from forwarding a
right-click to a program reading the mouse, and it names the pane that was
clicked rather than trusting "the active tab".

### `Esc Esc` clears the line (`ui/src/lib/lineEditor.ts`)

There is no portable sequence for "discard the current line", because the shells
disagree about what `Escape` means at all:

- **`cmd.exe` and PSReadLine bind `Escape` itself to clearing the line.** Nothing
  needs sending, so both presses are forwarded untouched. Intercepting would put
  a delay in front of behaviour that is already correct.
- **readline shells (bash/zsh/fish) treat `Escape` as the meta prefix.** It clears
  nothing, and `Esc Esc` is *filename completion* in bash. So the pair is
  swallowed and `Ctrl+E Ctrl+U` is sent in its place — end of line, then discard
  backwards, which clears the whole line no matter where the cursor sits. `Ctrl+U`
  alone would leave the tail of a line the user had gone back to edit.

Two guards keep that from stealing a key the user meant for something else. A
lone `Escape` is held for 400ms and released as a real `Escape` if no second one
arrives — and released *immediately* if any other key is pressed, so `Esc f`
reaches the shell in order and at full speed. And while the terminal is on the
**alternate screen** — where `vim`, `less` and `htop` live — both presses are
forwarded untouched, because `Esc Esc` there means "make sure I am in normal
mode". The alternate screen is the reliable signal for "a program, not a prompt,
is reading these keys".

---

## 6. Build phases

Each phase ends at something runnable, so progress is verifiable rather than
theoretical.

| # | Phase | Done when |
|---|---|---|
| **0** | Scaffold: Tauri v2 + Svelte + Vite, WSL→Windows build scripts | `cargo tauri dev` opens a window |
| **1** | One hardcoded `cmd.exe` via ConPTY → xterm.js, bidirectional | You can type `dir` and see real output |
| **2** | Session registry, left pane, multiple tabs, switch / kill / resize | Several live terminals, independently killable |
| **3** | Shell profiles + auto-detection: cmd, PS 5.1, pwsh, WSL distros, Git Bash | New-terminal menu lists what is actually installed |
| **4** | App-wide elevation + de-elevated helper for normal sessions | Admin and non-admin tabs coexist; each reports the right level in `whoami /groups` |
| **5** | SSH: `russh` transport, host-key verification, saved connections, DPAPI vault | ✅ Saved connection connects with no manual credential entry |
| **6** | UX: search, copy/paste, themes/fonts, persistence, keyboard shortcuts, settings | Feels like a real product — copy/paste and the pinned-connection strip are in |
| **7** | Packaging: MSI/NSIS installer, icon, signing story | Installable `.msi` that runs on a clean machine |

Phase 1 is deliberately the *second* thing built — ConPTY plumbing is the
highest-risk piece, so it gets de-risked before any UI investment.

Phase 6 is where the `frontend-design` skill gets used properly; earlier phases
stay intentionally plain so effort is not spent styling things that may change.

---

## 7. Known gotchas (already identified)

- **`wsl.exe -l -q` outputs UTF-16LE.** Naive UTF-8 parsing yields garbage
  (`main਀main`). Prefer enumerating distros from the registry at
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Lxss`, which is both correctly
  typed and gives the default-distro GUID.
- **Killing a shell must kill its children.** `taskkill` on the shell alone
  orphans grandchildren. Use a **Windows job object** per session with
  `KILL_ON_JOB_CLOSE` so the whole process tree dies with the tab.
- **ConPTY does not reach EOF when the child exits.** The master pipe stays
  open as long as the pseudoconsole lives, so a reader thread waiting for EOF
  blocks forever and any "session ended" callback hung off it never fires. That
  made `exit` look like a frozen terminal. Detect exit by **waiting on the child
  process** instead (`Child::wait` on its own thread, with a `ChildKiller` kept
  behind for `kill()`). Covered by `pty::tests::exit_ends_the_session`.
- **ConPTY resize races.** Resizing while output is streaming can corrupt the
  screen; debounce resize and serialize it against writes.
- **`portable-pty` sideloads `conpty.dll` from `PATH`.** It deliberately
  prefers a sideloaded ConPTY over `kernel32`, loading it by bare name — which
  searches `PATH`. WezTerm is installed on this machine, is on `PATH`, and ships
  both `conpty.dll` and `OpenConsole.exe` (Feb 2024). We silently ran *WezTerm's*
  console host and spawning a shell hung forever at **0% CPU with no error** —
  about the least debuggable failure mode there is. Cost ~25 minutes in Phase 1.
  Fixed by `win::harden_dll_search()` (`SetDefaultDllDirectories`), which drops
  `PATH` and the CWD from the DLL search order and must run before the first PTY
  is created. It doubles as DLL-planting hardening, which matters because this
  app is meant to run elevated.
- **ConPTY blocks on a cursor-position query at startup.** It emits `ESC[6n`
  and waits for a terminal to answer before the shell produces anything.
  xterm.js answers automatically, so the app is fine — but any *test* driving a
  PTY directly must play the terminal and reply (e.g. `ESC[1;1R`), or it will
  see nothing but the query and time out.
- **PowerShell 5.1 vs `pwsh`** are different binaries at different paths and
  must be detected separately, not assumed.
- **CRLF:** add `.gitattributes` early — files are authored from WSL and
  compiled by Windows tooling.
- **Debug builds load `devUrl`, not the bundled `dist`.** Running
  `target\debug\shaman.exe` without Vite up gives a blank window and no IPC —
  which looks exactly like a broken app. Cost real debugging time in Phase 0.
  `scripts/verify.sh` starts Vite first, which is what makes the check valid.
- **Verify the UI from logs, not screenshots.** WebView2 renders out of
  process, so "page failed to render" and "capture didn't work" produce the same
  black image. The `UI_READY` beacon (frontend → `ui_ready` command → tracing)
  is unambiguous. Screenshots do work once something is actually rendered.
- **Kill dev servers by port, never by process name.** Other projects on this
  machine run `node.exe` too; `scripts/verify.ps1` only stops whatever listens
  on 5173.
- **`Get-Content -Raw` returns `$null` for an empty file**, so `.Trim()` on it
  throws. Use `[string]::IsNullOrWhiteSpace`.
- **`svelte.config.js` must live in Vite's root** (`ui/`), not the repo root, or
  the plugin silently falls back to defaults.
- **Screenshots must be taken from a DPI-aware process.** This display runs at
  150%, so the window is 1800x1200 device pixels. A DPI-*unaware* PowerShell
  gets virtualized coordinates — `GetWindowRect` reports 1200x800, and
  `PrintWindow` then fills only the top-left two-thirds of the window. The
  cropped third happened to contain the window controls, which looked like a
  rendering bug in the app when nothing was wrong. `screenshot.ps1` now calls
  `SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2)` first. **Do not trust a
  screenshot whose dimensions don't match `window.innerWidth * devicePixelRatio`.**
- **A JS error mid-component silently truncates the render.** Svelte stops at
  the throw and everything after it simply never appears — no error anywhere
  visible, because WebView2's console cannot be read from outside. `main.ts`
  now forwards `error` and `unhandledrejection` to the Rust log, which
  `verify.sh` captures. Check that log before believing markup "didn't render".
- **`overflow` creates a clipping context.** `overflow-y: auto` on the sidebar
  clipped any menu that extended beyond it. Scrolling belongs on the inner list,
  never on a container that popovers must escape.
- **One duplex named pipe + `try_clone` deadlocks.** `try_clone` duplicates the
  *handle*, but both refer to the same file object, and Windows serialises I/O
  on synchronous (non-overlapped) handles. A reader thread parked in `ReadFile`
  therefore blocks every concurrent `WriteFile` — forever, with no error. The
  helper handshake succeeded and then the very first command hung. Fixed with
  **two pipes, one per direction** (`-c2s`, `-s2c`), connected in a fixed order.
  The alternative is overlapped I/O, which is far more machinery for the same
  result.
- **The lower-integrity side must create the pipe.** An object created by an
  elevated process carries a high mandatory label, which blocks a
  medium-integrity peer from writing to it. Higher integrity can always open
  lower, so the helper listens and the app connects.
- **A custom app manifest REPLACES Tauri's default — it does not merge.**
  Tauri's `windows-app-manifest.xml` exists to declare a dependency on
  `Microsoft.Windows.Common-Controls 6.0.0.0`. Supplying our own manifest for
  `requestedExecutionLevel` dropped it, so the process bound to comctl32 v5,
  which does not export `TaskDialogIndirect`, and the app died at startup with
  *"The procedure entry point TaskDialogIndirect could not be located in the
  dynamic link library"*. Any custom manifest must carry that dependency block.
- **UIPI blocks screenshots of an elevated window from a normal process.**
  `PrintWindow` against a high-integrity window from a medium-integrity
  PowerShell silently returns a blank (white) bitmap — it looks exactly like a
  broken UI. Capture the elevated app from an elevated shell.
- **The installer must ship the helper.** `bundle.externalBin` stages it as a
  Tauri sidecar (`binaries/shaman-helper-<triple>.exe`); the bundler strips the
  triple and places it beside `shaman.exe`, which is where `helper_path()`
  looks. Without it an *installed* copy elevates fine but every ordinary
  terminal fails — invisible when running from `target/release`. Verify after
  changing bundling with:
  `msiexec /a <msi> /qn TARGETDIR=<dir>` then check both exes are present.
- **`verify.sh` used to run whatever exe was last built.** Vite serves the
  frontend fresh while the Rust half is stale, so a newly added command exists
  in the JS and rejects at runtime — and the run still reports PASS, because
  `UI_READY` has nothing to do with it. Two separate changes were briefly
  "verified" against a binary that predated them. It now builds first.
- **A blocking synchronous Tauri command freezes the whole window.** Sync
  commands run on the main thread, so anything that waits — `ssh_connect`
  waiting out a 15s connect timeout against a host that is off — parks the event
  loop. Every other tab, the menus and the window controls stop responding, and
  it reads as "the app hung", not "that one connection failed". `new_window` had
  already hit this and been made `async`; SSH had the same bug and now uses
  `async` plus `spawn_blocking`. **Any command that can wait must be `async`.**
  Confirmed with a log-timed probe: issuing an unrelated IPC call one second
  into a doomed connect, the reply was logged 15s later on the old build and
  1.0s later on the fixed one.
- **An unreachable host does not refuse the connection, it ignores it.** A
  device that is off or on another network swallows the SYN, so Windows retries
  for ~21s before giving up — the failure everyone actually hits is also the
  slowest. Shaman connects the TCP socket itself on a short `REACH_TIMEOUT`
  budget and hands the live stream to `russh::client::connect_stream`, so the
  common failure is fast and specific while a reachable host still pays for
  exactly one connection. Handshake and auth carry their own separate deadlines;
  without an auth deadline, russh's hour-long inactivity timeout is what governs
  a server that answers and then goes quiet.
- **`shaman-helper.exe` must NOT carry an elevation manifest.** If it inherits
  `requireAdministrator`, every "de-elevated" terminal is silently an admin
  shell — the exact failure this design exists to prevent. `release.sh` checks
  the binary is present; the manifest difference is asserted by the spike.

---

## 8. Open questions (not blocking — decide when we reach them)

1. **Split panes** — real tmux-style splits, or strictly one terminal per tab?
   Affects the layout model, so worth deciding before Phase 6.
2. **Session restore** — should tabs (and SSH reconnects) come back on relaunch?
3. **Code signing** — unsigned installers trigger SmartScreen warnings. Fine for
   personal use; matters if this is ever distributed.
