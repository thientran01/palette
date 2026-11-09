import { beforeEach, describe, expect, it, vi } from "vitest";

const listen = vi.fn(async () => () => {});
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("@tauri-apps/api/webviewWindow", () => ({
  getCurrentWebviewWindow: () => ({ label: "main" }),
}));

const { listenHere } = await import("./listenHere");

describe("listenHere", () => {
  beforeEach(() => listen.mockClear());

  // A global listen() (target Any) hears every window's emit_to: the widget
  // took the search window's "window-shown: false" as its own and froze its
  // karaoke. The listener must be targeted at this window.
  it("targets the current window, never Any", async () => {
    const handler = () => {};
    await listenHere("window-shown", handler);
    expect(listen).toHaveBeenCalledWith("window-shown", handler, {
      target: { kind: "WebviewWindow", label: "main" },
    });
  });
});
