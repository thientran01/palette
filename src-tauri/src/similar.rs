/*
 * more_like_this: current track → Last.fm similars → Spotify uris → the
 * Pulse-managed up-next list (upnext.rs's feeder does the rest).
 *
 * Candidate ordering encodes the spike findings (2026-07-11, real history):
 * seed-artist entries are SKIPPED first — the top of every Last.fm list is
 * "more by this artist" (match 1.0–0.9), and Thien's ask is genre discovery,
 * not a artist radio — and a match floor guards the mush tail (below ~0.05
 * the graph returns unrelated filler: tripleS's list ends in Jazmine
 * Sullivan at 0.016). Same-artist entries backfill only when the
 * different-artist pool runs dry.
 *
 * Adds go through upnext::append one at a time (each persists + emits), so
 * rows arrive incrementally in the open queue panel — earned arrival for
 * content the user explicitly asked for.
 */
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Manager};

use crate::{lastfm, settings, spotify, upnext};

const ADD_CAP: usize = 5;
const FETCH_LIMIT: u32 = 15;
const MATCH_FLOOR: f64 = 0.05;
/// Gap between Spotify search calls — the requeue cadence; api_call already
/// honors one Retry-After on top.
const RESOLVE_GAP: Duration = Duration::from_millis(120);

/// discovery_picks fan: up to SEED_CAP seed tracks, walked only until
/// PICK_TARGET picks land, ≤2 resolved picks per seed, and — because a
/// candidate that MISSES on Spotify still costs a search + gap — at most
/// TRIES_PER_SEED resolve attempts per seed and TRIES_TOTAL overall. Worst
/// case stays a bounded 10 Spotify calls (quick-review catch: the
/// earlier "≤4" claim only counted successes; a miss-heavy seed walked every
/// candidate). A hard Last.fm error on any seed breaks the whole walk.
///
/// Discovery asks Last.fm for a longer list than more-like-this, and walks
/// more seeds: seeds are the user's most-played tracks, whose neighborhoods
/// are exactly the ones they already know, so as history grows the played
/// exclusion can eat all 15 similars of both seeds and leave the section
/// silently empty. Extra seeds only cost Last.fm calls, and only when the
/// earlier seeds came up short. (The empty section Thien hit in Oct 2026 was
/// a different cause: track.getSimilar returning nothing at all, which
/// lastfm::similar_or_artist_fallback covers.)
const DISCOVERY_FETCH_LIMIT: u32 = 50;
const SEED_CAP: usize = 4;
const PICK_TARGET: usize = 3;
const PER_SEED: usize = 2;
const TRIES_PER_SEED: usize = 5;
const TRIES_TOTAL: usize = 10;

/// Seeds whose Last.fm lists are fetched side by side. The log shows the
/// first two seeds fill all three picks on almost every run, so a batch of
/// two covers the usual case in one round without spending calls on seeds
/// that are never walked.
const SEED_BATCH: usize = 2;
/// Wall-clock budget for one discovery run. Once it passes, the walk stops
/// and returns whatever it has, so a stalled network can't hold the
/// skeleton up for the sum of every request's timeout.
const DISCOVERY_DEADLINE: Duration = Duration::from_secs(8);

/// One run at a time (the enrich_in_flight shape) — a double click must not
/// race two fills into the same list.
static IN_FLIGHT: AtomicBool = AtomicBool::new(false);

/// discovery_picks has its OWN gate: the search window's empty-state fetch and
/// the queue's more-like-this button must never block each other.
static DISCOVERY_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

/// Discovery-first candidate ordering, the shared spike-verified rule (see the
/// module header): different-artist entries above the match floor first (real
/// genre discovery, not artist radio), then same-artist backfill only when the
/// different-artist pool runs dry. Below-floor different-artist entries are the
/// mush tail — dropped entirely.
fn ordered<'a>(
    seed_artist: &str,
    similars: &'a [lastfm::SimilarTrack],
) -> Vec<&'a lastfm::SimilarTrack> {
    let seed = seed_artist.trim().to_lowercase();
    let same = |s: &lastfm::SimilarTrack| s.artist.trim().to_lowercase() == seed;
    similars
        .iter()
        .filter(|s| s.score >= MATCH_FLOOR && !same(s))
        .chain(
            similars
                .iter()
                .filter(|s| s.score >= MATCH_FLOOR && same(s)),
        )
        .collect()
}

