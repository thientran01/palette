/*
 * reactive vote kernel — the pref is a fact, not a defaulted true. A realm
 * that votes before prefs_seed lands (or votes true against a persisted off)
 * is what kept WASAPI + FFT running after the user turned the separator off.
 * Module state outlives a test, so every case gets a virgin import.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

type Reactive = typeof import("./reactive");

const setReactiveEnabled = vi.fn();
const prefsSeed = vi.fn();
let settingsCb: ((change: { key: string; value: unknown }) => void) | null;
let reducedMotion = false;

function installMatchMedia(): void {
  vi.stubGlobal("window", {
    matchMedia: (query: string) => ({
      matches: reducedMotion,
      media: query,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    }),
  });
}

async function load(): Promise<Reactive> {
  installMatchMedia();
  vi.resetModules();
  vi.doMock("./backend", () => ({
    SPECTRUM_BINS: 16,
    onAudioBands: vi.fn(),
    commands: {
      setReactiveEnabled,
      prefsSeed,
    },
    onSettingsChanged: (cb: (change: { key: string; value: unknown }) => void) => {
      settingsCb = cb;
      return () => {
        settingsCb = null;
      };
    },
  }));
  return import("./reactive");
}

beforeEach(() => {
  setReactiveEnabled.mockReset();
  prefsSeed.mockReset();
  settingsCb = null;
  reducedMotion = false;
});

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("reactive vote", () => {
  it("does not vote before the pref seed resolves", async () => {
    let resolveSeed!: (s: { reactive_separator: boolean }) => void;
    prefsSeed.mockImplementation(
      () =>
        new Promise<{ reactive_separator: boolean }>((resolve) => {
          resolveSeed = resolve;
        }),
    );
    const reactive = await load();
    reactive.initReactive();
    expect(setReactiveEnabled).not.toHaveBeenCalled();
    resolveSeed({ reactive_separator: true });
    await Promise.resolve();
    expect(setReactiveEnabled).toHaveBeenCalled();
  });

  it("pref-off seed votes false when reduced motion is off", async () => {
    prefsSeed.mockResolvedValue({ reactive_separator: false });
    const reactive = await load();
    reactive.initReactive();
    await Promise.resolve();
    expect(setReactiveEnabled).toHaveBeenCalledTimes(1);
    expect(setReactiveEnabled).toHaveBeenCalledWith(false);
  });

  it("settings-changed flips the vote", async () => {
    prefsSeed.mockResolvedValue({ reactive_separator: true });
    const reactive = await load();
    reactive.initReactive();
    await Promise.resolve();
    expect(setReactiveEnabled).toHaveBeenLastCalledWith(true);
    settingsCb!({ key: "reactive_separator", value: false });
    expect(setReactiveEnabled).toHaveBeenLastCalledWith(false);
  });
});
