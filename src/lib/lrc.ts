import type { NowPlaying } from "../types";

export interface LyricWord {
  t: number;
  text: string;
  end?: number;
  /** Acoustic spelling checkpoints within the original word span. */
  points?: { t: number; fraction: number }[];
  /** Source-timed vocal phrase, not an acoustically measured word. */
  timing?: "phrase";
  /** Stamp of the LRC line this word belongs to (align.rs). Attachment
   * keys on this when present — a word placed before its own stamp must
   * not land on the previous row. */
  line_t?: number;
}

export interface LyricLine {
  /** Line start in ms. */
  t: number;
  text: string;
  /** Present on synthesized instrumental-break rows only: the ms the break
   * ends (the next line's start, or track end for the outro). The row
   * renders as the five-dot countdown instead of text. */
  end?: number;
  words?: LyricWord[];
  /** Display fallback only; saved acoustic timing is never rewritten. */
  alignmentFallback?: "missing" | "invalid" | "late-entry";
  /** Repeated-oh backing phrase spans the source line, not its printed tail. */
  backingPhrase?: { textStart: number; t: number; end: number };
}

/** A marked gap must run at least this long to earn a break row — short
 * fills (and every unmarked gap) keep the previous line current. Five
 * dots over 7s = 1.4s/dot, about the shortest cadence that still reads
 * as a countdown. */
const BREAK_MIN_MS = 7_000;
/** Dots per break row — the living separator's capsule count. */
export const BREAK_DOTS = 5;

/**
 * Parse LRC text into sorted timed lines with instrumental-break rows
 * synthesized where the singer has actually stopped. Handles multiple
 * timestamps per line (`[00:12.30][01:02.00]chorus`). Empty timestamped
 * lines (`[01:22.75] `) are uploader-marked vocal-end points — they pin a
 * break's exact start. Gaps without a marker keep the previous line
 * current: LRC has starts only, and a guessed sung hold was inventing
 * rest rows mid-phrase. An intro before the first line still
 * synthesizes — there is no previous lyric. A marked outro (empty stamp
 * after the last line) still synthesizes to track duration.
 */
export function parseLrc(lrc: string, durationMs: number): LyricLine[] {
  const lines: LyricLine[] = [];
  const markers: number[] = [];
  for (const raw of lrc.split(/\r?\n/)) {
    const stamps = [...raw.matchAll(/\[(\d+):(\d+(?:\.\d+)?)\]/g)];
    if (stamps.length === 0) continue;
    const text = raw.replace(/\[\d+:\d+(?:\.\d+)?\]/g, "").trim();
    for (const m of stamps) {
      const t = Math.round((Number(m[1]) * 60 + Number(m[2])) * 1000);
      if (text) lines.push({ t, text });
      else markers.push(t);
    }
  }
  lines.sort((a, b) => a.t - b.t);
  markers.sort((a, b) => a - b);
  // Pathological-upload guard: the caller caps the OUTPUT at 600 rows, but
  // synthesis cost is input-shaped (markers.find per line) — bound it here
  // so a thousands-of-stamps file can't stall the main thread first.
  lines.length = Math.min(lines.length, 600);
  markers.length = Math.min(markers.length, 600);
  return withBreaks(lines, markers, durationMs);
}

