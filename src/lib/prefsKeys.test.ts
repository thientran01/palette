/** The shipped bracket shortcuts must also be capturable in Preferences. */
import { describe, expect, it, vi } from "vitest";
vi.mock("./backend", () => ({}));
import { mainKeyToken } from "../Prefs";

describe("Preferences accelerator capture", () => {
  it("maps physical bracket keys to the shipped accelerator tokens", () => {
    expect(mainKeyToken({ code: "BracketLeft" })).toBe("[");
    expect(mainKeyToken({ code: "BracketRight" })).toBe("]");
  });

  it("preserves existing supported keys and rejects modifier-only input", () => {
    expect(mainKeyToken({ code: "KeyP" })).toBe("p");
    expect(mainKeyToken({ code: "Digit0" })).toBe("0");
    expect(mainKeyToken({ code: "ArrowLeft" })).toBe("left");
    expect(mainKeyToken({ code: "ControlLeft" })).toBeNull();
  });
});
