import { describe, expect, it } from "vitest";

import { KIND_TITLE, profileKind, slotKind } from "./kinds";

describe("slotKind", () => {
  it("calls a plain shell local and an elevated one admin", () => {
    expect(slotKind({ ssh: null, elevated: false })).toBe("local");
    expect(slotKind({ ssh: null, elevated: true })).toBe("admin");
  });

  it("calls anything with an SSH target remote", () => {
    const ssh = { host: "10.0.0.5", port: 22, username: "root" };
    expect(slotKind({ ssh, elevated: false })).toBe("remote");
    // Remote wins outright: `elevated` describes *local* privilege, so an SSH
    // tab must never come back red.
    expect(slotKind({ ssh, elevated: true })).toBe("remote");
  });
});

describe("profileKind", () => {
  it("splits the admin variants out from the ordinary shells", () => {
    expect(profileKind({ elevated: false })).toBe("local");
    expect(profileKind({ elevated: true })).toBe("admin");
  });
});

describe("KIND_TITLE", () => {
  it("names every kind, so colour is never the only cue", () => {
    for (const kind of ["local", "admin", "remote"] as const) {
      expect(KIND_TITLE[kind].length).toBeGreaterThan(0);
    }
  });
});
