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
