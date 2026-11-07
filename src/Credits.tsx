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
import { groupCredits } from "./lib/credits";
import { commands, onSettingsChanged, type CreditRow } from "./lib/backend";
import type { NowPlaying } from "./types";
import type { LyricsScale } from "./LyricsPanel";

function creditsKeyOf(np: NowPlaying | null): string | null {
  return np && np.player !== "none" && np.title ? `${np.artist}|${np.title}` : null;
}

/** Credit rows for the CURRENT track, or [] (loading, none, no token).
 * Each answer is stamped with the key it was fetched for, so a slow answer
 * for the previous song can never show under the new one. */
export function useCredits(np: NowPlaying | null): CreditRow[] {
  const key = creditsKeyOf(np);
  const [state, setState] = useState<{ key: string | null; rows: CreditRow[] }>({ key: null, rows: [] });
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
        if (request === serial.current) setState({ key, rows: c.rows });
      })
      .catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, tokenEpoch]);
  return state.key === key ? state.rows : [];
}

const CLOTHES = {
  base: {
    box: "px-3 pb-3 pt-8",
    rule: "mb-5 w-8",
    section: "mb-5",
    title: "mb-2 text-[10.5px] tracking-[0.08em]",
    grid: "grid-cols-2 gap-x-4 gap-y-2.5",
    name: "text-[13.5px] leading-[18px]",
    roles: "text-[11.5px] leading-4",
    source: "text-[11px]",
  },
  focus: {
    box: "px-6 pb-6 pt-14",
    rule: "mb-8 w-12",
    section: "mb-8",
    title: "mb-3 text-[13px] tracking-[0.1em]",
    grid: "grid-cols-3 gap-x-8 gap-y-4",
    name: "text-[22px] leading-[28px]",
    roles: "text-[15px] leading-[22px]",
    source: "text-[14px]",
  },
} as const satisfies Record<LyricsScale, unknown>;

/** The credits block, rendered as the lyric list's last child: a short
 * hairline, then Production / Songwriting / Performers / Engineering, each
 * a grid of people with their roles underneath. No data-line, so a click on
 * it never seeks. */
export function CreditsBlock({ rows, scale = "base" }: { rows: CreditRow[]; scale?: LyricsScale }) {
  const groups = useMemo(() => groupCredits(rows), [rows]);
  if (groups.length === 0) return null;
  const c = CLOTHES[scale];
  return (
    <section aria-label="Song credits" className={`select-text text-left ${c.box}`}>
      <div aria-hidden className={`h-px bg-fg/15 ${c.rule}`} />
      {groups.map((g) => (
        <div key={g.title} className={c.section}>
          <h3 className={`font-medium uppercase text-muted ${c.title}`}>{g.title}</h3>
          <ul className={`grid ${c.grid}`}>
            {g.people.map((p) => (
              <li key={p.name} className="min-w-0 break-words">
                <p className={`font-medium text-fg/90 ${c.name}`}>{p.name}</p>
                <p className={`text-muted ${c.roles}`}>{p.roles.join(" · ")}</p>
              </li>
            ))}
          </ul>
        </div>
      ))}
      <p className={`text-muted ${c.source}`}>Credits from Genius</p>
    </section>
  );
}
