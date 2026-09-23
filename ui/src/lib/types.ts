/** A shell that `shaman-core` found installed on this machine. */
export type ShellProfile = {
  id: string;
  label: string;
  program: string;
  args: string[];
  /** Launches at high integrity. Shown in red in the menu. */
  elevated: boolean;
};

/** Structured failure from `ssh_connect`. */
export type SshFailureInfo = {
  kind?: "auth" | "unreachable" | "session" | "unknownHostKey" | "hostKeyChanged";
  message: string;
  fingerprint?: string;
  expectedFingerprint?: string;
};

/** A saved SSH connection. Never carries the password. */
export type SavedConnection = {
  id: string;
  /** What we dial. */
  host: string;
  port: number;
  username: string;
  authKind?: string;
  /** A name you gave it. Display only. */
  name?: string;
};

/** A host key Shaman trusts. */
export type TrustedHost = {
  /** Raw store key, used to remove the entry. */
  key: string;
  host: string;
  port: number;
  fingerprint: string;
};

/**
 * One button on the quick-open strip.
 *
 * Carries only what to open. `label` is a snapshot from the moment it was
 * pinned and is used only when the target no longer resolves — see
 * `resolvePin` in `pins.ts`.
 */
export type Pin = {
  kind: "local" | "saved";
  /** A `ShellProfile.id`, or a `SavedConnection.id`. */
  target: string;
  label: string;
};

/** A private key discovered in ~/.ssh. */
export type DiscoveredKey = {
  path: string;
  name: string;
  /** Needs a passphrase to unlock. */
  encrypted: boolean;
};