function withBreaks(lines: LyricLine[], markers: number[], durationMs: number): LyricLine[] {
  if (lines.length === 0) return lines;
  const out: LyricLine[] = [];
  // Intro: no previous lyric to keep current — the widget otherwise sits
  // with nothing highlighted until the first vocal (idx -1).
  if (lines[0].t >= BREAK_MIN_MS) out.push({ t: 0, text: "", end: lines[0].t });
  for (let i = 0; i < lines.length; i++) {
    // Mid-track and outro need an uploader empty-timestamp pin. LRC only
    // has line STARTS; guessing a ≤5s sung hold invented a rest row
    // mid-phrase whenever the next stamp was ≥12s away (a 13s vocal
    // hold). Short unmarked gaps already kept the previous line current;
    // long unmarked gaps now do the same. The outro's endpoint is the
    // track duration; a mismatched upload whose timeline overruns it
    // just skips the break (negative span below).
    const nextT = i + 1 < lines.length ? lines[i + 1].t : durationMs;
    const marker = markers.find((m) => m > lines[i].t && m < nextT);
    const phraseEnd = marker ?? nextT;
    // MMS cannot reliably time the sustained repeated vowel in standalone
    // "Oh-oh" phrases (Let Me In opening). Use the source phrase interval;
    // never reinterpret mixed lyrics/ad-libs or fabricate syllable onsets.
    // One hyphenated token preserves the existing span/wrapping structure.
    const phrase = /^\(?oh(?:-oh)+[.!?,]?\)?$/i.test(lines[i].text)
      && (i + 1 < lines.length || marker !== undefined)
      && Number.isFinite(phraseEnd) && phraseEnd > lines[i].t;
    const backing = /\(oh(?:[\s,-]+oh)+[.!?]?\)$/i.exec(lines[i].text);
    const hasEndpoint = (i + 1 < lines.length || marker !== undefined)
      && Number.isFinite(phraseEnd) && phraseEnd > lines[i].t;
    const line = backing && backing.index > 0 && lines[i].text.slice(0, backing.index).trim()
      && hasEndpoint ? { ...lines[i], backingPhrase: {
        textStart: backing.index, t: lines[i].t, end: phraseEnd,
      } } : lines[i];
    out.push(phrase ? { ...line, words: [{
      t: lines[i].t, text: lines[i].text, end: phraseEnd,
      line_t: lines[i].t, timing: "phrase",
    }] } : line);
    if (marker !== undefined && nextT - marker >= BREAK_MIN_MS) {
      out.push({ t: marker, text: "", end: nextT });
    }
  }
  return out;
}

/** Filled dots (0..BREAK_DOTS) for a break row at `positionMs`. The first
 * dot lights the moment the break starts (the countdown announces itself)
 * and each further dot at another fifth — the fifth lands at 80%, so the
 * full ladder has a beat on screen before the vocal returns. `leadMs` is
 * the same VOCAL_LEAD table value currentLineIndex uses — threaded as a
 * parameter so the dots and the row's `current` flag can never disagree
 * (dot 1 lands exactly when the row goes current, the handoff exactly when
 * it stops). */
export function breakDotsFilled(line: LyricLine, positionMs: number, leadMs: number): number {
  if (line.end === undefined) return 0;
  const p = positionMs + leadMs;
  const seg = (line.end - line.t) / BREAK_DOTS;
  if (seg <= 0) return BREAK_DOTS;
  return Math.min(Math.max(Math.floor((p - line.t) / seg) + 1, 0), BREAK_DOTS);
}

/** Ms of 1x playback until the next dot fills, or null when all are lit.
 * Shares breakDotsFilled's math so the scheduler and the render can never
 * disagree about a breakpoint (the msUntilNextLine discipline). */
export function msUntilNextDot(line: LyricLine, positionMs: number, leadMs: number): number | null {
  if (line.end === undefined) return null;
  const filled = breakDotsFilled(line, positionMs, leadMs);
  if (filled >= BREAK_DOTS) return null;
  const seg = (line.end - line.t) / BREAK_DOTS;
  return Math.max(line.t + filled * seg - leadMs - positionMs, 0);
}

/** The highlight leads the vocal by this much — SMTC positions lag what the
 * user actually hears, by a per-player amount: Apple Music floors its pushed
 * positions to whole seconds (up to ~1s behind the audio), Spotify pushes
 * ms-precise pairs. Values are pre-soak guesses — tune during the live soak.
 * One table, threaded as a parameter to both functions below, so the index
 * search and the boundary scheduler can never disagree about where a line
 * starts. */
export const VOCAL_LEAD_MS: Record<NowPlaying["player"], number> = {
  // The #22 soak showed the AM clock rides ~0.5–1s HOT (freeze-at-max +
  // 2000ms band ratchet the display above the floored reports) — that ride
  // already IS the lead. Start at 0; measure live before adding any.
  apple_music: 0,
  spotify: 50, // soak-tune: ms-precise timeline, lead only covers render lag
  other: 250,
  none: 250, // unreachable (no lyrics without a session) — table completeness
};

