import { describe, expect, it } from "vitest";

import { adoptedSlot, handoffFor, newSlot, newSshSlot, type Handoff } from "./slots";

/**
 * A terminal dragged into another window has to come out the far side as the
 * same tab: same shell or same host, same name, same colour coding. The payload
 * crosses the backend as opaque JSON, so nothing there would catch a field that
 * stopped being carried — these tests are the only thing that would.
 */
describe("handing a tab to another window", () => {
  it("carries a local tab's identity across", () => {
    const slot = { ...newSlot("pwsh", "PowerShell 7", false), sessionId: 7 };
    const rebuilt = adoptedSlot(handoffFor(slot, 7, "screen"));

    expect(rebuilt.profileId).toBe("pwsh");
    expect(rebuilt.title).toBe("PowerShell 7");
    expect(rebuilt.elevated).toBe(false);
    expect(rebuilt.ssh).toBeNull();
  });

  it("keeps an admin tab elevated, so it stays red in its new window", () => {
    const slot = { ...newSlot("cmd-admin", "Command Prompt (Admin)", true), sessionId: 3 };
    expect(adoptedSlot(handoffFor(slot, 3, "")).elevated).toBe(true);
  });

  it("carries a remote tab's target, not just its name", () => {
    const ssh = {
      host: "box.example",
      port: 22,
      username: "ash",
      auth: { kind: "password", password: "" } as const,
      savedId: "saved-1",
    };
    const slot = { ...newSshSlot(ssh, "the box"), sessionId: 11 };
    const rebuilt = adoptedSlot(handoffFor(slot, 11, ""));

    expect(rebuilt.title).toBe("the box");
    expect(rebuilt.ssh).toEqual(ssh);
  });

  it("attaches to the running session rather than starting a new one", () => {
    const slot = { ...newSlot("cmd", "Command Prompt", false), sessionId: 42 };
    const rebuilt = adoptedSlot(handoffFor(slot, 42, "[32mhello"));

    expect(rebuilt.adopt).toEqual({ sessionId: 42, snapshot: "[32mhello" });
    // The id the shell already has, not a placeholder waiting to be filled in.
    expect(rebuilt.sessionId).toBe(42);
  });

  it("gives the adopted tab a key of its own", () => {
    // Keys are per-window counters, so the source window's key means nothing
    // here — two windows would otherwise collide on their first tab.
    const slot = { ...newSlot("cmd", "Command Prompt", false), sessionId: 1 };
    const handoff: Handoff = handoffFor(slot, 1, "");

    const a = adoptedSlot(handoff);
    const b = adoptedSlot(handoff);
    expect(a.key).not.toBe(b.key);
  });

  it("starts a freshly opened tab unadopted", () => {
    expect(newSlot("cmd", "Command Prompt", false).adopt).toBeNull();
  });
});
