/**
 * Rough written syllables for Latin-script lyric words, so the karaoke
 * rise can move one sound at a time (letterRise.ts). A Hangul block or a
 * kana is already one sung syllable; an English word is not: "strength"
 * is eight letters and one sound, "away" is two sounds, and the "e" in
 * "love" or the comma in "away," make no sound at all. Thien
 * (2026-11-10): English "feels like some parts get skipped or goes over
 * too fast or goes too slowly".
 *
 * A spelling heuristic, not a dictionary: vowel groups are nuclei; a
 * silent "e" (final, or before a suffix like "-ly", "-less") folds into
 * the syllable before it, as do "-es"/"-ed" where they add no sound; the
 * consonants between two nuclei split the usual way (one consonant starts
 * the next syllable; of two, the first ends this one, unless the pair is
 * one sound like "th"). A miss only moves a letter into the neighbouring
 * syllable, which still rises on time.
 */

const LATIN = /\p{Script=Latin}/u;
const NON_LATIN_LETTER = /[^\P{L}\p{Script=Latin}]/u;
/** Consonant pairs that sound as one and start a syllable together. */
const ONE_SOUND = new Set(["th", "sh", "ch", "ph", "wh", "gh", "qu"]);
/** Consonant pairs that end a syllable together ("sing·er", "rock·et"). */
const END_PAIR = new Set(["ck", "ng"]);
/** Endings after which a single "e" before them is silent ("lovely"). */
const SILENT_E_SUFFIX = ["ly", "less", "ness", "ful", "ment", "thing"];

/** True for a word written only in Latin letters (English, romanized). */
export function isLatinWord(text: string): boolean {
  return LATIN.test(text) && !NON_LATIN_LETTER.test(text);
}

const base = (g: string) => g.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();
const AEIOU = (c: string | undefined) => c !== undefined && "aeiou".includes(c);

/**
 * Index of the first grapheme of each syllable after the first, over a
 * word's letter graphemes (letterRise.ts letterSegments, letters only, so
 * punctuation is included: it renders as its own span). Punctuation and
 * apostrophes stay with the syllable beside them.
 */
export function syllableStarts(letters: readonly string[]): number[] {
  // Work on the alphabetic letters only; map back to graphemes at the end.
  const idx = letters.map((_, i) => i).filter(i => /^[a-z]$/.test(base(letters[i])));
  const g = idx.map(i => base(letters[i]));
  const n = g.length;
  const vowel = g.map((c, i) => {
    if (AEIOU(c)) return true;
    // "y" is a vowel after a consonant: "my", "baby", "crying" (cry·ing),
    // but not "yeah" or "beyond".
    return c === "y" && i > 0 && !AEIOU(g[i - 1]);
  });
  const nuclei: [number, number][] = [];
  for (let i = 0; i < n; i++) {
    if (!vowel[i]) continue;
    const last = nuclei[nuclei.length - 1];
    if (last && last[1] === i - 1 && !(g[i - 1] === "y")) last[1] = i;
    else nuclei.push([i, i]);
  }
  // Silent "e": a lone "e" after vowel-consonant, at the end ("love") or
  // before a suffix ("lovely", "timeless"), and "-es"/"-ed" that add no
  // syllable ("makes", "loved", but "faces", "wanted").
  for (let k = nuclei.length - 1; k >= 1; k--) {
    const [s, e] = nuclei[k];
    if (s !== e || g[s] !== "e") continue;
    const c1 = g[s - 1];
    if (s - 1 <= nuclei[k - 1][1]) continue; // no consonant before it
    const tail = g.slice(s + 1).join("");
    const isLast = k === nuclei.length - 1;
    let silent = false;
    if (isLast && tail === "") silent = !(c1 === "l" && s - 2 > nuclei[k - 1][1]); // not "ta·ble"
    else if (isLast && tail === "d") silent = !"td".includes(c1);
    else if (isLast && tail === "s") silent = !"sxzcg".includes(c1) && !(c1 === "h" && "sc".includes(g[s - 2]));
    else if (!isLast && s - 2 === nuclei[k - 1][1]) silent = SILENT_E_SUFFIX.some(x => tail.startsWith(x));
    if (silent) nuclei.splice(k, 1);
  }
  const starts: number[] = [];
  for (let k = 1; k < nuclei.length; k++) {
    const from = nuclei[k - 1][1] + 1;
    const to = nuclei[k][0];
    const between = to - from;
    let start = to;
    if (between === 1) start = from;
    else if (between >= 2) {
      const lastTwo = g[to - 2] + g[to - 1];
      const firstTwo = g[from] + g[from + 1];
      if (between === 2 && END_PAIR.has(firstTwo)) start = to;
      else if (ONE_SOUND.has(lastTwo)) start = to - 2;
      else start = to - 1;
    }
    starts.push(idx[start]);
  }
  return starts;
}

/** The word's letter graphemes cut into syllables, for tests and tools. */
export function syllables(letters: readonly string[]): string[] {
  const cuts = [0, ...syllableStarts(letters), letters.length];
  return cuts.slice(0, -1).map((c, i) => letters.slice(c, cuts[i + 1]).join(""));
}
