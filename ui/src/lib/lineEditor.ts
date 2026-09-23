/**
 * Which line editor is reading at the far end of a session.
 *
 * This exists for exactly one gesture — `Esc Esc` to clear the command being
 * typed — because there is no portable escape sequence for "discard the current
 * line". The two families genuinely disagree about what `Escape` even means:
 *
 * * **`escape`** — `cmd.exe`'s cooked-mode editor and PSReadLine both bind
 *   `Escape` itself to clearing the line, so nothing has to be sent: pressing it
 *   twice already works, and intercepting would only add latency to a keystroke
 *   that is already correct.
 * * **`readline`** — bash, zsh and fish treat `Escape` as the *meta prefix*, so
 *   it clears nothing on its own and `Esc Esc` is, in bash, filename completion.
 *   These need `Ctrl+E Ctrl+U` (end of line, then discard everything before it)
 *   sent in its place.
 *
 * Unrecognised local shells are treated as `escape`, which is the safe default:
 * a stray `Escape` sent to a readline shell is absorbed as a dead prefix,
 * whereas `Ctrl+E Ctrl+U` sent to something that does not understand it can
 * leave control characters on the line.
 */
export type LineEditor = "escape" | "readline";

/** What `Esc Esc` must send to clear the line, for the `readline` family. */
export const CLEAR_LINE = "\x05\x15"; // Ctrl+E, then Ctrl+U

export function lineEditorFor(session: { profileId?: string; ssh?: unknown }): LineEditor {
  // A remote shell is whatever the host runs, and that is overwhelmingly a
  // readline-family shell rather than a Windows one.
  if (session.ssh) return "readline";

  const id = session.profileId ?? "";
  // `wsl:<distro>` — the distro's login shell is doing the line editing.
  if (id.startsWith("wsl:")) return "readline";
  if (id.startsWith("git-bash") || id.startsWith("bash") || id.startsWith("msys")) {
    return "readline";
  }

  return "escape";
}