/** Index of the line active at `positionMs` (-1 before the first line). */
export function currentLineIndex(lines: LyricLine[], positionMs: number, leadMs: number): number {
  const p = positionMs + leadMs;
  let lo = 0;
  let hi = lines.length - 1;
  let ans = -1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (lines[mid].t <= p) {
      ans = mid;
      lo = mid + 1;
    } else {
      hi = mid - 1;
    }
  }
  return ans;
}

/** Ms of 1x playback until the line after `idx` becomes current, or null when
 * `idx` is the last line (nothing left to schedule). Guaranteed positive when
 * `idx === currentLineIndex(lines, positionMs)` — a position at the boundary
 * already belongs to the next line. */
export function msUntilNextLine(
  lines: LyricLine[],
  idx: number,
  positionMs: number,
  leadMs: number,
): number | null {
  const next = lines[idx + 1];
  if (!next) return null;
  return Math.max(next.t - leadMs - positionMs, 0);
}

/** Keep existing word-sized spans (and wrapping) while mapping the backing
 * phrase's text sweep onto the source interval. These are display fractions,
 * not estimates of each sung syllable. Fail closed if source text differs. */
function backingWords(line: LyricLine, words: LyricWord[]): LyricWord[] {
  const phrase = line.backingPhrase;
  if (!phrase || words.map(w => w.text).join("") !== line.text) return words;
  const size = line.text.length - phrase.textStart;
  const time = (offset: number) => phrase.t + (phrase.end - phrase.t) * (offset - phrase.textStart) / size;
  let offset = 0;
  return words.flatMap(word => {
    const start = offset;
    offset += word.text.length;
    if (offset <= phrase.textStart) return [word];
    const cut = Math.max(phrase.textStart - start, 0);
    const backing: LyricWord = { ...word, text: word.text.slice(cut),
      t: time(start + cut), end: time(offset), timing: "phrase", points: undefined };
    return cut ? [{ ...word, text: word.text.slice(0, cut), points: undefined }, backing] : [backing];
  });
}

/** A source-window disagreement is a guardrail, not model confidence.
 * Only severe late entrances are rejected. Early vocals, short calls, long
 * instrumental gaps and overlapping/parenthesized voices stay acoustic. */
function alignmentFallback(line: LyricLine, nextT: number, words: LyricWord[]): LyricLine["alignmentFallback"] {
  if (!words.length) return "missing";
  if (words.some(w => !Number.isFinite(w.t) ||
    (w.end !== undefined && (!Number.isFinite(w.end) || w.end < w.t)))) return "invalid";
  const window = nextT - line.t;
  if (!Number.isFinite(line.t) || !Number.isFinite(window) || window <= 0 || window > 20_000 ||
    /[()（）]/u.test(line.text) || line.backingPhrase) return undefined;
  const first = Math.min(...words.map(w => w.t));
  return first - line.t > Math.max(1500, window * 0.75) ? "late-entry" : undefined;
}

export function attachWords(lines: LyricLine[], words: LyricWord[]): LyricLine[] {
  if (words.length === 0) return lines;
  // Explicit line ownership preserves source order even with overlapping voices.
  const source = words.every(w => w.line_t !== undefined)
    ? words : words.slice().sort((a, b) => a.t - b.t);
  // Words that name their line attach by stamp; older payloads without
  // line_t fall back to the time window (and can misfile a word that sits
  // before its stamp — the reason line_t exists).
  return lines.map((line, i) => {
    if (line.end !== undefined || line.words?.[0]?.timing === "phrase") return line;
    const nextT = i + 1 < lines.length ? lines[i + 1].t : Number.POSITIVE_INFINITY;
    const mine = source.filter((w) =>
      w.line_t !== undefined ? w.line_t === line.t : w.t >= line.t && w.t < nextT,
    );
    const fallback = alignmentFallback(line, nextT, mine);
    return fallback ? { ...line, words: undefined, alignmentFallback: fallback }
      : { ...line, words: backingWords(line, mine), alignmentFallback: undefined };
  });
}

