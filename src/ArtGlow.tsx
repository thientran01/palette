/*
 * Audio-reactive halo behind the album art — the app's only reactive accent
 * surface. Apple-waveform-style lobes: plump round-capped capsules emanate
 * from the cover's edges, each bouncing on its own spectrum bin. One tiny
 * canvas at half-resolution backing store (the bilinear upscale is the
 * softener — no filter passes), redrawn at ~30fps from envelope-smoothed
 * band energies. The art itself never moves; only light does.
 */
import { useEffect, useRef } from "react";
import { SPECTRUM_BINS, type AudioBands } from "./lib/backend";
import { Envelope, expressive, subscribeBands } from "./lib/reactive";

export type GlowPlacement = "around" | "bottom" | "sides" | "topbottom";
export const GLOW_PLACEMENTS: readonly GlowPlacement[] = ["around", "bottom", "sides", "topbottom"];
export const LOBES_MIN = 2;
export const LOBES_MAX = 10;

// Keep in sync with --accent in index.css — only hit if the computed read fails.
const FALLBACK_ACCENT = "232 122 90";
/** Stop drawing once every envelope has decayed below this. */
const IDLE_EPS = 0.004;

interface Scene {
  ctx: CanvasRenderingContext2D;
  c: number; // canvas center, css px
  half: number; // art half-size
  radius: number; // art corner radius
  pad: number; // canvas padding beyond the art
  s: number; // size scale (1 = 72px card art)
  rgb: string;
  level: number;
  spec: number[];
}

/** Along-edge axis (ax, ay), outward normal (ox, oy), and a spectrum shift so
 * the bottom edge skews bassier than the top. */
interface Edge {
  ax: number;
  ay: number;
  ox: number;
  oy: number;
  binShift: number;
}

const BOTTOM: Edge = { ax: 1, ay: 0, ox: 0, oy: 1, binShift: 0 };
const TOP: Edge = { ax: 1, ay: 0, ox: 0, oy: -1, binShift: 4 };
const LEFT: Edge = { ax: 0, ay: 1, ox: -1, oy: 0, binShift: 2 };
const RIGHT: Edge = { ax: 0, ay: 1, ox: 1, oy: 0, binShift: 2 };

const EDGES: Record<GlowPlacement, Edge[]> = {
  around: [BOTTOM, TOP, LEFT, RIGHT],
  bottom: [BOTTOM],
  sides: [LEFT, RIGHT],
  topbottom: [TOP, BOTTOM],
};

/** Soft-edged capsules perpendicular to each edge, tallest motion in the
 * center (center lobes ride the lowest bins — the classic Apple silhouette). */
function drawLobes(sc: Scene, placement: GlowPlacement, count: number): void {
  const { ctx, c, half, radius, pad, s, rgb, spec } = sc;
  const usable = half * 2 - 2 * (radius + 2); // stay on the flat part of the edge
  const w = Math.min(12 * s, Math.max(4 * s, (usable / count) * 0.55));
  const maxLen = pad - 4;
  ctx.lineCap = "round";
  // k-th closest lobe to the edge's center gets the k-th lowest bin; the
  // equidistant mirrored pair lands on adjacent bins so it never moves in
  // robotic lockstep.
  const order = Array.from({ length: count }, (_, j) => j).sort(
    (a, b) => Math.abs(a - (count - 1) / 2) - Math.abs(b - (count - 1) / 2),
  );
  for (const edge of EDGES[placement]) {
    for (let k = 0; k < count; k++) {
      const j = order[k];
      const bin = Math.min(
        SPECTRUM_BINS - 1,
        Math.round((k * 11) / Math.max(1, count - 1)) + edge.binShift,
      );
      const e = spec[bin];
      const t = -usable / 2 + ((j + 0.5) * usable) / count;
      const len = Math.min(maxLen, (3 + e * 18) * s);
      // Base tucks 2px under the art so the near cap is hidden behind it.
      const bx = c + edge.ax * t + edge.ox * (half - 2);
      const by = c + edge.ay * t + edge.oy * (half - 2);
      const tx = bx + edge.ox * len;
      const ty = by + edge.oy * len;
      // Gentle glow falloff behind, then the capsule body.
      ctx.strokeStyle = `rgb(${rgb} / ${(0.12 * (0.3 + e * 0.7)).toFixed(3)})`;
      ctx.lineWidth = w * 2.1;
      ctx.beginPath();
      ctx.moveTo(bx, by);
      ctx.lineTo(tx, ty);
      ctx.stroke();
      ctx.strokeStyle = `rgb(${rgb} / ${(0.6 * (0.3 + e * 0.7)).toFixed(3)})`;
      ctx.lineWidth = w;
      ctx.beginPath();
      ctx.moveTo(bx, by);
      ctx.lineTo(tx, ty);
      ctx.stroke();
    }
  }
}

