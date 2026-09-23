/**
 * Copy and paste against one named terminal.
 *
 * Both take the terminal rather than resolving "the active tab": right-click
 * names the pane it happened in, and the two are the same today only because an
 * inactive pane takes no pointer events. A paste landing in a tab other than
 * the one clicked would be a bad way to find that out.
 *
 * Both hand focus back when they are done. From the Edit menu that matters —
 * clicking a menu item takes focus to do it. From the keyboard the terminal
 * already has focus and refocusing is a no-op.
 */
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";

import type { TerminalApi } from "./terminalApi";

/**
 * `clearAfter` is for the right-click gesture, which otherwise gives no sign it
 * did anything: no menu opens and nothing is printed, so the highlight going
 * out is the acknowledgement. The Edit menu leaves the selection up — clicking
 * a menu item is its own confirmation.
 */
export async function copyFrom(terminal: TerminalApi | undefined, clearAfter = false) {
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

export async function pasteInto(terminal: TerminalApi | undefined) {
  if (!terminal) return;

  try {
    const text = await readText();
    if (text) terminal.paste(text);
  } catch (e) {
    console.error("paste failed", e);
  }
  terminal.focus();
}
