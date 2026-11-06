/*
 * The summon search (design "A1"): a Raycast-style floating search pane in
 * its OWN webview window (src-tauri/src/search.rs — created hidden at
 * setup, shown by Ctrl+Alt+S). One verb: get to a song in under a second.
 * Type → debounced Spotify search; Enter plays now (the context-preserving
 * jump; from silence it starts playback outright); Shift+Enter queues to
 * Pulse's up-next and stays open so several tracks queue in one summon.
 * Esc / blur / background click dismiss.
 *
 * The empty state answers "what do you want to hear?" in two sections: "From
 * your history" (up to four resurfacing picks over the local play log) and
 * "Something different" (up to three Last.fm-similar discoveries, gated on a
 * Last.fm key + Spotify, filled in below the history rows). Both rotate on a
 * day-block seed — stable within a sitting, fresh a few times a day, never a
 * per-summon shuffle. Default content, not a feature surface: verbs live on
 * rows, not on the chrome.
 *
 * Realm notes (this is the codebase's first second window): this module
 * runs in its own JS realm — its own backend listeners, no reactive vote
 * (this pane renders no Waveform; lib.rs ORs only inserted keys, so a
 * missing Search entry is not a true vote), no posClock (the
 * search window never renders position). Announcement suppression for a play
 * lives in the MAIN realm, armed by the backend's "spotify-jump" emit —
 * never armed from here.
 *
 * FULLY MONOCHROME (Thien, 2026-07-17): accent lives where the song is
 * VISIBLE — widget and focus room render the playing track's identity, so
 * the song's extracted color has an anchor there; this pane renders no
 * now-playing content, and a song-colored queue flash arrived with no
 * referent (tried both the resting hue and a synced song accent — both
 * read wrong). The queued-row flash is a neutral fg/20 brightening (a
 * clear pop above the fg/10 selection wash) + the "Queued ·" note; the
 * realm runs no art→accent pipeline (the --accent var here only ever
 * holds the resting value, effectively unused).
 */
