/**
 * The connect dialog's state machine.
 *
 * The dialog stays open in a "connecting" state while the tab tries to reach
 * the host. On failure the tab is discarded and the error comes back into the
 * dialog with the fields intact, so a wrong password costs one field rather
 * than the whole form.
 */
import type { HostKeyPrompt } from "../ConnectDialog.svelte";
import type { SshFailureInfo } from "../types";

export class SshFlow {
  open = $state(false);
  connecting = $state(false);
  error = $state<string | null>(null);
  hostKey = $state<HostKeyPrompt | null>(null);

  /**
   * The tab this dialog is waiting on. A plain field, not `$state`: it is
   * bookkeeping for matching callbacks, and nothing renders from it.
   */
  #pendingKey: number | null = null;

  /** Terminal → New Remote Connection. */
  begin() {
    this.error = null;
    this.hostKey = null;
    this.connecting = false;
    this.open = true;
  }

  /** A tab has been created for this attempt; wait on it. */
  submitted(key: number) {
    this.error = null;
    this.hostKey = null;
    this.connecting = true;
    this.#pendingKey = key;
  }

  cancel() {
    this.open = false;
    this.connecting = false;
    this.error = null;
    this.hostKey = null;
    this.#pendingKey = null;
  }

  /** Reached the remote shell. Returns false if this wasn't the tab we awaited. */
  connected(key: number): boolean {
    if (key !== this.#pendingKey) return false;
    this.#pendingKey = null;
    this.connecting = false;
    this.open = false;
    return true;
  }

  /**
   * The attempt failed. Returns false if this wasn't the tab we awaited, in
   * which case the caller should leave it alone.
   *
   * A true return means the dialog is showing the reason and the caller should
   * drop the dead tab.
   */
  failed(key: number, failure: SshFailureInfo): boolean {
    if (key !== this.#pendingKey) return false;
    this.#pendingKey = null;
    this.connecting = false;

    // An unverified host key isn't an error to retype past -- it's a decision.
    // Show the fingerprint and let the user accept it explicitly.
    if (failure.kind === "unknownHostKey" || failure.kind === "hostKeyChanged") {
      this.error = null;
      this.hostKey = {
        changed: failure.kind === "hostKeyChanged",
        fingerprint: failure.fingerprint ?? "(unknown)",
        expectedFingerprint: failure.expectedFingerprint,
        message: failure.message,
      };
    } else {
      this.hostKey = null;
      this.error = failure.message;
    }

    return true;
  }
}
