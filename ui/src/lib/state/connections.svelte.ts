/**
 * Saved SSH connections, the keys discovered in `~/.ssh`, and the toast that
 * offers to remember a host you have just reached.
 *
 * Passwords never appear here. The UI deals in ids; the secret is decrypted in
 * the backend, used, and dropped.
 */
import { invoke } from "@tauri-apps/api/core";

import type { SshRequest } from "../ConnectDialog.svelte";
import { findSaved } from "../pins";
import type { DiscoveredKey, SavedConnection } from "../types";

export class Connections {
  list = $state<SavedConnection[]>([]);
  keys = $state<DiscoveredKey[]>([]);

  /** The connection the toast is currently offering to save. */
  offer = $state<SshRequest | null>(null);

  async refresh() {
    try {
      this.list = await invoke<SavedConnection[]>("saved_connections");
    } catch (e) {
      console.error("saved_connections failed", e);
    }
  }

  async refreshKeys() {
    try {
      this.keys = await invoke<DiscoveredKey[]>("ssh_keys");
    } catch (e) {
      console.error("ssh_keys failed", e);
    }
  }

  /** The stored entry matching a live target, if there is one. */
  find(ssh: SshRequest): SavedConnection | undefined {
    return findSaved(this.list, ssh);
  }

  /** Store a remote target. Returns the entry, which carries its new id. */
  async save(request: SshRequest): Promise<SavedConnection | null> {
    try {
      const stored = await invoke<SavedConnection>("save_connection", { target: request });
      await this.refresh();
      await this.refreshKeys();
      return stored;
    } catch (e) {
      console.error("save_connection failed", e);
      return null;
    }
  }

  async rename(id: string, name: string) {
    try {
      await invoke("rename_connection", { id, name });
      await this.refresh();
    } catch (e) {
      console.error("rename_connection failed", e);
    }
  }

  /**
   * Forget a saved connection.
   *
   * The backend also drops any pin pointing at it, so the caller must refresh
   * the pin strip afterwards — see `removeConnection` in App.
   */
  async remove(id: string) {
    try {
      await invoke("remove_connection", { id });
      await this.refresh();
      await this.refreshKeys();
    } catch (e) {
      console.error("remove_connection failed", e);
    }
  }

  /**
   * Landed in the shell of a remote host: offer to remember it, unless it is
   * already saved or the prompt has been turned off.
   */
  async maybeOffer(ssh: SshRequest) {
    try {
      if (!(await invoke<boolean>("suggest_saving_enabled"))) return;

      const alreadySaved = await invoke<boolean>("connection_is_saved", {
        host: ssh.host,
        port: ssh.port,
        username: ssh.username,
      });
      if (alreadySaved) return;

      this.offer = ssh;
    } catch (e) {
      console.error("could not check saved connections", e);
    }
  }

  async acceptOffer() {
    const offer = this.offer;
    this.offer = null;
    if (offer) await this.save(offer);
  }

  /** "Never show again" from the save prompt. */
  async neverOfferAgain() {
    this.offer = null;
    try {
      await invoke("set_suggest_saving", { enabled: false });
    } catch (e) {
      console.error("set_suggest_saving failed", e);
    }
  }
}
