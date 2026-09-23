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
  };
}
