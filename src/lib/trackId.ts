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
 * snapshot will correct. `prev` is the song before this one and its last
 * duration (SongMemory.previous after rememberSong). Two
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

/** The song now playing and the one before it, each with the last duration
 * seen for it. isProvisionalDuration must compare against the PREVIOUS
 * song: a skip's fields land piecemeal (an album fix, then the duration), and
 * remembering only "the last payload" let the album straggler overwrite the
 * old song with the new song at the stale duration, so the lookup stopped
 * waiting and fetched with the wrong duration. */
export type SongMemory = {
  current: { id: string | null; durationMs: number } | null;
  previous: { id: string | null; durationMs: number } | null;
};

export const NO_SONG_MEMORY: SongMemory = { current: null, previous: null };

/** Folds one keyed payload into the memory: the same song refreshes its
 * duration and keeps the song before it; a new song pushes the old one back. */
export function rememberSong(memory: SongMemory, id: string | null, durationMs: number): SongMemory {
  if (memory.current !== null && memory.current.id === id) {
    return { previous: memory.previous, current: { id, durationMs } };
  }
  return { previous: memory.current, current: { id, durationMs } };
}
