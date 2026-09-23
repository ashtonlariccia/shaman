import { describe, expect, it } from "vitest";

import { clipboardShortcut, type Chord } from "./keys";

function chord(over: Partial<Chord>): Chord {
  return {
    ctrlKey: false,
    shiftKey: false,
    altKey: false,
    metaKey: false,
    key: "",
    code: "",
    ...over,
  };
}

describe("clipboardShortcut", () => {
  it("matches Ctrl+Shift+C and Ctrl+Shift+V", () => {
    // Chromium reports the letter uppercase while Shift is held.
    const press = (key: string, code: string) =>
      clipboardShortcut(chord({ ctrlKey: true, shiftKey: true, key, code }));

    expect(press("C", "KeyC")).toBe("copy");
    expect(press("V", "KeyV")).toBe("paste");
  });

  it("leaves the shell's own Ctrl+C and Ctrl+V alone", () => {
    // Without Shift these belong to the program in the terminal: interrupt, and
    // readline's literal-next. Stealing them would be the worse bug.
    expect(clipboardShortcut(chord({ ctrlKey: true, key: "c", code: "KeyC" }))).toBeNull();
    expect(clipboardShortcut(chord({ ctrlKey: true, key: "v", code: "KeyV" }))).toBeNull();
  });

  it("ignores presses without Ctrl, and unrelated letters", () => {
    expect(clipboardShortcut(chord({ shiftKey: true, key: "C", code: "KeyC" }))).toBeNull();
    expect(
      clipboardShortcut(chord({ ctrlKey: true, shiftKey: true, key: "X", code: "KeyX" })),
    ).toBeNull();
  });

  it("ignores Alt and Meta, so AltGr can't trip it", () => {
    // AltGr arrives as Ctrl+Alt on Windows, and AltGr+Shift+C is a real
    // character on some European layouts.
    expect(
      clipboardShortcut(chord({ ctrlKey: true, shiftKey: true, altKey: true, key: "C", code: "KeyC" })),
    ).toBeNull();
    expect(
      clipboardShortcut(
        chord({ ctrlKey: true, shiftKey: true, metaKey: true, key: "C", code: "KeyC" }),
      ),
    ).toBeNull();
  });

  it("follows the keycap on Dvorak rather than the position", () => {
    // Dvorak's C sits where QWERTY has I, and its J sits where QWERTY has C.
    // The user pressed the key labelled C; the other one must stay inert.
    expect(
      clipboardShortcut(chord({ ctrlKey: true, shiftKey: true, key: "C", code: "KeyI" })),
    ).toBe("copy");
    expect(
      clipboardShortcut(chord({ ctrlKey: true, shiftKey: true, key: "J", code: "KeyC" })),
    ).toBeNull();
  });

  it("falls back to position when the layout produces no Latin letter", () => {
    // Cyrillic: nothing on the keyboard reports "c", so the label can never
    // match and position is all that is left to go on.
    expect(
      clipboardShortcut(chord({ ctrlKey: true, shiftKey: true, key: "С", code: "KeyC" })),
    ).toBe("copy");
    expect(
      clipboardShortcut(chord({ ctrlKey: true, shiftKey: true, key: "М", code: "KeyV" })),
    ).toBe("paste");
  });

  it("ignores a bare modifier press", () => {
    // Holding Ctrl+Shift fires keydown for Shift itself before any letter.
    expect(
      clipboardShortcut(chord({ ctrlKey: true, shiftKey: true, key: "Shift", code: "ShiftLeft" })),
    ).toBeNull();
  });
});
