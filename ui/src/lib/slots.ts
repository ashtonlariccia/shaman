/**
 * A sidebar entry.
 *
 * `key` is generated on the frontend and is what the UI keys off, because a
 * terminal exists in the list before the backend has handed back a session id.
 * `sessionId` fills in once the shell is actually running.
 */
import type { SshRequest } from "./ConnectDialog.svelte";

export type Slot = {
  key: number;
  sessionId: number | null;
  /** Which detected shell this tab runs (empty for SSH tabs). */
  profileId: string;
  /** Set when this tab is a remote connection rather than a local shell. */
  ssh: SshRequest | null;
  title: string;
  /** Runs at high integrity. Drives the red status dot. */
  elevated: boolean;
  /**
   * Set when this tab arrived from another window: the shell is already
   * running, so the view attaches to it rather than starting anything.
   */
  adopt: Adoption | null;
};

/** A session to take over, and the screen it last had. */
export type Adoption = {
  sessionId: number;
  /** xterm's own serialisation of the buffer, written before attaching. */
  snapshot: string;
};

/**
 * Everything the receiving window needs to rebuild a tab that is already
 * running somewhere else.
 *
 * This is what crosses the backend during a drag. The backend treats it as
 * opaque JSON — what a tab *is* stays a frontend concern — so this type is the
 * only definition of the shape, on both ends of the move.
 */
export type Handoff = {
  sessionId: number;
  profileId: string;
  ssh: SshRequest | null;
  title: string;
  elevated: boolean;
  snapshot: string;
};

let nextKey = 1;

export function newSlot(profileId: string, title: string, elevated: boolean): Slot {
  return {
    key: nextKey++,
    sessionId: null,
    profileId,
    ssh: null,
    title,
    elevated,
    adopt: null,
  };
}

/// `title` overrides the default `user@host` — saved connections use their
/// chosen name so the sidebar matches what you called the machine.
export function newSshSlot(ssh: SshRequest, title?: string): Slot {
  return {
    key: nextKey++,
    sessionId: null,
    profileId: "",
    ssh,
    title: title?.trim() || `${ssh.username}@${ssh.host}`,
    elevated: false,
    adopt: null,
  };
}

/** A tab dragged in from another window. The session behind it never stopped. */
export function adoptedSlot(handoff: Handoff): Slot {
  return {
    key: nextKey++,
    sessionId: handoff.sessionId,
    profileId: handoff.profileId,
    ssh: handoff.ssh,
    title: handoff.title,
    elevated: handoff.elevated,
    adopt: { sessionId: handoff.sessionId, snapshot: handoff.snapshot },
  };
}

/** What a live tab has to say about itself to be rebuilt elsewhere. */
export function handoffFor(slot: Slot, sessionId: number, snapshot: string): Handoff {
  return {
    sessionId,
    profileId: slot.profileId,
    ssh: slot.ssh,
    title: slot.title,
    elevated: slot.elevated,
    snapshot,
  };
}
