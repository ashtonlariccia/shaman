/**
 * Cell backgrounds at the window's opacity, instead of always solid.
 *
 * A program that colours a cell's background itself -- Claude Code's prompt
 * and diff lines, a gutter, a status bar -- gets a rectangle from xterm's WebGL
 * renderer, and `RectangleRenderer._updateRectangle` gives every one of them
 * `alpha = 1`, hardcoded. On a translucent window that is a solid dark band
 * across the pane with the desktop showing all around it. Nothing in the theme
 * or the options reaches that alpha, so this reaches into the renderer and
 * rewrites the colour on its way to the GPU.
 *
 * `_addRectangle` is the one place it goes through: every call to it comes from
 * `_updateRectangle` (the viewport clear and the cursor use
 * `_addRectangleFloat`), so wrapping it touches cell backgrounds and nothing
 * else. The method lives on the prototype, which every terminal's renderer
 * shares, so it is patched once and the opacity is one module-level number.
 */
import type { WebglAddon } from "@xterm/addon-webgl";

import { BACKGROUND } from "./theme";

type AddRectangle = (
  array: Float32Array,
  offset: number,
  x1: number,
  y1: number,
  width: number,
  height: number,
  r: number,
  g: number,
  b: number,
  a: number,
) => void;

let opacity = 1;

/** The surface colour as the 0-1 floats the renderer compares with. */
const SURFACE = [1, 3, 5].map((i) => parseInt(BACKGROUND.slice(i, i + 2), 16) / 255);

/** 0-1, the appearance setting. Takes effect on the next full redraw. */
export function setCellBackgroundOpacity(value: number) {
  opacity = Math.min(1, Math.max(0, value));
}

/**
 * What to hand the renderer so that `rgb` lands on screen at `alpha`.
 *
 * Not simply `[...rgb, alpha]`. The canvas is premultiplied and the renderer
 * blends with `SRC_ALPHA, ONE_MINUS_SRC_ALPHA` on *all four* channels, so over
 * the cleared canvas a rectangle of (c, a) is stored as colour `c·a` with
 * alpha `a·a`. The compositor then reads that as a colour at opacity a², which
 * is fainter than asked and lets the backdrop bleed through brighter than the
 * colour should allow. Passing `a = √alpha` and `c·√alpha` stores exactly
 * `c·alpha` at `alpha`.
 *
 * A rectangle in the surface colour is dropped altogether. That is what the
 * renderer paints behind italic, dim and underlined text on the *default*
 * background (see theme.ts) -- a rectangle that should never have existed, and
 * which at anything under 100% was a faintly more solid patch behind every
 * styled word.
 */
export function cellColor(r: number, g: number, b: number, alpha: number): [number, number, number, number] {
  if (isSurface(r, g, b) || alpha <= 0) return [0, 0, 0, 0];
  const k = Math.sqrt(alpha);
  return [r * k, g * k, b * k, k];
}

function isSurface(r: number, g: number, b: number) {
  const eps = 0.5 / 255;
  return Math.abs(r - SURFACE[0]) < eps && Math.abs(g - SURFACE[1]) < eps && Math.abs(b - SURFACE[2]) < eps;
}

const PATCHED = Symbol("shaman.cellBackgrounds");

/**
 * Patch the renderer behind `addon`. Call after `loadAddon` on an opened
 * terminal; harmless to call again. If the addon's insides have moved in an
 * update this does nothing and says so, rather than breaking the terminal.
 */
export function translucentCellBackgrounds(addon: WebglAddon) {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const rectangles = (addon as any)._renderer?._rectangleRenderer?.value;
  const proto = rectangles && Object.getPrototypeOf(rectangles);
  const original: AddRectangle | undefined = proto?._addRectangle;
  if (typeof original !== "function") {
    console.warn("webgl renderer layout changed; cell backgrounds stay opaque");
    return;
  }
  if (proto[PATCHED]) return;
  proto[PATCHED] = true;
  proto._addRectangle = function (this: unknown, array, offset, x1, y1, width, height, r, g, b, a) {
    const [cr, cg, cb, ca] = cellColor(r, g, b, opacity * a);
    original.call(this, array, offset, x1, y1, width, height, cr, cg, cb, ca);
  } satisfies AddRectangle;
}
