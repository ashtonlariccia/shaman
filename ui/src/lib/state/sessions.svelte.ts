/**
 * The open tabs.
 *
 * Owns the slot list, which one is active, and the handle each live
 * TerminalView hands back. Everything that adds or removes a tab goes through
 * here, so "which tab is selected after this one closes" is answered in one
 * place rather than at every call site.
 */
import { invoke } from "@tauri-apps/api/core";

import type { SshRequest } from "../ConnectDialog.svelte";
import {
  adoptedSlot,
  handoffFor,
  newSlot,
  newSshSlot,
  type Handoff,
  type Slot,
} from "../slots";
import type { TerminalApi } from "../terminalApi";
import type { ShellProfile } from "../types";

export class Sessions {
  slots = $state<Slot[]>([]);
  activeKey = $state<number | null>(null);

  /**
   * One entry per mounted TerminalView, so menu actions can reach the active
   * terminal. Deliberately not `$state`: nothing renders from it, and making a
   * Map reactive would only cost churn on every mount.
   */
  readonly terminals = new Map<number, TerminalApi>();

  /** The tab every menu action operates on. */
  get active(): Slot | null {
    return this.slots.find((s) => s.key === this.activeKey) ?? null;
  }

  get activeTerminal(): TerminalApi | undefined {
    return this.activeKey === null ? undefined : this.terminals.get(this.activeKey);
  }

  open(profile: ShellProfile): Slot {
    return this.#add(newSlot(profile.id, profile.label, profile.elevated));
  }

  /** `title` overrides `user@host` — saved connections use the name you gave them. */
  openSsh(request: SshRequest, title?: string): Slot {
    return this.#add(newSshSlot(request, title));
  }

  /** A terminal dragged in from another window. Its shell never stopped. */
  adopt(handoff: Handoff): Slot {
    return this.#add(adoptedSlot(handoff));
  }

  #add(slot: Slot): Slot {
    this.slots = [...this.slots, slot];
    this.activeKey = slot.key;
    return slot;
  }

  select(key: number) {
    this.activeKey = key;
  }

  /** The backend has handed back a session id for a tab that was already open. */
  markOpened(key: number, sessionId: number) {
    this.slots = this.slots.map((s) => (s.key === key ? { ...s, sessionId } : s));
  }

  /**
   * Point a remote tab at the saved connection it has just become, so it
   * reconnects from the vault and stops being offered for saving.
   */
  attachSavedId(key: number, savedId: string) {
    this.slots = this.slots.map((s) =>
      s.key === key && s.ssh ? { ...s, ssh: { ...s.ssh, savedId } } : s,
    );
  }

  /**
   * Drop a tab from the list and move selection somewhere sensible.
   *
   * Does not touch the backend: use it when the session is already gone (the
   * shell exited, or a connection failed). [`close`] is the one that kills.
   */
  drop(key: number) {
    const index = this.slots.findIndex((s) => s.key === key);
    if (index === -1) return;

    this.slots = this.slots.filter((s) => s.key !== key);
    this.terminals.delete(key);

    if (this.activeKey === key) {
      // Prefer the neighbour on the left, which is what tabbed UIs tend to do.
      const next = this.slots[index - 1] ?? this.slots[index] ?? null;
      this.activeKey = next?.key ?? null;
    }
  }

  /** `exit` in the shell, or the process dying. The row just goes. */
  dropBySessionId(sessionId: number) {
    const slot = this.slots.find((s) => s.sessionId === sessionId);
    if (slot) this.drop(slot.key);
  }

  async close(key: number) {
    const slot = this.slots.find((s) => s.key === key);
    if (!slot) return;

    // Unmounting TerminalView also closes the session, but doing it here means
    // the process tree is gone before the row disappears.
    if (slot.sessionId !== null) {
      try {
        await invoke("session_close", { id: slot.sessionId });
      } catch (e) {
        console.error("session_close failed", e);
      }
    }

    this.drop(key);
  }

  closeActive() {
    if (this.activeKey !== null) void this.close(this.activeKey);
  }

  /**
   * Kill every terminal in this window.
   *
   * What closing a window means, now that closing one no longer ends the
   * application: the shells belonged to this window, and leaving them running
   * with nothing attached would be a leak nobody can see or reach.
   */
  async closeAll() {
    const ids = this.slots.map((s) => s.sessionId).filter((id) => id !== null);
    this.slots = [];
    this.activeKey = null;
    this.terminals.clear();

    await Promise.all(
      ids.map((id) =>
        invoke("session_close", { id }).catch((e) =>
          console.error("session_close failed", e),
        ),
      ),
    );
  }

  /**
   * Move a live terminal to another window, or to one opened for it.
   *
   * The session is never stopped and never restarted: it is unhooked from this
   * window's output channel, the row goes, and the receiving window hooks it
   * back up. The screen travels as a snapshot because only this window still
   * has the scrollback — the PTY has no memory of what it printed.
   *
   * `target` is a window label, or null to open a new window at (`x`, `y`).
   */
  async handoff(key: number, target: string | null, x: number, y: number) {
    const slot = this.slots.find((s) => s.key === key);
    // A tab still connecting has nothing to move: there is no session behind it
    // yet, and the attempt would kill the dialog's retry.
    if (!slot || slot.sessionId === null) return;

    const id = slot.sessionId;

    // `detach` is what stops the unmount below from killing the shell, so a tab
    // whose view is somehow missing must stay where it is: dropping the row
    // without it would close the very session we are trying to move.
    const view = this.terminals.get(key);
    if (!view) return;
    const snapshot = view.detach();

    try {
      await invoke("session_detach", { id });
    } catch (e) {
      console.error("session_detach failed", e);
      return;
    }

    const handoff = handoffFor(slot, id, snapshot);

    // Drop the row before the other window builds its own, so the terminal is
    // never in two places at once. `drop` rather than `close`: the shell lives.
    this.drop(key);

    try {
      await invoke("handoff_session", { target, payload: handoff, x, y });
    } catch (e) {
      // Nothing took it. Take it back rather than orphaning a running shell
      // that no window can reach.
      console.error("handoff_session failed", e);
      this.adopt(handoff);
    }
  }

  /**
   * Restart the active shell in place, for when it hangs.
   *
   * Swapping the slot's key is what does the work: the `{#each}` is keyed, so a
   * new key unmounts the old TerminalView (closing its session and killing the
   * process tree) and mounts a fresh one with the same profile.
   */
  refreshActive() {
    const current = this.active;
    if (!current) return;

    const replacement = newSlot(current.profileId, current.title, current.elevated);
    this.slots = this.slots.map((s) => (s.key === current.key ? replacement : s));
    this.terminals.delete(current.key);
    this.activeKey = replacement.key;
  }
}
