/**
 * Turning stored pins into buttons.
 *
 * A pin holds an id, not a name, so the strip has to look the name up every
 * time it draws. That is the whole point: renaming a saved connection in
 * File → Manage Saved Connections renames its pin, and a local pin always reads
 * exactly as it does in Terminal → New Local Terminal.
 */
import { connectionLines } from "./connectionLabel";
import { profileKind, type Kind } from "./kinds";
import type { Pin, SavedConnection, ShellProfile } from "./types";

/** Enough of an SSH tab to find its entry in the store. */
export type SshTarget = {
  host: string;
  port: number;
  username: string;
  savedId?: string;
};

/**
 * The saved entry a remote tab corresponds to, if there is one.
 *
 * A tab opened *from* a saved connection knows its id outright. One typed into
 * the connect dialog doesn't, so it is matched on what actually identifies a
 * target — host (case-insensitively), port and user, the same triple the
 * backend's store keys on.
 */
export function findSaved(
  saved: SavedConnection[],
  target: SshTarget,
): SavedConnection | undefined {
  if (target.savedId) return saved.find((c) => c.id === target.savedId);
  return saved.find(
    (c) =>
      c.host.toLowerCase() === target.host.toLowerCase() &&
      c.port === target.port &&
      c.username === target.username,
  );
}

/** A pin with its live name attached, ready to render. */
export type ResolvedPin = {
  pin: Pin;
  /** The live name, or the snapshot when the target is gone. */
  label: string;
  /** Second line for the tooltip: the address, or why it can't be opened. */
  detail: string;
  /** False when the shell was uninstalled or the connection deleted. */
  available: boolean;
  /**
   * Which colour it wears. Note this is finer than `Pin.kind`: a pinned admin
   * shell is a `local` pin but an `admin` kind, and stays red on the strip
   * exactly as it is red in the menu.
   */
  kind: Kind;
};

export function resolvePin(
  pin: Pin,
  profiles: ShellProfile[],
  saved: SavedConnection[],
): ResolvedPin {
  if (pin.kind === "local") {
    const profile = profiles.find((p) => p.id === pin.target);
    return {
      pin,
      label: profile?.label ?? pin.label,
      detail: profile
        ? `New ${profile.label} terminal`
        : "This shell is no longer installed",
      available: profile !== undefined,
      // An uninstalled shell can no longer say whether it was elevated; it
      // renders as unavailable either way, so plain local is the safe guess.
      kind: profile ? profileKind(profile) : "local",
    };
  }

  const connection = saved.find((c) => c.id === pin.target);
  if (!connection) {
    return {
      pin,
      label: pin.label,
      detail: "This saved connection no longer exists",
      available: false,
      kind: "remote",
    };
  }

  const lines = connectionLines(connection);
  return {
    pin,
    label: lines.primary,
    detail: lines.secondary,
    available: true,
    kind: "remote",
  };
}

/** Same button? Kind and target only — a renamed pin is still the same pin. */
export function samePin(a: Pin, b: Pin): boolean {
  return a.kind === b.kind && a.target === b.target;
}

/**
 * Which gap a drag is hovering, given the horizontal midpoint of every chip.
 *
 * "Gap n" means *before* the chip currently at n, and a gap equal to the number
 * of chips means past the right-hand end. Crossing a chip's midpoint — rather
 * than touching its edge — is what commits to swapping with it, so a chip does
 * not flicker between two slots while the pointer sits over its boundary.
 */
export function dropGap(midpoints: number[], x: number): number {
  let gap = 0;
  while (gap < midpoints.length && x > midpoints[gap]) gap++;
  return gap;
}

/**
 * The index a chip dragged from `from` should end up at, in the list as it will
 * be once the chip has been lifted out of it.
 *
 * The subtraction is the whole subtlety: a gap to the *right* of where the chip
 * started counts one position too many, because that count still includes the
 * chip itself.
 */
export function dropIndex(midpoints: number[], x: number, from: number): number {
  const gap = dropGap(midpoints, x);
  return gap > from ? gap - 1 : gap;
}
