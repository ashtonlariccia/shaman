/**
 * The operations the menu bar performs on a terminal.
 *
 * Deliberately narrow: the menus should not reach into xterm internals, so each
 * TerminalView hands out exactly these few when it mounts.
 */
export type TerminalApi = {
  /** The current selection, or null if nothing is selected. */
  copySelection: () => string | null;
  /**
   * Drop the highlight.
   *
   * Used after a right-click copy, where it is the only sign anything happened:
   * the gesture opens no menu and prints nothing, so a selection that stayed lit
   * would leave "did that work?" as the outcome.
   */
  clearSelection: () => void;
  /**
   * Send clipboard text to the shell.
   *
   * Pass the text exactly as it came off the clipboard: the implementation
   * normalises line endings and applies bracketed paste. Pre-mangling it here
   * would defeat that.
   */
  paste: (text: string) => void;
  focus: () => void;
  /**
   * Give up this terminal without killing what is running in it.
   *
   * Returns xterm's serialisation of the screen and scrollback, for the window
   * taking it over to write into a fresh terminal. The view stops owning the
   * session at this point: unmounting it will no longer close it, which is the
   * whole difference between moving a tab and closing one.
   */
  detach: () => string;
};
