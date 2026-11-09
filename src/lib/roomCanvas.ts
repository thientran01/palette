/*
 * Focus mode's room waveform, drawn on a canvas while it plays.
 *
 * The room bars are 12px wide and up to 100px tall. As DOM spans written at
 * ~30fps they looked choppy (a kick moved a bar ~50px in one step, where the
 * pill's 9px bars move a pixel or two) and low quality (scaleY squashes a
 * pill's round ends into flat ellipses). Drawing them on one canvas gives
 * exact capsules at any height and, measured in the focus page, costs about
 * a third of the main thread the 30fps DOM writes did even at twice the
 * frame rate, because a canvas frame touches no style, layout or paint.
 *
 * Only the "alive" phase is drawn here. The settle and bloom beats stay DOM
 * (Waveform.tsx) with their CSS transitions; Waveform hands the bars over at
 * the phase boundaries. This module is the pure part: geometry, easing,
 * frame pacing and the draw.
 */

/** One capsule in CSS px, relative to the canvas: center, width, total
 * height (cap to cap) and opacity. */
export type Capsule = { cx: number; cy: number; w: number; h: number; a: number };

/** Shortest gap between drawn frames. Bands arrive at ~30Hz and the
 * envelopes interpolate between them, so the bars stay smooth up to the
 * display rate, but each frame costs a little. 13ms keeps whole-frame
 * pacing on common refresh rates: 60Hz draws every frame, 120Hz every 2nd
 * (60fps), 144Hz every 2nd (72fps), 165Hz every 3rd (55fps), 240Hz every
 * 4th (60fps). Even pacing matters more than the last few fps: a gate that
 * alternates between 2 and 3 display frames judders. */
export const ROOM_FRAME_MS = 13;

/** Center x of each bar for a row laid out like the DOM bars
 * (justify-between, `n` bars of width `barW` across `rowW`). */
export function roomCenters(n: number, rowW: number, barW: number): number[] {
  if (n <= 1) return [rowW / 2];
  const step = (rowW - barW) / (n - 1);
  return Array.from({ length: n }, (_, i) => barW / 2 + i * step);
}

/** A CSS cubic-bezier(x1, y1, x2, y2) as a function of progress 0..1, so the
 * canvas entrance runs on the same curve as the DOM's EASE transitions. */
export function bezierEase([x1, y1, x2, y2]: readonly number[]): (t: number) => number {
  const cx = 3 * x1;
  const bx = 3 * (x2 - x1) - cx;
  const ax = 1 - cx - bx;
  const cy = 3 * y1;
  const by = 3 * (y2 - y1) - cy;
  const ay = 1 - cy - by;
  const sampleX = (s: number) => ((ax * s + bx) * s + cx) * s;
  const sampleY = (s: number) => ((ay * s + by) * s + cy) * s;
  const slopeX = (s: number) => (3 * ax * s + 2 * bx) * s + cx;
  return (t: number) => {
    if (t <= 0) return 0;
    if (t >= 1) return 1;
    // Newton first (fast, usually converges in a few steps), bisection if
    // the slope flattens out.
    let s = t;
    for (let i = 0; i < 8; i++) {
      const err = sampleX(s) - t;
      if (Math.abs(err) < 1e-6) return sampleY(s);
      const d = slopeX(s);
      if (Math.abs(d) < 1e-6) break;
      s -= err / d;
    }
    let lo = 0;
    let hi = 1;
    s = t;
    for (let i = 0; i < 30; i++) {
      const x = sampleX(s);
      if (Math.abs(x - t) < 1e-6) break;
      if (x < t) lo = s;
      else hi = s;
      s = (lo + hi) / 2;
    }
    return sampleY(s);
  };
}

const lerp = (a: number, b: number, t: number) => a + (b - a) * t;

/** The entrance: where a bar was when the canvas took over (`from`, read off
 * the DOM dot) blended toward where the music puts it now (`to`). Geometry
 * and opacity ride separate progress values because the DOM ran them on
 * different durations (size 220ms, opacity 140ms). */
export function blendCapsule(from: Capsule, to: Capsule, tGeom: number, tAlpha: number): Capsule {
  return {
    cx: lerp(from.cx, to.cx, tGeom),
    cy: lerp(from.cy, to.cy, tGeom),
    w: lerp(from.w, to.w, tGeom),
    h: lerp(from.h, to.h, tGeom),
    a: lerp(from.a, to.a, tAlpha),
  };
}

/** Match the canvas backing store to its CSS box at this DPR. Returns true
 * when it was resized (which also clears it). */
export function fitCanvas(canvas: HTMLCanvasElement, cssW: number, cssH: number, dpr: number): boolean {
  const w = Math.max(1, Math.round(cssW * dpr));
  const h = Math.max(1, Math.round(cssH * dpr));
  if (canvas.width === w && canvas.height === h) return false;
  canvas.width = w;
  canvas.height = h;
  return true;
}

/** Clear and draw the capsules. A capsule is never shorter than it is wide
 * (it bottoms out as a circle, like the DOM dot). */
export function drawCapsules(
  g: CanvasRenderingContext2D,
  caps: readonly Capsule[],
  color: string,
  dpr: number,
): void {
  g.setTransform(1, 0, 0, 1, 0, 0);
  g.clearRect(0, 0, g.canvas.width, g.canvas.height);
  g.setTransform(dpr, 0, 0, dpr, 0, 0);
  g.fillStyle = color;
  for (const c of caps) {
    if (c.a <= 0.001 || c.w <= 0) continue;
    const h = Math.max(c.h, c.w);
    g.globalAlpha = Math.min(1, c.a);
    g.beginPath();
    g.roundRect(c.cx - c.w / 2, c.cy - h / 2, c.w, h, c.w / 2);
    g.fill();
  }
  g.globalAlpha = 1;
}
