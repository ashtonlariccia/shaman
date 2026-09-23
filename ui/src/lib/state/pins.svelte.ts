/**
 * The quick-open strip along the bottom of the window.
 *
 * Pins store ids, never names: a local pin is a shell profile id and a remote
 * one is a saved-connection id. Names are resolved at draw time, so renaming a
 * connection renames its pin without `pins.json` ever being touched.
 */
import { invoke } from "@tauri-apps/api/core";

import { resolvePin, samePin, type ResolvedPin } from "../pins";
import type { Pin, SavedConnection, ShellProfile } from "../types";

export class Pins {
  list = $state<Pin[]>([]);

  async refresh() {
    try {
      this.list = await invoke<Pin[]>("pinned_connections");
    } catch (e) {
      console.error("pinned_connections failed", e);
    }
  }

  /** What the strip draws: each pin paired with whatever its id still names. */
  resolved(profiles: ShellProfile[], saved: SavedConnection[]): ResolvedPin[] {
    return this.list.map((p) => resolvePin(p, profiles, saved));
  }

  has(pin: Pin | null): boolean {
    return pin !== null && this.list.some((p) => samePin(p, pin));
  }

  async add(pin: Pin) {
    try {
      this.list = await invoke<Pin[]>("pin_connection", { pin });
    } catch (e) {
      console.error("pin_connection failed", e);
    }
  }

  async remove(pin: Pin) {
    try {
      this.list = await invoke<Pin[]>("unpin_connection", {
        kind: pin.kind,
        target: pin.target,
      });
    } catch (e) {
      console.error("unpin_connection failed", e);
    }
  }

  /** Dragged (or Ctrl+Arrowed) to a new position on the strip. */
  async move(pin: Pin, index: number) {
    // Optimistic: the strip has already shown the drop, and waiting for the
    // round trip would snap it back for a frame first.
    const without = this.list.filter((p) => !samePin(p, pin));
    const moved = this.list.find((p) => samePin(p, pin));
    if (moved) this.list = [...without.slice(0, index), moved, ...without.slice(index)];

    try {
      this.list = await invoke<Pin[]>("move_pin", { kind: pin.kind, target: pin.target, index });
    } catch (e) {
      console.error("move_pin failed", e);
      // Whatever the backend actually holds is the truth; put it back.
      await this.refresh();
    }
  }
}
