/** Host keys Shaman trusts, and the dialog that lets you forget one. */
import { invoke } from "@tauri-apps/api/core";

import type { TrustedHost } from "../types";

export class TrustedHosts {
  list = $state<TrustedHost[]>([]);

  async refresh() {
    try {
      this.list = await invoke<TrustedHost[]>("trusted_hosts");
    } catch (e) {
      console.error("trusted_hosts failed", e);
    }
  }

  /** The next connection to that host will ask again. */
  async forget(key: string) {
    try {
      await invoke("forget_trusted_host", { key });
      await this.refresh();
    } catch (e) {
      console.error("forget_trusted_host failed", e);
    }
  }
}
