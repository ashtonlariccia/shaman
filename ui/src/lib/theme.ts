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

/**
 * `#rrggbb` plus an alpha pair, which is how xterm takes a translucent colour.
 *
 * Kept at full opacity when alpha is 1 rather than emitting `...ff`: identical
 * to render, but it keeps the common case a plain six-digit colour in the
 * devtools.
 */
export function withAlpha(hex: string, alpha: number): string {
  if (alpha >= 1) return hex;
  const pair = Math.round(Math.max(0, alpha) * 255)
    .toString(16)
    .padStart(2, "0");
  return `${hex}${pair}`;
}

export function terminalTheme(appearance: Appearance): ITheme {
  const alpha = appearance.backgroundOpacity / 100;
  return {
    ...ANSI,
    background: withAlpha(BACKGROUND, alpha),
    cursor: appearance.cursorColor,
    // The glyph *under* the block cursor. It has to stay opaque or the
    // character beneath a translucent window shows through the cursor itself.
    cursorAccent: BACKGROUND,
  };
}