/// The exclude-key contract shared with the frontend (Search.tsx): a track's
/// identity for dedupe is its lowercased, trimmed `artist\u{1}title`. Kept in
/// lockstep on both sides — a drift silently stops excluding.
fn norm_key(artist: &str, title: &str) -> String {
    format!(
        "{}\u{1}{}",
        artist.trim().to_lowercase(),
        title.trim().to_lowercase()
    )
}

/// Statuses: "ok:<n>" | "no_matches" | "no_data" | "no_key" (absent) |
/// "bad_key" (rejected) | "busy" | "disconnected" | "offline". Blocking
/// (two HTTP hops) — dedicated pool.
#[tauri::command]
pub async fn more_like_this(app: AppHandle, title: String, artist: String) -> String {
    tauri::async_runtime::spawn_blocking(move || run(&app, &title, &artist))
        .await
        .unwrap_or_else(|e| {
            log::error!("more_like_this task panicked: {e}");
            IN_FLIGHT.store(false, Ordering::SeqCst);
            "offline".into()
        })
}

fn run(app: &AppHandle, title: &str, artist: &str) -> String {
    if IN_FLIGHT
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return "busy".into();
    }
    let out = fill(app, title, artist);
    IN_FLIGHT.store(false, Ordering::SeqCst);
    out
}

fn fill(app: &AppHandle, title: &str, artist: &str) -> String {
    if !spotify::connected(app) {
        return "disconnected".into();
    }
    let Some(key) = settings::get_string(app, "lastfm_api_key").filter(|k| !k.is_empty()) else {
        return "no_key".into();
    };
    let similars = match lastfm::similar_or_artist_fallback(&key, title, artist, FETCH_LIMIT) {
        Ok(v) => v,
        Err(e) => return e.into(),
    };
    if similars.is_empty() {
        return "no_data".into();
    }

    let candidates = ordered(artist, &similars);
    if candidates.is_empty() {
        return "no_data".into();
    }

    let mut added = 0usize;
    let mut resolved_any = false;
    for c in candidates {
        if added >= ADD_CAP {
            break;
        }
        if resolved_any {
            std::thread::sleep(RESOLVE_GAP);
        }
        resolved_any = true;
        let Some(track) = spotify::search_best(app, &c.title, &c.artist) else {
            continue; // no Spotify match / transient — skip, keep walking
        };
        // Never queue what's playing, never double-queue — BOTH re-read per
        // add: the walk takes seconds, and a track change or a user drag/add
        // can interleave with it (quick-review catch on the once-up-front
        // now_uri snapshot).
        if spotify::now_uri(app).as_deref() == Some(track.uri.as_str())
            || upnext::uris(app).contains(&track.uri)
        {
            continue;
        }
        upnext::append(app, track);
        added += 1;
    }
    if added == 0 {
        "no_matches".into()
    } else {
        format!("ok:{added}")
    }
}

/*
 * discovery_picks: the search window's "Something different" section. Same
 * Last.fm→Spotify similarity engine as more_like_this, but it RETURNS resolved
 * tracks instead of appending to up-next — the empty state renders them as
 * playable rows, seeded from what the user actually listens to.
 *
 * The frontend passes 1–4 seeds (rotated by its day-block) and the normalized
 * keys of everything already in the history pool, so "different" means
 * genuinely un-played. Excluding BEFORE resolving means a dead candidate never
 * costs a Spotify search.
 */
#[derive(serde::Deserialize)]
pub struct Seed {
    pub title: String,
    pub artist: String,
}

#[derive(serde::Serialize)]
pub struct DiscoveryPick {
    /// The played track this suggestion came from — the row's "Because you
    /// played X" label.
    pub seed_title: String,
    pub track: spotify::QueueTrack,
}

#[derive(serde::Serialize)]
pub struct DiscoveryResult {
    pub status: String,
    pub picks: Vec<DiscoveryPick>,
}

