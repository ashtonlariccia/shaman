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

/**
 * The same colour, invisible.
 *
 * The alpha is what makes the terminal see-through: the WebGL renderer clears
 * its canvas to this, and a cell using the default background is never painted
 * at all, so the chrome behind shows through. That much only needs the `00`.
 *
 * The **RGB still has to be right**, because of a bug in the WebGL addon. It
 * decides whether a cell needs a background rectangle with `bg !== 0` against
 * the whole packed attribute word -- and that word carries the style flags,
 * not just the colour. Italic, dim, overline, and any underline style or
 * colour each set a bit in it. So a styled cell on a *default* background is
 * judged to need a rectangle, which `_updateRectangle` then fills with this
 * colour and `alpha = 1`, hardcoded.
 *
 * With `#00000000` that rectangle is opaque black, and every italic word and
 * every squiggle-underlined diagnostic in a full-screen program wears a black
 * box. With the real surface colour it is painted the colour it is sitting on,
 * and nobody can tell it was drawn. Any terminal whose background is opaque
 * has been getting away with this for free.
 *
 * cellBackgrounds.ts goes further and drops a rectangle in this colour
 * outright, so below 100% opacity a styled run is not even a faintly more
 * solid patch.
 */
const TRANSPARENT_BACKGROUND = `${BACKGROUND}00`;

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
    // Transparent, always -- the `.stage` behind it is what carries the
    // opacity. If both did, the terminal would end up more opaque than the
    // sidebar beside it at the same setting.
    //
    // Only the *default* background goes transparent here. Cells a program
    // has coloured itself keep their colour, at the window's opacity -- see
    // cellBackgrounds.ts.
    //
    // Transparent, but not colourless -- see TRANSPARENT_BACKGROUND.
    background: TRANSPARENT_BACKGROUND,
    cursor: appearance.cursorColor,
    // The glyph under a block cursor. Opaque on purpose: the cursor has to
    // stay legible against whatever is showing through the window.
    cursorAccent: BACKGROUND,
  };
}