export function currentWordIndex(words: LyricWord[], positionMs: number, leadMs: number): number {
  const p = positionMs + leadMs;
  let lo = 0;
  let hi = words.length - 1;
  let ans = -1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (words[mid].t <= p) {
      ans = mid;
      lo = mid + 1;
    } else {
      hi = mid - 1;
    }
  }
  return ans;
}

/** Short tokens and Hangul blocks retain the onset attack (DUR[1]).
 * Longer English spellings use acoustic checkpoints or measured duration. */
export const WORD_ATTACK_MS = 90;

export function wordWipe(
  words: LyricWord[],
  positionMs: number,
  leadMs: number,
): { index: number; frac: number } | null {
  if (words.length === 0) return null;
  const i = currentWordIndex(words, positionMs, leadMs);
  if (i < 0) return null;
  return { index: i, frac: wordWipeFraction(words[i], positionMs, leadMs, words[i + 1]?.t) };
}

function validWordPoints(w: LyricWord, end: number) {
  const points = w.points;
  return w.timing !== "phrase" && points && points.length >= 2 &&
    points[0].t === w.t && points[0].fraction === 0 &&
    points[points.length - 1].t === end && points[points.length - 1].fraction === 1 &&
    points.every((point, i) => Number.isFinite(point.t) && Number.isFinite(point.fraction) &&
      point.fraction >= 0 && point.fraction <= 1 &&
      (i === 0 || (point.t >= points[i - 1].t && point.fraction >= points[i - 1].fraction)))
    ? points : undefined;
}

function sustainedWord(w: LyricWord): boolean {
  return w.timing === "phrase" || (/^[\x00-\x7f]*$/.test(w.text) &&
    /^[^a-z]*[a-z][a-z'-]{3,}[^a-z]*$/i.test(w.text));
}

/** Song-clock instant at which this word's displayed fill reaches 100%. */
export function wordFillEnd(w: LyricWord, nextT?: number): number {
  const end = w.end ?? nextT;
  if (end === undefined) return w.t;
  const points = validWordPoints(w, end);
  if (points) return points.find(point => point.fraction === 1)!.t;
  const span = Math.max(end - w.t, 1);
  return w.t + (sustainedWord(w) ? span : Math.min(WORD_ATTACK_MS, span));
}

/** Each span owns its progress: simultaneous voices cannot share a cursor. */
export function wordWipeFraction(w: LyricWord, positionMs: number, leadMs: number, nextT?: number): number {
  if (positionMs + leadMs < w.t) return 0;
  const end = w.end ?? nextT;
  if (end === undefined) return 1;
  const span = Math.max(end - w.t, 1);
  const points = validWordPoints(w, end);
  if (points) {
    const p = positionMs + leadMs;
    let a = points[0];
    for (let i = 1; i < points.length; i++) {
      // CTC often emits a letter for one frame, then blanks during its hold.
      // Fill that letter through the hold, up to the next letter's onset.
      // Keep the first 100% checkpoint so completion bloom never moves later.
      while (points[i].fraction < 1 && i + 1 < points.length &&
        points[i + 1].fraction === points[i].fraction) i++;
      const b = points[i];
      if (p < b.t) {
        const u = Math.max(0, (p - a.t) / Math.max(b.t - a.t, 1));
        // A gentle lag gives held letters weight without adding clock delay.
        // The curve keeps moving throughout and meets the next onset exactly.
        const eased = u - 0.12 * Math.sin(Math.PI * u);
        return a.fraction + (b.fraction - a.fraction) * eased;
      }
      a = b;
    }
    return 1;
  }
  // Legacy caches have no inner timing. Longer English words use their
  // measured duration; never manufacture phonetic syllable boundaries.
  const sustained = sustainedWord(w);
  const attack = sustained ? span : Math.min(WORD_ATTACK_MS, span);
  const p = positionMs + leadMs;
  const u = Math.min(Math.max((p - w.t) / attack, 0), 1);
  return sustained ? u : 1 - (1 - u) ** 3;
}