/// Statuses: "ok" | "no_key" (absent) | "bad_key" (rejected) | "disconnected"
/// | "offline" (transport/5xx) | "no_data" (every seed dry) | "busy". Blocking
/// (Last.fm + Spotify hops) on the dedicated pool.
#[tauri::command]
pub async fn discovery_picks(
    app: AppHandle,
    seeds: Vec<Seed>,
    exclude: Vec<String>,
) -> DiscoveryResult {
    tauri::async_runtime::spawn_blocking(move || run_discovery(&app, seeds, exclude))
        .await
        .unwrap_or_else(|e| {
            log::error!("discovery_picks task panicked: {e}");
            DISCOVERY_IN_FLIGHT.store(false, Ordering::SeqCst);
            DiscoveryResult {
                status: "offline".into(),
                picks: Vec::new(),
            }
        })
}

fn run_discovery(app: &AppHandle, seeds: Vec<Seed>, exclude: Vec<String>) -> DiscoveryResult {
    if DISCOVERY_IN_FLIGHT
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return DiscoveryResult {
            status: "busy".into(),
            picks: Vec::new(),
        };
    }
    let out = discover(app, seeds, exclude);
    DISCOVERY_IN_FLIGHT.store(false, Ordering::SeqCst);
    out
}

fn discover(app: &AppHandle, seeds: Vec<Seed>, exclude: Vec<String>) -> DiscoveryResult {
    let empty = |status: &str| {
        log::info!("discovery: {status}");
        DiscoveryResult {
            status: status.into(),
            picks: Vec::new(),
        }
    };
    if !spotify::connected(app) {
        return empty("disconnected");
    }
    let Some(key) = settings::get_string(app, "lastfm_api_key").filter(|k| !k.is_empty()) else {
        return empty("no_key");
    };

    let started = Instant::now();
    let mut cache = DiskCache::load(app);
    let seeds: Vec<Seed> = seeds.into_iter().take(SEED_CAP).collect();
    // Each seed's Last.fm list, fetched lazily in SEED_BATCH rounds.
    let mut lists: Vec<Option<Result<Vec<lastfm::SimilarTrack>, &'static str>>> =
        vec![None; seeds.len()];

    let excl: HashSet<String> = exclude.into_iter().collect();
    let mut picks: Vec<DiscoveryPick> = Vec::new();
    // Cross-set artist diversity (the "no artist radio" doctrine) + uri dedupe,
    // both spanning all seeds so two seeds can't surface the same artist twice.
    let mut picked_artists: HashSet<String> = HashSet::new();
    let mut picked_uris: HashSet<String> = HashSet::new();
    // A hard verdict (rejected key / unreachable) is worth reporting only when
    // nothing resolved; a merely dry seed is not an error.
    let mut hard_err: Option<&'static str> = None;
    let mut tries_total = 0usize;
    // Per-seed "similars/excluded/tried/picked" for the outcome log line. The
    // UI shows a failed run as one short note, so the log is the only place
    // the detail of a dry or failing walk is visible.
    let mut trace: Vec<String> = Vec::new();

    for (i, seed) in seeds.iter().enumerate() {
        if picks.len() >= PICK_TARGET || tries_total >= TRIES_TOTAL {
            break;
        }
        if started.elapsed() >= DISCOVERY_DEADLINE {
            trace.push("deadline".into());
            // Nothing found before the deadline reads as unreachable, which
            // the UI offers to retry; a dry answer would not.
            hard_err.get_or_insert("offline");
            break;
        }
        if lists[i].is_none() {
            let batch: Vec<usize> = (i..seeds.len())
                .filter(|j| lists[*j].is_none())
                .take(SEED_BATCH)
                .collect();
            for (j, r) in fetch_similars(&key, &seeds, &batch, &mut cache) {
                lists[j] = Some(r);
            }
        }
        let similars = match lists[i].take().unwrap_or(Err("no_data")) {
            Ok(v) => v,
            Err(e) => {
                trace.push(e.to_string());
                // bad_key is terminal for every seed (same key); offline breaks
                // too, so a second seed can't spend another timeout.
                if e == "bad_key" || e == "offline" {
                    hard_err = Some(e);
                    break;
                }
                continue; // "no_data" for this seed — try the next
            }
        };
        let cands = ordered(&seed.artist, &similars);
        let mut next = 0usize;
        let mut from_seed = 0usize;
        let mut tried = 0usize;
        let mut excluded = 0usize;
        loop {
            let want = (PER_SEED - from_seed)
                .min(PICK_TARGET.saturating_sub(picks.len()))
                .min(TRIES_PER_SEED - tried)
                .min(TRIES_TOTAL.saturating_sub(tries_total));
            if want == 0 || started.elapsed() >= DISCOVERY_DEADLINE {
                break;
            }
            // The next `want` candidates worth a Spotify search, resolved
            // together. Exclusion/dedupe checks are free — only a real
            // resolve attempt counts against the try budgets.
            let mut batch: Vec<&lastfm::SimilarTrack> = Vec::new();
            let mut batch_artists: HashSet<String> = HashSet::new();
            while batch.len() < want && next < cands.len() {
                let c = cands[next];
                let ca = c.artist.trim().to_lowercase();
                if batch_artists.contains(&ca) {
                    // Same artist as a candidate already in flight: hold it
                    // for the next batch, where it backs up a miss.
                    break;
                }
                next += 1;
                if excl.contains(&norm_key(&c.artist, &c.title)) {
                    excluded += 1;
                    continue;
                }
                if picked_artists.contains(&ca) {
                    continue;
                }
                batch_artists.insert(ca);
                batch.push(c);
            }
            if batch.is_empty() {
                break;
            }
            tried += batch.len();
            tries_total += batch.len();
            for (c, hit) in batch.iter().zip(resolve_all(app, &batch, &mut cache)) {
                let Some(track) = hit else {
                    continue; // no Spotify match / transient — keep walking
                };
                // Belt-and-suspenders: the resolved uri could still collide
                // with an already-picked one (two Last.fm titles resolving to
                // one track).
                if picked_uris.contains(&track.uri) {
                    continue;
                }
                picked_uris.insert(track.uri.clone());
                picked_artists.insert(c.artist.trim().to_lowercase());
                picks.push(DiscoveryPick {
                    seed_title: seed.title.clone(),
                    track,
                });
                from_seed += 1;
            }
        }
        trace.push(format!("{}/{excluded}/{tried}/{from_seed}", similars.len()));
    }
    cache.save();

    let status = if !picks.is_empty() {
        "ok"
    } else {
        hard_err.unwrap_or("no_data")
    };
    log::info!(
        "discovery: {status}, {} picks in {}ms; per seed similars/excluded/tried/picked: [{}]",
        picks.len(),
        started.elapsed().as_millis(),
        trace.join(", ")
    );
    DiscoveryResult {
        status: status.into(),
        picks,
    }
}

