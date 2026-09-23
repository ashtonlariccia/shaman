/**
 * Which clipboard shortcut, if any, a key press is.
 *
 * Ctrl+Shift rather than plain Ctrl+C/V: a terminal owes those to the program
 * running in it -- Ctrl+C is interrupt, Ctrl+V is readline's literal-next -- so
 * taking them for the clipboard would break the shell to fix the menu.
 */
export type ClipboardShortcut = "copy" | "paste";

/** The parts of a key press that decide this. Narrow so tests can fake it. */
export type Chord = Pick<
  KeyboardEvent,
  "ctrlKey" | "shiftKey" | "altKey" | "metaKey" | "key" | "code"
>;

const ASCII_LETTER = /^[a-z]$/;

/**
 * Whether this press is the given letter, judged by label first and position
 * second.
 *
 * `key` is what the layout produced and `code` is where the key sits on a US
 * keyboard, and neither alone is right. On Dvorak the C the user pressed is not
 * the key at QWERTY's C position, so position would fire on the wrong keycap.
 * On a Cyrillic layout `key` is "с" and no Latin letter is reachable at all, so
 * the label never matches and position is the only thing left. Trusting the
 * label whenever it is a plain Latin letter, and falling back to position when
 * it isn't, gets both right.
 */
function pressed(event: Chord, letter: string, code: string): boolean {
  const key = event.key.toLowerCase();
  return ASCII_LETTER.test(key) ? key === letter : event.code === code;
}

export function clipboardShortcut(event: Chord): ClipboardShortcut | null {
  // Alt and Meta are not "close enough": AltGr arrives as Ctrl+Alt on Windows,
  // so accepting Alt here would eat characters on European layouts.
  if (!event.ctrlKey || !event.shiftKey || event.altKey || event.metaKey) return null;
  if (pressed(event, "c", "KeyC")) return "copy";
  if (pressed(event, "v", "KeyV")) return "paste";
  return null;
}
