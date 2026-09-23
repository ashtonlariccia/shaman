/**
 * The palette xterm is given.
 *
 * Catppuccin Charcoal, matching `app.css`. The two must agree: a blue tab and
 * blue terminal text are meant to be the same blue, and `kinds.ts` leans on
 * that to make the colour code mean something.
 *
 * Only the cursor and the background vary at runtime, from the appearance
 * settings — everything else is fixed, because the ANSI palette is what
 * programs in the terminal are addressing.
 */
import type { ITheme } from "@xterm/xterm";

import type { Appearance } from "./state/appearance.svelte";

/** The editor surface, as `--bg` in app.css. */
export const BACKGROUND = "#242424";

const ANSI: ITheme = {
  foreground: "#d8d8d8",
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
};

export function terminalTheme(appearance: Appearance): ITheme {
  return {
    ...ANSI,
    // Fully transparent, always -- the `.stage` behind it is what carries the
    // opacity. If both did, the terminal would end up more opaque than the
    // sidebar beside it at the same setting.
    //
    // Only the *default* background goes transparent. Cells a program has
    // coloured itself keep their own background, which is what you want: a
    // `ls` listing stays readable through a translucent window.
    background: "#00000000",
    cursor: appearance.cursorColor,
    // The glyph under a block cursor. Opaque on purpose: the cursor has to
    // stay legible against whatever is showing through the window.
    cursorAccent: BACKGROUND,
  };
}