/// Last.fm lists for the seeds at `idx`: cached ones straight from disk, the
/// rest fetched side by side. Only non-empty answers are cached, so a dry or
/// failed seed is asked again next time.
fn fetch_similars(
    key: &str,
    seeds: &[Seed],
    idx: &[usize],
    cache: &mut DiskCache,
) -> Vec<(usize, Result<Vec<lastfm::SimilarTrack>, &'static str>)> {
    let mut out = Vec::new();
    let mut missing = Vec::new();
    for &j in idx {
        let s = &seeds[j];
        match cache.similars(&s.artist, &s.title) {
            Some(v) => out.push((j, Ok(v))),
            None => missing.push(j),
        }
    }
    let fetched: Vec<_> = std::thread::scope(|scope| {
        let handles: Vec<_> = missing
            .iter()
            .map(|&j| {
                let s = &seeds[j];
                scope.spawn(move || {
                    (
                        j,
                        lastfm::similar_or_artist_fallback(
                            key,
                            &s.title,
                            &s.artist,
                            DISCOVERY_FETCH_LIMIT,
                        ),
                    )
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or((usize::MAX, Err("offline"))))
            .filter(|(j, _)| *j != usize::MAX)
            .collect()
    });
    for (j, r) in fetched {
        if let Ok(v) = &r {
            if !v.is_empty() {
                let s = &seeds[j];
                cache.put_similars(&s.artist, &s.title, v.clone());
            }
        }
        out.push((j, r));
    }
    out
}

/// Spotify matches for `batch`, in order: cached ones from disk, the rest
/// searched side by side. A batch is at most PICK_TARGET searches, so this
/// stays well inside Spotify's rate limits; api_call still honors a
/// Retry-After. Only hits are cached, so a transient miss is retried later.
fn resolve_all(
    app: &AppHandle,
    batch: &[&lastfm::SimilarTrack],
    cache: &mut DiskCache,
) -> Vec<Option<spotify::QueueTrack>> {
    let mut out: Vec<Option<spotify::QueueTrack>> = batch
        .iter()
        .map(|c| cache.resolved(&c.artist, &c.title))
        .collect();
    let todo: Vec<usize> = (0..batch.len()).filter(|&i| out[i].is_none()).collect();
    let found: Vec<(usize, Option<spotify::QueueTrack>)> = std::thread::scope(|scope| {
        let handles: Vec<_> = todo
            .iter()
            .map(|&i| {
                let c = batch[i];
                scope.spawn(move || (i, spotify::search_best(app, &c.title, &c.artist)))
            })
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok()).collect()
    });
    for (i, hit) in found {
        if let Some(t) = &hit {
            cache.put_resolved(&batch[i].artist, &batch[i].title, t.clone());
        }
        out[i] = hit;
    }
    out
}

/*
 * The discovery disk cache: Last.fm lists per seed and Spotify matches per
 * candidate, in app_data/discovery_cache.json. Seeds are the user's
 * most-played tracks, so the same few come back block after block, and
 * before this every summon after a restart repeated the same dozen Last.fm
 * calls and Spotify searches. With a warm cache a repeat seed costs no
 * network at all.
 *
 * Lists expire after SIMILARS_TTL so the listening graph can drift; matches
 * last longer because a Spotify track uri for a given title and artist
 * rarely changes. Each map is capped, oldest first out. Losing the file
 * only costs a cold fetch, so a failed read or write is logged and ignored.
 */
const CACHE_FILE: &str = "discovery_cache.json";
const SIMILARS_TTL_MS: u64 = 3 * 24 * 60 * 60 * 1000;
const RESOLVED_TTL_MS: u64 = 30 * 24 * 60 * 60 * 1000;
const SIMILARS_CAP: usize = 200;
const RESOLVED_CAP: usize = 1000;

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct CacheData {
    #[serde(default)]
    similars: HashMap<String, (u64, Vec<lastfm::SimilarTrack>)>,
    #[serde(default)]
    resolved: HashMap<String, (u64, spotify::QueueTrack)>,
}

struct DiskCache {
    path: Option<PathBuf>,
    data: CacheData,
    now: u64,
    dirty: bool,
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl DiskCache {
    fn load(app: &AppHandle) -> Self {
        let path = app.path().app_data_dir().ok().map(|d| d.join(CACHE_FILE));
        let data = path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
        Self::with(path, data, unix_ms())
    }

    fn with(path: Option<PathBuf>, data: CacheData, now: u64) -> Self {
        let mut c = DiskCache {
            path,
            data,
            now,
            dirty: false,
        };
        c.prune();
        c
    }

    fn prune(&mut self) {
        let now = self.now;
        let (s0, r0) = (self.data.similars.len(), self.data.resolved.len());
        self.data
            .similars
            .retain(|_, (at, _)| now.saturating_sub(*at) < SIMILARS_TTL_MS);
        self.data
            .resolved
            .retain(|_, (at, _)| now.saturating_sub(*at) < RESOLVED_TTL_MS);
        cap_oldest(&mut self.data.similars, SIMILARS_CAP);
        cap_oldest(&mut self.data.resolved, RESOLVED_CAP);
        if self.data.similars.len() != s0 || self.data.resolved.len() != r0 {
            self.dirty = true;
        }
    }

    fn similars(&self, artist: &str, title: &str) -> Option<Vec<lastfm::SimilarTrack>> {
        self.data
            .similars
            .get(&norm_key(artist, title))
            .map(|(_, v)| v.clone())
    }

    fn put_similars(&mut self, artist: &str, title: &str, v: Vec<lastfm::SimilarTrack>) {
        self.data
            .similars
            .insert(norm_key(artist, title), (self.now, v));
        self.dirty = true;
    }

    fn resolved(&self, artist: &str, title: &str) -> Option<spotify::QueueTrack> {
        self.data
            .resolved
            .get(&norm_key(artist, title))
            .map(|(_, t)| t.clone())
    }

    fn put_resolved(&mut self, artist: &str, title: &str, t: spotify::QueueTrack) {
        self.data
            .resolved
            .insert(norm_key(artist, title), (self.now, t));
        self.dirty = true;
    }

    fn save(&mut self) {
        if !self.dirty {
            return;
        }
        self.prune();
        let Some(path) = &self.path else { return };
        match serde_json::to_string(&self.data) {
            Ok(json) => {
                if let Err(e) = settings::write_atomic(path, json.as_bytes()) {
                    log::warn!("discovery: cache write failed: {e}");
                }
            }
            Err(e) => log::warn!("discovery: cache encode failed: {e}"),
        }
        self.dirty = false;
    }
}

/// Drop the oldest entries until at most `cap` remain.
fn cap_oldest<V>(map: &mut HashMap<String, (u64, V)>, cap: usize) {
    if map.len() <= cap {
        return;
    }
    let mut ages: Vec<(u64, String)> = map.iter().map(|(k, (at, _))| (*at, k.clone())).collect();
    ages.sort();
    for (_, k) in ages.into_iter().take(map.len() - cap) {
        map.remove(&k);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sim(artist: &str, title: &str) -> lastfm::SimilarTrack {
        lastfm::SimilarTrack {
            title: title.into(),
            artist: artist.into(),
            score: 0.5,
        }
    }

    #[test]
    fn cache_round_trips_and_keys_like_the_exclude_contract() {
        let mut c = DiskCache::with(None, CacheData::default(), 1_000);
        c.put_similars("Red Velvet", "Psycho", vec![sim("SEULGI", "28 Reasons")]);
        let hit = c
            .similars("  red velvet ", "PSYCHO")
            .expect("normalized hit");
        assert_eq!(hit[0].title, "28 Reasons");
        assert!(c.similars("Red Velvet", "Feel My Rhythm").is_none());
    }

    #[test]
    fn cache_expires_and_caps_oldest_first() {
        let mut data = CacheData::default();
        data.similars.insert("old".into(), (0, vec![sim("a", "b")]));
        data.similars
            .insert("fresh".into(), (SIMILARS_TTL_MS, vec![sim("a", "b")]));
        let c = DiskCache::with(None, data, SIMILARS_TTL_MS + 1);
        assert!(c.data.similars.contains_key("fresh"));
        assert!(!c.data.similars.contains_key("old"));
        assert!(c.dirty);

        let mut m: HashMap<String, (u64, ())> = HashMap::new();
        for i in 0..5u64 {
            m.insert(format!("k{i}"), (i, ()));
        }
        cap_oldest(&mut m, 3);
        let mut left: Vec<_> = m.keys().cloned().collect();
        left.sort();
        assert_eq!(left, vec!["k2", "k3", "k4"]);
    }

    #[test]
    fn ordered_puts_other_artists_first_and_drops_the_mush_tail() {
        let list = vec![
            lastfm::SimilarTrack {
                title: "same".into(),
                artist: "Seed".into(),
                score: 0.9,
            },
            lastfm::SimilarTrack {
                title: "other".into(),
                artist: "Other".into(),
                score: 0.5,
            },
            lastfm::SimilarTrack {
                title: "mush".into(),
                artist: "Filler".into(),
                score: 0.01,
            },
        ];
        let got: Vec<_> = ordered("seed", &list)
            .iter()
            .map(|s| s.title.as_str())
            .collect();
        assert_eq!(got, vec!["other", "same"]);
    }
}
