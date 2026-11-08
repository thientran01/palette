/*
 * The karaoke lyrics surface â€” lifted out of App.tsx (mechanical move,
 * 2026-07-11) so the focus window's realm can import it without importing
 * the whole widget. Everything lyric-shaped lives here: the fetch hook
 * (useLyrics + lyricsKeyOf), the scheduled line index (useLyricIndex), the
 * rows, and the panel with its auto-follow / wheel-browse / arrival-cascade
 * machinery. Two type scales: "base" (the 380px expanded view) and "focus"
 * (the fullscreen takeover) â€” same grammar, bigger clothes.
 */
import { memo, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import {
  BREAK_DOTS,
  attachWords,
  breakDotsFilled,
  currentLineIndex,
  msUntilNextDot,
  msUntilNextLine,
  parseLrc,
  wordTimedStarts,
  type LyricLine,
  type LyricWord,
} from "./lib/lrc";
import { commands, onKaraokeReady, onSyncLibraryChanged } from "./lib/backend";
import { CreditsBlock, NO_CREDITS, type Credits } from "./Credits";
import { planRoll, rollOffsetAt, singingEndMs, type RollPlan } from "./lib/creditsRoll";
import * as posClock from "./lib/posClock";
import { describeWordLead, useWordLead } from "./lib/wordLead";
import type { NowPlaying } from "./types";
import { isProvisionalDuration, trackIdOf } from "./lib/trackId";
import { driveWordRows, type WordRow } from "./lib/wordWipeDriver";
import { keepHangulWords, lyricWrapSegments } from "./lib/lyricWrap";

/** Current lyric line by SCHEDULING, not sampling: one timeout armed for the
 * next line boundary, recomputed on every kernel anchor event (seek, pause,
 * track change, accepted push). On fire the index is re-derived from the
 * clock â€” never blind-incremented â€” so throttled webview timers can only
 * delay a transition, never derail it. Transitions land with timer precision
 * instead of on a 250ms sampling grid. */
export function useLyricIndex(
  source: LyricLine[], leadMs: number, wordLeadMs: number | null = null,
): number {
  // With word wipes on, a row goes current when its first word starts to
  // paint, not only at its LRC stamp (lib/lrc.ts wordTimedStarts).
  const lines = useMemo(
    () => (wordLeadMs === null ? source : wordTimedStarts(source, wordLeadMs)),
    [source, wordLeadMs],
  );
  const [idx, setIdx] = useState(() => currentLineIndex(lines, posClock.now(), leadMs));
  useEffect(() => {
    let timer: number | undefined;
    const sync = () => {
      window.clearTimeout(timer);
      timer = undefined;
      const pos = posClock.now();
      const i = currentLineIndex(lines, pos, leadMs);
      setIdx(i); // same value â†’ React bails
      if (!posClock.isPlaying()) return; // clock frozen â€” anchor event re-arms
      const delay = msUntilNextLine(lines, i, pos, leadMs);
      if (delay === null) return; // last line â€” nothing left to schedule
      // Cap long waits and re-verify on fire, against timer throttling.
      timer = window.setTimeout(sync, Math.min(delay, 30_000));
    };
    sync();
    const unsubscribe = posClock.subscribe(sync);
    return () => {
      unsubscribe();
      window.clearTimeout(timer);
    };
  }, [lines, leadMs]);
  return idx;
}

/** Filled-dot count for the CURRENT break row â€” the same schedule-not-sample
 * pattern as useLyricIndex: one timeout armed for the next fifth-of-the-break
 * breakpoint, re-derived from the clock on fire and on every kernel anchor
 * event (seek, pause, accepted push), so a seek into the middle of a break
 * catches the dots up instantly and a pause freezes them. Inactive rows
 * (not current) schedule nothing. */
function useBreakDots(line: LyricLine, leadMs: number, active: boolean): number {
  // Lazy init like useLyricIndex â€” a row that mounts (or re-activates) mid-
  // break must paint the right count on its first frame, not flash 0 until
  // the post-paint effect corrects it.
  const [filled, setFilled] = useState(() =>
    active ? breakDotsFilled(line, posClock.now(), leadMs) : 0,
  );
  useEffect(() => {
    if (!active) {
      // Park at 0 so a LATER re-activation (a seek back into this break)
      // can't flash the previous stint's stale count for a frame.
      setFilled(0);
      return;
    }
    let timer: number | undefined;
    const sync = () => {
      window.clearTimeout(timer);
      timer = undefined;
      const pos = posClock.now();
      setFilled(breakDotsFilled(line, pos, leadMs)); // same value â†’ React bails
      if (!posClock.isPlaying()) return; // clock frozen â€” anchor event re-arms
      const delay = msUntilNextDot(line, pos, leadMs);
      if (delay === null) return; // all five lit â€” nothing left to schedule
      // Cap long waits and re-verify on fire, against timer throttling.
      timer = window.setTimeout(sync, Math.min(delay, 30_000));
    };
    sync();
    const unsubscribe = posClock.subscribe(sync);
    return () => {
      unsubscribe();
      window.clearTimeout(timer);
    };
  }, [line, leadMs, active]);
  return active ? filled : 0;
}

/** Keep word spans mounted across line changes. A single frame driver can
 * paint an upcoming onset or an overlapping tail without changing line
 * ownership, the scroll schedule, or React state at each word boundary. */
function useWordWipe(
  list: React.RefObject<HTMLDivElement | null>,
  lines: LyricLine[], currentLine: number, leadMs: number,
  enabled: boolean,
): void {
  useLayoutEffect(() => {
    if (!enabled) return;
    const root = list.current;
    if (!root) return;
    const rows: WordRow[] = [];
    for (const el of root.querySelectorAll<HTMLElement>("[data-word-row]")) {
      const index = Number(el.dataset.wordRow);
      const words = lines[index]?.words;
      if (!words?.length) continue;
      rows.push({ index, words, style: el.style, el,
        spans: Array.from(el.querySelectorAll<HTMLElement>("[data-word]"), span => span.style) });
    }
    return driveWordRows(rows, currentLine, leadMs, posClock, {
      request: cb => requestAnimationFrame(cb), cancel: id => cancelAnimationFrame(id),
    });
  }, [list, lines, currentLine, leadMs, enabled]);
}

/** Soft edge on the wipe, in px â€” a hard stop strobes at 60fps. */
const WIPE_FEATHER = "5px";
const WORD_GRADIENT = `linear-gradient(to right, var(--word-bright, currentColor) calc(var(--wipe, 0%) - 16px), var(--word-sheen, var(--word-bright, currentColor)) calc(var(--wipe, 0%) - ${WIPE_FEATHER}), currentColor calc(var(--wipe, 0%) + ${WIPE_FEATHER}))`;

export type LyricsState =
  // "none" = a definitive served miss (LRCLIB has no lyrics for this track);
  // "offline" = the fetch bailed on a transport failure (offline/DNS/timeout),
  // NOT recorded as a miss â€” a distinct, honest caption ("unavailable â€”
  // offline" vs "No synced lyrics"). lyrics.rs sets the `offline` flag.
  // EVERY terminal state stamps the track key it answers for (null = the
  // no-track resting state), so consumers can tell a fresh verdict from the
  // previous track's stale one â€” see lyricsVerdictOf.
  | { status: "loading" }
  | { status: "none" | "offline"; key: string | null }
  | { status: "synced"; lines: LyricLine[]; key: string };

/** The track identity a lyric fetch is keyed on. Consumers compare a synced
 * state's stamped key against the CURRENT track's key before rendering â€”
 * useLyrics flips to "loading" one render after a track change, and that
 * one-render gap would otherwise pair the new track's header with the old
 * track's lines (and freeze that ghost into an exit animation). */
export function lyricsKeyOf(np: NowPlaying | null): string | null {
  return np && np.player !== "none" && np.title && np.duration_ms > 0
    ? `${np.artist}|${np.title}|${np.album}|${np.duration_ms}`
    : null;
}

/** How long a lookup waits when the new song still reports the previous
 * song's duration (isProvisionalDuration). Two heartbeats: the correction
 * normally lands on the first, and a genuine equal-duration pair only pays
 * this once. */
const PROVISIONAL_DURATION_WAIT_MS = 1000;

/** Fetch + parse synced lyrics per track; "none" is a definitive miss. */
export function useLyrics(np: NowPlaying | null): LyricsState {
  const [state, setState] = useState<LyricsState>({ status: "none", key: null });
  const lastKey = useRef<string | null>(null);
  const key = lyricsKeyOf(np);
  const fetchSerial = useRef(0);
  useEffect(() => {
    let alive = true;
    const off = onSyncLibraryChanged(() => {
      if (!np || !key) return;
      const request = ++fetchSerial.current;
      const current = () => alive && request === fetchSerial.current && lastKey.current === key;
      void commands.lyrics(np.artist, np.title, np.album, np.duration_ms).then(l => {
        if (!current()) return;
        const parsed = l.synced ? parseLrc(l.synced, np.duration_ms).slice(0, 600) : [];
        const lines = l.words?.length ? attachWords(parsed, l.words) : parsed;
        setState(previous => lines.length
          ? {status: "synced", lines, key}
          : previous.status === "synced" && previous.key === key
            ? previous : {status: l.offline ? "offline" : "none", key});
      }).catch(() => {
        if (current()) setState(previous => previous.status === "synced" && previous.key === key
          ? previous : {status: "offline", key});
      });
    });
    return () => { alive = false; off(); };
  // A library event reloads the current identity, not its playback position.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);
  // The last song this hook keyed and its duration, for spotting a skip
  // whose first payload still carries the previous song's duration.
  const lastSong = useRef<{ id: string | null; durationMs: number } | null>(null);
  useEffect(() => {
    if (!key || !np) {
      lastKey.current = null;
      setState({ status: "none", key: null });
      return;
    }
    if (key === lastKey.current) return;
    lastKey.current = key;
    const id = trackIdOf(np);
    // A stale duration's lookup would miss (LRCLIB matches duration ±4s) or
    // win the race and seat lines that the corrected key then tears down:
    // the lyric body entering twice. Wait briefly for the correction (the
    // next heartbeat snapshot, ~500ms); it re-keys this effect, which
    // cancels the timer and fetches at once.
    const provisional = isProvisionalDuration(lastSong.current, id, np.duration_ms);
    lastSong.current = { id, durationMs: np.duration_ms };
    setState({ status: "loading" });
    let alive = true;
    const request = ++fetchSerial.current;
    const fetchNow = () => {
      void commands.lyrics(np.artist, np.title, np.album, np.duration_ms).then((l) => {
        if (!alive || lastKey.current !== key || request !== fetchSerial.current) return;
        const parsed = l.synced ? parseLrc(l.synced, np.duration_ms).slice(0, 600) : [];
        const lines = l.words && l.words.length > 0 ? attachWords(parsed, l.words) : parsed;
        setState(
          lines.length > 0
            ? { status: "synced", lines, key }
            : l.offline
              ? { status: "offline", key }
              : { status: "none", key },
        );
      });
    };
    const timer = provisional ? window.setTimeout(fetchNow, PROVISIONAL_DURATION_WAIT_MS) : null;
    if (timer === null) fetchNow();
    return () => {
      alive = false;
      if (timer !== null) window.clearTimeout(timer);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);
  useEffect(() => {
    return onKaraokeReady((p) => {
      const incoming = `${p.artist}|${p.title}|${p.album}|${p.duration_ms}`;
      if (incoming !== lastKey.current) return;
      setState((prev) => {
        if (prev.status !== "synced" || prev.key !== incoming) return prev;
        return { ...prev, lines: attachWords(prev.lines, p.words) };
      });
    });
  }, []);
  return state;
}

export type LyricsVerdict = "pending" | "synced" | "none" | "offline";

/** What the lyrics state says about the CURRENT track. A stale state â€” the
 * previous track's verdict, still standing during useLyrics' one-render
 * loading gap â€” reads as "pending", never as an answer: captions and surface
 * routing read this, so a stale "No synced lyrics" can't show against a new
 * track. */
export function lyricsVerdictOf(np: NowPlaying | null, lyrics: LyricsState): LyricsVerdict {
  if (lyrics.status === "loading") return "pending";
  return lyrics.key === lyricsKeyOf(np) ? lyrics.status : "pending";
}

/** How long the surface router holds its seat while a new track's verdict is
 * pending before conceding to the art fallback. Cache hits resolve in a few
 * frames and typical LRCLIB round trips land well inside it; past this, art
 * is better company for the wait than a quiet lyric seat. */
export const HOLD_GRACE_MS = 2500;

/** The verdict-gated hold: true while a track change's fetch interlude should
 * keep the surface router in its current seat â€” the interlude never moves the
 * surface; only verdicts (or this grace expiring) do. The expiry resets AT
 * RENDER TIME on a key change (React's adjust-state-during-render pattern),
 * never in an effect: an effect reset lands one render late, leaking the
 * previous pendency's `expired` into the first render after a track change â€”
 * as a bare boolean that defeated the hold on exactly the flash frame (mock
 * recorder, 2026-07-23), and as a persisted per-key stamp it defeated the
 * hold for a REPLAYED key (prev back to a slow-fetch track; AM's session
 * vanish/return) whose old expiry still matched (quick-review catch). */
export function useSeatHold(np: NowPlaying | null, verdict: LyricsVerdict): boolean {
  const key = lyricsKeyOf(np);
  const [graceKey, setGraceKey] = useState<string | null>(null);
  const [expired, setExpired] = useState(false);
  if (key !== graceKey) {
    // Synchronous re-arm for the new pendency; React re-renders before commit.
    setGraceKey(key);
    setExpired(false);
  }
  useEffect(() => {
    if (!key) return;
    const t = window.setTimeout(() => setExpired(true), HOLD_GRACE_MS);
    return () => window.clearTimeout(t);
  }, [key]);
  // key === null (no track) holds nothing â€” there's no seat to protect.
  return verdict === "pending" && key !== null && !expired;
}

/** Browsing hands control back three ways, fastest first: the "Now" chip
 * (instant), scrolling back toward the live line into the re-latch band
 * (instant), or wheel idle (this timeout â€” the fallback, not the only path,
 * so it can sit tighter than the old 3500ms). */
const BROWSE_RESUME_MS = 2000;
/** Fraction of the viewport height around the live offset that counts as
 * "back at now": a wheel tick that lands inside this band while moving
 * TOWARD the live line re-latches auto-follow immediately. Toward-only, so
 * the first tick of a browse (inside the band but moving away) still opens
 * a browse instead of being swallowed. Also the chip's visibility gate â€”
 * inside the band the live line is on screen and a chip would be noise. */
const RELATCH_BAND = 0.5;

/** Arrival cascade (index.css `.lyrics-entering`): rows radiate outward from
 * the current line. The step is per-row stagger spacing (like the settle
 * ladder's beat constants, it lives off the DUR scale deliberately â€” it's
 * spacing, not a duration); distance caps so far rows arrive together
 * instead of trailing forever. */
const CASCADE_STEP_MS = 30;
const CASCADE_CAP = 10;
/** Head start so the art view's exhale clears before rows land. */
const CASCADE_BASE_MS = 90; // DUR[1]
/** base + cap*step + row animation (200ms) + slack; also bounds the anchor
 * row's ignite sweep. Keep in sync with index.css's anchor ignite rule
 * (540ms hold + 260ms sweep = exactly this). The class is removed only AFTER every
 * row animation has finished â€” ripping it off mid-cascade would snap
 * fill-mode-held rows to full opacity in one frame. */
const ENTRANCE_DONE_MS = 800;

export type LyricsScale = "base" | "focus";

/** Per-scale clothes + physics, same grammar in two rooms. Both use uniform
 * layout rows with recession by paint only (per-line opacity and a resting
 * ink scale), so no line ever rewraps as the highlight moves. Focus is the
 * compact view scaled up to 44px with a darker fade (2026-11-07 redesign;
 * it replaced the 2026-07-12 Soundboard tiers and asymmetric mask). */
const SCALE = {
  base: {
    // 17px type (was 16px until 2026-11-08: most lines filled only a third
    // to two-thirds of the row, Thien). One step up keeps nearly every line
    // on one row; 18px and 20px were tried and Thien preferred 17. Leading
    // 1.4 keeps the row the same height as the old 16px/1.5 one, and is
    // tighter than the gap between lyrics, so a wrapped line reads as one.
    row: "px-3 py-1.5 text-[17px] leading-[1.4] tracking-[0.025em]",
    anchor: 0.4,
    anchorMaxPx: Infinity,
    // The compact view stays full: a three-line cut left most of the panel
    // empty (Thien, 2026-11-06), so every line shows and only the
    // brightness falls off with distance (±4 means "4 or further"). Sung and
    // upcoming lines fade symmetrically (Thien preferred it to a dimmer past).
    recede: { "-4": 0.25, "-3": 0.25, "-2": 0.45, "-1": 0.7, "1": 0.7, "2": 0.45, "3": 0.25, "4": 0.25 },
    // Depth of field (2026-11-07, replacing the accent bar): non-current
    // ink is drawn smaller than its layout box and goes out of focus with
    // distance, so the current line is the only sharp, full-size one. The
    // next/previous line stays sharp so you can read ahead. Paint-only, so
    // wrapping never changes.
    restScale: 0.95,
    // Lowered ~40% after Thien found the first pass too strong (2026-11-07).
    blur: { "-4": 0.7, "-3": 0.7, "-2": 0.4, "2": 0.4, "3": 0.7, "4": 0.7 },
    // Height of the surface-colored fades at the viewport's top and bottom
    // edges (the edge fades at the end of LyricsPanel).
    fade: "h-7",
    chipTop: "top-8",
    chipBottom: "bottom-8",
    // Break-row dots â€” #79's original tuning: h-8 matches the base lyric row
    // height, 6px dots on gap-1 (two-thirds the dot) read as one countdown.
    breakRow: "h-8 px-3 py-1",
    breakDot: "h-[6px] w-[6px]",
    breakGap: "gap-1",
    // The accent line left of the current lyric (the pre-redesign marker's
    // size, a hair thicker). barH is its one-text-line height in px; a
    // wrapped line's bar grows by each extra text line (placeAccentBar).
    bar: "w-[3px]",
    barH: 17,
  },
  focus: {
    // The compact view's proportions scaled up (Thien, 2026-11-07: the
    // compact lyrics "look like a spotlight"): rows ~2.25× the type size,
    // normal line height, letters slightly open instead of pulled together.
    row: "px-6 py-4 text-[44px] leading-normal tracking-[0.01em]",
    // Mid-room (0.5 of the full-height lyric box, 2026-11-06), but never
    // more than 420px down: on a tall room that left four lines above the
    // current one, and Thien wants three (2026-11-07). 420px is three
    // 102px rows plus the 72px top fade and a little air.
    anchor: 0.5,
    anchorMaxPx: 420,
    // Every upcoming line shows (a 5-line cut was tried 2026-11-06 and
    // Thien preferred the full ladder), but darker than the compact view: on
    // a whole screen the lit line should read like a theatre spotlight with
    // the rest in the dark (Thien, 2026-11-07). At most three sung lines
    // show above it (no "-4"), Thien 2026-11-07.
    recede: { "-3": 0.1, "-2": 0.2, "-1": 0.38, "1": 0.38, "2": 0.2, "3": 0.1, "4": 0.1 },
    // Blur scales with the 44px type: ±1 is soft but readable.
    restScale: 0.94,
    blur: { "-3": 2.5, "-2": 1.5, "-1": 0.6, "1": 0.6, "2": 1.5, "3": 2.5, "4": 2.5 },
    // Same lighting as the compact view: the per-line fade (recede) does
    // the dimming, and the edge fades only soften the very edges of the box
    // (2026-11-07; the old 260px ramps left most lines half-faded).
    fade: "h-[72px]",
    chipTop: "top-64",
    chipBottom: "bottom-84",
    // Break-row dots scaled up for the room â€” bigger dots on a wider gap so
    // the countdown reads at fullscreen distance (the feel knob for the
    // focus break row, 2026-07-13).
    breakRow: "px-6 py-3",
    breakDot: "h-[12px] w-[12px]",
    breakGap: "gap-2",
    bar: "w-[5px]",
    barH: 36,
  },
} as const satisfies Record<LyricsScale, unknown>;

/** Where the current line's center sits, in px from the top of a lyric
 * box `h` tall (SCALE.anchor, capped at SCALE.anchorMaxPx). */
function anchorPx(scale: LyricsScale, h: number): number {
  return Math.min(h * SCALE[scale].anchor, SCALE[scale].anchorMaxPx);
}

/** Recession (2026-11-06 redesign): the current line is bright and the
 * lines around it fade evenly with distance, sung and upcoming alike; any
 * distance SCALE.recede doesn't list is hidden. Distances
 * count SUNG rows only, so a dormant (collapsed) break row never steals the
 * "next line" slot; a current break sits between its neighbors (the line
 * before it is -1, the line after it +1). Clamped to ±4 so a line advance
 * re-renders only the rows whose distance actually changed (row memo). */
export function lineDistances(lines: LyricLine[], idx: number): number[] {
  const sungBefore: number[] = [];
  let n = 0;
  for (const l of lines) {
    sungBefore.push(n);
    if (l.end === undefined) n++;
  }
  const cur = idx < 0 ? 0 : sungBefore[idx] ?? 0;
  const between = idx < 0 || lines[idx]?.end !== undefined;
  return lines.map((_, i) => {
    let d = sungBefore[i] - cur;
    if (between && i > idx) d += 1;
    return Math.max(-4, Math.min(4, d));
  });
}

/** Opacity for a row at `distance` from the current line; 0 = hidden. While
 * the user wheel-browses every row comes back (you scrolled to READ — the
 * falloff would hide exactly what you came for) and the falloff re-settles
 * on re-latch. */
function recedeOpacity(scale: LyricsScale, distance: number, browsing: boolean): number {
  if (distance === 0 || browsing) return 1;
  const table = SCALE[scale].recede as Record<string, number | undefined>;
  return table[String(distance)] ?? 0;
}

/** Resting blur (px) for a row at `distance`; 0 = sharp. Browsing clears it
 * along with the fade. */
function recedeBlur(scale: LyricsScale, distance: number, browsing: boolean): number {
  if (distance === 0 || browsing) return 0;
  const table = SCALE[scale].blur as Record<string, number | undefined>;
  return table[String(distance)] ?? 0;
}

/** The accent line's motion. One bar sits in the lyric VIEWPORT (not the
 * scrolling list) at the current row's on-screen spot: its row center in the
 * list minus the offset the list is heading to. Once the line is anchored
 * that spot is the anchor, so the bar holds perfectly still while lines
 * slide into it, whatever the layout does (a break opening or closing just
 * re-anchors the list). When the spot itself moves (the walk down at the
 * start of a song, the last lines), it moves with the list's own
 * transition, 220ms in-out, so it stays level with its line. Height eases
 * to the line's text height (one row or wrapped). Appearing grows it from
 * its center; going into a break shrinks it away. Earlier versions lived in
 * the list or hopped on their own curve and overshot their line (Thien,
 * 2026-11-08). */
/** How long a newly shown bar tracks its row frame by frame: the list's
 * 220ms scroll plus a break's 330ms hold and 260ms fold, with slack. */
const APPEAR_GLUE_MS = 900;
type AccentBarState = { y: number | null; first: boolean; raf?: number };

function placeAccentBar(
  el: HTMLSpanElement, row: HTMLElement | null, offset: number, scale: LyricsScale,
  shown: AccentBarState, opts: { instant: boolean; delayFirstMs: number },
): void {
  const reduced = window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
  // Position (translate) and grow/shrink (scale) are separate properties so
  // an instant re-seat never cuts a grow-in short. background-color: an art
  // change retints it like every accent surface.
  const tint = "background-color 220ms var(--ease-out-tk)";
  const grow = (delay: number) =>
    `scale 220ms var(--ease-out-tk) ${delay}ms, opacity 140ms var(--ease-out-tk) ${delay}ms`;
  if (!row) {
    cancelAnimationFrame(shown.raf ?? 0);
    shown.raf = undefined;
    if (shown.y !== null) {
      el.style.transition = reduced ? "none"
        : `scale 200ms var(--ease-in-out-tk), opacity 200ms var(--ease-in-out-tk), ${tint}`;
      el.style.scale = "1 0";
      el.style.opacity = "0";
    }
    shown.y = null;
    return;
  }
  const cs = getComputedStyle(row);
  const lineH = parseFloat(cs.lineHeight) || 0;
  const content = row.offsetHeight - parseFloat(cs.paddingTop) - parseFloat(cs.paddingBottom);
  const h = Math.round(Math.max(content - lineH, 0) + SCALE[scale].barH);
  const y = Math.round(row.offsetTop + row.offsetHeight / 2 - h / 2 - offset);
  if (shown.y === null) {
    // Appear: grow from its center beside the line where the line is on
    // screen, and stay glued to it (one write per frame) while the list
    // scrolls it up and the break folds away, since that motion isn't one
    // curve the bar could share. Seating it at the target instead lit an
    // empty spot, or the next line, before the lyric got there.
    const box = el.parentElement;
    const glue = () => {
      const b = box?.getBoundingClientRect();
      const r = row.getBoundingClientRect();
      return b ? Math.round(r.top - b.top + r.height / 2 - h / 2) : y;
    };
    el.style.transition = "none";
    el.style.height = `${h}px`;
    el.style.translate = `0 ${glue()}px`;
    el.style.scale = "1 0";
    el.style.opacity = "0";
    void el.offsetHeight;
    el.style.transition = reduced ? "none" : `translate 0s, ${grow(shown.first ? opts.delayFirstMs : 0)}, ${tint}`;
    cancelAnimationFrame(shown.raf ?? 0);
    if (!reduced) {
      const until = performance.now() + APPEAR_GLUE_MS + (shown.first ? opts.delayFirstMs : 0);
      const follow = () => {
        if (!row.isConnected || shown.y === null) return;
        el.style.translate = `0 ${glue()}px`;
        shown.raf = performance.now() < until ? requestAnimationFrame(follow) : undefined;
      };
      shown.raf = requestAnimationFrame(follow);
    }
  } else if (opts.instant && shown.raf !== undefined) {
    // Still glued after appearing: the frame loop already tracks the row.
    el.style.height = `${h}px`;
    shown.y = y;
    return;
  } else {
    cancelAnimationFrame(shown.raf ?? 0);
    shown.raf = undefined;
    // A line change moves with the list's own scroll; a layout change (a
    // break folding away, a resize) or a browse re-seats it at once, since
    // the row itself has already moved there.
    el.style.transition = reduced ? "none" : opts.instant
      ? `translate 0s, height 0s, ${grow(0)}, ${tint}`
      : `translate 220ms var(--ease-in-out-tk), height 220ms var(--ease-in-out-tk), ${grow(0)}, ${tint}`;
    el.style.height = `${h}px`;
    el.style.translate = `0 ${y}px`;
  }
  el.style.scale = "1 1";
  el.style.opacity = "1";
  shown.y = y;
  shown.first = false;
}

/** One lyric line. Memoized so a line advance reconciles the two rows whose
 * `current` flipped instead of the whole list; clicks are delegated to the
 * list container via data-line, so rows carry no per-render closures. */
const LyricLineRow = memo(function LyricLineRow({
  text,
  index,
  current,
  seekable,
  cascadeDelayMs,
  anchor,
  scale,
  distance,
  browsing,
  words,
}: {
  text: string;
  index: number;
  current: boolean;
  seekable: boolean;
  /** Stable per mount â€” distance from the line that was current when the
   * panel mounted. Inert until the container carries .lyrics-entering. */
  cascadeDelayMs: number;
  /** The mount anchor: the ONLY row whose ignite sweep is held back for the
   * closing beat. Scoping the hold here (not to whatever row is current)
   * means a mid-entrance line advance ignites the new row instantly, with no
   * JS disarm and no engine-dependent delay retargeting. */
  anchor: boolean;
  scale: LyricsScale;
  /** Signed distance in sung lines from the current one, clamped to ±4
   * (lineDistances). */
  distance: number;
  browsing: boolean;
  words?: LyricWord[];
}) {
  const Tag = seekable ? "button" : "div";
  const timed = words && words.length > 0;
  // The current line never changes font-weight: a heavier weight has wider
  // glyphs, so a line that fits on one row inactive could wrap to two the
  // moment it lit up. It stands out by brightness alone (a text-stroke faux
  // bold was tried and read too heavy on Windows, Thien 2026-11-06). Timed
  // rows are font-medium in every state already.
  const tone = current ? (timed ? "text-muted/80" : "text-fg") : "text-muted/80";
  const opacity = recedeOpacity(scale, distance, browsing);
  const blur = recedeBlur(scale, distance, browsing);
  return (
    <Tag
      {...(seekable
        ? {
            type: "button" as const,
            "data-line": index,
            "aria-label": `Seek to ${text}`,
            // Out of the tab order â€” dozens of lines would otherwise sit
            // between the header and transport; keyboard seek lives on
            // the progress slider.
            tabIndex: -1,
          }
        : {})}
      data-word-row={timed ? index : undefined}
      data-current={current ? "true" : undefined}
      data-distance={distance}
      data-browsing={browsing ? "true" : undefined}
      data-blurred={blur > 0 ? "true" : undefined}
      data-cascade
      {...(anchor ? { "data-anchor": true } : {})}
      style={
        {
          "--cascade-delay": `${cascadeDelayMs}ms`,
          "--lyric-recede": opacity,
          "--lyric-rest-scale": SCALE[scale].restScale,
          "--lyric-rest-filter": blur > 0 ? `blur(${blur}px)` : "none",
          "--lyric-ignite-from": SCALE[scale].recede["1"],
        } as React.CSSProperties
      }
      className={`lyric-row relative whitespace-pre-wrap ${keepHangulWords(text) ? "break-keep" : ""} rounded-md text-left transition-colors duration-3 ease-out-tk ${SCALE[scale].row} ${timed ? "font-medium" : ""} ${tone} ${
        // A faded-out row is not a click target: no hover wash over
        // empty space, and the click goes nowhere you can't see.
        opacity === 0 ? "pointer-events-none" : seekable ? "cursor-pointer hover:bg-fg/5" : ""
      }`}
    >
      <span className="lyric-ink">
      {timed
        ? words.map((w, wi) => (
            // Every span carries the same gradient (drawn only while the
            // row is data-wiping, see index.css); useWordWipe moves only
            // its --wipe stop, so a word boundary is a style write, not
            // a re-render or a class swap.
            <span
              key={`${w.t}-${wi}`}
              data-word
              className="inline-block"
              style={{ "--word-gradient": WORD_GRADIENT } as React.CSSProperties}
            >
              {w.text}
            </span>
          ))
        : lyricWrapSegments(text).map((seg, si) =>
            seg.keep ? (
              <span key={si} className="whitespace-nowrap">
                {seg.text}
              </span>
            ) : (
              seg.text
            ),
          )}
      </span>
    </Tag>
  );
});

/** Per-dot stagger for the break-row exit: when the vocal resumes the dots
 * fade out RIGHT-TO-LEFT, rightmost first, one step apart â€” the waveform
 * settle's cadence collapsed to a single axis. Entrance is a plain
 * together-fade (the current state carries no delay). */
const BREAK_DOT_EXIT_STEP_MS = 55; // DUR-adjacent; 4Â·55 + 220 fade â‰ˆ waveform settle

/** An instrumental-break row: five dots counting down the gap, one igniting
 * per fifth of the break â€” Apple Music's idle-progress idiom in the living
 * separator's capsule vocabulary (a content event, and the ONE ambient
 * living instance per view stays the Waveform). Accent on the lit dots is
 * content feedback, like the waveform and progress fill, not chrome (the
 * dots ARE the current indicator). Filled dots keep
 * background-color in the transition so an art-change retint sweeps them
 * like every accent surface.
 *
 * The row is a COLLAPSING line, Apple's "a new line is made for the break":
 * dormant it takes no vertical space (grid-rows 0fr, -mt-1 eating the flex
 * gap the zero-height row would still claim), so you never see reserved
 * emptiness ahead of a break; when the break goes current the line opens
 * (0frâ†’1fr) and the dots fade in. On the vocal returning the dots fade out
 * right-to-left FIRST (the collapse is delayed ~330ms), then the now-empty
 * line closes. Purely decorative â€” aria-hidden, no seek target (a break has
 * no lyric to sing to). Scale-aware via SCALE[scale]. */
const BreakRow = memo(function BreakRow({
  line,
  current,
  leadMs,
  scale,
}: {
  line: LyricLine;
  current: boolean;
  leadMs: number;
  scale: LyricsScale;
}) {
  const filled = useBreakDots(line, leadMs, current);
  return (
    <div
      aria-hidden
      // grid-rows 0frâ†”1fr is a content-height glide that needs no hardcoded
      // height (works at both scales). Open is immediate; the collapse waits
      // (transition-delay) so the dots' right-to-left fade plays out in a
      // still-open row before the empty line closes. -mt-1 while collapsed
      // cancels the 4px flex-column gap a zero-height row would still take,
      // so a dormant break reads as ordinary line spacing.
      className={`grid ${
        current
          ? "mt-0 grid-rows-[1fr] [transition:grid-template-rows_260ms_var(--ease-out-tk),margin-top_260ms_var(--ease-out-tk)]"
          : "-mt-1 grid-rows-[0fr] [transition:grid-template-rows_260ms_var(--ease-out-tk),margin-top_260ms_var(--ease-out-tk)] [transition-delay:330ms]"
      }`}
    >
      <div className="overflow-hidden">
        <div className={`flex items-center ${SCALE[scale].breakRow} ${SCALE[scale].breakGap}`}>
      {Array.from({ length: BREAK_DOTS }, (_, i) => (
        <span
          key={i}
          aria-hidden
          // The exit stagger lives on the HIDDEN state, so it governs the
          // fade-OUT (current â†’ not-current): rightmost dot leaves first,
          // each BREAK_DOT_EXIT_STEP_MS later. The current state carries no
          // delay, so the fade-IN is a plain together-fade.
          style={
            current
              ? undefined
              : { transitionDelay: `${(BREAK_DOTS - 1 - i) * BREAK_DOT_EXIT_STEP_MS}ms` }
          }
          // A fill is instant at the breakpoint; the 220ms sweep + grow is
          // how "instant" stays smooth (the retint timing, EASE.out). The
          // CURRENT row's unlit dots hold at 75% (bg-muted above the 3:1
          // non-text floor) so you can read which fifth you're in. Hidden
          // dots keep bg-accent so a completed break (all five lit â€” the
          // common exit) fades out AS accent instead of snapping muted first.
          className={`rounded-full ${SCALE[scale].breakDot} [transition:background-color_220ms_var(--ease-out-tk),scale_220ms_var(--ease-out-tk),opacity_220ms_var(--ease-out-tk)] ${
            !current
              ? "scale-90 bg-accent opacity-0"
              : i < filled
                ? "scale-100 bg-accent opacity-100"
                : "scale-90 bg-muted opacity-75"
          }`}
        />
      ))}
        </div>
      </div>
    </div>
  );
});

/** True once playback passes `at` (null = never). The schedule-not-sample
 * pattern of useLyricIndex: one timeout, re-derived on every clock anchor
 * event, so a seek back before `at` flips it off at once. */
function usePastPosition(at: number | null): boolean {
  const [past, setPast] = useState(() => at !== null && posClock.now() >= at);
  useEffect(() => {
    if (at === null) {
      setPast(false);
      return;
    }
    let timer: number | undefined;
    const sync = () => {
      window.clearTimeout(timer);
      const pos = posClock.now();
      setPast(pos >= at);
      if (pos >= at || !posClock.isPlaying()) return;
      timer = window.setTimeout(sync, Math.min(at - pos, 30_000));
    };
    sync();
    const unsubscribe = posClock.subscribe(sync);
    return () => {
      unsubscribe();
      window.clearTimeout(timer);
    };
  }, [at]);
  return at !== null && past;
}

export function LyricsPanel({
  lines,
  seekable,
  leadMs,
  entrance = false,
  scale = "base",
  karaoke = true,
  credits = NO_CREDITS,
  durationMs = 0,
}: {
  lines: LyricLine[];
  seekable: boolean;
  leadMs: number;
  /** True when this mount is a real within-expanded arrival (the user was
   * watching the big-art fallback) â€” plays the anchor-outward cascade and
   * holds the anchor line's ignite sweep back as the closing beat. */
  entrance?: boolean;
  scale?: LyricsScale;
  /** Disables word wipes only; line highlighting and following stay active. */
  karaoke?: boolean;
  /** Song credits, shown after the last line (Credits.tsx). */
  credits?: Credits;
  /** Track length, for pacing the credits roll (lib/creditsRoll.ts). */
  durationMs?: number;
}) {
  // Words fire ahead of their aligned onset by the nudgeable word lead
  // (on top of the per-player line lead). A nudge shows a short caption;
  // the seed never does.
  const wordLead = useWordLead();
  const idx = useLyricIndex(lines, leadMs, karaoke ? wordLead.leadMs : null);
  const [leadCaption, setLeadCaption] = useState<string | null>(null);
  useEffect(() => {
    if (wordLead.nudges === 0) return;
    setLeadCaption(describeWordLead(wordLead.leadMs));
    const t = window.setTimeout(() => setLeadCaption(null), 1600);
    return () => window.clearTimeout(t);
  }, [wordLead.nudges, wordLead.leadMs]);
  const viewportRef = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  useWordWipe(listRef, lines, idx, leadMs + wordLead.leadMs, karaoke);
  const barRef = useRef<HTMLSpanElement>(null);
  const barShown = useRef<AccentBarState>({ y: null, first: true });
  const [autoOffset, setAutoOffset] = useState(0);
  // Wheel-scrolling pauses auto-follow; it resumes via the "Now" chip,
  // scrolling back into the re-latch band, or a short idle (see RELATCH_BAND
  // / BROWSE_RESUME_MS).
  const [manualOffset, setManualOffset] = useState<number | null>(null);
  const resumeTimer = useRef<number | undefined>(undefined);

  // The anchor line at mount â€” cascade delays radiate from it, stable across
  // re-renders so a mid-entrance line advance can't reshuffle delays. The
  // entrance ends by timeout only (after every row animation has finished);
  // the ignite hold is scoped to the anchor row via data-anchor, so nothing
  // needs disarming when the song advances mid-cascade.
  const entranceIdx = useRef(idx);
  const [entering, setEntering] = useState(entrance);
  useEffect(() => {
    if (!entering) return;
    const t = window.setTimeout(() => setEntering(false), ENTRANCE_DONE_MS);
    return () => window.clearTimeout(t);
  }, [entering]);

  // The mount anchor must not animate: the offsetTop/clientHeight reads in
  // the layout effect force a reflow at translateY(0) BEFORE the offset
  // lands, which arms the transform transition and slides the whole list
  // ~hundreds of px on mount. Transition class only after the first paint.
  // Two frames, not a plain effect: a mount inside a click (closing the
  // queue remounts focus's lyric column) flushes passive effects before the
  // first paint, which armed the transition early and slid the list down
  // from the top to the current line on every queue close.
  const [anchored, setAnchored] = useState(false);
  useEffect(() => {
    let second = 0;
    const first = requestAnimationFrame(() => {
      second = requestAnimationFrame(() => setAnchored(true));
    });
    return () => {
      cancelAnimationFrame(first);
      cancelAnimationFrame(second);
    };
  }, []);

  // Head/tail scroll room so the FIRST and LAST lines can rise to the anchor
  // band instead of jamming against the top/bottom frame with nothing past
  // them (Thien, 2026-07-12 â€” focus mode pinned the opening/closing lines to
  // the edges). Sized to the anchor: padTop lets the first line rest where
  // every other current line does; padBottom lets the last line reach it. The
  // offset clamp still forbids over-scroll, so the ends stop exactly at the
  // anchor. The compact view takes only the tail room: a song starts with
  // the first line at the top and the highlight walks down to the anchor
  // (Thien, 2026-11-06: a centered first line left the top of the panel
  // empty), and at the end the list keeps scrolling so the last line stays
  // at the anchor instead of sinking to the bottom. Re-measured on resize
  // (the fullscreen room can change monitors). */
  const [pad, setPad] = useState<{ top: number; bottom: number }>({ top: 16, bottom: 16 });
  useLayoutEffect(() => {
    const viewport = viewportRef.current;
    if (!viewport) return;
    const measure = () => {
      const h = viewport.clientHeight;
      const anchor = anchorPx(scale, h);
      setPad({ top: scale === "focus" ? Math.round(anchor) : 16, bottom: Math.round(h - anchor) });
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(viewport);
    return () => ro.disconnect();
  }, [scale]);

  const maxOffset = (): number => {
    const viewport = viewportRef.current;
    const list = listRef.current;
    if (!viewport || !list) return 0;
    return Math.max(list.scrollHeight - viewport.clientHeight, 0);
  };

  // Anchor the current line SCALE.anchor down the viewport via a
  // compositor-friendly translate. Rounded â€” fractional offsets put text off
  // the pixel grid.
  const autoTarget = (): number | null => {
    const viewport = viewportRef.current;
    const list = listRef.current;
    if (!viewport || !list) return null;
    const line = list.children[Math.max(idx, 0)] as HTMLElement | undefined;
    if (!line) return null;
    const anchor = anchorPx(scale, viewport.clientHeight);
    const raw = idx < 0 ? 0 : line.offsetTop + line.offsetHeight / 2 - anchor;
    return Math.round(Math.min(Math.max(raw, 0), maxOffset()));
  };
  const applyAnchor = (layoutOnly = false) => {
    const target = autoTarget();
    if (target !== null) setAutoOffset(target);
    placeBar(target, layoutOnly);
  };
  // The accent line (placeAccentBar) follows the offset the list is heading
  // to: the browse offset while wheel-browsing (instant, like the list), else
  // the auto target. Hidden on a break, before the first line and during
  // the credits roll. On a real arrival it grows in as the closing beat,
  // with the anchor row's ignite (index.css: 540ms hold).
  const placeBar = (target: number | null, instant: boolean) => {
    const bar = barRef.current;
    const list = listRef.current;
    if (!bar || !list) return;
    const sung = idx >= 0 && lines[idx] !== undefined && lines[idx].end === undefined && !rolling;
    const row = sung ? (list.children[idx] as HTMLElement | undefined) ?? null : null;
    const offset = manualOffset ?? target ?? autoOffset;
    placeAccentBar(bar, row, offset, scale, barShown.current,
      { instant: instant || manualOffset !== null, delayFirstMs: entrance ? 540 : 0 });
  };
  const applyAnchorRef = useRef(applyAnchor);
  applyAnchorRef.current = applyAnchor;

  // eslint-disable-next-line react-hooks/exhaustive-deps
  useLayoutEffect(() => applyAnchorRef.current(), [idx, lines, scale, pad.top, pad.bottom]);

  // Re-anchor on any list-height change too, not just line advance. A break
  // row opens/closes over 260ms (grid-rows glide); anchoring only at the
  // advance would measure that mid-animation â€” scrollHeight short by the
  // break's height â€” and never recompute, stranding the break's dots
  // off-screen for its whole duration (worst at the outro, where the break
  // is the last row). The observer keeps autoOffset glued to the settling
  // layout; manualOffset still wins the displayed offset while browsing.
  useEffect(() => {
    const list = listRef.current;
    if (!list) return;
    const ro = new ResizeObserver(() => applyAnchorRef.current(true));
    ro.observe(list);
    return () => ro.disconnect();
  }, []);

  useEffect(() => () => window.clearTimeout(resumeTimer.current), []);

  // Track change swaps `lines` â€” drop any in-progress browse so the new
  // track doesn't render scrolled to the old track's arbitrary offset.
  useEffect(() => {
    setManualOffset(null);
    window.clearTimeout(resumeTimer.current);
  }, [lines]);

  const browsing = manualOffset !== null;
  const distances = lineDistances(lines, idx);
  const offset = manualOffset ?? autoOffset;

  // Credits roll: once the singing is over, playback position drives the
  // list's transform instead of the line anchor. The roll itself is ONE
  // linear CSS transition to the end of the credits, so the compositor
  // animates it at full frame rate (per-frame whole-pixel writes looked
  // stepped at 14-40 px/s: Thien, 2026-11-07). It is re-planned only on a
  // clock anchor event (pause, seek, resume) or a resize. A wheel browse
  // pauses it.
  const creditsRef = useRef<HTMLDivElement>(null);
  const singingEnd = useMemo(() => singingEndMs(lines), [lines]);
  const hasCredits = credits.groups.length > 0;
  const rolling = usePastPosition(hasCredits ? singingEnd : null) && !browsing;
  // Re-seat the accent line when a browse moves the list (instantly, like
  // the list) or the credits roll takes over (hidden).
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useLayoutEffect(() => applyAnchorRef.current(), [manualOffset, rolling]);
  const rollPlan = useRef<RollPlan | null>(null);
  useLayoutEffect(() => {
    const list = listRef.current;
    const viewport = viewportRef.current;
    const box = creditsRef.current;
    if (!rolling || singingEnd === null || !list || !viewport || !box) return;
    let startTimer: number | undefined;
    let running = false; // a linear roll transition is in flight
    let parkedAt: number | null = null; // parked at the credits' top, here
    const shownY = (): number => {
      const t = getComputedStyle(list).transform;
      return t && t !== "none" ? -new DOMMatrixReadOnly(t).m42 : 0;
    };
    const plan = (): RollPlan => {
      // offsetTop ignores the transform. The edges clear the mask fades.
      const h = viewport.clientHeight;
      const top = scale === "focus" ? Math.min(240, h * 0.26) : 28;
      const bottom = scale === "focus" ? Math.min(320, h * 0.34) : 28;
      const max = maxOffset();
      const from = Math.min(Math.max(box.offsetTop - top, 0), max);
      const to = Math.min(Math.max(box.offsetTop + box.offsetHeight + bottom - h, from), max);
      return planRoll(singingEnd, durationMs, from, to);
    };
    const sync = (arrive: boolean) => {
      window.clearTimeout(startTimer);
      const p = plan();
      rollPlan.current = p;
      const pos = posClock.now();
      const y = rollOffsetAt(p, pos);
      const playing = posClock.isPlaying();
      // A player's routine position correction lands within a few px of
      // where the roll already is: re-pinning for it would read as a hitch.
      if (running && playing && pos >= p.start && Math.abs(shownY() - y) < 8) return;
      running = false;
      if (pos < p.start) {
        // Parked at the credits' top: arrive on the house 220ms glide, then
        // start the roll when its time comes. Re-syncs while parked in the
        // same spot (the observer's first call) must not cut the glide.
        if (parkedAt === null || Math.abs(parkedAt - y) >= 1) {
          list.style.transition = arrive || parkedAt === null
            ? "transform var(--transition-duration-4, 220ms) var(--ease-in-out-tk)"
            : "none";
          list.style.transform = `translateY(${-y}px)`;
          parkedAt = y;
        }
        if (playing) startTimer = window.setTimeout(() => sync(false), p.start - pos);
        return;
      }
      parkedAt = null;
      // Pin the current spot with no transition, then (when playing) hand the
      // remaining distance to one linear transition.
      list.style.transition = "none";
      list.style.transform = `translateY(${-y}px)`;
      if (!playing || y >= p.to) return;
      void list.offsetHeight; // commit the pinned spot before transitioning
      list.style.transition = `transform ${Math.round((p.to - y) / p.speed)}ms linear`;
      list.style.transform = `translateY(${-p.to}px)`;
      running = true;
    };
    sync(true);
    const unsubscribe = posClock.subscribe(() => sync(false));
    const ro = new ResizeObserver(() => sync(false));
    ro.observe(viewport);
    ro.observe(box);
    return () => {
      unsubscribe();
      ro.disconnect();
      window.clearTimeout(startTimer);
      list.style.transition = "";
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rolling, singingEnd, durationMs, scale, credits]);
  /** Where the roll is right now (for a wheel browse to start from). */
  const rollNow = (): number =>
    rollPlan.current ? Math.round(rollOffsetAt(rollPlan.current, posClock.now())) : autoOffset;
  const band = (viewportRef.current?.clientHeight ?? 0) * RELATCH_BAND;
  // Where "now" sits relative to the browse â€” drives the chip's edge/arrow.
  const nowBelow = browsing && autoOffset > (manualOffset as number);
  const showReturn = browsing && Math.abs((manualOffset as number) - autoOffset) > band;

  const relatch = () => {
    setManualOffset(null);
    window.clearTimeout(resumeTimer.current);
  };

  const onWheel = (e: React.WheelEvent) => {
    const prev = manualOffset ?? (rolling ? rollNow() : autoOffset);
    const next = Math.round(Math.min(Math.max(prev + e.deltaY, 0), maxOffset()));
    // Magnetic re-latch: arriving inside the band while closing on the live
    // line IS the resume gesture â€” no idle wait. (When not browsing, prev is
    // autoOffset and the distance can only grow, so this never traps the
    // opening tick.)
    if (Math.abs(next - autoOffset) < Math.abs(prev - autoOffset) && Math.abs(next - autoOffset) <= band) {
      relatch();
      return;
    }
    setManualOffset(next);
    window.clearTimeout(resumeTimer.current);
    resumeTimer.current = window.setTimeout(() => setManualOffset(null), BROWSE_RESUME_MS);
  };

  return (
    <div
      ref={viewportRef}
      onWheel={onWheel}
      // Its own text plane (index.css): the blurred and karaoke rows get
      // layers of their own, and this keeps them from pulling the header
      // and the sharp rows out of ClearType.
      className="text-plane relative min-h-0 flex-1 overflow-hidden"
      aria-label="Lyrics"
    >
      <div
        ref={listRef}
        onClick={(e) => {
          if (!seekable) return;
          const row = (e.target as HTMLElement).closest("[data-line]");
          const line = row ? lines[Number(row.getAttribute("data-line"))] : undefined;
          if (!line) return;
          // Land the lead ahead of the line start: the highlight maps position
          // p to the line whose t â‰¤ p + lead, so seeking to t itself flips the
          // highlight to the successor whenever the gap to it is under the
          // lead. [t - lead, next.t - lead) is the only interval guaranteed to
          // highlight the clicked line â€” and the runway into the vocal is what
          // a click-to-sing-along wants anyway. (The 0-clamp voids the
          // guarantee for lines inside the first lead-width of the track â€”
          // there is no earlier position to seek to; a neighboring intro line
          // may highlight instead.)
          commands.seekAbs(Math.max(line.t - leadMs, 0));
          relatch(); // hand control back to auto-follow
        }}
        className={`flex flex-col gap-1 ${
          browsing || !anchored ? "" : "[transition:transform_220ms_var(--ease-in-out-tk)]"
        } ${entering ? "lyrics-entering" : ""}`}
        style={{
          // While rolling, the roll effect owns the transform.
          transform: rolling ? undefined : `translateY(${-offset}px)`,
          paddingTop: pad.top,
          paddingBottom: pad.bottom,
        }}
      >
        {lines.map((line, i) => {
          const anchor = i === Math.max(entranceIdx.current, 0);
          const cascadeDelayMs =
            CASCADE_BASE_MS +
            Math.min(Math.abs(i - Math.max(entranceIdx.current, 0)), CASCADE_CAP) * CASCADE_STEP_MS;
          // Synthesized instrumental-break rows (line.end set) render the
          // five-dot countdown; sung lines render as text.
          return line.end !== undefined ? (
            <BreakRow key={`${line.t}-${i}`} line={line} current={i === idx} leadMs={leadMs} scale={scale} />
          ) : (
            <LyricLineRow
              key={`${line.t}-${i}`}
              text={line.text}
              index={i}
              current={i === idx}
              seekable={seekable}
              scale={scale}
              distance={distances[i]}
              browsing={browsing}
              anchor={anchor}
              cascadeDelayMs={cascadeDelayMs}
              words={karaoke ? line.words : undefined}
            />
          );
        })}
        <div ref={creditsRef}>
          <CreditsBlock credits={credits} scale={scale} />
        </div>
      </div>
      {/* The accent line (placeAccentBar), in viewport coordinates. The
       * album accent, like the waveform and progress fill, never on the
       * words. */}
      <span
        ref={barRef}
        aria-hidden
        data-bar
        className={`pointer-events-none absolute left-0 top-0 rounded-full bg-accent opacity-0 ${SCALE[scale].bar}`}
      />
      {/* Edge fades: surface-colored gradients painted OVER the lines, not
       * a mask-image on the viewport. A mask (like will-change on the list,
       * or any filter) makes Chromium draw the lines grayscale, and on
       * Windows that is the soft, slightly blurry text Thien noticed
       * (2026-11-08). Text under an overlay keeps ClearType. The lyrics
       * always sit on bg-surface, so this looks the same as the old mask. */}
      <div
        aria-hidden
        className={`pointer-events-none absolute inset-x-0 top-0 bg-linear-to-b from-surface to-surface/0 ${SCALE[scale].fade}`}
      />
      <div
        aria-hidden
        className={`pointer-events-none absolute inset-x-0 bottom-0 bg-linear-to-t from-surface to-surface/0 ${SCALE[scale].fade}`}
      />
      {/* Word-lead caption: the one piece of feedback the nudge hotkeys
       * give. Same chip grammar AND the same per-scale top seat as the
       * return-to-now button (the focus room's ladder starts far lower).
       * Move to the opposite scale seat while the upper return is visible. */}
      <div
        aria-live="polite"
        className={`pointer-events-none absolute inset-x-0 z-10 flex justify-center ${showReturn && !nowBelow ? SCALE[scale].chipBottom : SCALE[scale].chipTop}`}
      >
        {karaoke && leadCaption && (
          <span className="rounded-full border border-border/10 bg-surface-2/90 px-2.5 py-1 text-[11px] leading-none text-muted [animation:caption-in_140ms_var(--ease-out-tk)_both]">
            {leadCaption}
          </span>
        )}
      </div>
      {/* Return-to-now chip â€” neutral chrome, on the edge the live line
       * sits past, outside the re-latch
       * band only (inside it the line is on screen and a wheel-back
       * re-latches anyway). 32px offsets clear the 28px mask fade. Exits
       * plain with the browse, per the house transition rule. */}
      {showReturn && (
        <button
          type="button"
          aria-label="Now â€” back to the current line"
          onClick={relatch}
          className={`absolute left-1/2 z-10 flex -translate-x-1/2 items-center rounded-full border border-border/10 bg-surface-2/90 p-1.5 leading-none text-muted [transition:color_140ms_var(--ease-out-tk),scale_90ms_var(--ease-out-tk)] [animation:caption-in_140ms_var(--ease-out-tk)_both] hover:text-fg active:scale-95 ${
            nowBelow ? SCALE[scale].chipBottom : SCALE[scale].chipTop
          }`}
        >
          <svg
            width="9"
            height="9"
            viewBox="0 0 10 10"
            fill="none"
            aria-hidden
            className={nowBelow ? "" : "rotate-180"}
          >
            <path d="M2 3.5 5 6.5 8 3.5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
        </button>
      )}
    </div>
  );
}
