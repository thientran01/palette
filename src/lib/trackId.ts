import type { NowPlaying } from "../types";

/**
 * The perceived SONG identity: what a track-change slide announces, and the
 * only key the slide epoch, the slide direction and the track-fade spans
 * follow. Deliberately narrower than lyricsKeyOf, which also carries album
 * and duration because the lyrics lookup needs them.
 *
 * Duration is the field that arrives late. The media properties (title,
 * artist, album) announce themselves with an event, but the timeline is not
 * subscribed (media.rs) and lands on the next full heartbeat snapshot, about
 * 500ms later. The first payload of a skip can still carry the previous
 * track's duration (or none). Keyed on lyricsKeyOf, that straggler was a
 * second "track change" outside SLIDE_SETTLE_MS (400ms), so every slide ran
 * again on the same song: Thien's "the entrance plays, then replays, then
 * settles" (2026-11-07). Album is left out for the same reason; a straggler
 * there updates in place.
 */
export function trackIdOf(np: NowPlaying | null): string | null {
  return np && np.player !== "none" && np.title ? `${np.player}|${np.artist}|${np.title}` : null;
}

/**
 * True when a new song's payload still reports the PREVIOUS song's duration,
 * so the duration is probably the stale timeline that the next heartbeat
 * snapshot will correct. `prev` is the last song seen and its duration. Two
 * different songs with the exact same millisecond duration are rare, and
 * the cost of a wrong guess is only a short delay (see useLyrics).
 */
export function isProvisionalDuration(
  prev: { id: string | null; durationMs: number } | null,
  id: string | null,
  durationMs: number,
): boolean {
  return prev !== null && prev.id !== null && id !== null && prev.id !== id && durationMs > 0 && prev.durationMs === durationMs;
}
