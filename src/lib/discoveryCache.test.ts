import { describe, expect, it } from "vitest";
import { createDiscoveryCache } from "./discoveryCache";

function memStore() {
  const data = new Map<string, string>();
  return {
    data,
    getItem: (k: string) => data.get(k) ?? null,
    setItem: (k: string, v: string) => void data.set(k, v),
  };
}

describe("discoveryCache", () => {
  it("survives a restart through storage", () => {
    const store = memStore();
    createDiscoveryCache<string>(store).set(100, ["a", "b"]);
    expect(createDiscoveryCache<string>(store).get(100)).toEqual(["a", "b"]);
  });

  it("keeps only the newest blocks", () => {
    const store = memStore();
    const c = createDiscoveryCache<string>(store);
    c.set(1, ["old"]);
    c.set(2, ["mid"]);
    c.set(3, ["new"]);
    const again = createDiscoveryCache<string>(store);
    expect(again.get(1)).toBeUndefined();
    expect(again.get(2)).toEqual(["mid"]);
    expect(again.get(3)).toEqual(["new"]);
  });

  it("deletes persist", () => {
    const store = memStore();
    const c = createDiscoveryCache<string>(store);
    c.set(5, ["x"]);
    c.delete(5);
    expect(createDiscoveryCache<string>(store).get(5)).toBeUndefined();
  });

  it("works without storage and shrugs off garbage", () => {
    const c = createDiscoveryCache<string>(null);
    c.set(1, ["x"]);
    expect(c.get(1)).toEqual(["x"]);

    const store = memStore();
    store.data.set("pulse.discoveryCache", "{not json");
    expect(createDiscoveryCache<string>(store).get(1)).toBeUndefined();

    const throwing = {
      getItem: () => {
        throw new Error("blocked");
      },
      setItem: () => {
        throw new Error("blocked");
      },
    };
    const t = createDiscoveryCache<string>(throwing);
    t.set(2, ["y"]);
    expect(t.get(2)).toEqual(["y"]);
  });
});
