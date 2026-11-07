/*
 * Crisp small covers. The widget shows a ~300px cover (Spotify's GSMTC
 * thumbnail) in a 26–52px box, a 6–12x shrink. WebView2 does that shrink at
 * paint time with mipmapped bilinear filtering, which averages the cover into
 * mush at these sizes: faces and type go soft (Thien, 2026-11-07: "looks
 * really low quality in the tiny box").
 *
 * Fix: resample the cover ONCE to the box's exact device-pixel size
 * (css size × devicePixelRatio) with a Lanczos-3 filter, and show that
 * bitmap. It lands 1:1 on the screen's pixels, so the browser has nothing
 * left to filter. Lanczos is the filter photo tools use for downscaling: it
 * keeps edges without the aliasing of a nearest/bilinear shrink and without
 * an added sharpen pass.
 *
 * Upscales are left alone (the original url is returned): resampling can't
 * add detail the source doesn't have, and the browser's own upscale filter
 * is fine.
 */
import { useEffect, useState, useSyncExternalStore } from "react";

const LOBES = 3;

function lanczos(x: number): number {
  if (x === 0) return 1;
  if (x <= -LOBES || x >= LOBES) return 0;
  const px = Math.PI * x;
  return (LOBES * Math.sin(px) * Math.sin(px / LOBES)) / (px * px);
}

/** Per output pixel: the first source index and its normalized weights. The
 * kernel widens by the shrink factor so every source pixel contributes. */
function weights(srcLen: number, outLen: number): { start: Int32Array; w: Float32Array[] } {
  const scale = srcLen / outLen;
  const support = LOBES * scale;
  const start = new Int32Array(outLen);
  const w: Float32Array[] = [];
  for (let o = 0; o < outLen; o++) {
    const center = (o + 0.5) * scale - 0.5;
    const lo = Math.max(0, Math.ceil(center - support));
    const hi = Math.min(srcLen - 1, Math.floor(center + support));
    const row = new Float32Array(hi - lo + 1);
    let sum = 0;
    for (let i = lo; i <= hi; i++) {
      const v = lanczos((i - center) / scale);
      row[i - lo] = v;
      sum += v;
    }
    for (let i = 0; i < row.length; i++) row[i] /= sum;
    start[o] = lo;
    w.push(row);
  }
  return { start, w };
}

/**
 * Lanczos-3 resample of an RGBA buffer (`srcW`×`srcH`) down to `outW`×`outH`.
 * Separable (rows, then columns) and alpha-premultiplied, so a transparent
 * edge doesn't bleed dark fringes. Pure, for tests.
 */
export function resampleRGBA(
  src: Uint8ClampedArray,
  srcW: number,
  srcH: number,
  outW: number,
  outH: number,
): Uint8ClampedArray {
  const xs = weights(srcW, outW);
  const ys = weights(srcH, outH);
  // Horizontal pass: srcH rows × outW, premultiplied floats.
  const mid = new Float32Array(srcH * outW * 4);
  for (let y = 0; y < srcH; y++) {
    const rowOff = y * srcW;
    for (let x = 0; x < outW; x++) {
      const row = xs.w[x];
      const s0 = xs.start[x];
      let r = 0;
      let g = 0;
      let b = 0;
      let a = 0;
      for (let k = 0; k < row.length; k++) {
        const p = (rowOff + s0 + k) * 4;
        const al = src[p + 3] / 255;
        const wk = row[k];
        r += src[p] * al * wk;
        g += src[p + 1] * al * wk;
        b += src[p + 2] * al * wk;
        a += src[p + 3] * wk;
      }
      const m = (y * outW + x) * 4;
      mid[m] = r;
      mid[m + 1] = g;
      mid[m + 2] = b;
      mid[m + 3] = a;
    }
  }
  // Vertical pass, then un-premultiply into bytes (the clamped array rounds
  // and clamps the Lanczos lobes' small overshoot).
  const out = new Uint8ClampedArray(outW * outH * 4);
  for (let y = 0; y < outH; y++) {
    const col = ys.w[y];
    const s0 = ys.start[y];
    for (let x = 0; x < outW; x++) {
      let r = 0;
      let g = 0;
      let b = 0;
      let a = 0;
      for (let k = 0; k < col.length; k++) {
        const m = ((s0 + k) * outW + x) * 4;
        const wk = col[k];
        r += mid[m] * wk;
        g += mid[m + 1] * wk;
        b += mid[m + 2] * wk;
        a += mid[m + 3] * wk;
      }
      const o = (y * outW + x) * 4;
      const al = a / 255;
      out[o] = al > 0 ? r / al : 0;
      out[o + 1] = al > 0 ? g / al : 0;
      out[o + 2] = al > 0 ? b / al : 0;
      out[o + 3] = a;
    }
  }
  return out;
}

/**
 * Center-square crop of a decoded image, resampled to `side`×`side` on a
 * canvas. Null when the image is no bigger than `side` (an upscale — leave
 * it to the browser) or its pixels can't be read (a cross-origin image
 * without CORS taints the canvas).
 */
