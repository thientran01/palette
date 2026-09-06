import { describe, expect, it } from "vitest";
import { createVocalCue } from "./vocalCue";

describe("vocal anticipation", () => {
  const cue=createVocalCue([{t:1000,end:2000,text:"held"},{t:3000,end:3200,text:"next"}]);
  it("gathers before the onset and settles into a steady hold", () => {
    expect(cue(700)).toBe(0);
    expect(cue(870)).toBeCloseTo(.5);
    expect(cue(999)).toBeGreaterThan(.99);
    expect(cue(1000)).toBe(1);
    expect(cue(1070)).toBeCloseTo(.25);
    expect(cue(1500)).toBe(.18);
    expect(cue(1900)).toBe(.18);
  });
  it("goes quiet in gaps and after the final word, with no invented beats", () => {
    expect(cue(2200)).toBe(0);
    expect(cue(2600)).toBe(0);
    expect(cue(2870)).toBeCloseTo(.5);
    expect(cue(3500)).toBe(0);
    expect(cue(1500)).toBe(.18); // Pure sampling also supports backward seek.
  });
  it("handles overlapping voices in source order without losing a held vocal", () => {
    const overlap=createVocalCue([{t:2000,end:2200,text:"short"},{t:1000,end:4000,text:"held"}]);
    expect(overlap(2500)).toBe(.18);
    expect(overlap(4100)).toBe(0);
  });
  it("excludes source-phrase estimates and tolerates empty or invalid data", () => {
    expect(createVocalCue([{t:1000,end:3000,text:"oh-oh",timing:"phrase"}])(1000)).toBe(0);
    expect(createVocalCue([])(1000)).toBe(0);
    expect(createVocalCue([{t:NaN,text:"bad"}])(1000)).toBe(0);
  });
  it("blends dense entrances instead of blinking off between syllables", () => {
    const dense=createVocalCue(Array.from({length:8},(_,i)=>({t:1000+i*80,end:1080+i*80,text:"한"})));
    for(let p=1000;p<1560;p+=10) expect(dense(p)).toBeGreaterThan(.7);
  });
});
