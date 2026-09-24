import { describe, expect, it } from "vitest";

import type { Terminal } from "@xterm/xterm";

import { installTerminalQueries } from "./termQueries";

/** The hex a capability name travels as. ASCII throughout, so a byte a char. */
const hex = (text: string) =>
  [...text].map((c) => c.charCodeAt(0).toString(16).padStart(2, "0")).join("");

const unhex = (text: string) =>
  (text.match(/../g) ?? []).map((pair) => String.fromCharCode(parseInt(pair, 16))).join("");

/**
 * Just enough of xterm's parser to hold the two handlers and hand them input.
 * The real Terminal needs a DOM and a canvas; what is under test is the replies.
 */
function fakeTerminal() {
  type Dcs = (data: string, params: (number | number[])[]) => boolean;
  const dcs: Record<string, Dcs> = {};
  let osc11: ((data: string) => boolean) | undefined;
  let sgr: ((params: (number | number[])[]) => boolean) | undefined;
  const replies: string[] = [];

  const term = {
    parser: {
      registerDcsHandler(id: { intermediates: string; final: string }, cb: Dcs) {
        dcs[id.intermediates + id.final] = cb;
        return { dispose() {} };
      },
      registerOscHandler(ident: number, cb: (data: string) => boolean) {
        if (ident === 11) osc11 = cb;
        return { dispose() {} };
      },
      registerCsiHandler(id: { final: string }, cb: (p: (number | number[])[]) => boolean) {
        if (id.final === "m") sgr = cb;
        return { dispose() {} };
      },
    },
  } as unknown as Terminal;

  installTerminalQueries(term, (data) => replies.push(data));
  return {
    replies,
    getCap: (...names: string[]) => dcs["+q"](names.map(hex).join(";"), []),
    decrqss: (setting: string) => dcs["$q"](setting, []),
    sgr: (...params: (number | number[])[]) => sgr!(params),
    osc11: (data: string) => osc11!(data),
  };
}

describe("XTGETTCAP", () => {
  it("claims 24-bit colour under both names programs ask by", () => {
    const t = fakeTerminal();
    t.getCap("Tc", "RGB");
    // `1+r` is the "yes"; the name comes back in the hex it was asked in.
    expect(t.replies).toEqual(["\x1bP1+r5463\x1b\\", "\x1bP1+r524742\x1b\\"]);
  });

  it("answers the colour-setting capabilities with their sequences", () => {
    const t = fakeTerminal();
    t.getCap("setrgbb");
    const [name, value] = t.replies[0].slice(5, -2).split("=");
    expect(unhex(name)).toBe("setrgbb");
    expect(unhex(value)).toBe("\\E[48:2:%p1%d:%p2%d:%p3%dm");
  });

  it("says no rather than nothing to a capability we don't have", () => {
    const t = fakeTerminal();
    t.getCap("kbs");
    expect(t.replies).toEqual(["\x1bP0+r6b6273\x1b\\"]);
  });

  it("does not mistake junk for a capability name", () => {
    const t = fakeTerminal();
    t.getCap(); // an empty query
    expect(t.replies).toEqual(["\x1bP0+r\x1b\\"]);
  });
});

describe("OSC 11", () => {
  it("reports the background that is actually on screen, not the transparent one", () => {
    const t = fakeTerminal();
    expect(t.osc11("?")).toBe(true);
    expect(t.replies).toEqual(["\x1b]11;rgb:2424/2424/2424\x1b\\"]);
  });

  it("leaves a program *setting* the background to xterm", () => {
    const t = fakeTerminal();
    expect(t.osc11("rgb:1e1e/1e1e/2e2e")).toBe(false);
    expect(t.replies).toEqual([]);
  });
});

describe("DECRQSS", () => {
  // The exact probe from neovim's tui_query_extended_underline:
  //   out(tui, S_LEN("\x1b[0m\x1b[4:3m\x1bP$qm\x1b\\"));
  // Seeing `4:3` come back is what lets nvim emit SGR 58, which is what makes
  // a diagnostic squiggle red instead of a flat line in the text colour.
  it("reports the undercurl nvim just set, which is what unlocks SGR 58", () => {
    const t = fakeTerminal();
    t.sgr(0);
    t.sgr(4, [3]);
    t.decrqss("m");
    expect(t.replies).toEqual(["\x1bP1$r0;4:3m\x1b\\"]);
  });

  it("passes SGR through rather than swallowing it", () => {
    // `false` = not handled, so xterm's own handler still applies the attribute.
    expect(fakeTerminal().sgr(4, [3])).toBe(false);
  });

  it("tracks the styles back off again", () => {
    const t = fakeTerminal();
    t.sgr(4, [3]);
    t.sgr(24);
    t.decrqss("m");
    expect(t.replies).toEqual(["\x1bP1$r0m\x1b\\"]);
  });

  it("declines settings it does not track instead of guessing", () => {
    const t = fakeTerminal();
    t.decrqss(" q"); // DECSCUSR, the cursor shape
    expect(t.replies).toEqual(["\x1bP0$r\x1b\\"]);
  });
});

describe("terminfo string capabilities", () => {
  it("spells Escape the way terminfo does, as a backslash and an E", () => {
    const t = fakeTerminal();
    t.getCap("setrgbf");
    const value = unhex(t.replies[0].slice(5, -2).split("=")[1]);
    expect(value.slice(0, 2)).toBe("\\E");
    expect(value).toBe("\\E[38:2:%p1%d:%p2%d:%p3%dm");
  });
});
