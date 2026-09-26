import { describe, expect, it } from "vitest";

import { isWebUrl } from "./links";

describe("isWebUrl", () => {
  it("accepts http and https", () => {
    expect(isWebUrl("https://example.com")).toBe(true);
    expect(isWebUrl("http://localhost:5173/x?y#z")).toBe(true);
    expect(isWebUrl("HTTPS://Example.com")).toBe(true);
  });

  it("refuses every other scheme", () => {
    for (const url of [
      "file:///C:/Windows/System32/calc.exe",
      "ms-settings:privacy",
      "javascript:alert(1)",
      "example.com",
    ]) {
      expect(isWebUrl(url), url).toBe(false);
    }
  });

  it("refuses a URL with no host", () => {
    expect(isWebUrl("https://")).toBe(false);
    expect(isWebUrl("http:///path")).toBe(false);
  });

  it("refuses whitespace, quotes and control characters", () => {
    expect(isWebUrl("https://a.com/ x")).toBe(false);
    expect(isWebUrl('https://a.com/"x')).toBe(false);
    expect(isWebUrl("https://a.com/\x1bx")).toBe(false);
  });
});
