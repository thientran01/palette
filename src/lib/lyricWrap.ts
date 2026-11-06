/*
 * Line-wrapping help for untimed lyric rows (Thien, 2026-11-06). Chromium
 * breaks after a hyphen, so "dreaming of you (oh-oh)" could end a row on
 * "(oh-" and strand "oh)" alone on the next. A short parenthetical ad-lib is
 * kept together as one unit; a long one could overflow the row as a single
 * unit, so it may still wrap at its spaces and only its hyphenated words are
 * held together. Pure text in, segments out: the same text always wraps the
 * same way, current line or not.
 */

/** Longest parenthetical (brackets included) kept on one row. About half a
 * compact row, so a kept unit can always fit on a row of its own. */
export const MAX_KEPT_PAREN = 24;

export interface WrapSegment {
  text: string;
  /** Render as white-space: nowrap. */
  keep: boolean;
}

const PAREN = /\([^()]*\)|（[^（）]*）/g;
const HYPHENATED = /\S*[-‐]\S*/g;

function holdHyphenated(text: string, out: WrapSegment[]): void {
  let last = 0;
  for (const m of text.matchAll(HYPHENATED)) {
    if (m.index > last) out.push({ text: text.slice(last, m.index), keep: false });
    out.push({ text: m[0], keep: true });
    last = m.index + m[0].length;
  }
  if (last < text.length) out.push({ text: text.slice(last), keep: false });
}

export function lyricWrapSegments(text: string): WrapSegment[] {
  const out: WrapSegment[] = [];
  let last = 0;
  for (const m of text.matchAll(PAREN)) {
    if (m.index > last) out.push({ text: text.slice(last, m.index), keep: false });
    if ([...m[0]].length <= MAX_KEPT_PAREN) out.push({ text: m[0], keep: true });
    else holdHyphenated(m[0], out);
    last = m.index + m[0].length;
  }
  if (last < text.length) out.push({ text: text.slice(last), keep: false });
  return out;
}

const HANGUL = /[ᄀ-ᇿ㄰-㆏가-힯]/;
const KANA_OR_HAN = /[぀-ヿ㐀-䶿一-鿿]/;

/** True for a line with Hangul and no Japanese/Chinese. Such a line gets
 * word-break: keep-all so a Korean word never splits between syllables.
 * keep-all would also stop breaks inside Japanese and Chinese, which have
 * no spaces to break at, so a line with any kana or Han keeps the default. */
export function keepHangulWords(text: string): boolean {
  return HANGUL.test(text) && !KANA_OR_HAN.test(text);
}
