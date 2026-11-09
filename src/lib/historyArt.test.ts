import { describe, expect, it } from "vitest";
import { createArtCache, withinBudget } from "./historyArt";

function memStore() {
  const data = new Map<string, string>();
  return {
    getItem: (k: string) => data.get(k) ?? null,
    setItem: (k: string, v: string) => void data.set(k, v),
  };
}

describe("history art cache", () => {
  it("keeps covers across a restart", () => {
    const store = memStore();
    createArtCache(store).set("k1", "https://i.scdn.co/image/a", 1000);
    expect(createArtCache(store).get("k1", 2000)).toBe("https://i.scdn.co/image/a");
    expect(createArtCache(store).get("k2", 2000)).toBeUndefined();
  });

  it("drops a cover after a month", () => {
    const c = createArtCache(memStore());
    c.set("k1", "u", 0);
    expect(c.get("k1", 29 * 86_400_000)).toBe("u");
    expect(c.get("k1", 31 * 86_400_000)).toBeUndefined();
  });

  it("evicts the oldest past the cap, and clears", () => {
    const store = memStore();
    const c = createArtCache(store);
    for (let i = 0; i < 1501; i++) c.set(`k${i}`, `u${i}`, 0);
    expect(c.get("k0", 0)).toBeUndefined();
    expect(c.get("k1500", 0)).toBe("u1500");
    c.clear();
    expect(createArtCache(store).get("k1500", 0)).toBeUndefined();
  });

  it("survives unreadable or throwing storage", () => {
    const bad = {
      getItem: () => "{not json",
      setItem: () => {
        throw new Error("quota");
      },
    };
    const c = createArtCache(bad);
    c.set("k", "u", 0);
    expect(c.get("k", 0)).toBe("u");
    expect(createArtCache(null).get("k")).toBeUndefined();
  });
});

describe("withinBudget", () => {
  it("returns what landed in time and the fallback for the rest", async () => {
    const slow = new Promise<string>((r) => setTimeout(() => r("slow"), 200));
    const failed = Promise.reject(new Error("x"));
    const out = await withinBudget([Promise.resolve("fast"), slow, failed], 30, "none");
    expect(out).toEqual(["fast", "none", "none"]);
  });

  it("doesn't wait out the budget when everything is done", async () => {
    const t = Date.now();
    expect(await withinBudget([Promise.resolve(1), Promise.resolve(2)], 5000, 0)).toEqual([1, 2]);
    expect(Date.now() - t).toBeLessThan(1000);
  });
});