import { Fragment, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { commands, onSearchSection, onSearchShown } from "./lib/backend";
import { RowThumb, useSpotifyStatus } from "./Queue";
import { playNowNote } from "./lib/playNowStatus";
import { browserStorage, createDiscoveryCache } from "./lib/discoveryCache";
import { nextSection, SECTIONS } from "./lib/searchLibrary";
import { SpotifyConnectButton } from "./SpotifyConnectButton";
import {
  BackIcon,
  EnterKeyIcon,
  HistoryIcon,
  LibraryIcon,
  PlayIcon,
  SearchIcon,
  ShiftKeyIcon,
  SongsIcon,
  TabKeyIcon,
  UpKeyIcon,
} from "./icons/searchIcons";
import {
  drillItems,
  historyItems,
  ItemTile,
  libraryItems,
  ReconnectButton,
  trackRow,
  useHistoryList,
  useHistoryThumbs,
  isPlayingTarget,
  useLibrary,
  useNowContext,
  useOpenedTracks,
  usePublicPlaylists,
  type Item,
  type OpenTarget,
  type SearchRow,
} from "./SearchSections";
import { IN_TAURI, type HistoryEntry, type QueueTrack, type SearchSection } from "./types";

const RESULT_LIMIT = 8;
/** History rows rendered at once; scrolling (or arrowing) near the end
 * renders more, then pages the log. */
const HISTORY_RENDER_STEP = 60;
/** The browser mock's starting section (`?window=search&section=library`);
 * live, every summon names its section. */
const INITIAL_SECTION: SearchSection = (() => {
  const s = IN_TAURI ? null : new URLSearchParams(window.location.search).get("section");
  return s === "library" || s === "history" ? s : "songs";
})();
const SECTION_LABEL: Record<SearchSection, string> = {
  songs: "Songs",
  library: "Library",
  history: "History",
};
const SECTION_GLYPH: Record<SearchSection, () => React.ReactNode> = {
  songs: () => <SongsIcon />,
  library: () => <LibraryIcon />,
  history: () => <HistoryIcon />,
};
const DEBOUNCE_MS = 250;
/** History pages scanned for the resurfacing picks (3 × 200 entries). */
const RESURFACE_PAGES = 3;
/** A pick must have been genuinely listened to (cumulative). */
const RESURFACE_MIN_MS = 60_000;

/** Resolve-on-demand cache for resurfaced rows (the Queue.tsx uriCache
 * pattern) — successes only: the search window lives for the whole app
 * session, and caching a transient miss would permanently dead-row a pick.
 * Actions are user-paced, so an occasional re-miss can't storm. */
const uriCache = new Map<string, string>();
async function resolveUri(row: SearchRow): Promise<string | null> {
  if (row.uri) return row.uri;
  const hit = uriCache.get(row.key);
  if (hit) return hit;
  const uri = await commands.spotifyResolveUri(row.title, row.artist);
  if (uri) uriCache.set(row.key, uri);
  return uri;
}

/** One history track, aggregated across its listens. */
interface Agg {
  key: string;
  title: string;
  artist: string;
  /** Cumulative ms listened, all time in the scanned window. */
  ms: number;
  /** Cumulative ms listened within the last 7 days (the "on repeat" signal). */
  ms7: number;
  last: number;
  uri: string | null;
}

const DAY_MS = 86_400_000;

/** The freshness clock for the empty state: three blocks a day (morning /
 * afternoon / evening, local time). Rotation is per day-PART now, not per day
 * — the picks feel curated and hold within a sitting, but the list has arcs
 * across the day. Still zero per-summon randomness (a shuffle read as a bug;
 * a dated pick reads as curation). Drives history rotation, discovery seeds,
 * and the discovery cache key — one integer explains all the freshness.
 * The day component derives from LOCAL midnight to match the local hour
 * buckets — a raw epoch-day rolls at UTC midnight (4-5pm PT), which injected
 * a phantom mid-block reshuffle (quick-review catch, 2026-07-16). */
function blockSeed(now: number): number {
  const d = new Date(now);
  const localDay = Math.floor((now - d.getTimezoneOffset() * 60_000) / DAY_MS);
  const h = d.getHours();
  return localDay * 3 + (h < 12 ? 0 : h < 18 ? 1 : 2);
}

/** The exclude-key contract shared with similar.rs's `norm_key`: a track's
 * dedupe identity is its lowercased, trimmed `artist‹U+0001›title`. Keep the
 * two in lockstep — a drift silently stops "different" from meaning un-played.
 * The separator is the ESCAPE sequence, never a literal control byte: an
 * invisible U+0001 in source fooled three review agents into reporting the
 * contract broken (quick-review, 2026-07-16), and no formatter/copy-paste
 * can be trusted to preserve it. */
function discoveryKey(artist: string, title: string): string {
  return `${artist.trim().toLowerCase()}\u0001${title.trim().toLowerCase()}`;
}

/** Up to four discovery seeds, rotated by the day-block so suggestions refresh
 * a few times a day, each from a different artist so two seeds don't pull the
 * same artist's neighborhood. The backend walks them in order and stops once
 * it has enough picks, so the later seeds only cost anything when the earlier
 * ones come up dry (a most-played seed's neighbors are often all already
 * played — similar.rs). */
const SEED_COUNT = 4;
function pickSeeds(pool: Agg[], seed: number): { title: string; artist: string }[] {
  const top = pool.slice(0, 10);
  if (!top.length) return [];
  const a = top[seed % top.length];
  const seeds = [{ title: a.title, artist: a.artist }];
  const artists = new Set([a.artist.trim().toLowerCase()]);
  const start = (seed * 7 + 3) % top.length;
  for (let i = 0; i < top.length && seeds.length < SEED_COUNT; i++) {
    const c = top[(start + i) % top.length];
    const artist = c.artist.trim().toLowerCase();
    if (artists.has(artist)) continue;
    artists.add(artist);
    seeds.push({ title: c.title, artist: c.artist });
  }
  return seeds;
}

/** Discovery rows keyed by day-block, kept across restarts (see
 * lib/discoveryCache), so a re-summon inside a block is network-free; a new
 * block misses (≤3 fetch rounds/day). Only successful ("ok") fetches are
 * cached. */
const discoveryCache = createDiscoveryCache<SearchRow>(browserStorage());

/** Wait before the one automatic retry of a failed discovery fetch. */
const DISCOVERY_RETRY_MS = 1500;

/** The quiet line a discovery fetch that came back empty-handed leaves, so
 * the section doesn't just vanish after its skeleton ("sometimes it just
 * doesn't load", Thien, 2026-11-06). Null = say nothing: the gated statuses
 * (no key, Spotify disconnected) are already explained elsewhere. */
function discoveryFailureNote(status: string | null): string | null {
  switch (status) {
    case null:
    case "offline":
    case "busy":
      return "Couldn't load suggestions. Press ↑ to try again";
    case "bad_key":
      return "Last.fm rejected the API key";
    case "no_data":
      return "No new suggestions right now";
    default:
      return null;
  }
}

/** The "From your history" section: up to four resurfacing picks over the
 * local play log, each a relative measure (the log is young; absolute
 * thresholds would answer nothing for months) with a short reason label, and
 * artist-diversified so the four don't read as one artist. Returns the picks
 * AND the aggregate pool, so the discovery seeds reuse it instead of
 * re-scanning history. Deterministic within a day-block (see blockSeed). */
async function computeResurfaced(
  now: number,
  offset = 0,
): Promise<{ rows: SearchRow[]; pool: Agg[]; played: Agg[] }> {
  const entries: HistoryEntry[] = [];
  let before: number | null = null;
  for (let i = 0; i < RESURFACE_PAGES; i++) {
    const page = await commands.historyPage(before, 200);
    entries.push(...page);
    if (page.length < 200) break;
    before = page[page.length - 1].started_at_ms;
  }
  const byKey = new Map<string, Agg>();
  for (const e of entries) {
    const cur = byKey.get(e.key) ?? {
      key: e.key,
      title: e.title,
      artist: e.artist,
      ms: 0,
      ms7: 0,
      last: 0,
      uri: null,
    };
    cur.ms += e.ms_listened;
    if (now - e.started_at_ms < 7 * DAY_MS) cur.ms7 += e.ms_listened;
    cur.last = Math.max(cur.last, e.ended_at_ms);
    cur.uri = cur.uri ?? e.spotify_uri;
    byKey.set(e.key, cur);
  }
  // Every track the log has seen (any listen, even below the pick floor) —
  // the exclude source for discovery, so "different" means genuinely un-played.
  const played = [...byKey.values()];
  const all = played.filter((t) => t.ms >= RESURFACE_MIN_MS);
  const rested = all.filter((t) => now - t.last > DAY_MS);
  const pool = (rested.length ? rested : all).slice().sort((a, b) => b.ms - a.ms);

  const picks: { agg: Agg; reason: string }[] = [];
  const taken = new Set<string>();
  // Push the FIRST candidate (from a priority-ordered list) that is neither
  // already taken nor a repeat artist — so a strategy whose top choice was
  // already claimed still contributes its next-best instead of silently
  // yielding nothing. A relaxed top-up pass below fills any remainder when
  // diversity ran the pool dry (a one-artist library still fills its rows).
  const pushFirst = (cands: Agg[], reason: string, start = 0): void => {
    const n = cands.length;
    for (let k = 0; k < n; k++) {
      const agg = cands[(start + k) % n];
      if (taken.has(agg.key)) continue;
      const artist = agg.artist.trim().toLowerCase();
      if (picks.some((p) => p.agg.artist.trim().toLowerCase() === artist)) continue;
      picks.push({ agg, reason });
      taken.add(agg.key);
      return;
    }
  };

  // Each strategy walks its ranked list from `offset`, so a pull-to-refresh
  // (offset++) advances the whole section to the next-best of each category —
  // the labels stay category-accurate, and mod-wrap means it never empties.
  // 1. The most lived-in track (pool is sorted by ms desc).
  pushFirst(pool, rested.length ? "Haven't heard in a while" : "Most played", offset);
  // 2. A prior month's same date, when one exists (usually absent).
  const today = new Date(now).getDate();
  pushFirst(
    all.filter((t) => new Date(t.last).getDate() === today && now - t.last > 25 * DAY_MS),
    "On this day",
    offset,
  );
  // 3. On repeat this week.
  pushFirst(
    all.filter((t) => t.ms7 >= RESURFACE_MIN_MS).sort((a, b) => b.ms7 - a.ms7),
    "On repeat lately",
    offset,
  );
  // 4. Heavy once, untouched for a month+.
  pushFirst(
    all.filter((t) => now - t.last > 30 * DAY_MS).sort((a, b) => b.ms - a.ms),
    "Forgotten favorite",
    offset,
  );
  // 5. A rotating deep cut from the top listens (day-block wildcard + refresh
  // offset), starting the walk at the combined offset.
  const rest = pool.filter((t) => !taken.has(t.key)).slice(0, 10);
  if (rest.length) pushFirst(rest, "Today's pick", (blockSeed(now) + offset) % rest.length);
  // Top-up: if diversity left us short of four, backfill from the pool
  // (offset-walked too) ignoring the artist constraint (one-artist library).
  for (let k = 0; k < pool.length; k++) {
    if (picks.length >= 4) break;
    const agg = pool[(offset + k) % pool.length];
    if (taken.has(agg.key)) continue;
    picks.push({ agg, reason: "Played before" });
    taken.add(agg.key);
  }

  const rows: SearchRow[] = await Promise.all(
    picks.slice(0, 4).map(async (p) => ({
      key: p.agg.key,
      title: p.agg.title,
      artist: p.agg.artist,
      artUrl: await artForHistory(p.agg),
      track: null,
      uri: p.agg.uri,
      reason: p.reason,
    })),
  );
  return { rows, pool, played };
}

/** Disk thumb first (captured while the widget was visible). Hidden listens
 * and eviction leave a null — fall back to a Spotify cover so Search does
 * not show the note glyph for a track the user has already heard. */
async function artForHistory(agg: Agg): Promise<string | null> {
  const thumb = await commands.historyThumbUrl(agg.key);
  if (thumb) return thumb;
  try {
    const res = await commands.spotifySearch(
      `track:${agg.title} artist:${agg.artist}`,
      1,
    );
    if (res.status !== "ok") return null;
    return res.tracks[0]?.art_url ?? null;
  } catch {
    return null;
  }
}

/** The queued-row confirmation: one extra wash over the row (on top of the
 * selection) that holds, then fades — opacity only (.row-flash). Mounted for
 * the flash's 900ms; keyed by flash count so a second queue replays it. */
function RowFlash() {
  return <span aria-hidden className="row-flash pointer-events-none absolute inset-0 rounded-md bg-fg/10" />;
}

/** A ghost of one result row — shown where real rows will land while a fetch
 * runs (discovery fill and first search results), so the wait fills its space
 * with the shape of the answer instead of a void. Geometry mirrors the real
 * row exactly (h-52, 32px thumb, gap-3 px-3.5, reason bar flush right → in-place
 * swap, zero shift); widths stagger so three read as content, not a grid.
 * Opacity-breathing only (.skeleton-pulse): the classic shimmer is a translate
 * sweep, and that motion vocabulary belongs to the waveform. The breathe is
 * delayed per COLUMN (thumb → text → reason), identical across rows, so the
 * crest reads as a left-to-right sweep — a per-ROW stagger read as
 * top-to-bottom (Thien, 2026-07-16). Visual-only (presentation + aria-hidden)
 * — rows landing are the AT-visible event, as before. */
const SKELETON_WIDTHS: readonly [string, string, string][] = [
  ["42%", "28%", "21%"],
  ["33%", "22%", "17%"],
  ["46%", "25%", "23%"],
  ["37%", "31%", "19%"],
  ["44%", "20%", "22%"],
];
/** The left→right feel knob: per-column delay into the 1300ms breathe.
 * Paired with the crest shape in skeleton-breathe — the two together set how
 * gently the light passes (crest widened 8/22→12/34 + delay 220→280 after
 * "too fast, cadence good" — Thien, 2026-07-16). */
const SKELETON_COL_DELAY_MS = 280;
function SkeletonRow({ index, reason = false }: { index: number; reason?: boolean }) {
  const [titleW, artistW, reasonW] = SKELETON_WIDTHS[index % SKELETON_WIDTHS.length];
  const col = (n: number) => ({ animationDelay: `${n * SKELETON_COL_DELAY_MS}ms` });
  return (
    <div role="presentation" aria-hidden className="flex h-[52px] items-center gap-3 px-3.5">
      {/* Thumb one tone brighter than the text bars — real rows are bright
          art beside muted text, and a single tone read as a flat gray grid. */}
      <span
        className="skeleton-pulse h-[32px] w-[32px] shrink-0 rounded-md bg-surface-2"
        style={col(0)}
      />
      <span className="flex min-w-0 flex-1 flex-col gap-[7px]">
        <span
          className="skeleton-pulse h-[10px] rounded-full bg-fg/5"
          style={{ width: titleW, ...col(1) }}
        />
        <span
          className="skeleton-pulse h-[8px] rounded-full bg-fg/5"
          style={{ width: artistW, ...col(1) }}
        />
      </span>
      {/* Only the discovery wait promises a reason label — search results
          never carry one, so their ghosts must not either. */}
      {reason && (
        <span
          className="skeleton-pulse h-[8px] shrink-0 rounded-full bg-fg/5"
          style={{ width: reasonW, ...col(2) }}
        />
      )}
    </div>
  );
}

/** One elevated keycap for the search-row hint. The old inline glyphs
 * ("↵ play · ⇧↵ queue") sat in the same 11px muted run as their labels and
 * read as compressed noise — you couldn't tell what to press (Thien,
 * 2026-07-16). First cut was a cream neutral-inversion fill; Thien's live
 * verdicts shaped the rest: "needs to look a bit more elevated" (Raycast's
 * footer chips as the direction) → a RAISED dark key — fill one step above
 * the bar, hairline top light + soft drop shadow for keycap depth; then
 * "the icons are too cramped" → the UNICODE arrows were the problem (font
 * glyphs go mushy at 11px), so the keys draw their own stroke SVGs in the
 * house icon language (1.5 stroke, round caps — the search glyph's
 * grammar) inside a slightly wider chip. Never accent. A one-off because
 * these are single GLYPH caps, not chord strings (Keycaps.tsx). */
function HintKey({ children }: { children: React.ReactNode }) {
  return (
    <kbd className="inline-flex h-[18px] min-w-[20px] items-center justify-center rounded-[5px] border border-border/[0.08] bg-fg/[0.09] px-[3px] not-italic text-fg/90 shadow-[0_1px_2px_rgb(0_0_0/0.35),inset_0_1px_0_rgb(var(--fg)/0.07)]">
      {children}
    </kbd>
  );
}

type HintKeyName = "enter" | "shift" | "esc";
/** The hint run for Library and History: Raycast's footer grammar, as in
 * the Songs row — verb first, then its key(s), hairlines between groups. */
function SectionHints({ verbs }: { verbs: [string, HintKeyName][] }) {
  const keys = (k: HintKeyName) =>
    k === "enter" ? (
      <HintKey>
        <EnterKeyIcon />
      </HintKey>
    ) : k === "shift" ? (
      <span className="flex items-center gap-[3px]">
        <HintKey>
          <ShiftKeyIcon />
        </HintKey>
        <HintKey>
          <EnterKeyIcon />
        </HintKey>
      </span>
    ) : (
      <HintKey>
        <span className="px-[2px] text-[10.5px] leading-none">Esc</span>
      </HintKey>
    );
  const spoken: Record<HintKeyName, string> = { enter: "Enter", shift: "Shift+Enter", esc: "Escape" };
  return (
    <>
      <span className="sr-only">
        {verbs.map(([verb, k]) => `${spoken[k]} ${verb === "back" ? "goes back" : `${verb}s`}.`).join(" ")}
      </span>
      <span aria-hidden className="flex shrink-0 items-center gap-2 text-[11px] text-muted/85">
        {verbs.map(([verb, k], i) => (
          <Fragment key={verb}>
            {i > 0 && <span className="h-3 w-px bg-border/10" />}
            <span className="flex items-center gap-1.5">
              {verb}
              {keys(k)}
            </span>
          </Fragment>
        ))}
      </span>
    </>
  );
}

export default function Search() {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<QueueTrack[] | null>(null);
  const [searching, setSearching] = useState(false);
  const [searchStatus, setSearchStatus] = useState<"ok" | "disconnected" | "offline">("ok");
  const [resurfaced, setResurfaced] = useState<SearchRow[]>([]);
  // "Something different" — Last.fm discovery, additive below the history
  // rows, gated on a Last.fm key + Spotify (fills in after the history rows).
  const [discovery, setDiscovery] = useState<SearchRow[]>([]);
  // A discovery fetch is in flight with nothing to show yet — the section
  // header + skeleton rows render on this so the wait is visible (a silent
  // multi-second fill read as broken — Thien's live feedback, 2026-07-16).
  // One state covers both summon-fill and pull-refresh: same underlying wait.
  const [discoveryPending, setDiscoveryPending] = useState(false);
  // The refresh number the current rows landed with (0 = summon/mount). >0
  // keys a remount + .row-swap-in ripple so a pull visibly re-presents the
  // set — next to the skeleton's visible wait, history's instant swap read
  // as "nothing happened". Summon fills stay plain by construction.
  const [swapTick, setSwapTick] = useState(0);
  const [selected, setSelected] = useState(0);
  const [note, setNote] = useState<string | null>(null);
  const [flashKeys, setFlashKeys] = useState<ReadonlySet<string>>(new Set());
  // Bumped per flash so queueing the same row again replays its flash.
  const [flashSeq, setFlashSeq] = useState(0);
  const spotify = useSpotifyStatus();
  // Live connection for the summon-time discovery fetch (the onSearchShown
  // listener is registered once and must not close over a stale value).
  const connectedRef = useRef(spotify.connected);
  connectedRef.current = spotify.connected;
  const inputRef = useRef<HTMLInputElement>(null);
  const searchGen = useRef(0);
  // Bumped per empty-state refresh so a slow discovery fetch from a prior
  // summon can't land over a newer one.
  const resurfaceGen = useRef(0);
  // Pull-to-refresh (Up at the top slot): a monotonic offset that rotates BOTH
  // sections to a fresh set. Reset to 0 on each summon.
  const refreshCount = useRef(0);
  // Discovery keys already surfaced this session — fed into the exclude so a
  // refresh brings genuinely new suggestions, not the same neighbors. Cleared
  // on summon.
  const discoveryShown = useRef<Set<string>>(new Set());
  // The gen that owns the in-flight discovery fetch (0 = idle) — the Up
  // handler blocks new pulls while set. A TOKEN, not a boolean: a superseded
  // run's finally must not clear a newer run's flag (quick-review catch —
  // a stale clear let a third rapid pull slip the guard, get "busy" from the
  // backend single-flight, and silently blank the section).
  const refreshBusy = useRef(0);
  // The in-flight discoveryPicks promise — new runs AWAIT it before fetching,
  // so the backend's single-flight never bounces the newer (gen-winning)
  // request while the older one's result gets gen-discarded anyway
  // (quick-review catch: a summon during a pull's fetch starved discovery).
  const discoveryFlight = useRef<Promise<unknown> | null>(null);
  // Last.fm key presence, resolved once per summon (one local IPC) and read
  // SYNCHRONOUSLY by pulls — see the single-commit note in refreshEmptyState.
  const lastfmKeyRef = useRef(false);
  const noteTimer = useRef<number | null>(null);
  const flashTimers = useRef(new Map<string, number>());
  const busy = useRef(false);
  // Hover only steals the selection on a REAL pointer move (>3px): the
  // search window spawns near the cursor, and a hand resting on the mouse must
  // not snap keyboard navigation back to whatever row it happens to cover
  // (quick-review catch).
  const lastMouse = useRef<{ x: number; y: number } | null>(null);
  // Hidden at launch (search.rs create-once-hidden). Empty-state fetches wait
  // for the first search-shown. The browser mock never fires that event (the
  // page is already the shown window), so the mock starts already-shown.
  const shownRef = useRef(!IN_TAURI);

  // ---- sections: Songs | Library | History ----
  const [section, setSection] = useState<SearchSection>(INITIAL_SECTION);
  const sectionRef = useRef(section);
  sectionRef.current = section;
  // One filter per section, so Tab never throws a Songs query away.
  const [libQuery, setLibQuery] = useState("");
  const [histQuery, setHistQuery] = useState("");
  const [drillQuery, setDrillQuery] = useState("");
  // The playlist (or Liked Songs) open inside Library; Esc goes back.
  const [opened, setOpened] = useState<OpenTarget | null>(null);
  // The Library selection to restore on the way back out.
  const libReturn = useRef(0);
  // Bumped when the input should refocus and select-all (summon, section switch).
  const [focusTick, setFocusTick] = useState(0);
  const listRef = useRef<HTMLDivElement>(null);
  const [histRender, setHistRender] = useState(HISTORY_RENDER_STEP);

  const lib = useLibrary(section === "library" && spotify.connected);
  const openedTracks = useOpenedTracks(opened, lib);
  const pub = usePublicPlaylists(libQuery, section === "library" && !opened && spotify.connected);
  const history = useHistoryList(section === "history");
  // A typed filter searches every play: walk the rest of the log (a local
  // read, 200 a page) instead of matching only the pages scrolled so far.
  // Re-checked as pages land, so a scroll page in flight doesn't strand it.
  const histFiltering = section === "history" && histQuery.trim() !== "";
  const histLoaded = history.entries.length;
  useEffect(() => {
    if (histFiltering && histLoaded > 0) history.loadAll();
  }, [histFiltering, histLoaded, history.loadAll]);
  const nowCtx = useNowContext(section === "library" && spotify.connected, focusTick);
  const libItems: Item[] = useMemo(() => {
    if (section !== "library") return [];
    return opened
      ? drillItems(opened, openedTracks.tracks, drillQuery, nowCtx)
      : libraryItems(lib, libQuery, pub.playlists, nowCtx);
  }, [section, opened, openedTracks.tracks, drillQuery, lib, libQuery, pub.playlists, nowCtx]);
  // History gets its own memo: sharing one with Library made every arrow
  // press rebuild (and re-format the play time of) every history row,
  // because a Library input changes identity on each render.
  const histItems: Item[] = useMemo(
    () =>
      section === "history"
        ? historyItems(history.entries, histQuery, Date.now(), history.exhausted)
        : [],
    [section, history.entries, histQuery, history.exhausted],
  );
  const items = section === "history" ? histItems : libItems;
  const shownItems = section === "history" ? items.slice(0, histRender) : items;
  const thumbs = useHistoryThumbs(
    section === "history" ? shownItems.map((it) => it.thumbKey ?? "").filter(Boolean) : [],
  );
  const drilling = section === "library" && opened !== null;

  const goSection = useCallback((next: SearchSection) => {
    setSection(next);
    setSelected(0);
    setHistRender(HISTORY_RENDER_STEP);
    setFocusTick((t) => t + 1);
  }, []);
  // A summon hotkey while the window is up: its own section toggles it
  // away, another section switches (search.rs toggle).
  useEffect(
    () =>
      onSearchSection((next) => {
        if (next === sectionRef.current) commands.searchHide();
        else {
          setOpened(null);
          goSection(next);
        }
      }),
    [goSection],
  );
  useEffect(() => {
    if (focusTick === 0) return;
    inputRef.current?.focus();
    inputRef.current?.select();
  }, [focusTick]);
  // A new filter restarts the selection at the top (the first song when a
  // playlist is open; its Play button is the slot above).
  useEffect(() => {
    setSelected(opened ? 1 : 0);
    setHistRender(HISTORY_RENDER_STEP);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [libQuery, histQuery, drillQuery]);

  const showNote = (msg: string, holdMs = 2400) => {
    setNote(msg);
    if (noteTimer.current !== null) window.clearTimeout(noteTimer.current);
    noteTimer.current = window.setTimeout(() => setNote(null), holdMs);
  };

  const flash = (key: string) => {
    setFlashSeq((n) => n + 1);
    setFlashKeys((s) => new Set(s).add(key));
    const prev = flashTimers.current.get(key);
    if (prev !== undefined) window.clearTimeout(prev);
    flashTimers.current.set(
      key,
      window.setTimeout(() => {
        flashTimers.current.delete(key);
        setFlashKeys((s) => {
          const next = new Set(s);
          next.delete(key);
          return next;
        });
      }, 900),
    );
  };
  useEffect(
    () => () => {
      flashTimers.current.forEach((t) => window.clearTimeout(t));
      if (noteTimer.current !== null) window.clearTimeout(noteTimer.current);
    },
    [],
  );

  // Debounced, latest-wins search.
  useEffect(() => {
    const q = query.trim();
    if (!q) {
      setResults(null);
      setSearching(false);
      setSearchStatus("ok");
      return;
    }
    setSearching(true);
    const t = window.setTimeout(() => {
      const gen = ++searchGen.current;
      void commands.spotifySearch(q, RESULT_LIMIT).then((r) => {
        if (gen !== searchGen.current) return;
        setSearching(false);
        setSearchStatus(r.status);
        setResults(r.tracks);
        setSelected(0);
      });
    }, DEBOUNCE_MS);
    return () => window.clearTimeout(t);
  }, [query]);

  // The empty state: history rows first (local, instant), then a gated,
  // fire-and-forget discovery fetch that fills in below — never blocking the
  // history rows. One block seed drives the whole refresh.
  const refreshEmptyState = useCallback(async (refreshN = 0) => {
    const now = Date.now();
    const gen = ++resurfaceGen.current;
    const { rows, pool, played } = await computeResurfaced(now, refreshN);
    if (gen !== resurfaceGen.current) return;

    // Resolve EVERYTHING that decides the discovery section BEFORE touching
    // state, so rows + tick + section land in ONE commit — an await between
    // them unmounted the "Something different" header for a frame and the
    // list height tore on every pull (design-eng pass, 2026-07-16). The key
    // check is one local IPC per summon, cached in a ref for pulls.
    if (refreshN === 0) {
      lastfmKeyRef.current = await commands.lastfmHasKey();
      if (gen !== resurfaceGen.current) return;
    }
    // Discovery is gated on a Last.fm key + a live Spotify session (URIs must
    // resolve to be playable). Its absence is silent — ambient content, not a
    // feature whose failure needs narrating.
    const canDiscover = connectedRef.current && pool.length > 0 && lastfmKeyRef.current;
    // The seed rotates per refresh, so each pull pulls a different seed's
    // neighborhood. The pristine block (refreshN 0) is cached so a re-summon
    // inside the block is free; a user-driven refresh always fetches fresh.
    const seed = blockSeed(now) + refreshN;
    const playedKeys = new Set(played.map((t) => discoveryKey(t.artist, t.title)));
    let cached = refreshN === 0 && canDiscover ? discoveryCache.get(seed) : undefined;
    if (cached) {
      // Re-validate a same-block hit against what's been played SINCE it was
      // cached — playing a suggestion then re-summoning must not re-suggest
      // it (quick-review catch). An emptied hit falls through to a fresh
      // fetch rather than pinning a dead section for the rest of the block.
      cached = cached.filter((r) => !playedKeys.has(discoveryKey(r.artist, r.title)));
      if (!cached.length) {
        discoveryCache.delete(seed);
        cached = undefined;
      }
    }
    const remember = (list: SearchRow[]) =>
      list.forEach((r) => discoveryShown.current.add(discoveryKey(r.artist, r.title)));

    // The one commit: history rows, the swap tick (= the refresh number, so
    // summon/mount resets it and renders plain), and the discovery section's
    // fate — cached rows, skeletons (pending), or silently absent.
    setResurfaced(rows);
    setSwapTick(refreshN);
    if (cached) {
      remember(cached);
      setDiscovery(cached);
      setDiscoveryPending(false);
      return;
    }
    setDiscovery([]);
    setDiscoveryPending(canDiscover);
    if (!canDiscover) return;

    const seeds = pickSeeds(pool, seed);
    // Exclude EVERY played track (not just the rested seed pool — a track heard
    // in the last day is in `played` but NOT in `pool`) AND everything a prior
    // refresh already surfaced this session, so a pull can't repeat a pick.
    const exclude = [...playedKeys, ...discoveryShown.current];
    // Serialize behind any in-flight fetch (its result is gen-discarded, but
    // racing it would get THIS request bounced "busy" by the backend's
    // single-flight); re-check gen — a newer run may have started meanwhile.
    if (discoveryFlight.current) {
      await discoveryFlight.current.catch(() => {});
      if (gen !== resurfaceGen.current) return;
      // The run we waited on was often fetching this very block (a quick
      // hide + re-summon). It caches its result even though its gen lost,
      // so take that instead of paying for a second walk.
      const hit =
        refreshN === 0
          ? discoveryCache.get(seed)?.filter((r) => !playedKeys.has(discoveryKey(r.artist, r.title)))
          : undefined;
      if (hit?.length) {
        remember(hit);
        setDiscovery(hit);
        setDiscoveryPending(false);
        return;
      }
    }
    refreshBusy.current = gen;
    let res: Awaited<ReturnType<typeof commands.discoveryPicks>> | null = null;
    // A transport hiccup reads as null, the same as a non-ok status.
    const attempt = () => commands.discoveryPicks(seeds, exclude).catch(() => null);
    // One quiet retry for a transient failure (a network blip, or the
    // backend's single-flight still finishing a superseded run) before the
    // section gives up. The retry is part of the same flight, so a newer run
    // still waits behind it.
    const flight = (async () => {
      const first = await attempt();
      if (first && first.status !== "offline" && first.status !== "busy") return first;
      if (gen !== resurfaceGen.current) return first;
      await new Promise((r) => window.setTimeout(r, DISCOVERY_RETRY_MS));
      return gen === resurfaceGen.current ? attempt() : first;
    })();
    discoveryFlight.current = flight;
    try {
      res = await flight;
    } finally {
      // Owner-only clears throughout: a superseded run's finally must not
      // free a newer run's token or flight (the same clobber class twice).
      if (discoveryFlight.current === flight) discoveryFlight.current = null;
      if (refreshBusy.current === gen) refreshBusy.current = 0;
      // A stale gen never touches pending — the newer run owns it now.
      if (gen === resurfaceGen.current) setDiscoveryPending(false);
    }
    if (!res || res.status !== "ok") {
      const msg = discoveryFailureNote(res ? res.status : null);
      if (msg && gen === resurfaceGen.current) showNote(msg, 4000);
      return;
    }
    const seen = new Set(rows.map((r) => r.uri).filter((u): u is string => !!u));
    const rows2: SearchRow[] = res.picks
      .filter((p) => !seen.has(p.track.uri))
      .slice(0, 3)
      .map((p) => ({
        key: p.track.uri,
        title: p.track.title,
        artist: p.track.artist,
        artUrl: p.track.art_url,
        track: p.track,
        uri: p.track.uri,
        reason: `Because you played ${p.seed_title}`,
      }));
    // Cache only a NON-EMPTY pristine set: `if (cached)` treats [] as a hit,
    // and one pathological empty post-filter would otherwise pin the section
    // dead for the whole block (quick-review catch). Cached even when this
    // run was superseded, so the newer run waiting on it can reuse it.
    if (refreshN === 0 && rows2.length) discoveryCache.set(seed, rows2);
    if (gen !== resurfaceGen.current) return;
    remember(rows2);
    setDiscovery(rows2);
  }, []);

  // Summon signal: refocus, select-all the stale query, fresh empty state.
  useEffect(
    () =>
      onSearchShown((next) => {
        shownRef.current = true;
        // Every summon starts at the top of its section, outside any
        // opened playlist.
        setOpened(null);
        goSection(next);
        inputRef.current?.focus();
        inputRef.current?.select();
        setNote(null);
        setSelected(0);
        // Every summon starts from the curated set; a pull-refresh advances
        // from there.
        refreshCount.current = 0;
        discoveryShown.current.clear();
        void refreshEmptyState(0);
      }),
    [refreshEmptyState, goSection],
  );
  // After the first show, a Spotify connect flip refills discovery without
  // waiting for the next summon. Skip while still hidden so a launch-time
  // status seed does not scan history or hit Last.fm for a window nobody
  // summoned. After the first show this is the same flip-refresh as before.
  useEffect(() => {
    inputRef.current?.focus();
    if (!shownRef.current) return;
    refreshCount.current = 0;
    discoveryShown.current.clear();
    void refreshEmptyState(0);
  }, [refreshEmptyState, spotify.connected]);

  const hasQuery = query.trim().length > 0;
  // Row-source flips (typing the first char, clearing) restart selection at
  // the top — without this the RAW selected can sit past the new list and
  // ArrowUp reads as dead until it walks back into range. The swap tick
  // resets with it: after a pull, a type-then-clear round trip re-showed the
  // SAME rows through the pull ripple — replayed motion for content that
  // didn't change (unlicensed; the ripple is pull feedback only).
  useEffect(() => {
    setSelected(0);
    setSwapTick(0);
  }, [hasQuery]);

  // Discovery appends BELOW history, so a late-arriving discovery fill never
  // shifts the selected index (the clamp handles list growth).
  const rows: SearchRow[] = hasQuery
    ? (results ?? []).map(trackRow)
    : [...resurfaced, ...discovery];
  // The list the keys walk: Songs' rows, or the section's items (an opened
  // playlist's Play button is slot 0, its songs follow).
  const listLen =
    section === "songs" ? rows.length : drilling ? shownItems.length + 1 : shownItems.length;
  const sel = Math.min(selected, Math.max(listLen - 1, 0));
  const itemAt = (i: number): Item | "play" | undefined =>
    drilling ? (i === 0 ? "play" : shownItems[i - 1]) : shownItems[i];
  const selItem = section === "songs" ? undefined : itemAt(sel);
  // Keep the keyboard selection on screen in the long lists.
  useEffect(() => {
    if (section === "songs") return;
    document.getElementById(`search-opt-${sel}`)?.scrollIntoView({ block: "nearest" });
  }, [sel, section]);

  const playRow = async (row: SearchRow) => {
    // Double-press / slow resolve: tell the user instead of a dead click.
    if (busy.current) {
      showNote("Still working…");
      return;
    }
    busy.current = true;
    try {
      // Resolve BEFORE dismissing — it's one fast search for un-enriched
      // history rows (instant for search results), and "couldn't find it"
      // still has a surface to land on.
      const uri = await resolveUri(row);
      if (!uri) {
        showNote("Couldn't find it on Spotify");
        return;
      }
      // Await the jump WHILE Search is still open. Dismissing first and
      // ignoring the status (the old light-exit path) swallowed no_device /
      // busy / diverged — the click felt dead. Hide only when playback
      // actually started; keep the note seat for failures.
      let status: string;
      try {
        status = await commands.playNow(uri);
      } catch {
        showNote("Spotify unreachable");
        return;
      }
      const fail = playNowNote(status);
      if (fail) {
        showNote(fail);
        return;
      }
      setQuery("");
      setNote(null);
      commands.searchHide();
    } finally {
      busy.current = false;
    }
  };

  const queueRow = async (row: SearchRow) => {
    // Same one-write-at-a-time gate as playRow: a held Shift+Enter
    // key-repeats, and upnext_add has no dedupe (quick-review catch).
    if (busy.current) {
      showNote("Still working…");
      return;
    }
    busy.current = true;
    try {
      const uri = await resolveUri(row);
      if (!uri) {
        showNote("Couldn't find it on Spotify");
        return;
      }
      try {
        commands.upnextAdd(
          row.track ?? {
            uri,
            title: row.title,
            artist: row.artist,
            album: "",
            duration_ms: 0,
            art_url: row.artUrl,
          },
        );
      } catch {
        showNote("Couldn't queue that");
        return;
      }
      flash(row.key);
      showNote(`Queued · ${row.title}`);
    } finally {
      busy.current = false;
    }
  };

  // Disconnected → an in-place Connect button (below); connected-but-offline
  // still narrates. `gated` blocks Enter-to-play and hides the ↵/⇧↵ hint.
  const offline = spotify.connected && searchStatus === "offline";
  const gated = !spotify.connected || offline;

  const songsKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      commands.searchHide();
    } else if (e.key === "ArrowDown") {
      // Step from the CLAMPED position — the raw state can sit past a
      // shrunken list, where stepping it would strand the highlight.
      e.preventDefault();
      setSelected(Math.min(sel + 1, Math.max(rows.length - 1, 0)));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      // Pull-to-refresh: Up while ALREADY at the top slot rolls a fresh set
      // (the phone gesture). Only in the empty state, with rows to replace,
      // and never while a fetch is in flight (would answer "busy"). No
      // toast: the skeleton wait + the row-swap ripple ARE the refresh
      // narration — the "Fresh picks" caption was double-speak (Thien,
      // 2026-07-17).
      if (!hasQuery && sel === 0 && !gated && rows.length > 0 && !refreshBusy.current) {
        refreshCount.current += 1;
        void refreshEmptyState(refreshCount.current);
        return;
      }
      setSelected(Math.max(sel - 1, 0));
    } else if (e.key === "Enter" && rows[sel] && !gated) {
      e.preventDefault();
      // A HELD Enter/Shift+Enter key-repeats, and the busy gate frees in a
      // microtask (resolveUri resolves synchronously for enriched rows)
      // before the next repeat keydown — so a repeat slips the gate and
      // upnext_add, which has no dedupe, double-queues (audit A6-5). One
      // physical press = one action: ignore auto-repeat outright. The play
      // path dismisses on its first fire, but a held repeat still reached
      // playNow twice before the hide landed — this covers both verbs.
      if (e.repeat) return;
      if (e.shiftKey) void queueRow(rows[sel]);
      else void playRow(rows[sel]);
    }
  };

  // ---- Library / History verbs ----

  /** Play a playlist (or Liked Songs) as the new context, optionally from
   * one of its songs. Same one-at-a-time gate and stay-open-on-failure as
   * playRow. */
  const playContext = async (contextUri: string, offsetUri?: string) => {
    if (busy.current) {
      showNote("Still working…");
      return;
    }
    busy.current = true;
    try {
      let status: string;
      try {
        status = await commands.playContext(contextUri, offsetUri);
      } catch {
        showNote("Spotify unreachable");
        return;
      }
      const fail = playNowNote(status);
      if (fail) {
        showNote(fail);
        return;
      }
      setNote(null);
      commands.searchHide();
    } finally {
      busy.current = false;
    }
  };
  const playTarget = (t: OpenTarget) => {
    // Liked Songs plays as the account's collection context, which needs
    // the account id the library load returned.
    if (t.kind === "liked" && !lib.userId) {
      showNote("Couldn't start Liked Songs");
      return;
    }
    void playContext(t.uri);
  };
  const openTarget = (t: OpenTarget) => {
    libReturn.current = sel;
    setOpened(t);
    setDrillQuery("");
    setSelected(1);
  };
  const closeTarget = () => {
    setOpened(null);
    setSelected(libReturn.current);
    setFocusTick((n) => n + 1);
  };
  /** Enter (or a click) on slot `i`; `shift` = Shift+Enter. */
  const activate = (i: number, shift: boolean) => {
    if (!spotify.connected) {
      showNote("Connect Spotify to play");
      return;
    }
    const it = itemAt(i);
    if (!it) return;
    if (it === "play") {
      if (opened) playTarget(opened);
      return;
    }
    if (it.open) {
      if (shift) playTarget(it.open);
      else openTarget(it.open);
      return;
    }
    if (!it.song) return;
    if (shift) void queueRow(it.song);
    else if (it.context && it.song.uri) {
      if (it.context.endsWith(":collection") && !lib.userId) void playRow(it.song);
      else void playContext(it.context, it.song.uri);
    } else void playRow(it.song);
  };

  const sectionKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      if (drilling) closeTarget();
      else commands.searchHide();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      const next = Math.min(sel + 1, Math.max(listLen - 1, 0));
      setSelected(next);
      if (section === "history" && next >= histRender - 5) {
        if (histRender < items.length) setHistRender((r) => r + HISTORY_RENDER_STEP);
        else history.loadMore();
      }
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setSelected(Math.max(sel - 1, 0));
    } else if (e.key === "Enter" && listLen > 0) {
      e.preventDefault();
      // One physical press = one action (see the Songs handler).
      if (e.repeat) return;
      activate(sel, e.shiftKey);
    }
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    // Tab walks the section strip (Shift+Tab backwards), like Raycast. The
    // input is the pane's only focus stop, so nothing else wants Tab.
    if (e.key === "Tab" && !e.ctrlKey && !e.altKey && !e.metaKey) {
      e.preventDefault();
      goSection(nextSection(section, e.shiftKey ? -1 : 1));
      return;
    }
    if (section === "songs") songsKeyDown(e);
    else sectionKeyDown(e);
  };

  const onListScroll = () => {
    if (section !== "history") return;
    const el = listRef.current;
    if (!el || el.scrollTop + el.clientHeight < el.scrollHeight - 160) return;
    if (histRender < items.length) setHistRender((r) => r + HISTORY_RENDER_STEP);
    else history.loadMore();
  };

  // The input belongs to the current section's filter.
  const inputValue =
    section === "songs" ? query : section === "history" ? histQuery : drilling ? drillQuery : libQuery;
  const setInput = (v: string) => {
    if (section === "songs") setQuery(v);
    else if (section === "history") setHistQuery(v);
    else if (drilling) setDrillQuery(v);
    else setLibQuery(v);
  };
  const placeholder =
    section === "songs"
      ? "Play something…"
      : section === "history"
        ? "What did you play?"
        : opened
          ? `Filter ${opened.name}…`
          : "Find a playlist…";
  const inputLabel =
    section === "songs"
      ? "Search Spotify"
      : section === "history"
        ? "Search your history"
        : opened
          ? `Filter ${opened.name}`
          : "Find a playlist";

  // ---- Library / History rendering ----

  const hoverSelect = (i: number) => (e: React.MouseEvent) => {
    const prev = lastMouse.current;
    lastMouse.current = { x: e.clientX, y: e.clientY };
    if (prev && Math.abs(prev.x - e.clientX) + Math.abs(prev.y - e.clientY) < 3) return;
    if (prev) setSelected(i);
  };
  const prose = (text: string) => (
    <p className="m-0 px-3.5 py-2.5 text-[13px] text-muted">{text}</p>
  );
  const sectionHeader = (text: string) => (
    <p
      role="presentation"
      className="m-0 px-3.5 pb-1.5 pt-2 text-[11px] uppercase tracking-widest text-muted"
    >
      {text}
    </p>
  );
  /** One Library / History row — the Songs row's exact geometry. History
   * draws its own sticky day headers (`inlineHeader` false) and keeps rows
   * clear of them when the selection scrolls into view. */
  const renderItem = (it: Item, slot: number, inlineHeader = true) => {
    const art = it.artUrl ?? (it.thumbKey ? (thumbs.get(it.thumbKey) ?? null) : null);
    return (
      <Fragment key={it.key}>
        {inlineHeader && it.header && sectionHeader(it.header)}
        <div
          id={`search-opt-${slot}`}
          role="option"
          aria-selected={slot === sel}
          onMouseMove={hoverSelect(slot)}
          onClick={(e) => activate(slot, e.shiftKey)}
          // Selection snaps (no transition): arrows move it many times a
          // second, and a fade made the highlight trail the key.
          className={`relative flex h-[52px] shrink-0 cursor-pointer select-none items-center gap-3 rounded-md px-3.5 ${
            inlineHeader ? "" : "scroll-mt-[31px]"
          } ${slot === sel ? "bg-fg/10" : ""}`}
        >
          {it.song && flashKeys.has(it.song.key) && <RowFlash key={flashSeq} />}
          {it.tile ? <ItemTile tile={it.tile} url={art} size={32} /> : <RowThumb url={art} size={32} />}
          <span className="flex min-w-0 flex-1 flex-col">
            <span className="truncate text-[14px] font-medium leading-5 text-fg">{it.title}</span>
            {it.subtitle && <span className="truncate text-[12px] leading-4 text-muted">{it.subtitle}</span>}
          </span>
          {it.right && (
            <span
              className={`max-w-[45%] shrink-0 truncate text-right text-[11px] tabular-nums ${
                it.rightActive ? "font-medium text-fg" : "text-muted/85"
              }`}
            >
              {it.right}
            </span>
          )}
        </div>
      </Fragment>
    );
  };
  const skeletons = (n: number) =>
    Array.from({ length: n }, (_, i) => <SkeletonRow key={`sk${i}`} index={i} />);

  const renderDrill = (t: OpenTarget) => (
    <div id="search-list" role="listbox" aria-label={t.name}>
      {/* Where you are, in the slot "Your library" held: the input alone
          (showing the playlist's name) read like a filter that its songs
          failed. "Library" is the visible back, same as Esc. */}
      <p
        role="presentation"
        className="m-0 flex min-w-0 items-center gap-1.5 px-3.5 pb-1.5 pt-2 text-[11px] uppercase tracking-widest text-muted"
      >
        <button
          type="button"
          tabIndex={-1}
          onClick={closeTarget}
          aria-label="Back to Library"
          className="inline-flex shrink-0 items-center gap-1 uppercase tracking-widest text-muted [transition:color_140ms_var(--ease-out-tk)] hover:text-fg"
        >
          <BackIcon />
          Library
        </button>
        <span aria-hidden="true">/</span>
        <span className="truncate">{t.name}</span>
      </p>
      {/* The opened playlist: identity + Play. Slot 0, so Up from the first
          song reaches Play and Enter there plays the whole thing. */}
      <div
        id="search-opt-0"
        role="option"
        aria-selected={sel === 0}
        aria-label={`Play ${t.name}`}
        onMouseMove={hoverSelect(0)}
        className="flex items-center gap-3 px-3.5 pb-2.5 pt-1.5"
      >
        <ItemTile tile={t.kind === "liked" ? "liked" : "playlist"} url={t.artUrl} size={48} />
        <span className="flex min-w-0 flex-1 flex-col">
          <span className="truncate text-[16px] font-medium leading-[22px] text-fg">{t.name}</span>
          {(t.subtitle || isPlayingTarget(t, nowCtx)) && (
            <span className="truncate text-[12px] leading-4 text-muted">
              {[t.subtitle, isPlayingTarget(t, nowCtx) ? "Playing" : ""].filter(Boolean).join(" · ")}
            </span>
          )}
        </span>
        <button
          type="button"
          tabIndex={-1}
          onClick={() => activate(0, false)}
          // Outlined until selected: a filled button out-shouted the
          // selected song row, which is what Enter actually plays.
          // No transition: Up/Down select it, so it snaps like a row.
          className={`inline-flex h-[26px] shrink-0 items-center gap-1.5 rounded-md pl-2 pr-2.5 text-[12px] font-medium ring-1 ring-inset ${
            sel === 0 ? "bg-fg/20 text-fg ring-transparent" : "text-muted ring-fg/15 hover:bg-fg/10 hover:text-fg"
          }`}
        >
          <PlayIcon />
          Play
        </button>
      </div>
      {openedTracks.status === "loading" ? (
        skeletons(6)
      ) : openedTracks.status === "restricted" ? (
        // Spotify (February 2026) lists songs only for playlists the user
        // owns or collaborates on; playback of the whole playlist still works.
        <p className="m-0 px-3.5 py-2 text-[13px] leading-5 text-muted">
          Spotify only shares the songs of playlists you own or collaborate on. Play still works.
        </p>
      ) : openedTracks.status === "offline" ? (
        prose("Spotify unreachable")
      ) : shownItems.length === 0 ? (
        prose(drillQuery.trim() ? "No songs here match" : "This playlist is empty")
      ) : (
        shownItems.map((it, i) => renderItem(it, i + 1))
      )}
    </div>
  );

  const renderLibrary = () => {
    if (!spotify.connected) {
      return (
        <div className="flex flex-col items-start gap-1.5 px-3.5 py-2.5">
          <SpotifyConnectButton />
          <p className="m-0 text-[12px] text-muted/85">Connect Spotify to see your playlists and Liked Songs.</p>
        </div>
      );
    }
    if (lib.status === "needs_reconnect") {
      return (
        <div className="flex flex-col items-start gap-1.5 px-3.5 py-2.5">
          <ReconnectButton />
          <p className="m-0 text-[12px] text-muted/85">
            Reconnect once so Palette can read your playlists and Liked Songs.
          </p>
        </div>
      );
    }
    if (lib.status === "offline") return prose("Spotify unreachable");
    if (lib.status !== "ok") return skeletons(7);
    if (opened) return renderDrill(opened);
    const q = libQuery.trim();
    const hasPublicHeader = items.some((it) => it.header === "Playlists on Spotify");
    return (
      <div id="search-list" role="listbox" aria-label="Your library">
        {shownItems.map((it, i) => renderItem(it, i))}
        {q && pub.pending && !hasPublicHeader && (
          <>
            {sectionHeader("Playlists on Spotify")}
            <p className="m-0 px-3.5 py-2 text-[12px] text-muted/85">Searching Spotify…</p>
          </>
        )}
        {q && !pub.pending && items.length === 0 && prose("Nothing in your library or on Spotify matches")}
      </div>
    );
  };

  const renderHistory = () => {
    if (history.entries.length === 0) {
      return prose("Songs you play will gather here.");
    }
    if (shownItems.length === 0)
      return prose(history.filling ? "Searching your history…" : "Nothing in your history matches");
    // One block per day, so its header sticks to the top while that day
    // scrolls by and the next day's header pushes it off: deep in History
    // the time alone didn't say which day you were in (Thien, 2026-11-06).
    const days: { label: string; summary: string | null; rows: { it: Item; slot: number }[] }[] = [];
    shownItems.forEach((it, slot) => {
      if (it.header || days.length === 0)
        days.push({ label: it.header ?? "", summary: it.headerRight ?? null, rows: [] });
      days[days.length - 1].rows.push({ it, slot });
    });
    return (
      <div id="search-list" role="listbox" aria-label="History">
        {days.map((day) => (
          <div key={day.rows[0].it.key} role="group" aria-label={day.label}>
            {/* Sticky, opaque (the card's own surface) so rows pass under
                it. -top-1.5 cancels the list's p-1.5, so it pins flush to
                the list's top edge with no sliver of row showing above. */}
            <p
              role="presentation"
              className="sticky -top-1.5 z-10 m-0 flex items-baseline justify-between gap-3 bg-surface px-3.5 pb-1.5 pt-2 text-[11px] text-muted"
            >
              <span className="uppercase tracking-widest">{day.label}</span>
              {/* What that day added up to, under the play-time column. A
                  day still cut off at the page edge shows nothing yet. */}
              {day.summary && (
                <span
                  title="Time you actually listened. A skipped song counts only the part you heard."
                  className="shrink-0 tabular-nums text-muted/85"
                >
                  {day.summary}
                </span>
              )}
            </p>
            {day.rows.map(({ it, slot }) => renderItem(it, slot, false))}
          </div>
        ))}
      </div>
    );
  };

  return (
    // The window is transparent and fixed-size; the p-3 gutter gives the
    // shell's shadow room. A click on the gutter (outside the shell)
    // dismisses — the fixed window's unused area must never read as dead.
    <div
      className="flex h-screen w-screen flex-col p-3"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) commands.searchHide();
      }}
      onKeyDown={onKeyDown}
    >
      {/* The queue popover's shadow recipe (App.tsx) — not a third invented
          elevation; it also stays inside the 12px gutter, the transparent-
          window clip budget the card shadow once overflowed. */}
      <div
        // Keep focus in the input: the root onKeyDown (Esc/arrows/Enter) lives
        // on a non-focusable div, so a mousedown on non-interactive panel
        // content moves focus to <body> and the pane goes keyboard-dead until
        // the next click (audit A6-9). preventDefault suppresses the focus
        // shift without touching click, so row clicks still play and the input
        // keeps its native caret/selection.
        onMouseDown={(e) => {
          if (e.target !== inputRef.current) e.preventDefault();
        }}
        className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-xl border border-border/10 bg-surface shadow-xl shadow-black/40"
      >
        {/* Search row — the search's one verb. */}
        <div className="flex items-center gap-3 px-5 pb-3 pt-4">
          <span className={searching ? "text-fg" : "text-muted"}>
            <SearchIcon />
          </span>
          <input
            ref={inputRef}
            value={inputValue}
            onChange={(e) => setInput(e.target.value)}
            placeholder={placeholder}
            aria-label={inputLabel}
            role="combobox"
            aria-expanded={listLen > 0}
            aria-controls="search-list"
            aria-activedescendant={listLen > 0 ? `search-opt-${sel}` : undefined}
            spellCheck={false}
            autoCorrect="off"
            autoComplete="off"
            // focus-visible:outline-none out-specificities index.css's global
            // accent focus ring — it's built for 28px buttons, and on a bare
            // full-width input it rendered as a square orange bar (Thien's
            // live feedback). The caret is this input's focus signal; it is
            // the pane's only focusable element.
            className="search-input min-w-0 flex-1 bg-transparent text-[18px] text-fg outline-none placeholder:text-muted focus-visible:[outline:none]"
          />
          {section !== "songs" && spotify.connected && listLen > 0 && (
            <SectionHints
              verbs={
                drilling
                  ? [["play", "enter"], ["queue", "shift"], ["back", "esc"]]
                  : selItem && selItem !== "play" && selItem.open
                    ? [["open", "enter"], ["play", "shift"]]
                    : [["play", "enter"], ["queue", "shift"]]
              }
            />
          )}
          {section === "songs" && !gated && (
            <>
              {/* AT mirror: the visual chips are glyph soup to a reader
                  ("return symbol upwards arrow…") — hide them and say it in
                  words, the skeletons' aria-hidden pattern. */}
              <span className="sr-only">
                Enter plays. Shift+Enter queues.
                {!hasQuery && rows.length > 0 ? " Up arrow refreshes suggestions." : ""}
              </span>
              <span aria-hidden className="flex shrink-0 items-center gap-2 text-[11px] text-muted/85">
              {/* "↑ more" only when there ARE rows to re-roll — the Up guard
                  requires rows, and an empty-history user was being promised
                  a no-op (quick-review catch). Raycast's footer grammar:
                  verb label first, then its key(s); groups separated by a
                  hairline, not middots. */}
              <span className="flex items-center gap-1.5">
                play
                <HintKey><EnterKeyIcon /></HintKey>
              </span>
              <span className="h-3 w-px bg-border/10" />
              <span className="flex items-center gap-1.5">
                queue
                <span className="flex items-center gap-[3px]">
                  <HintKey><ShiftKeyIcon /></HintKey>
                  <HintKey><EnterKeyIcon /></HintKey>
                </span>
              </span>
              {!hasQuery && rows.length > 0 && (
                <>
                  <span className="h-3 w-px bg-border/10" />
                  <span className="flex items-center gap-1.5">
                    more
                    <HintKey><UpKeyIcon /></HintKey>
                  </span>
                </>
              )}
              </span>
            </>
          )}
        </div>

        {/* The section strip: what the window is finding. Tab walks it.
            pl-3.5 lines the tab glyphs up with the magnifier; pr-5 matches
            the input row so "next section" ends on the same edge as the
            play/queue hints above it. */}
        <div
          role="tablist"
          aria-label="Search sections"
          className="flex items-center gap-0.5 border-b border-border/10 pb-2.5 pl-3.5 pr-5"
        >
          {SECTIONS.map((sec) => (
            <button
              key={sec}
              type="button"
              role="tab"
              // The input stays the pane's only focus stop (panel mousedown
              // keeps focus there); Tab is the strip's key.
              tabIndex={-1}
              aria-selected={sec === section}
              onClick={() => {
                if (sec === section && drilling) closeTarget();
                else if (sec !== section) goSection(sec);
              }}
              // No transition: Tab flips the active tab, so it snaps like
              // the row selection.
              className={`inline-flex h-[26px] items-center gap-1.5 rounded-md pl-2 pr-2.5 text-[12.5px] font-medium ${
                sec === section ? "bg-fg/10 text-fg" : "text-muted hover:text-fg"
              }`}
            >
              {SECTION_GLYPH[sec]()}
              {SECTION_LABEL[sec]}
            </button>
          ))}
          <span aria-hidden className="ml-auto flex items-center gap-1.5 text-[11px] text-muted/85">
            next section
            <HintKey>
              <TabKeyIcon />
            </HintKey>
          </span>
        </div>

        {/* AT mirror of the states the visuals paint for sighted users (the
            skeletons are aria-hidden): search-in-flight — a status the old
            "Searching…" text used to provide — the discovery wait, and the
            landed suggestion count (quick-review a11y catches, 2026-07-16). */}
        <span role="status" className="sr-only">
          {section === "library"
            ? lib.status === "loading"
              ? "Loading your library"
              : libQuery.trim() && pub.pending
                ? "Searching Spotify playlists"
                : ""
            : section === "history"
              ? ""
              : hasQuery && searching && rows.length === 0
            ? "Searching Spotify"
            : !hasQuery && discoveryPending
              ? "Finding suggestions"
              : !hasQuery && discovery.length > 0
                ? `${discovery.length} suggestion${discovery.length === 1 ? "" : "s"} added`
                : ""}
        </span>

        {/* One left line for the whole pane: the list container's p-1.5 (6px)
            plus px-3.5 (14px) lands rows, headers, prose, and skeletons on
            the SAME 20px inset as the search row's px-5 glyph — the input's
            magnifier, section labels, and row thumbs share one vertical
            line (the alignment sweep, 2026-07-16). */}
        <div
          ref={listRef}
          onScroll={onListScroll}
          className="flex min-h-0 flex-1 flex-col overflow-y-auto p-1.5 [scrollbar-width:none]"
        >
          {section === "library" ? (
            renderLibrary()
          ) : section === "history" ? (
            renderHistory()
          ) : !spotify.connected ? (
            <div className="flex flex-col items-start gap-1.5 px-3.5 py-2.5">
              <SpotifyConnectButton />
              <p className="m-0 text-[12px] text-muted/85">Connect Spotify to search and play.</p>
            </div>
          ) : offline ? (
            <p className="m-0 px-3.5 py-2.5 text-[13px] text-muted">Spotify unreachable</p>
          ) : (
            <>
              {!hasQuery && rows.length === 0 && (
                <p className="m-0 px-3.5 py-2.5 text-[13px] text-muted">
                  Type to search Spotify — tracks you play will gather here.
                </p>
              )}
              {/* First results still in flight — the same ghost rows as the
                  discovery wait (the glyph in the search row tints too), at
                  the count results actually land with (3 ghosts → 8 rows was
                  a vertical jump), and reason-less like real result rows. */}
              {hasQuery &&
                searching &&
                rows.length === 0 &&
                Array.from({ length: RESULT_LIMIT }, (_, i) => (
                  <SkeletonRow key={i} index={i} />
                ))}
              {hasQuery && !searching && rows.length === 0 && (
                <p className="m-0 px-3.5 py-2.5 text-[13px] text-muted">No matches on Spotify</p>
              )}
              <div
                id="search-list"
                role="listbox"
                aria-label={hasQuery ? "Search results" : "Recommendations"}
              >
                {rows.map((row, i) => {
                  // Section headers ride INSIDE the flat list so keyboard nav
                  // spans both sections with one index; presentation role so a
                  // reader skips them as options.
                  const header =
                    !hasQuery && i === 0 && resurfaced.length > 0
                      ? "From your history"
                      : !hasQuery && discovery.length > 0 && i === resurfaced.length
                        ? "Something different"
                        : null;
                  return (
                    // The tick prefix remounts the SET on a pull so the ripple
                    // is uniform (partially-animating rows read as a bug, and
                    // a class flip on survivors would replay anyway); headers
                    // remount too but carry no animation — chrome holds still.
                    <Fragment key={`${swapTick}:${row.key}`}>
                      {header && (
                        <p
                          role="presentation"
                          className="m-0 px-3.5 pb-1.5 pt-2 text-[11px] uppercase tracking-widest text-muted"
                        >
                          {header}
                        </p>
                      )}
                  <div
                    id={`search-opt-${i}`}
                    role="option"
                    aria-selected={i === sel}
                    onMouseMove={(e) => {
                      const prev = lastMouse.current;
                      lastMouse.current = { x: e.clientX, y: e.clientY };
                      if (prev && Math.abs(prev.x - e.clientX) + Math.abs(prev.y - e.clientY) < 3) return;
                      if (prev) setSelected(i);
                    }}
                    onClick={() => void playRow(row)}
                    // Selection snaps, as in renderItem: no trailing fade
                    // behind the arrow keys.
                    className={`relative flex h-[52px] cursor-pointer select-none items-center gap-3 rounded-md px-3.5 ${
                      i === sel ? "bg-fg/10" : ""
                    } ${!hasQuery && swapTick > 0 ? "row-swap-in" : ""}`}
                    style={
                      // Stagger is LOCAL to each section's landing moment:
                      // history staggers from the pull commit; discovery rows
                      // mount seconds later and stagger from their own arrival
                      // — a flat-index delay left them invisible for 100ms+
                      // AFTER the skeleton wait (design-eng pass, 2026-07-16).
                      !hasQuery && swapTick > 0
                        ? {
                            animationDelay: `${(i < resurfaced.length ? i : i - resurfaced.length) * 20}ms`,
                          }
                        : undefined
                    }
                  >
                    {flashKeys.has(row.key) && <RowFlash key={flashSeq} />}
                    <RowThumb url={row.artUrl} size={32} />
                    <span className="flex min-w-0 flex-1 flex-col">
                      <span className="truncate text-[14px] font-medium leading-5 text-fg">{row.title}</span>
                      <span className="truncate text-[12px] leading-4 text-muted">{row.artist}</span>
                    </span>
                    {/* Why, flush right — keeps the empty state at 7×52 so the
                        born-at-size window seats every row (search.rs H=586).
                        Native title carries the full string when the label is
                        clipped; stacking it under identity blew past six rows. */}
                    {row.reason ? (
                      <span
                        title={row.reason}
                        className="max-w-[45%] shrink-0 truncate text-right text-[11px] text-muted/85"
                      >
                        {row.reason}
                      </span>
                    ) : null}
                  </div>
                    </Fragment>
                  );
                })}
                {/* Discovery in flight, nothing to show yet (summon-fill OR
                    pull-refresh — same wait): a ghost header and three ghost
                    rows hold the exact space the picks will land in, so a
                    multi-second Last.fm→Spotify walk never reads as broken
                    and the swap is in-place. The header is a ghost too: the
                    real "Something different" over empty bars read as a
                    section that had loaded with nothing in it (Thien,
                    2026-11-06). Same box as the real header, so the swap doesn't shift. */}
                {!hasQuery && discoveryPending && discovery.length === 0 && (
                  <>
                    <p
                      aria-hidden
                      data-ghost-header
                      className="m-0 px-3.5 pb-1.5 pt-2 text-[11px] uppercase tracking-widest text-muted"
                    >
                      {/* Zero-width text keeps the real header's line box. */}
                      {"\u200b"}
                      <span className="skeleton-pulse inline-block h-[7px] w-[124px] rounded-full bg-fg/5 align-middle" />
                    </p>
                    {[0, 1, 2].map((i) => (
                      <SkeletonRow key={i} index={i} reason />
                    ))}
                  </>
                )}
              </div>
            </>
          )}
        </div>

        {/* Quiet feedback line — the search's toast. */}
        <p aria-live="polite" className="m-0 min-h-[30px] border-t border-border/10 px-5 py-1.5 text-[12px] leading-[18px] text-muted">
          {note}
        </p>
      </div>
    </div>
  );
}
