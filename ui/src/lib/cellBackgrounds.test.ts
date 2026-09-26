import { describe, expect, it } from "vitest";

import { cellColor } from "./cellBackgrounds";

/** What the renderer's blend leaves in the premultiplied canvas: (c·a, a·a). */
function stored([r, g, b, a]: number[]) {
  return [r * a, g * a, b * a, a * a];
}

describe("a program-coloured cell background", () => {
  it("is untouched at full opacity", () => {
    expect(cellColor(0.2, 0.4, 0.6, 1)).toEqual([0.2, 0.4, 0.6, 1]);
  });

  // The renderer blends alpha with SRC_ALPHA too, so handing it the target
  // alpha straight would land at alpha squared.
  it("lands on the canvas at exactly the opacity asked for", () => {
    const [r, g, b, a] = stored(cellColor(0.2, 0.4, 0.6, 0.5));
    expect(a).toBeCloseTo(0.5);
    expect([r, g, b]).toEqual([0.2 * 0.5, 0.4 * 0.5, 0.6 * 0.5].map((v) => expect.closeTo(v)));
  });

  it("vanishes at zero opacity", () => {
    expect(cellColor(0.2, 0.4, 0.6, 0)[3]).toBe(0);
  });

  // The phantom rectangle xterm draws behind styled text on the default
  // background, in the surface colour. It was never meant to exist.
  it("is dropped when it is the surface colour", () => {
    expect(cellColor(0x24 / 255, 0x24 / 255, 0x24 / 255, 1)[3]).toBe(0);
  });
});
