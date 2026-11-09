/*
 * Full-size cover for Focus. GSMTC hands Palette Spotify's ~300px thumbnail,
 * and Focus shows the cover in a box about 560px wide at 1440p, so it was
 * upscaled almost 2x and looked soft (Thien, 2026-11-07). When Spotify is
 * connected, the per-track enrichment read also carries the album's 640px
 * image (spotify.rs NowCover), and Focus uses that instead.
 *
 * The enrichment read lands a beat after a skip and can still describe the
 * previous track, so the big cover is used only while its title and artist
 * match what GSMTC says is playing. It is also used only once it has loaded:
 * until then, and if it fails, the GSMTC cover stays.
 *
 * Title and artist can't tell two releases of one song apart (the single
 * picked from Search while the album version played), so that case is the
 * backend's: it drops the cover on a release switch and re-reads it for the
 * new album (spotify.rs enrich_now). Before that, Focus kept the first
 * release's cover for the whole listen (Thien, 2026-11-09).
 */
import { useEffect, useState } from "react";
import { onSpotifyNowCover } from "./backend";
import type { NowCover, NowPlaying } from "../types";

const norm = (s: string) => s.trim().toLowerCase();

/** The full-size cover url when `cover` belongs to the track `np` is playing
 * on Spotify, else null. GSMTC and the Web API can list artists differently
 * (one vs all, joined), so artists only need to overlap. Pure, for tests. */
export function matchingCover(np: NowPlaying | null, cover: NowCover | null): string | null {
  if (!np || !cover || np.player !== "spotify") return null;
  if (norm(np.title) !== norm(cover.title)) return null;
  const a = norm(np.artist);
  const b = norm(cover.artist);
  if (a && b && !a.includes(b) && !b.includes(a)) return null;
  return cover.url;
}

/** The cover Focus should show: Spotify's full-size image once it has loaded
 * for the playing track, `fallback` (the GSMTC cover) otherwise. */
export function useFullCover(np: NowPlaying | null, fallback: string | null): string | null {
  const [cover, setCover] = useState<NowCover | null>(null);
  useEffect(() => onSpotifyNowCover(setCover), []);
  const want = matchingCover(np, cover);
  const [loaded, setLoaded] = useState<string | null>(null);
  useEffect(() => {
    if (!want || want === loaded) return;
    let alive = true;
    const img = new Image();
    img.onload = () => {
      if (alive) setLoaded(want);
    };
    img.src = want;
    return () => {
      alive = false;
    };
  }, [want, loaded]);
  return want && loaded === want ? want : fallback;
}
