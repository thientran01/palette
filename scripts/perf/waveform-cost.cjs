// Waveform main-thread cost in the Vite browser mock, per widget mode.
//
// Start the mock first (node .cursor/skills/verify-pulse/scripts/launch.mjs,
// port 1422), then: node scripts/perf/waveform-cost.cjs <label> [seconds]
// Env: URL (default http://localhost:1422/), RUNS (default 3, median kept).
//
// Each scene is measured three ways: fed (a visible window), "hidden: no
// payloads" (what audio.rs BandFanout gives a hidden window; the mock's
// __mockUnfed), and "waveform off" (reduced motion, so no Waveform work at
// all: the floor). Headless Chromium on a shared machine is noisy (about
// ±5 ms/s on task time between runs), so compare medians, not single runs.
const { chromium } = require("playwright");
const URL = process.env.URL || "http://localhost:1422/";
const SECS = Number(process.argv[3] || 10);

const SCENES = [
  { name: "pill", path: "/", mode: "pill", vp: { width: 420, height: 120 } },
  { name: "card", path: "/", mode: "card", vp: { width: 420, height: 220 } },
  { name: "expanded art (lg)", path: "/", mode: "expanded", vp: { width: 420, height: 640 } },
  { name: "focus (room)", path: "/?window=focus", mode: "card", vp: { width: 1600, height: 900 } },
];

async function measure(browser, scene, reduced, variant) {
  const ctx = await browser.newContext({ viewport: scene.vp, reducedMotion: reduced ? "reduce" : "no-preference" });
  const page = await ctx.newPage();
  await page.goto(URL.replace(/\/$/, "") + scene.path);
  await page.evaluate((mode) => {
    localStorage.setItem("pulse.launchMode", mode);
    if (mode === "expanded") localStorage.setItem("pulse.expandedView", "art");
    else localStorage.removeItem("pulse.expandedView");
  }, scene.mode);
  await page.reload();
  await page.waitForTimeout(4000); // bloom + settle in
  // Count rAF callbacks + transform writes from inside the page.
  await page.evaluate(() => {
    window.__raf = 0;
    const o = window.requestAnimationFrame;
    window.requestAnimationFrame = (cb) => o((t) => { window.__raf++; cb(t); });
    window.__tw = 0;
    const mo = new MutationObserver((recs) => { window.__tw += recs.length; });
    for (const el of document.querySelectorAll("span.origin-center.will-change-transform")) mo.observe(el, { attributes: true, attributeFilter: ["style"] });
  });
  if (variant === "unfed") { await page.evaluate(() => window.__mockUnfed(600000)); await page.waitForTimeout(1000); await page.evaluate(() => { window.__raf = 0; window.__tw = 0; }); }
  const cdp = await ctx.newCDPSession(page);
  await cdp.send("Performance.enable");
  const m0 = Object.fromEntries((await cdp.send("Performance.getMetrics")).metrics.map((m) => [m.name, m.value]));
  await page.waitForTimeout(SECS * 1000);
  const m1 = Object.fromEntries((await cdp.send("Performance.getMetrics")).metrics.map((m) => [m.name, m.value]));
  const counts = await page.evaluate(() => ({ raf: window.__raf, tw: window.__tw }));
  const bars = await page.evaluate(() => document.querySelectorAll("span.origin-center.will-change-transform").length);
  await ctx.close();
  const d = (k) => m1[k] - m0[k];
  return {
    scene: scene.name + (reduced ? " [waveform off]" : variant === "unfed" ? " [hidden: no payloads]" : ""),
    bars,
    "task ms/s": +((d("TaskDuration") * 1000) / SECS).toFixed(2),
    "script ms/s": +((d("ScriptDuration") * 1000) / SECS).toFixed(2),
    "style ms/s": +((d("RecalcStyleDuration") * 1000) / SECS).toFixed(2),
    "layout ms/s": +((d("LayoutDuration") * 1000) / SECS).toFixed(2),
    "styles/s": +(d("RecalcStyleCount") / SECS).toFixed(1),
    "layouts/s": +(d("LayoutCount") / SECS).toFixed(1),
    "rAF/s": +(counts.raf / SECS).toFixed(1),
    "transform writes/s": +(counts.tw / SECS).toFixed(1),
  };
}

(async () => {
  const browser = await chromium.launch();
  const rows = [];
  const runs = Number(process.env.RUNS || 3);
  for (const scene of SCENES) {
    for (const [reduced, variant] of [[false, "fed"], [false, "unfed"], [true, "fed"]]) {
      const rs = [];
      for (let r = 0; r < runs; r++) rs.push(await measure(browser, scene, reduced, variant));
      // median per numeric column
      const med = { ...rs[0] };
      for (const k of Object.keys(med)) {
        if (typeof med[k] !== "number") continue;
        const v = rs.map((x) => x[k]).sort((a, b) => a - b);
        med[k] = v[Math.floor(v.length / 2)];
      }
      rows.push(med);
    }
  }
  await browser.close();
  console.log(process.argv[2] || "");
  console.table(rows);
})();
