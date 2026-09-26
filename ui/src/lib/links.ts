/**
 * Clickable links in a terminal.
 *
 * Two kinds reach the same handler:
 *
 * - **Plain URLs in the output**, found by `@xterm/addon-web-links`. Nothing
 *   the program has to do; `https://…` printed by anything becomes a link.
 * - **OSC 8 hyperlinks**, where a program marks a stretch of text as a link to
 *   a target it names (`ls --hyperlink`, `gh`, `cargo`'s error codes, Claude
 *   Code). xterm.js parses those itself and calls `linkHandler`.
 *
 * **Ctrl+click opens, a plain click does not.** A plain click is for placing a
 * selection, and for the program itself when it reads the mouse; opening a
 * browser every time someone clicks near a URL to select it would be a hazard.
 * It is also what Windows Terminal and VS Code do.
 *
 * **The target is always shown before it is followed.** An OSC 8 link's text is
 * whatever the program chose to print, and it need not resemble where the link
 * goes -- so hovering one raises a card with the real URL. Only http(s) is
 * offered at all; the backend enforces the same rule (`links.rs`), since what a
 * terminal displays comes from programs it has no reason to trust.
 */
import type { Terminal } from "@xterm/xterm";
import { WebLinksAddon } from "@xterm/addon-web-links";

/** The same rule as `shaman_core::links::check`, for deciding what to offer. */
export function isWebUrl(url: string): boolean {
  if (url.length > 2048) return false;
  if (/[\s"\u0000-\u001f\u007f]/.test(url)) return false;
  const m = /^https?:\/\/([^/?#]*)/i.exec(url);
  return m !== null && m[1].length > 0;
}

export type LinkHover = {
  url: string;
  /** Viewport coordinates of the pointer, for placing the card. */
  x: number;
  y: number;
};

export type LinkCallbacks = {
  /** Pointer rests on a link; `null` when it leaves. */
  onhover: (hover: LinkHover | null) => void;
  open: (url: string) => void;
};

/** Wire both kinds of link into `t`. Returns an uninstaller. */
export function installLinks(t: Terminal, { onhover, open }: LinkCallbacks): () => void {
  const activate = (event: MouseEvent, url: string) => {
    if (!event.ctrlKey || !isWebUrl(url)) return;
    event.preventDefault();
    onhover(null);
    open(url);
  };
  const hover = (event: MouseEvent, url: string) => {
    if (isWebUrl(url)) onhover({ url, x: event.clientX, y: event.clientY });
  };
  const leave = () => onhover(null);

  const addon = new WebLinksAddon(activate, { hover, leave });
  t.loadAddon(addon);

  t.options.linkHandler = {
    activate,
    hover,
    leave,
    // xterm's default refuses anything but http(s) already; said explicitly so
    // a future default does not quietly widen it.
    allowNonHttpProtocols: false,
  };

  return () => {
    addon.dispose();
    t.options.linkHandler = null;
    onhover(null);
  };
}
