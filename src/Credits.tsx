/*
 * Song credits under the last lyric line (Thien, 2026-11-06: the lyrics end
 * where the highlight is, then the credits). Data comes from Genius through
 * credits.rs, which caches each answer on disk, so a replayed song costs no
 * network. The lookup starts when the track does, so the credits are ready
 * long before the last line.
 *
 * Laid out like liner notes (2026-11-07, "make the credits prettier"): a
 * "Credits" eyebrow on a hairline, then the roles in three runs (writing,
 * performance, engineering) as a two-column grid of label-over-names cells.
 * A long cell spans the row and the grid backfills around it. Genius's
 * "Hangul (Romanized)" names keep both, the romanization muted, and a
 * "Member (Group)" suffix that just repeats the artist is dropped.
 *
 * Quiet by design: a song with no confident match, no token, or no network
 * shows nothing at all. Neutral chrome only, per the design rules: muted
 * labels, fg names, no accent, nothing animates.
 */
import { Fragment, useEffect, useRef, useState } from "react";
import { commands, onSettingsChanged, type CreditRow } from "./lib/backend";
import type { NowPlaying } from "./types";
import type { LyricsScale } from "./LyricsPanel";

/** What the credits block needs for the current track. */
export interface Credits {
  rows: CreditRow[];
  /** The Genius song page, for the source link. */
  url: string | null;
  /** The track's artist, to drop "(Group)" suffixes that repeat it. */
  artist: string;
}

export const NO_CREDITS: Credits = { rows: [], url: null, artist: "" };

function creditsKeyOf(np: NowPlaying | null): string | null {
  return np && np.player !== "none" && np.title ? `${np.artist}|${np.title}` : null;
}

/** Credits for the CURRENT track, or NO_CREDITS (loading, none, no token).
 * Each answer is stamped with the key it was fetched for, so a slow answer
 * for the previous song can never show under the new one. */
export function useCredits(np: NowPlaying | null): Credits {
  const key = creditsKeyOf(np);
  const [state, setState] = useState<{ key: string | null; credits: Credits }>({
    key: null,
    credits: NO_CREDITS,
  });
  // Adding or fixing the token in Settings retries the current song.
  const [tokenEpoch, setTokenEpoch] = useState(0);
  useEffect(
    () => onSettingsChanged(({ key: k }) => {
      if (k === "genius_token") setTokenEpoch((n) => n + 1);
    }),
    [],
  );
  const serial = useRef(0);
  useEffect(() => {
    if (!key || !np) return;
    const request = ++serial.current;
    const artist = np.artist;
    void commands
      .songCredits(np.artist, np.title)
      .then((c) => {
        if (request === serial.current) setState({ key, credits: { rows: c.rows, url: c.url, artist } });
      })
      .catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, tokenEpoch]);
  return state.key === key ? state.credits : NO_CREDITS;
}

// ---------------------------------------------------------------------------
// Shaping

const WRITING = new Set(["Produced by", "Co-produced by", "Written by", "Lyrics", "Composition", "Arrangement"]);
const ENGINEERING = /record|mix|master|engineer|edit/i;

/** Rows arrive in credits.rs rank order (writing, then performance, then
 * engineering), so the runs are contiguous; this only finds the breaks. */
export function groupRows(rows: CreditRow[]): CreditRow[][] {
  const runOf = (r: CreditRow) => (WRITING.has(r.role) ? 0 : ENGINEERING.test(r.role) ? 2 : 1);
  const groups: CreditRow[][] = [[], [], []];
  for (const r of rows) groups[runOf(r)].push(r);
  return groups.filter((g) => g.length > 0);
}

const fold = (s: string) => s.toLowerCase().replace(/[^\p{L}\p{N}]+/gu, "");

/** "구종필 (JongPil Gu)" → main "구종필", aside "JongPil Gu".
 * "Sunwoo (THE BOYZ)" by THE BOYZ → main "Sunwoo", no aside. */
export function splitName(name: string, artist: string): { main: string; aside: string | null } {
  const m = /^(.+?)\s*\(([^()]+)\)$/.exec(name.trim());
  if (!m) return { main: name.trim(), aside: null };
  const aside = m[2].trim();
  const a = fold(artist);
  const f = fold(aside);
  if (a && f && (a === f || a.includes(f) || f.includes(a))) return { main: m[1], aside: null };
  return { main: m[1], aside };
}

