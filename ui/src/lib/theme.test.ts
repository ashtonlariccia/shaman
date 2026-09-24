import { describe, expect, it } from "vitest";

import { DEFAULTS } from "./state/appearance.svelte";
import { BACKGROUND, terminalTheme } from "./theme";

describe("the terminal background", () => {
  const background = terminalTheme(DEFAULTS).background!;

  it("is transparent, so the chrome behind the terminal is what you see", () => {
    expect(background.slice(-2)).toBe("00");
  });

  // The regression this file exists for. `#00000000` is the obvious way to
  // write "transparent" and it is wrong: xterm's WebGL renderer paints a
  // background rectangle for any cell whose *attribute word* is non-zero --
  // which includes italic, dim, overline and every underline style -- and
  // fills it with this colour at a hardcoded alpha of 1. The RGB half is
  // therefore visible even though the alpha says it cannot be, and getting it
  // wrong puts a black box behind every italic word and every underlined
  // diagnostic in vim. See theme.ts.
  it("carries the surface colour anyway, because the alpha is not always honoured", () => {
    expect(background.slice(0, 7)).toBe(BACKGROUND);
  });
});
