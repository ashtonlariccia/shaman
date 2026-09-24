/**
 * What Shaman answers when a program asks the terminal about itself.
 *
 * xterm.js answers most of these by itself. Two it gets wrong, and both land
 * as mangled colour in full-screen programs — nvim over SSH most visibly.
 *
 * **Truecolor.** xterm.js renders 24-bit colour, but nothing says so. A program
 * works it out from `$COLORTERM`, from a terminfo entry with `RGB`, or by
 * asking — and `xterm-256color`, which is what the SSH channel requests,
 * carries no `RGB`. `$COLORTERM` is set on local shells (see `pty.rs`) but
 * rarely survives the trip to a remote host, because sshd only forwards the
 * variables its `AcceptEnv` lists. So asking is the one route that always
 * works, and xterm.js registers no DCS handlers at all: the query goes
 * unanswered and the program concludes 256 colours.
 *
 * A modern colourscheme then falls back to its `cterm` approximations, which is
 * what turns a subtle indent guide into a near-black block: the 256-colour cube
 * has no `#1e1e2e`, so the scheme substitutes an explicit dark background where
 * truecolor would have left the cell alone.
 *
 * **The background colour.** `OSC 11 ?` asks what the default background is.
 * xterm.js answers with the theme's `background`, which Shaman sets to
 * `#00000000` so the window's own translucency shows through (see `theme.ts`) —
 * so the honest answer, pure black, is a lie about what is on screen. Programs
 * that blend against it produce colours for a black terminal on a #242424 one.
 */
import type { Terminal } from "@xterm/xterm";

import { BACKGROUND } from "./theme";

/** String terminator. The same form xterm.js uses for its own replies. */
const ST = "\x1b\\";

/**
 * Capabilities we claim by name alone.
 *
 * `Tc` and `RGB` are the two spellings of "24-bit colour"; terminals disagree
 * about which one to publish, so programs ask for both. `Su` is styled
 * underlines (`4:3` for undercurl), which xterm.js draws — it is what puts a
 * squiggle under a diagnostic instead of a flat line.
 */
const FLAGS = new Set(["Tc", "RGB", "Su"]);

/**
 * Capabilities that answer with a value: the sequences for 24-bit colour.
 *
 * `\\E` is terminfo's own spelling of Escape — a backslash and an E, two
 * characters, not the escape byte. Writing it as one character here would hand
 * back a capability that starts `E[38:2:` and prints an `E`.
 */
const STRINGS: Record<string, string> = {
  setrgbf: "\\E[38:2:%p1%d:%p2%d:%p3%dm",
  setrgbb: "\\E[48:2:%p1%d:%p2%d:%p3%dm",
};

/**
 * The underline the terminal currently has set, as an SGR parameter.
 *
 * Tracked only so DECRQSS can be answered honestly; see the handler below.
 * `0` is none, `1` single, `2` double, `3` curly, `4` dotted, `5` dashed —
 * the `4:x` subparameters.
 */
type UnderlineState = { style: number };

/**
 * Fold one SGR sequence into the tracked underline. Mirrors xterm's own rules.
 *
 * Note how subparameters arrive: `4:3` is **not** one slot holding `[4, 3]`.
 * It is the number `4`, followed by its subparameters as an array in the
 * *next* slot — `[4, [3]]`. Reading the style off the array's first element
 * instead of its second is the difference between reporting `4:3` and
 * reporting `4:1`, and nvim only enables coloured underlines on the former.
 */
function trackUnderline(params: (number | number[])[], state: UnderlineState): void {
  for (let i = 0; i < params.length; i++) {
    const p = params[i];
    if (Array.isArray(p)) continue; // consumed by its parent, below

    if (p === 4) {
      const sub = params[i + 1];
      let style = Array.isArray(sub) ? sub[0] : 1;
      // xterm's `_processUnderline`: an absent or unknown style is a plain
      // single underline. `4:0` is the one that genuinely means "off".
      if (style === undefined || style < 0 || style > 5) style = 1;
      state.style = style;
    } else if (p === 0 || p === 24) state.style = 0;
    else if (p === 21) state.style = 2;
  }
}

