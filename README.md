# Shaman

A tabbed terminal manager for Windows. One window for `cmd`, PowerShell,
elevated shells, WSL distros, and SSH connections — with a left sidebar to
switch between them, launch new ones, and kill them.

See [`PLAN.md`](PLAN.md) for architecture and the phase roadmap.

## Elevation

Release builds carry a `requireAdministrator` manifest, so **Shaman prompts for
UAC once at launch** and admin terminals then open with no further prompts.

That inverts the usual problem: children of an elevated process inherit its
privileges, so *ordinary* terminals have to be pushed back down. They are hosted
by `shaman-helper.exe`, which the app launches at medium integrity using the
desktop shell's token. The helper must sit beside `shaman.exe`.

Verified by spike when the model was built (see `PLAN.md` §4): in-process tabs
report **High** integrity, helper-hosted tabs report **Medium**.

Debug builds stay `asInvoker` so routine dev cycles don't fire UAC; admin tabs
refuse to open there, with an explanation.

## Status

Phase 5 complete — SSH with password **and private-key** auth, host-key
verification, saved connections, and a DPAPI-encrypted credential vault,
alongside local and admin terminals. Phase 6 has started: copy/paste and the
pinned-connection strip are in.

Connecting is staged, so a host that is off or on another network fails in
about five seconds with a plain reason instead of stalling: Shaman opens the TCP
socket first on a short budget, then hands that same socket to the SSH
handshake, which carries its own deadline, as does authentication. The attempt
runs off the main thread, so a doomed connection costs that one tab and leaves
the rest of the window usable. A tab that fails to connect stays open with the
error printed in it, for you to close when you have read it.

Keys are auto-discovered from `%USERPROFILE%\.ssh`. `ssh-agent`/Pageant is not
supported yet: russh 0.62 has an agent client but no `Signer` impl for it, so it
needs one written by hand.

