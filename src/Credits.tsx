/*
 * Song credits under the last lyric line (Thien, 2026-11-06: the lyrics end
 * where the highlight is, then the credits). Data comes from Genius through
 * credits.rs, which caches each answer on disk, so a replayed song costs no
 * network. The lookup starts when the track does, so the credits are ready
 * long before the last line.
 *
 * Quiet by design: a song with no confident match, no token, or no network
 * shows nothing at all. Neutral chrome only, per the design rules: muted
 * role labels, fg names, no accent.
 */
import { useEffect, useRef, useState } from "react";
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
    box: "px-3 pb-2 pt-7",
    grid: "gap-x-3 gap-y-1.5",
    role: "text-[11.5px] leading-[18px]",
    names: "text-[13px] leading-[18px]",
    source: "mt-4 text-[11px]",
  },
  focus: {
    box: "px-6 pb-4 pt-12",
    grid: "gap-x-6 gap-y-3",
    role: "text-[17px] leading-[28px]",
    names: "text-[22px] leading-[28px]",
    source: "mt-8 text-[15px]",
  },
} as const satisfies Record<LyricsScale, unknown>;

/** The credits block, rendered as the lyric list's last child. No
 * data-line, so a click on it never seeks. */
export function CreditsBlock({ rows, scale = "base" }: { rows: CreditRow[]; scale?: LyricsScale }) {
  if (rows.length === 0) return null;
  const c = CLOTHES[scale];
  return (
    <section aria-label="Song credits" className={`select-text text-left ${c.box}`}>
      <dl className={`grid grid-cols-[auto_1fr] ${c.grid}`}>
        {rows.map((r) => (
          <div key={r.role} className="contents">
            <dt className={`whitespace-nowrap text-muted ${c.role}`}>{r.role}</dt>
            <dd className={`text-fg/80 ${c.names}`}>{r.names.join(", ")}</dd>
          </div>
        ))}
      </dl>
      <p className={`text-muted ${c.source}`}>Credits from Genius</p>
    </section>
  );
}
