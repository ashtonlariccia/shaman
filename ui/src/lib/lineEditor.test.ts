import { describe, expect, it } from "vitest";

import { CLEAR_LINE, lineEditorFor } from "./lineEditor";

describe("lineEditorFor", () => {
  it("treats the Windows shells as ones where Escape already clears", () => {
    for (const profileId of ["cmd", "powershell", "powershell-admin", "pwsh"]) {
      expect(lineEditorFor({ profileId })).toBe("escape");
    }
  });

  it("treats WSL distros and SSH hosts as readline", () => {
    expect(lineEditorFor({ profileId: "wsl:Ubuntu" })).toBe("readline");
    expect(lineEditorFor({ profileId: "", ssh: { host: "10.0.0.5" } })).toBe("readline");
  });

  it("defaults an unknown shell to Escape rather than to control characters", () => {
    // Sending a stray Escape to a readline shell is absorbed; sending Ctrl+E
    // Ctrl+U to something that does not understand it leaves junk on the line.
    expect(lineEditorFor({ profileId: "something-new" })).toBe("escape");
    expect(lineEditorFor({})).toBe("escape");
  });
});

describe("CLEAR_LINE", () => {
  it("is Ctrl+E then Ctrl+U, so the whole line goes regardless of the cursor", () => {
    // Ctrl+U alone only kills backwards from the cursor, which would leave the
    // tail of a line the user had gone back to edit.
    expect(CLEAR_LINE).toBe("\x05\x15");
  });
});
