/*
 * Morphing icon geometry — benji.org/morphing-icons-with-claude, generalized.
 * Every icon is exactly 3 strokes; every stroke is one path with the identical
 * command skeleton "M x,y C … C …" (7 coordinate pairs), so motion tweens any
 * icon into any other by interpolating the numbers in place — no flubber.
 * ViewBox 16, strokeWidth 1.5, round caps/joins; straight lines are degenerate
 * cubics (control points at exactly 1/3 and 2/3, so straight→straight tweens
 * stay straight at every frame).
 *
 * Stroke order IS the correspondence map: stroke i always tweens to stroke i.
 * Roles: 0 = spine (structural anchor), 1 = action (the verb), 2 = detail.
 * EXCEPTION: `prev` stores its two vertical lines in swapped order (0 = stop
 * bar, 2 = flat edge) so play/pause/next → prev tweens pair near neighbors
 * instead of crossing the canvas — do NOT re-sort it to match the others.
 *
 * Dead strokes are zero-length points (opacity 0) parked ON visible geometry
 * of their own icon, placed where the stroke should appear to bud from when
 * it's born in a morph. They must stay mounted and opacity-hidden: a
 * zero-length path with round caps paints a dot if ever visible.
 */

export type MorphName =
  | "play"
  | "pause"
  | "prev"
  | "next"
  | "seekBack"
  | "seekFwd"
  | "pill"
  | "card"
  | "lyrics"
  | "note";

export type Stroke = { d: string; o: 0 | 1 };

export const STROKE_WIDTH = 1.5;

