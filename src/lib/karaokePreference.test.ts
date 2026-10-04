/** Turning karaoke off must survive reopening either room, startup races,
 * and failed writes. The setting never substitutes manual line scrolling. */
import { beforeEach, describe, expect, it, vi } from "vitest";

const backend = vi.hoisted(() => ({prefsSeed: vi.fn(), listen: vi.fn(), write: vi.fn()}));
vi.mock("./backend", () => ({
  commands: {prefsSeed: backend.prefsSeed, setKaraokeLyrics: backend.write},
  listenSettingsChanged: backend.listen,
}));
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((res, rej) => {resolve = res; reject = rej;});
  return {promise, resolve, reject};
}
let store: typeof import("./karaokePreference");
let emit: (change: {key: string; value: unknown}) => void;
let registration: ReturnType<typeof deferred<() => void>>;
let seed: ReturnType<typeof deferred<{karaoke_lyrics: boolean}>>;
beforeEach(async () => {
  vi.resetModules();
  backend.prefsSeed.mockReset(); backend.listen.mockReset(); backend.write.mockReset();
  registration = deferred<() => void>(); seed = deferred<{karaoke_lyrics: boolean}>();
  backend.listen.mockImplementation((cb: typeof emit) => {emit = cb; return registration.promise;});
  backend.prefsSeed.mockReturnValue(seed.promise);
  store = await import("./karaokePreference");
});
async function initialize(value = true) {
  const init = store.initKaraokePreference();
  registration.resolve(vi.fn()); seed.resolve({karaoke_lyrics: value});
  await init;
}
describe("karaoke preference across rooms", () => {
  it("withholds word wipes until the preference is read and registers before reading", async () => {
    expect(store.karaokeEnabled()).toBe(false);
    const init = store.initKaraokePreference();
    expect(backend.prefsSeed).not.toHaveBeenCalled();
    registration.resolve(vi.fn()); seed.resolve({karaoke_lyrics: true});
    await init;
    expect(store.karaokeEnabled()).toBe(true);
  });
  it("keeps a live off event instead of a stale startup snapshot", async () => {
    const init = store.initKaraokePreference();
    emit({key: "karaoke_lyrics", value: false});
    registration.resolve(vi.fn()); seed.resolve({karaoke_lyrics: true});
    await init;
    expect(store.karaokeEnabled()).toBe(false);
    emit({key: "reactive_separator", value: true});
    emit({key: "karaoke_lyrics", value: "true"});
    expect(store.karaokeEnabled()).toBe(false);
  });
  it("shares startup and restores an off preference for later consumers", async () => {
    const first = store.initKaraokePreference(), second = store.initKaraokePreference();
    registration.resolve(vi.fn()); seed.resolve({karaoke_lyrics: false});
    await Promise.all([first, second]); await store.initKaraokePreference();
    expect(backend.listen).toHaveBeenCalledTimes(1);
    expect(backend.prefsSeed).toHaveBeenCalledTimes(1);
    expect(store.karaokeEnabled()).toBe(false);
  });
  it("retries a failed seed without adding another listener", async () => {
    const init = store.initKaraokePreference(); registration.resolve(vi.fn());
    seed.reject(new Error("unavailable")); await init;
    backend.prefsSeed.mockResolvedValue({karaoke_lyrics: true});
    await store.initKaraokePreference();
    expect(backend.listen).toHaveBeenCalledTimes(1);
    expect(store.karaokeEnabled()).toBe(true);
  });
  it("keeps the old setting on rejection and reads state after a successful write", async () => {
    await initialize();
    backend.write.mockRejectedValueOnce(new Error("write rejected"));
    await expect(store.setKaraokePreference(false)).rejects.toThrow("write rejected");
    expect(store.karaokeEnabled()).toBe(true);
    backend.write.mockResolvedValueOnce(undefined);
    backend.prefsSeed.mockResolvedValueOnce({karaoke_lyrics: false});
    await store.setKaraokePreference(false);
    expect(backend.write).toHaveBeenLastCalledWith(false);
    expect(store.karaokeEnabled()).toBe(false);
  });
  it("does not overwrite a newer event with a write verification snapshot", async () => {
    await initialize();
    const reread = deferred<{karaoke_lyrics: boolean}>();
    backend.write.mockResolvedValue(undefined); backend.prefsSeed.mockReturnValueOnce(reread.promise);
    const write = store.setKaraokePreference(false);
    await vi.waitFor(() => expect(backend.prefsSeed).toHaveBeenCalledTimes(2));
    emit({key: "karaoke_lyrics", value: true});
    reread.resolve({karaoke_lyrics: false}); await write;
    expect(store.karaokeEnabled()).toBe(true);
  });
});