/** Rough rendered width in Latin-character units: Hangul, kana and CJK set
 * about twice as wide as an average Latin letter. */
function textUnits(s: string): number {
  let n = 0;
  for (const ch of s) n += /[ᄀ-ᇿ぀-ヿ㄰-㆏一-鿿가-힯]/.test(ch) ? 1.8 : 1;
  return n;
}

/** A cell that would wrap at half width spans the row instead. The
 * thresholds are what one half column holds at each scale (base ≈ 170px at
 * 13px, focus ≈ 400px at 22px). */
function isWide(r: CreditRow, scale: LyricsScale): boolean {
  return textUnits(r.names.join(", ")) > (scale === "focus" ? 32 : 23);
}

// ---------------------------------------------------------------------------
// Rendering

const CLOTHES = {
  base: {
    box: "px-3 pb-3 pt-8",
    eyebrow: "mb-4 gap-3 text-[11px]",
    group: "gap-x-4 gap-y-3",
    groupGap: "mt-5",
    role: "text-[11px] leading-4",
    names: "mt-0.5 text-[13px] leading-[19px]",
    source: "mt-6 text-[11px]",
    arrow: 9,
  },
  focus: {
    box: "px-6 pb-6 pt-14",
    eyebrow: "mb-7 gap-5 text-[15px]",
    group: "gap-x-10 gap-y-6",
    groupGap: "mt-9",
    role: "text-[15px] leading-6",
    names: "mt-1 text-[22px] leading-[30px]",
    source: "mt-10 text-[15px]",
    arrow: 12,
  },
} as const satisfies Record<LyricsScale, unknown>;

function Names({ names, artist }: { names: string[]; artist: string }) {
  return (
    <>
      {names.map((n, i) => {
        const { main, aside } = splitName(n, artist);
        return (
          <Fragment key={n}>
            {i > 0 && <span className="text-muted">, </span>}
            <span className="text-fg/85">{main}</span>
            {aside && <span className="text-muted"> {aside}</span>}
          </Fragment>
        );
      })}
    </>
  );
}

/** The credits block, rendered as the lyric list's last child. No
 * data-line, so a click on it never seeks. */
export function CreditsBlock({ credits, scale = "base" }: { credits: Credits; scale?: LyricsScale }) {
  const { rows, url, artist } = credits;
  if (rows.length === 0) return null;
  const c = CLOTHES[scale];
  return (
    <section aria-label="Song credits" className={`select-text text-left ${c.box}`}>
      <div className={`flex items-center uppercase tracking-widest text-muted ${c.eyebrow}`}>
        <span>Credits</span>
        <span aria-hidden className="h-px flex-1 bg-border/10" />
      </div>
      {groupRows(rows).map((group, gi) => (
        <dl
          key={group[0].role}
          className={`grid grid-flow-row-dense grid-cols-2 ${c.group} ${gi > 0 ? c.groupGap : ""}`}
        >
          {group.map((r) => (
            <div key={r.role} className={`min-w-0 ${isWide(r, scale) ? "col-span-2" : ""}`}>
              <dt className={`text-muted ${c.role}`}>{r.role}</dt>
              <dd className={c.names}>
                <Names names={r.names} artist={artist} />
              </dd>
            </div>
          ))}
        </dl>
      ))}
      {url ? (
        <button
          type="button"
          onClick={() => commands.openLink(url)}
          className={`inline-flex items-center gap-1 text-muted [transition:color_var(--transition-duration-2)_var(--ease-out-tk)] hover:text-fg ${c.source}`}
        >
          Credits from Genius
          <ArrowOut size={c.arrow} />
        </button>
      ) : (
        <p className={`text-muted ${c.source}`}>Credits from Genius</p>
      )}
    </section>
  );
}

/** The "opens in your browser" arrow, shared with the connector links. */
export function ArrowOut({ size = 9 }: { size?: number }) {
  return (
    <svg
      viewBox="0 0 10 10"
      width={size}
      height={size}
      fill="none"
      stroke="currentColor"
      strokeWidth="1.3"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <path d="M3 7 7 3M3.5 3H7v3.5" />
    </svg>
  );
}