export const ICONS: Record<MorphName, [Stroke, Stroke, Stroke]> = {
  // Outline triangle; centroid (not bbox) centered so it isn't left-heavy.
  // Stroke 0 is byte-identical to pause's left bar — the play↔pause morph
  // has a perfectly static welded anchor and the toggle is stroke 1 alone.
  play: [
    { d: "M 5.4,3.4 C 5.4,4.9 5.4,6.5 5.4,8.0 C 5.4,9.5 5.4,11.1 5.4,12.6", o: 1 },
    { d: "M 5.4,3.4 C 7.9,4.9 10.5,6.5 13.0,8.0 C 10.5,9.5 7.9,11.1 5.4,12.6", o: 1 },
    { d: "M 13.0,8.0 C 13.0,8.0 13.0,8.0 13.0,8.0 C 13.0,8.0 13.0,8.0 13.0,8.0", o: 0 },
  ],
  pause: [
    { d: "M 5.4,3.4 C 5.4,4.9 5.4,6.5 5.4,8.0 C 5.4,9.5 5.4,11.1 5.4,12.6", o: 1 },
    { d: "M 10.6,3.4 C 10.6,4.9 10.6,6.5 10.6,8.0 C 10.6,9.5 10.6,11.1 10.6,12.6", o: 1 },
    { d: "M 10.6,8.0 C 10.6,8.0 10.6,8.0 10.6,8.0 C 10.6,8.0 10.6,8.0 10.6,8.0", o: 0 },
  ],
  // 180° rotation twins with next; subordinate 7.2 height so the skips don't
  // outweigh play/pause; chevron apex welded onto the stop bar (round-cap T).
  prev: [
    { d: "M 3.8,4.4 C 3.8,5.6 3.8,6.8 3.8,8.0 C 3.8,9.2 3.8,10.4 3.8,11.6", o: 1 },
    { d: "M 12.0,4.4 C 9.3,5.6 6.5,6.8 3.8,8.0 C 6.5,9.2 9.3,10.4 12.0,11.6", o: 1 },
    { d: "M 12.0,4.4 C 12.0,5.6 12.0,6.8 12.0,8.0 C 12.0,9.2 12.0,10.4 12.0,11.6", o: 1 },
  ],
  next: [
    { d: "M 4.0,4.4 C 4.0,5.6 4.0,6.8 4.0,8.0 C 4.0,9.2 4.0,10.4 4.0,11.6", o: 1 },
    { d: "M 4.0,4.4 C 6.7,5.6 9.5,6.8 12.2,8.0 C 9.5,9.2 6.7,10.4 4.0,11.6", o: 1 },
    { d: "M 12.2,4.4 C 12.2,5.6 12.2,6.8 12.2,8.0 C 12.2,9.2 12.2,10.4 12.2,11.6", o: 1 },
  ],
  // 260° arc (center (8,8.2), r 4.7) ending at 12 o'clock where the tangent
  // is horizontal; arrowhead tip welds there, pointing along travel
  // (counterclockwise = rewind). No numeral — tooltip + time jump carry ±10s.
  seekBack: [
    { d: "M 3.4,9.0 C 4.1,13.0 9.0,14.3 11.6,11.2 C 14.2,8.1 12.0,3.5 8.0,3.5", o: 1 },
    { d: "M 10.0,2.1 C 9.3,2.6 8.7,3.0 8.0,3.5 C 8.7,4.0 9.3,4.4 10.0,4.9", o: 1 },
    { d: "M 8.0,3.5 C 8.0,3.5 8.0,3.5 8.0,3.5 C 8.0,3.5 8.0,3.5 8.0,3.5", o: 0 },
  ],
  // Exact horizontal mirror (x → 16−x) of seekBack.
  seekFwd: [
    { d: "M 12.6,9.0 C 11.9,13.0 7.0,14.3 4.4,11.2 C 1.8,8.1 4.0,3.5 8.0,3.5", o: 1 },
    { d: "M 6.0,2.1 C 6.7,2.6 7.3,3.0 8.0,3.5 C 7.3,4.0 6.7,4.4 6.0,4.9", o: 1 },
    { d: "M 8.0,3.5 C 8.0,3.5 8.0,3.5 8.0,3.5 C 8.0,3.5 8.0,3.5 8.0,3.5", o: 0 },
  ],
  // Containers are cut at their side midpoints (2.5,8)/(13.5,8) into a top
  // arch (L→R) and bottom arch (R→L). Identical cut + draw direction means
  // pill↔card is pure vertical inflation with both welds perfectly still.
  pill: [
    { d: "M 2.5,8.0 C 2.5,6.3 4.9,5.0 8.0,5.0 C 11.1,5.0 13.5,6.3 13.5,8.0", o: 1 },
    { d: "M 13.5,8.0 C 13.5,9.7 11.1,11.0 8.0,11.0 C 4.9,11.0 2.5,9.7 2.5,8.0", o: 1 },
    { d: "M 8.0,11.0 C 8.0,11.0 8.0,11.0 8.0,11.0 C 8.0,11.0 8.0,11.0 8.0,11.0", o: 0 },
  ],
  // The caption line (stroke 2) is what separates "card" from an empty frame
  // at 13px — and it is the stroke that becomes lyrics' current-line.
  card: [
    { d: "M 2.5,8.0 C 2.5,5.0 4.6,3.5 8.0,3.5 C 11.4,3.5 13.5,5.0 13.5,8.0", o: 1 },
    { d: "M 13.5,8.0 C 13.5,11.0 11.4,12.5 8.0,12.5 C 4.6,12.5 2.5,11.0 2.5,8.0", o: 1 },
    { d: "M 5.2,9.8 C 6.1,9.8 7.1,9.8 8.0,9.8 C 8.9,9.8 9.9,9.8 10.8,9.8", o: 1 },
  ],
  // Karaoke stack, not a hamburger: centered lines, ragged widths 7/10/5.4,
  // the long MIDDLE line is the current lyric. Bottom line drawn R→L to
  // match card's bottom arch direction (zero twist when the arch flattens).
  lyrics: [
    { d: "M 4.5,4.0 C 5.7,4.0 6.8,4.0 8.0,4.0 C 9.2,4.0 10.3,4.0 11.5,4.0", o: 1 },
    { d: "M 10.7,12.0 C 9.8,12.0 8.9,12.0 8.0,12.0 C 7.1,12.0 6.2,12.0 5.3,12.0", o: 1 },
    { d: "M 3.0,8.0 C 4.7,8.0 6.3,8.0 8.0,8.0 C 9.7,8.0 11.3,8.0 13.0,8.0", o: 1 },
  ],
  // Eighth note: stem = spine (maps rigidly onto play's left edge), flag =
  // action (curl opens into the apex chevron), notehead = detail (two 180°
  // cubics; melts into play's parked apex).
  note: [
    { d: "M 7.7,11.8 C 7.7,10.3 7.7,8.7 7.7,7.2 C 7.7,5.7 7.7,4.1 7.7,2.6", o: 1 },
    { d: "M 7.7,2.6 C 10.2,3.0 11.2,4.0 11.4,5.6 C 11.5,6.6 11.0,7.5 10.2,8.2", o: 1 },
    { d: "M 7.7,11.8 C 7.7,14.3 3.9,14.3 3.9,11.8 C 3.9,9.3 7.7,9.3 7.7,11.8", o: 1 },
  ],
};
