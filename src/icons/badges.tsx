/*
 * Source-app brand marks — same 16 grid and 1.5 round stroke as the morphing
 * set, but STATIC (brand marks never morph; the badge crossfades on app hop)
 * and free of the 3-stroke contract, so L/Z commands are allowed here and
 * must never enter the morph table.
 */
import { STROKE_WIDTH } from "./geometry";

export type PlayerId = "spotify" | "apple_music" | "other";

const BADGE_PATHS: Record<PlayerId, string[]> = {
  // Circle omitted at 14px — a 1.5-weight ring in a 16 grid would crush the
  // waves below legibility; the three tilted arcs carry the mark alone.
  spotify: [
    "M 2.8,5.4 C 6.2,3.9 10.4,4.1 13.4,5.9",
    "M 3.6,8.4 C 6.6,7.2 9.9,7.4 12.3,8.8",
    "M 4.4,11.2 C 6.8,10.3 9.4,10.4 11.3,11.5",
  ],
  // Beamed double eighth note (♫) — distinct from the single flagged "note".
  // Heads are near-solid dots at 14px; intended, noteheads should read filled.
  apple_music: [
    "M 6.0,11.8 L 6.0,4.9 L 12.6,3.4 L 12.6,10.6",
    "M 2.8,11.8 C 2.8,9.7 6.0,9.7 6.0,11.8 C 6.0,13.9 2.8,13.9 2.8,11.8",
    "M 9.4,10.6 C 9.4,8.5 12.6,8.5 12.6,10.6 C 12.6,12.7 9.4,12.7 9.4,10.6",
  ],
  // Generic media app: app-tile squircle (same superellipse corner language
  // as the card/pill glyphs) holding a play wedge nudged +0.6 right.
  other: [
    "M 8.0,2.5 C 11.9,2.5 13.5,4.1 13.5,8.0 C 13.5,11.9 11.9,13.5 8.0,13.5 C 4.1,13.5 2.5,11.9 2.5,8.0 C 2.5,4.1 4.1,2.5 8.0,2.5",
    "M 6.7,5.7 L 10.5,8.0 L 6.7,10.3 Z",
  ],
};

export function PlayerMark({ player, size = 14 }: { player: PlayerId; size?: number }) {
  return (
    <svg
      viewBox="0 0 16 16"
      width={size}
      height={size}
      fill="none"
      stroke="currentColor"
      strokeWidth={STROKE_WIDTH}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      {BADGE_PATHS[player].map((d, i) => (
        <path key={i} d={d} />
      ))}
    </svg>
  );
}
