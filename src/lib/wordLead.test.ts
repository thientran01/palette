/** The startup IPC snapshot must never overwrite a live nudge or miss one
 * while native listener registration is pending. Exercise the real store. */
import { beforeEach, describe, expect, it, vi } from "vitest";

const backend = vi.hoisted(() => ({
  wordLead: vi.fn(),
  onWordLead: vi.fn(),
}));
vi.mock("./backend", () => ({
  commands: { wordLead: backend.wordLead },
  onWordLead: backend.onWordLead,
}));

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}

let store: typeof import("./wordLead");
let emit: (value: number) => void;
let registration: ReturnType<typeof deferred<() => void>>;
let seed: ReturnType<typeof deferred<number>>;

beforeEach(async () => {
  vi.resetModules();
  backend.wordLead.mockReset();
  backend.onWordLead.mockReset();
  registration = deferred<() => void>();
  seed = deferred<number>();
  backend.onWordLead.mockImplementation((cb: typeof emit) => {
    emit = cb;
    return registration.promise;
  });
  backend.wordLead.mockReturnValue(seed.promise);
  store = await import("./wordLead");
});

describe("word lead initialization", () => {
  it("waits for registration before reading the initial value", async () => {
    const init = store.initWordLead();
    expect(backend.wordLead).not.toHaveBeenCalled();
    registration.resolve(vi.fn());
    await Promise.resolve();
    expect(backend.wordLead).toHaveBeenCalledTimes(1);
    seed.resolve(0);
    await init;
    expect(store.wordLeadMs()).toBe(0);
  });

  it("ignores a stale seed after a live nudge", async () => {
    const init = store.initWordLead();
    registration.resolve(vi.fn());
    await Promise.resolve();
    emit(200);
    seed.resolve(160);
    await init;
    expect(store.wordLeadMs()).toBe(200);
    emit(220);
    expect(store.wordLeadMs()).toBe(220);
  });

  it("keeps events delivered before registration acknowledges readiness", async () => {
    const init = store.initWordLead();
    emit(180);
    registration.resolve(vi.fn());
    seed.resolve(160);
    await init;
    expect(store.wordLeadMs()).toBe(180);
  });

  it("shares initialization across callers and does not reseed after success", async () => {
    const first = store.initWordLead();
    const second = store.initWordLead();
    registration.resolve(vi.fn());
    seed.resolve(-60);
    await Promise.all([first, second]);
    await store.initWordLead();
    expect(backend.onWordLead).toHaveBeenCalledTimes(1);
    expect(backend.wordLead).toHaveBeenCalledTimes(1);
    expect(store.wordLeadMs()).toBe(-60);
  });

  it("handles seed rejection and retries without duplicating the listener", async () => {
    const init = store.initWordLead();
    registration.resolve(vi.fn());
    await Promise.resolve();
    seed.reject(new Error("IPC unavailable"));
    await expect(Promise.resolve(init)).resolves.toBeUndefined();
    backend.wordLead.mockResolvedValue(240);
    await store.initWordLead();
    expect(store.wordLeadMs()).toBe(240);
    expect(backend.onWordLead).toHaveBeenCalledTimes(1);
    expect(backend.wordLead).toHaveBeenCalledTimes(2);
  });

  it("handles registration rejection and allows another attempt", async () => {
    const init = store.initWordLead();
    registration.reject(new Error("listen unavailable"));
    await expect(Promise.resolve(init)).resolves.toBeUndefined();
    expect(backend.wordLead).not.toHaveBeenCalled();
    backend.onWordLead.mockResolvedValue(vi.fn());
    backend.wordLead.mockResolvedValue(120);
    await store.initWordLead();
    expect(store.wordLeadMs()).toBe(120);
    expect(backend.onWordLead).toHaveBeenCalledTimes(2);
  });
});