Files live in `%APPDATA%\shaman\`: `known_hosts.json` (trusted fingerprints),
`connections.json` (saved targets; passwords are DPAPI blobs) and `pins.json`
(the bottom strip). Set `SHAMAN_DATA_DIR` to point them elsewhere — the test
suite does exactly that so it never touches the real ones.

Menus: File (New Window / Pin Connection / saved connections / Close / Exit),
Edit (Copy / Paste), Terminal (New Local Terminal / New Saved Connection / New
Remote Connection / Close Session / Refresh Session).

## Copy and paste

**Right-click copies the selection, or pastes when there is none** — the console
behaviour Windows has always had. Copying clears the highlight, which is the only
sign the gesture did anything: nothing is printed and no menu opens. The
right-click is consumed by Shaman, so a program reading the mouse (`vim` with
`set mouse=a`, `htop`) never sees it as a click in its own UI, and there is no
context menu on the terminal itself; the ones on the sidebar and the pinned strip
are unaffected.

`Ctrl+Shift+C` copies the selection, `Ctrl+Shift+V` pastes; both are also in the
Edit menu. Plain `Ctrl+C`/`Ctrl+V` are left to the program in the terminal —
interrupt, and readline's literal-next. `Shift+Insert` pastes too.

Paste goes **through xterm rather than straight to the shell**, which is what
makes it safe in an editor. Line endings become `\r` (a PTY's Enter), and when
the program has asked for bracketed paste — vim does, on entering — the text is
wrapped in `\e[200~ … \e[201~`. That wrapper tells vim the text came from a
clipboard rather than a keyboard, so it suspends autoindent and abbreviations
for the duration. Without it a pasted block stair-steps further right on every
line.

To select text while a program is reading the mouse itself (`vim` with
`set mouse=a`, `less`, `htop`), **hold Shift while dragging**: that keeps the
drag in the terminal instead of forwarding it to the program.

## Clearing the line

**`Esc Esc` discards the command you are typing.** What that takes depends on the
shell, so Shaman does not pretend otherwise (`ui/src/lib/lineEditor.ts`):

| Shell | How |
|---|---|
| `cmd`, PowerShell, `pwsh` | Nothing sent — `Escape` already clears the line there, so both presses go straight through |
| WSL, SSH (bash/zsh/fish) | The pair is swallowed and `Ctrl+E Ctrl+U` is sent instead: end of line, then discard everything before it |

`Escape` is the *meta prefix* in a readline shell, which is why the second one
has to be intercepted rather than forwarded — in bash, `Esc Esc` is filename
completion. A lone `Escape` is held for 400ms in case a second follows, and any
other keystroke releases it immediately, so `Esc f` (forward-word) still arrives
in the right order at full speed.

**Full-screen programs keep `Escape` for themselves.** `vim`, `less`, `htop` and
friends draw on the alternate screen, and while a terminal is on it Shaman
forwards both presses untouched — `Esc Esc` in `vim` still means "make sure I am
in normal mode".

## Output pacing

Terminal output is batched in Rust before it crosses into the webview: a batch
is sent when the shell goes quiet for ~3ms, when a byte has waited ~16ms (one
frame), or at 64KB. Quiet-first is what keeps an echoed keystroke immediate
while a build log still costs roughly one message per frame.

Those deadlines only mean anything because Shaman raises the process timer
resolution to 1ms while a session is open (`win::TimerResolution`). Windows'
default granularity is 15.6ms and Windows 11 made it per-process, so every wait
in the pump was rounding up to ~15ms — output arriving on a 15ms grid, which is
what choppy scrolling in a busy shell actually was. WebView2 renders in its own
process, so its timer settings do nothing for ours.

Input is queued the same way. Keystrokes arrive on the main thread, and writing
to a ConPTY or the helper's pipe blocks once the far end stops reading, so both
write paths hand off to a thread and return immediately — one program that has
stopped reading its input can no longer freeze the window.

## Colour code

Every terminal wears one of three colours, everywhere it appears — sidebar dot
and selected row, menu entries, the pinned strip, dialog titles:

| | | |
|---|---|---|
| **Local** | blue `#89b4fa` | `cmd`, PowerShell, WSL |
| **Admin** | red `#f38ba8` | anything running elevated |
| **Remote** | mauve `#cba6f7` | SSH, and every surface about SSH |

Catppuccin Mocha, matching the ANSI palette xterm is given, so a blue tab and
blue terminal text are the same blue. Two rules keep it meaningful: the colour
marks a *thing you can open*, never a command about one — Pin, Save and Manage
stay neutral — and remote is the kind that gets mauve because mauve is also the
app accent, so a remote surface and the app's own highlight agreeing is
harmless where a local one wearing the accent would mislead. Kind is derived in
one place (`ui/src/lib/kinds.ts`), and every colour is paired with an icon and a
tooltip so it is never the only cue.

## Pinned connections

A thin strip along the bottom of the window holds quick-open buttons for the
terminals you use most. **File → Pin Connection** pins the active tab; the same
entry reads *Unpin Connection* once it is pinned. On the strip itself,
right-click gives an Open/Unpin menu, and middle-click or `Delete` removes a pin
outright. **Drag a pin sideways to reorder the strip** — a line shows where it
will land, and the order is saved — or hold `Ctrl` and press `←`/`→` to move the
focused one without a mouse.

- Pins store an id, not a name. A local pin reads exactly as it does in
  *Terminal → New Local Terminal*; a remote one reads as the name you gave it in
  *File → Manage Saved Connections*, so renaming there renames the pin.
- The same target can only be pinned once — re-pinning just refreshes the label.
- **Pinning a remote tab saves it first** if it isn't saved already, because a
  pin has to point at something that outlives the session.
- Removing a saved connection removes its pin. A pin whose shell has been
  uninstalled stays visible but struck through, rather than vanishing silently.

To see what detection finds on a machine:

```bash
cargo.exe run -q -p shaman-core --example list_shells
```

## Layout

```
crates/shaman-core/   Session engine: PTY, SSH, elevation, vault. No Tauri deps.
crates/shaman-app/    Tauri shell: commands, IPC channels, window.
ui/                   Svelte + xterm.js frontend (Vite root).
scripts/              Dev helpers, run from WSL.
```

`shaman-core` deliberately has **zero Tauri dependencies**. All the hard logic
lives there so the UI layer stays replaceable.

## Development

This project targets **Windows** but is developed from **WSL**. WSL interop is
used to drive the Windows toolchain directly — there is no cross-compilation.

> **Always use the Windows binaries:** `cargo.exe`, `node.exe`, `git.exe`.
> The Linux `cargo`/`node` will build for the wrong target.
>
> `npm` on the WSL `PATH` resolves to a Windows shell script that fails under
> bash. Invoke it as `cmd.exe /c "npm ..."` — the scripts below already do.

```bash
./scripts/check.sh          # headless: fmt, clippy, tests, svelte-check
./scripts/verify.sh         # boot the app, assert UI_READY, screenshot, shut down
./scripts/dev.sh            # launch with HMR (opens a window, takes focus)
./scripts/release.sh        # standalone exe -- THIS is the one to double-click
./scripts/release.sh --bundle  # ...plus NSIS/MSI installers
./scripts/screenshot.sh     # capture the Shaman window to a PNG
```

### Which binary do I open?

| Binary | Needs a dev server? | Use it for |
|---|---|---|
| `target/release/shaman.exe` | No — UI is embedded | **Double-clicking. Normal use.** |
| `target/debug/shaman.exe` | **Yes**, Vite on :5173 | Development, via `dev.sh` |

Opening the *debug* exe on its own used to show a WebView2 "can't reach this
page" error, because it loads the UI from `localhost:5173`. It now detects the
missing dev server at startup and says so plainly instead.

`check.sh` is the default loop — it verifies everything without opening a
window. `verify.sh` is the end-to-end check: it is self-terminating, so unlike
`dev.sh` it won't sit in the foreground holding your attention.

`screenshot.sh` captures **only Shaman's own window**, via `PrintWindow`, so it
neither steals focus nor records anything else on screen.

### Two traps worth knowing up front

**Debug builds load `devUrl`, not the bundled `dist`.** Launching
`target\debug\shaman.exe` directly, with no Vite server running, produces a
blank window and no IPC — indistinguishable from a broken app. Use `verify.sh`
or `dev.sh`, both of which start Vite first.

**Verify the frontend from logs, not pixels.** WebView2 renders out of process,
so a page that failed to render and a screenshot that failed to capture look
identical (black). The app logs a `UI_READY` line once Svelte has mounted and
IPC has round-tripped; that is the signal to trust.

## Requirements

Already present on the current dev machine:

- Rust with the `x86_64-pc-windows-msvc` target
- Visual Studio Build Tools (MSVC linker)
- Node.js 20+
- WebView2 runtime (ships with Windows 11)

## Icons

`crates/shaman-app/icons/source.png` is a generated placeholder. Regenerate the
icon set after replacing it:

```bash
cmd.exe /c "npm run tauri -- icon crates/shaman-app/icons/source.png -o crates/shaman-app/icons"
```