export function crispCanvas(img: HTMLImageElement, side: number): HTMLCanvasElement | null {
  const w = img.naturalWidth;
  const h = img.naturalHeight;
  const s = Math.min(w, h);
  if (s <= side || side <= 0) return null;
  try {
    const src = document.createElement("canvas");
    src.width = s;
    src.height = s;
    const sctx = src.getContext("2d");
    if (!sctx) return null;
    sctx.drawImage(img, (w - s) / 2, (h - s) / 2, s, s, 0, 0, s, s);
    const px = sctx.getImageData(0, 0, s, s).data;
    const out = document.createElement("canvas");
    out.width = side;
    out.height = side;
    const octx = out.getContext("2d");
    if (!octx) return null;
    octx.putImageData(new ImageData(resampleRGBA(px, s, s, side, side), side, side), 0, 0);
    return out;
  } catch {
    return null;
  }
}

/** Decode `url` into an image element. Remote covers (Spotify's CDN sends
 * `Access-Control-Allow-Origin: *`) load in CORS mode so the canvas can read
 * them back. */
export function loadImage(url: string): Promise<HTMLImageElement | null> {
  return new Promise((resolve) => {
    const img = new Image();
    if (!url.startsWith("data:")) img.crossOrigin = "anonymous";
    img.onload = () => resolve(img);
    img.onerror = () => resolve(null);
    img.src = url;
  });
}

// ---- cache: one resample per (cover, device size) ----

/** Resolved results: the crisp data URL, or the original url when there's
 * nothing to gain (upscale) or the read failed. Bounded: the oldest result
 * is dropped first. */
const done = new Map<string, string>();
const pending = new Map<string, Promise<string>>();
const MAX_ENTRIES = 160;

function keyOf(url: string, side: number): string {
  return `${side}|${url}`;
}

function remember(key: string, value: string): void {
  done.delete(key);
  done.set(key, value);
  while (done.size > MAX_ENTRIES) {
    const oldest = done.keys().next().value;
    if (oldest === undefined) break;
    done.delete(oldest);
  }
}

function crisp(url: string, side: number): Promise<string> {
  const key = keyOf(url, side);
  const inflight = pending.get(key);
  if (inflight) return inflight;
  const p = loadImage(url)
    .then((img) => {
      const c = img ? crispCanvas(img, side) : null;
      // PNG: lossless, so the resample's edges survive the encode.
      return c ? c.toDataURL("image/png") : url;
    })
    .catch(() => url)
    .then((result) => {
      pending.delete(key);
      remember(key, result);
      return result;
    });
  pending.set(key, p);
  return p;
}

/** The screen's device-pixel ratio, live: it changes when the window moves to
 * a monitor with a different Windows scale setting. One shared listener for
 * every cover on the page (Search can show a hundred rows). */
const dprListeners = new Set<() => void>();
let dprQuery: MediaQueryList | null = null;

function armDprQuery(): void {
  // A resolution query only fires when LEAVING the given ratio, so it is
  // re-armed at the new ratio each time it fires.
  dprQuery?.removeEventListener("change", onDprChange);
  dprQuery = window.matchMedia(`(resolution: ${window.devicePixelRatio || 1}dppx)`);
  dprQuery.addEventListener("change", onDprChange);
}

function onDprChange(): void {
  armDprQuery();
  for (const l of dprListeners) l();
}

function subscribeDpr(listener: () => void): () => void {
  if (dprListeners.size === 0) armDprQuery();
  dprListeners.add(listener);
  return () => {
    dprListeners.delete(listener);
    if (dprListeners.size === 0) {
      dprQuery?.removeEventListener("change", onDprChange);
      dprQuery = null;
    }
  };
}

export function useDevicePixelRatio(): number {
  return useSyncExternalStore(subscribeDpr, () => window.devicePixelRatio || 1);
}

/**
 * The cover to put in a `cssSize`-px square box: a copy resampled to the
 * box's device pixels once it's ready, the original url until then (first
 * sight of a cover) or when resampling can't help.
 *
 * While a NEW local cover (a GSMTC data: URL) resamples — a few ms — the
 * previous crisp cover holds instead of flashing the soft original, so Art's
 * crossfade runs once, straight to the crisp image. A remote cover waits on
 * the network, so it shows the original right away rather than holding a
 * different song's cover.
 */
export function useCrispArt(url: string | null, cssSize: number): string | null {
  const dpr = useDevicePixelRatio();
  const side = Math.round(cssSize * dpr);
  const key = url ? keyOf(url, side) : null;
  const hit = key ? done.get(key) : undefined;
  const [shown, setShown] = useState<{ key: string; src: string } | null>(null);
  useEffect(() => {
    if (!url || !key || hit !== undefined) return;
    let alive = true;
    void crisp(url, side).then((src) => {
      if (alive) setShown({ key, src });
    });
    return () => {
      alive = false;
    };
  }, [url, side, key, hit]);
  if (!url || !key) return null;
  if (hit !== undefined) return hit;
  if (shown?.key === key) return shown.src;
  return shown && url.startsWith("data:") ? shown.src : url;
}