export function ArtGlow({
  artSize,
  pad,
  radius,
  placement,
  lobes,
}: {
  artSize: number;
  pad: number;
  /** Art corner radius in px — lobes keep off the rounded corners. */
  radius: number;
  placement: GlowPlacement;
  /** Capsules per edge. */
  lobes: number;
}) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  // Read through refs so tuning doesn't tear down the envelopes.
  const placementRef = useRef(placement);
  placementRef.current = placement;
  const lobesRef = useRef(lobes);
  lobesRef.current = lobes;

  useEffect(() => {
    // No reduced-motion gate here: under reduce, reactive.ts delivers no
    // events, so the loop never starts and the art's static resting shadow
    // carries the look — and a live settings flip re-animates without remount.
    const canvas = canvasRef.current;
    const ctx = canvas?.getContext("2d");
    if (!canvas || !ctx) return;

    const css = artSize + pad * 2;
    const half = artSize / 2;
    const c = css / 2;
    const s = artSize >= 120 ? 1.4 : 1;

    const levelEnv = new Envelope(45, 550);
    const specEnvs = Array.from({ length: SPECTRUM_BINS }, () => new Envelope(40, 500));

    let latest: AudioBands | null = null;
    let raf = 0;
    let running = false;
    let last = 0;
    let dprUsed = 0;

    const fit = () => {
      const dpr = window.devicePixelRatio || 1;
      if (dpr === dprUsed) return;
      dprUsed = dpr;
      // Half-res backing store: the upscale melts shapes into soft light.
      const px = Math.max(1, Math.round(css * dpr * 0.5));
      canvas.width = px;
      canvas.height = px;
      ctx.setTransform(px / css, 0, 0, px / css, 0, 0);
    };

    const frame = (now: number) => {
      raf = requestAnimationFrame(frame);
      if (now - last < 30) return; // ~30fps is plenty for light
      const dt = Math.min(now - last, 100);
      last = now;
      fit();

      const b = latest;
      let spec = b?.spectrum;
      if (b && (!spec || spec.length !== SPECTRUM_BINS)) {
        // Backend without fine bins — synthesize from the 3 bands.
        spec = Array.from({ length: SPECTRUM_BINS }, (_, i) => {
          const p = i / (SPECTRUM_BINS - 1);
          return p < 0.5 ? b.bass + (b.mid - b.bass) * p * 2 : b.mid + (b.high - b.mid) * (p - 0.5) * 2;
        });
      }

      const level = levelEnv.step(b?.level ?? 0, dt);
      const specNow = specEnvs.map((env, i) => env.step(spec?.[i] ?? 0, dt));

      ctx.clearRect(0, 0, css, css);
      const alpha = expressive(level);
      if (alpha >= 0.02) {
        const rgb =
          getComputedStyle(document.documentElement).getPropertyValue("--accent").trim() || FALLBACK_ACCENT;
        ctx.globalAlpha = alpha;
        drawLobes({ ctx, c, half, radius, pad, s, rgb, level, spec: specNow }, placementRef.current, lobesRef.current);
        ctx.globalAlpha = 1;
      }

      // Idle-stop once fully decayed; the next band event restarts the loop.
      const peak = Math.max(level, ...specNow);
      if (peak < IDLE_EPS && (b === null || b.level <= 0.001)) {
        running = false;
        cancelAnimationFrame(raf);
      }
    };

    const start = () => {
      if (running) return;
      running = true;
      last = performance.now();
      raf = requestAnimationFrame(frame);
    };

    // Zero payloads while idle stay idle; while running, the loop itself
    // decays to rest and stops.
    const unsub = subscribeBands((b) => {
      latest = b;
      if (b.level > 0.001) start();
    });

    return () => {
      unsub();
      running = false;
      cancelAnimationFrame(raf);
      ctx.clearRect(0, 0, css, css);
    };
  }, [artSize, pad, radius]);

  const css = artSize + pad * 2;
  return (
    <canvas
      ref={canvasRef}
      aria-hidden
      className="pointer-events-none absolute -z-10"
      style={{ left: -pad, top: -pad, width: css, height: css }}
    />
  );
}