const encoder = new TextEncoder();

function toHex(text: string): string {
  return [...encoder.encode(text)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

/** `null` for anything that isn't an even run of hex digits. */
function fromHex(hex: string): string | null {
  if (hex.length === 0 || hex.length % 2 !== 0 || !/^[0-9a-fA-F]+$/.test(hex)) return null;
  let out = "";
  for (let i = 0; i < hex.length; i += 2) out += String.fromCharCode(parseInt(hex.slice(i, i + 2), 16));
  return out;
}

/** `#242424` as the `rgb:2424/2424/2424` an OSC colour report is written in. */
function rgbReport(hexColor: string): string {
  const channel = (at: number) => hexColor.slice(at, at + 2).repeat(2);
  return `rgb:${channel(1)}/${channel(3)}/${channel(5)}`;
}

/**
 * Answer the two questions xterm.js leaves on the table. `reply` puts bytes on
 * the shell's input, which is where a terminal's answers go.
 *
 * Returns a teardown for the handlers.
 */
export function installTerminalQueries(term: Terminal, reply: (data: string) => void): () => void {
  const underline: UnderlineState = { style: 0 };

  const handlers = [
    // Watch SGR go past so DECRQSS below can report it. `false` means "not
    // handled" -- xterm's own handler still runs and does the actual work.
    term.parser.registerCsiHandler({ final: "m" }, (params) => {
      trackUnderline(params, underline);
      return false;
    }),

    // DECRQSS: `DCS $ q <setting> ST`, "what is your current <setting>?".
    //
    // This is how nvim decides whether it may draw a coloured undercurl. From
    // its `tui_query_extended_underline`:
    //
    //     out(tui, S_LEN("\x1b[0m\x1b[4:3m\x1bP$qm\x1b\\"));
    //
    // It sets an undercurl and immediately asks what the SGR state is. An
    // answer containing `4:3` means the terminal understood, and only then
    // does nvim set `can_set_underline_color` and start emitting SGR 58. No
    // answer -- xterm.js registers no DCS handlers at all -- and every
    // diagnostic comes back as a flat line in the foreground colour.
    //
    // Only the underline is reported, because it is the only attribute we
    // track; a request for any other setting gets the `0$r` "I don't know
    // that one" form rather than a confident wrong answer.
    term.parser.registerDcsHandler({ intermediates: "$", final: "q" }, (data) => {
      if (data !== "m") {
        reply(`\x1bP0$r${ST}`);
        return true;
      }
      const sgr = underline.style === 0 ? "0" : `0;4:${underline.style}`;
      reply(`\x1bP1$r${sgr}m${ST}`);
      return true;
    }),

    // XTGETTCAP: `DCS + q <hex name> ; <hex name> ... ST`, one reply per name.
    // A capability we don't have is answered too, with the `0` form -- silence
    // is indistinguishable from a terminal that is wedged.
    term.parser.registerDcsHandler({ intermediates: "+", final: "q" }, (data) => {
      for (const encoded of data.split(";")) {
        const name = fromHex(encoded);
        if (name !== null && FLAGS.has(name)) reply(`\x1bP1+r${encoded}${ST}`);
        else if (name !== null && name in STRINGS)
          reply(`\x1bP1+r${encoded}=${toHex(STRINGS[name])}${ST}`);
        else reply(`\x1bP0+r${encoded}${ST}`);
      }
      return true;
    }),

    // OSC 11. Only the query is ours; a program *setting* the background is
    // left to xterm.js, which is the thing that has to paint it.
    term.parser.registerOscHandler(11, (data) => {
      if (!data.startsWith("?")) return false;
      reply(`\x1b]11;${rgbReport(BACKGROUND)}${ST}`);
      return true;
    }),
  ];

  return () => handlers.forEach((h) => h.dispose());
}
