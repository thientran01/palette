/*
 * Song credits under the last lyric line (Thien, 2026-11-06: the lyrics end
 * where the highlight is, then the credits). Data comes from Genius through
 * credits.rs, which caches each answer on disk, so a replayed song costs no
 * network. The lookup starts when the track does, so the credits are ready
 * long before the last line.
 *
 * Quiet by design: a song with no confident match, no token, or no network
 * shows nothing at all. Neutral chrome only, per the design rules: muted
 * section titles and roles, fg names, no accent, nothing animated.
 */
import { useEffect, useMemo, useRef, useState } from "react";
import { groupCredits, type CreditGroup } from "./lib/credits";
import { commands, onSettingsChanged, type CreditRow } from "./lib/backend";
import type { NowPlaying } from "./types";
import type { LyricsScale } from "./LyricsPanel";

function creditsKeyOf(np: NowPlaying | null): string | null {
  return np && np.player !== "none" && np.title ? `${np.artist}|${np.title}` : null;
}

/** Credits for the CURRENT track, grouped for display, or [] (loading,
 * none, no token). Each answer is stamped with the key it was fetched for,
 * so a slow answer for the previous song can never show under the new one. */
export interface Credits {
  groups: CreditGroup[];
  /** The Genius song page, for the source link. */
  url: string | null;
}

export const NO_CREDITS: Credits = { groups: [], url: null };

export function useCredits(np: NowPlaying | null): Credits {
  const key = creditsKeyOf(np);
  const [state, setState] = useState<{ key: string | null; rows: CreditRow[]; url: string | null }>({
    key: null,
    rows: [],
    url: null,
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
    void commands
      .songCredits(np.artist, np.title)
      .then((c) => {
        if (request === serial.current) setState({ key, rows: c.rows, url: c.url });
      })
      .catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, tokenEpoch]);
  const fresh = state.key === key;
  const rows = fresh ? state.rows : NO_ROWS;
  const url = fresh ? state.url : null;
  const artist = np?.artist ?? "";
  return useMemo(() => ({ groups: groupCredits(rows, artist), url }), [rows, artist, url]);
}

const NO_ROWS: CreditRow[] = [];

const CLOTHES = {
  base: {
    box: "px-3 pb-3 pt-8",
    rule: "mb-5 w-8",
    section: "mb-5",
    title: "mb-2 text-[10.5px] tracking-[0.08em]",
    lines: "gap-y-2.5",
    roles: "text-[11.5px] leading-4",
    names: "text-[13.5px] leading-[19px]",
    source: "text-[11px]",
  },
  focus: {
    box: "px-6 pb-6 pt-14",
    rule: "mb-8 w-12",
    section: "mb-8",
    title: "mb-3 text-[13px] tracking-[0.1em]",
    lines: "gap-y-4",
    roles: "text-[15px] leading-[22px]",
    names: "text-[22px] leading-[30px]",
    source: "text-[14px]",
  },
} as const satisfies Record<LyricsScale, unknown>;

/** The credits block, rendered as the lyric list's last child: a short
 * hairline, then Production / Songwriting / Performers / Engineering. Each
 * line is a role set (muted) over the people who share it. No data-line, so
 * a click on it never seeks. */
export function CreditsBlock({ credits, scale = "base" }: { credits: Credits; scale?: LyricsScale }) {
  const { groups, url } = credits;
  if (groups.length === 0) return null;
  const c = CLOTHES[scale];
  return (
    <section aria-label="Song credits" className={`select-text text-left ${c.box}`}>
      <div aria-hidden className={`h-px bg-fg/15 ${c.rule}`} />
      {groups.map((g) => (
        <div key={g.title} className={c.section}>
          <h3 className={`font-medium uppercase text-muted ${c.title}`}>{g.title}</h3>
          <dl className={`grid ${c.lines}`}>
            {g.lines.map((l) => (
              <div key={l.roles.join("|")} className="min-w-0">
                <dt className={`text-muted ${c.roles}`}>{l.roles.join(" · ")}</dt>
                <dd className={`break-words font-medium text-fg/90 ${c.names}`}>
                  {l.names.map((n, i) => (
                    <span key={`${n.main}|${n.aside ?? ""}`}>
                      {n.main}
                      {n.aside && <span className="font-normal text-muted"> {n.aside}</span>}
                      {i < l.names.length - 1 && ", "}
                    </span>
                  ))}
                </dd>
              </div>
            ))}
          </dl>
        </div>
      ))}
      {url ? (
        <button
          type="button"
          onClick={() => commands.openLink(url)}
          className={`inline-flex items-center gap-1 text-muted underline decoration-fg/20 underline-offset-[3px] [transition:color_var(--transition-duration-2)_var(--ease-out-tk)] hover:text-fg ${c.source}`}
        >
          Credits from Genius
          <svg width="9" height="9" viewBox="0 0 10 10" fill="none" aria-hidden>
            <path d="M3 7 7 3M3.6 3H7v3.4" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
        </button>
      ) : (
        <p className={`text-muted ${c.source}`}>Credits from Genius</p>
      )}
    </section>
  );
}
