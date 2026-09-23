/**
 * How every terminal looks: font, cursor, and how solid the window is.
 *
 * Global on purpose — one look for every tab is what makes a window of mixed
 * shells read as one application.
 *
 * The backend is the authority on the stored value. It clamps font size and
 * opacity, so `save` adopts what comes back rather than what was sent; a
 * dialog that kept showing 900px after the store clamped it to 72 would be
 * lying about what is saved.
 */
import { invoke } from "@tauri-apps/api/core";

export type CursorShape = "block" | "bar" | "underline";
export type Material = "none" | "acrylic";

export type Appearance = {
  fontFamily: string;
  fontSize: number;
  cursorShape: CursorShape;
  cursorBlink: boolean;
  cursorColor: string;
  /** Percent. 100 is opaque; the window only goes translucent below it. */
  backgroundOpacity: number;
  material: Material;
};

/** Mirrors the Rust defaults, for the frame before the backend has answered. */
export const DEFAULTS: Appearance = {
  fontFamily: "Cascadia Mono",
  fontSize: 13,
  cursorShape: "block",
  cursorBlink: true,
  cursorColor: "#f5e0dc",
  backgroundOpacity: 100,
  material: "none",
};

export class AppearanceStore {
  current = $state<Appearance>({ ...DEFAULTS });

  /** The font stack xterm and the CSS both use. */
  get fontStack(): string {
    // The chosen face first, then the ones we can count on. A font that has
    // been uninstalled since it was picked then degrades to a monospace
    // fallback instead of to the proportional default, which would reflow
    // every column in the terminal.
    return `"${this.current.fontFamily}", "Cascadia Mono", Consolas, "Courier New", monospace`;
  }

  /** 0–1, as CSS and xterm want it. */
  get alpha(): number {
    return this.current.backgroundOpacity / 100;
  }

  async load() {
    try {
      this.current = await invoke<Appearance>("appearance");
    } catch (e) {
      console.error("appearance failed", e);
    }
  }

  /** Change some of it. Returns once the backend has stored and echoed it back. */
  async patch(change: Partial<Appearance>) {
    const next = { ...this.current, ...change };
    // Show it immediately: waiting for the round trip makes a dragged slider
    // feel like it is fighting back.
    this.current = next;

    try {
      this.current = await invoke<Appearance>("set_appearance", { appearance: next });
    } catch (e) {
      console.error("set_appearance failed", e);
      // Whatever is actually stored is the truth; put it back.
      await this.load();
    }
  }

  async reset() {
    await this.patch({ ...DEFAULTS });
  }
}
