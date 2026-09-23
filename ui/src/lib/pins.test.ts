import { describe, expect, it } from "vitest";

import { dropGap, dropIndex, findSaved, resolvePin, samePin } from "./pins";
import type { Pin, SavedConnection, ShellProfile } from "./types";

const profiles: ShellProfile[] = [
  { id: "cmd", label: "Command Prompt", program: "cmd.exe", args: [], elevated: false },
  {
    id: "powershell-admin",
    label: "Windows PowerShell (Admin)",
    program: "powershell.exe",
    args: [],
    elevated: true,
  },
];

const saved: SavedConnection[] = [
  { id: "c1", host: "10.0.0.5", port: 22, username: "root", name: "prod-db" },
  { id: "c2", host: "Server.Local", port: 2222, username: "me" },
];

describe("resolvePin", () => {
  it("names a local pin exactly as Terminal → New Local Terminal does", () => {
    const pin: Pin = { kind: "local", target: "cmd", label: "something stale" };
    const resolved = resolvePin(pin, profiles, saved);

    expect(resolved.label).toBe("Command Prompt");
    expect(resolved.available).toBe(true);
  });

  it("names a saved pin by the name given in Manage Saved Connections", () => {
    const pin: Pin = { kind: "saved", target: "c1", label: "10.0.0.5" };
    expect(resolvePin(pin, profiles, saved).label).toBe("prod-db");
  });

  it("falls back to the address for a connection that was never named", () => {
    const pin: Pin = { kind: "saved", target: "c2", label: "" };
    const resolved = resolvePin(pin, profiles, saved);

    expect(resolved.label).toBe("Server.Local");
    expect(resolved.detail).toContain("me:2222");
  });

  it("colours a pinned admin shell as admin, not as an ordinary local one", () => {
    expect(resolvePin({ kind: "local", target: "powershell-admin", label: "" }, profiles, saved).kind).toBe("admin");
    expect(resolvePin({ kind: "local", target: "cmd", label: "" }, profiles, saved).kind).toBe("local");
    expect(resolvePin({ kind: "saved", target: "c1", label: "" }, profiles, saved).kind).toBe("remote");
  });

  it("shows the snapshot label, unavailable, when the target is gone", () => {
    const uninstalled = resolvePin({ kind: "local", target: "pwsh", label: "PowerShell 7" }, profiles, saved);
    expect(uninstalled.label).toBe("PowerShell 7");
    expect(uninstalled.available).toBe(false);

    const deleted = resolvePin({ kind: "saved", target: "gone", label: "old-box" }, profiles, saved);
    expect(deleted.label).toBe("old-box");
    expect(deleted.available).toBe(false);
  });
});

describe("samePin", () => {
  it("ignores the label, so a rename is not a second pin", () => {
    expect(
      samePin({ kind: "saved", target: "c1", label: "10.0.0.5" }, { kind: "saved", target: "c1", label: "prod-db" }),
    ).toBe(true);
  });

  it("separates a local pin from a saved one sharing an id string", () => {
    expect(samePin({ kind: "local", target: "x", label: "" }, { kind: "saved", target: "x", label: "" })).toBe(false);
  });
});

describe("dragging a pin to a new position", () => {
  // Four chips, 20px apart, so the midpoints are easy to reason about.
  const mids = [10, 30, 50, 70];

  it("picks the gap by which midpoints the pointer has passed", () => {
    expect(dropGap(mids, 0)).toBe(0); // before everything
    expect(dropGap(mids, 20)).toBe(1);
    expect(dropGap(mids, 60)).toBe(3);
    expect(dropGap(mids, 999)).toBe(4); // past the end
  });

  it("does not move a chip until the pointer clears a neighbour's midpoint", () => {
    // Still inside its own slot, and inside the left half of the next one.
    expect(dropIndex(mids, 15, 0)).toBe(0);
    expect(dropIndex(mids, 29, 0)).toBe(0);
    // Past the second chip's midpoint: now they swap.
    expect(dropIndex(mids, 31, 0)).toBe(1);
  });

  it("discounts the dragged chip when moving right, but not when moving left", () => {
    // Dragging the first chip to the far right lands at the last index, not
    // past the end — the count still included the chip being carried.
    expect(dropIndex(mids, 999, 0)).toBe(3);
    // Dragging the last chip to the far left is a plain insert at zero.
    expect(dropIndex(mids, 0, 3)).toBe(0);
  });

  it("reports the chip's own index anywhere the drop would change nothing", () => {
    // Either side of chip 2's own midpoint is still chip 2's place.
    expect(dropIndex(mids, 45, 2)).toBe(2);
    expect(dropIndex(mids, 55, 2)).toBe(2);
  });

  it("never lands outside the strip, wherever the pointer goes", () => {
    for (const from of [0, 1, 2, 3]) {
      for (const x of [-500, 0, 25, 50, 75, 5000]) {
        const index = dropIndex(mids, x, from);
        expect(index).toBeGreaterThanOrEqual(0);
        expect(index).toBeLessThan(mids.length);
      }
    }
  });

  it("handles a strip of one, where every drop is a no-op", () => {
    expect(dropIndex([10], -100, 0)).toBe(0);
    expect(dropIndex([10], 100, 0)).toBe(0);
  });
});

describe("findSaved", () => {
  it("prefers the id a tab was opened with", () => {
    expect(findSaved(saved, { host: "elsewhere", port: 22, username: "nobody", savedId: "c1" })?.name).toBe("prod-db");
  });

  it("matches a typed-in target on host, port and user", () => {
    // Host is case-insensitive, the way the backend store keys it.
    expect(findSaved(saved, { host: "server.local", port: 2222, username: "me" })?.id).toBe("c2");
    expect(findSaved(saved, { host: "server.local", port: 22, username: "me" })).toBeUndefined();
    expect(findSaved(saved, { host: "server.local", port: 2222, username: "you" })).toBeUndefined();
  });
});
