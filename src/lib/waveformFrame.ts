/*
 * The Waveform frame loop treats a stream that stopped as silence.
 *
 * The backend feeds audio bands only to windows the user can see (audio.rs
 * BandFanout). A hidden window keeps its last payload forever, and a loop
 * stepping toward a frozen target never reaches its idle stop, so it would
 * spin every frame for nothing. Past BANDS_STALE_MS the bars fall to the
 * floor, exactly as they do after the backend's latch zero, and the loop
 * stops. A live stream arrives every ~33ms, so it never trips this.
 */

/** No payload for this long reads as silence. Matches audio.rs's
 * SILENCE_AFTER_MS for the process-capture path. */
export const BANDS_STALE_MS = 250;

export function bandsStale(now: number, lastBandAt: number): boolean {
  return now - lastBandAt > BANDS_STALE_MS;
}

/*
 * Live-range stretch for the pill's bars.
 *
 * The backend auto-gains each spectrum bin against its own recent peak, so on
 * real songs a bin rarely leaves the middle: sustained energy sits around
 * 0.3-0.4 and the beat only nudges it. Drawn as-is, the pill's bars idled at
 * half height and moved ~0.2px a frame — dots that barely seemed to move
 * (measured on five real songs, 2026-11-20). BarRange follows each bar's own
 * recent floor and ceiling (each snaps outward at once and eases back in over
 * RANGE_MS) and maps the envelope into that span, so a bar's motion fills its
 * height. RANGE_MIX keeps some of the absolute level, so a quiet section
 * still draws shorter than a loud one, and RANGE_MIN_SPAN stops a nearly
 * flat bin (a held synth pad) from blowing its noise up to full height.
 */
export const RANGE_MS = 1500;
export const RANGE_MIX = 0.75;
export const RANGE_MIN_SPAN = 0.18;

export class BarRange {
  private lo = 0;
  private hi = 0;

  step(e: number, dtMs: number): number {
    const k = 1 - Math.exp(-dtMs / RANGE_MS);
    this.lo = e < this.lo ? e : this.lo + (e - this.lo) * k;
    this.hi = e > this.hi ? e : this.hi + (e - this.hi) * k;
    const stretched = (e - this.lo) / Math.max(this.hi - this.lo, RANGE_MIN_SPAN);
    const v = RANGE_MIX * Math.min(1, Math.max(0, stretched)) + (1 - RANGE_MIX) * e;
    return Math.min(1, Math.max(0, v));
  }
}
