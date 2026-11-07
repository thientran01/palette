/** Statuses returned by spotify.rs play_now / start_playback / play_context. */
export type PlayNowStatus =
  | "ok"
  | "queued"
  | "partial"
  | "no_device"
  | "busy"
  | "diverged"
  | "gone"
  | "offline"
  | "disconnected"
  | "forbidden";

/**
 * Human copy for spotify_play_now status strings. Shared by Search's footer
 * note and the queue toast — one map so a new backend status cannot land in
 * one surface and stay silent in the other.
 *
 * Returns null for "ok" (the music changing is the confirmation).
 */
export function playNowNote(status: string): string | null {
  switch (status as PlayNowStatus) {
    case "ok":
      return null;
    case "queued":
      return "Added to Spotify — couldn’t confirm play now";
    case "partial":
      return "Played — some items couldn't re-queue";
    case "no_device":
      return "Open Spotify somewhere first";
    case "busy":
      return "Still landing the last jump";
    case "diverged":
    case "gone":
      return "Queue moved on — try again";
    case "offline":
    case "disconnected":
      return "Spotify unreachable";
    case "forbidden":
      // play_context's 403: Spotify's answer for non-Premium playback and
      // for restricted Connect devices.
      return "Spotify refused · playback needs Premium";
    default: {
      const _unknown: string = status;
      void _unknown;
      return null;
    }
  }
}
