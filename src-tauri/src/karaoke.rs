//! Local word-karaoke store, capture, and align.

use crate::acoustic;
use crate::align::{self, TimeMap, Word};
use crate::lyrics;
use crate::media::NowPlaying;
use crate::settings::{self, write_atomic};
use crate::spotify;
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Clone, Serialize)]
pub struct SyncStatus {
    phase: &'static str,
    detail: &'static str,
}
static SYNC_STATES: OnceLock<Mutex<VecDeque<(String, SyncStatus)>>> = OnceLock::new();
fn sync_states() -> std::sync::MutexGuard<'static, VecDeque<(String, SyncStatus)>> {
    SYNC_STATES
        .get_or_init(|| Mutex::new(VecDeque::new()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
fn sync_state(key: &str, phase: &'static str, detail: &'static str) {
    update_sync_state(key, phase, detail, true);
}
fn finish_sync_state(key: &str, phase: &'static str, detail: &'static str) {
    update_sync_state(key, phase, detail, false);
}
fn update_sync_state(key: &str, phase: &'static str, detail: &'static str, preserve_worker: bool) {
    let mut states = sync_states();
    // An interrupted replay must not hide a worker already finishing this song.
    if preserve_worker
        && phase == "waiting"
        && states
            .iter()
            .any(|(k, s)| k == key && s.phase == "processing")
    {
        return;
    }
    states.retain(|(k, _)| k != key);
    states.push_back((key.to_string(), SyncStatus { phase, detail }));
    while states.len() > 64 {
        states.pop_front();
    }
}

/// Snapshot covers mounts after an event and track switches during alignment.
/// In-memory only; no model, cache I/O, or settings work on this command.
#[tauri::command]
pub async fn karaoke_status(
    artist: String,
    title: String,
    album: String,
    duration_ms: i64,
) -> SyncStatus {
    let key = lyrics::key_for_ms(&artist, &title, &album, duration_ms);
    let recording = lock_rec().as_ref().is_some_and(|r| r.key == key);
    let stored = sync_states()
        .iter()
        .find(|(k, _)| k == &key)
        .map(|(_, s)| s.clone());
    if stored
        .as_ref()
        .is_some_and(|s| matches!(s.phase, "saved" | "processing"))
    {
        return stored.unwrap();
    }
    if recording {
        return SyncStatus {
            phase: "learning",
            detail: "Learning timing… Keep Palette visible and listen through the end.",
        };
    }
    stored.unwrap_or(SyncStatus {
        phase: "waiting",
        detail: "Play from the beginning with Palette visible to learn word timing.",
    })
}

const CACHE_MAX_FILES: usize = 500;
const TARGET_HZ: u32 = 16_000;
const MAX_SAMPLES: usize = 16_000 * 60 * 8;
const ARM_NEAR_START_MS: i64 = 8_000;
const PEAK_ABORT: f32 = 1e-3;
const MIN_LINE_COVERAGE: u32 = 30;
/// v5 binds word times to the exact synced LRC as well as the aligner recipe.
/// Older files lack source identity and must be recorded again.
const STORE_V: u32 = 5;
fn recipe() -> &'static str {
    if acoustic::assets().is_some() {
        acoustic::RECIPE
    } else {
        align::Stages::RECIPE
    }
}

/// A delivery deficit cannot distinguish buffered audio from missing audio.
/// Above normal packet jitter, discard rather than invent silence.
const GAP_ABORT_MS: u64 = 400;
/// Staleness projection cap: a position stamped longer ago than this is
/// not extrapolated further (the pair is the player's, not the clock's).
const STALE_CAP_MS: i64 = 5_000;
/// Pairs inside the last stretch of a track are not evidence: GSMTC clamps
/// the position at the duration while captured audio keeps flowing, so
/// every completed dump's final anchor sat +86..+471ms off an otherwise
/// ±10ms fit, and on "Who Knows" that pair crossed the seek band and
/// dropped a clean recording (2026-09-04).
const END_GUARD_MS: i64 = 1_500;
/// A seek is called only after this many CONSECUTIVE pairs disagree with
/// the fit — one glitched pair is a glitch; a real scrub keeps reporting
/// the new offset every ~4.5s.
const SEEK_CONFIRM: u8 = 2;
/// settings.json switch for the evidence dump (docs/specs/2026-09-04).
const DUMP_SETTING: &str = "karaokeDump";
const DUMP_DIR: &str = "karaoke-dumps";
/// ~8MB of i16 PCM per four-minute dump; keep the last few only.
const DUMP_MAX: usize = 5;

#[derive(Serialize, Deserialize)]
struct StoreFile {
    v: u32,
    #[serde(default)]
    recipe: String,
    #[serde(default)]
    synced: Option<String>,
    words: Vec<Word>,
    #[serde(default)]
    detail_v: u32,
}

#[derive(Serialize, Clone)]
pub struct KaraokeReady {
    pub artist: String,
    pub title: String,
    pub album: String,
    pub duration_ms: i64,
    pub words: Vec<Word>,
}

struct Rec {
    key: String,
    artist: String,
    title: String,
    album: String,
    duration_ms: i64,
    origin_ms: i64,
    rate_in: u32,
    phase: u32,
    acc: f32,
    n: u32,
    samples: Vec<f32>,
    peak: f32,
    /// Wall clock at the first delivered block + input frames received
    /// since: the pair that detects a delivery gap (see GAP_ABORT_MS).
    started: Option<Instant>,
    received: u64,
    /// (output sample index, position_ms) from every fresh pair the media
    /// loop saw while recording — the TimeMap's evidence.
    anchors: Vec<(usize, i64)>,
    /// position_at_ms of the last pair consumed (the arming pair counts),
    /// so a beat that merely re-emits the same pair adds nothing.
    last_anchor_at: i64,
    /// Consecutive pairs that disagreed with the fit (see SEEK_CONFIRM).
    seek_strikes: u8,
}

enum Anchor {
    Skip,
    Added,
    /// (residual ms, anchors so far, reported position) — logged so a
    /// false positive is diagnosable from the log alone.
    Seek(f64, usize, i64),
}

impl Rec {
    /// A lone unresolved seek strike is unsafe at stop/track change even
    /// though live detection waits for a second pair. Never count it as a miss.
    fn can_finalize(&self) -> bool {
        let pcm_ms = self.samples.len() as i64 * 1000 / TARGET_HZ as i64;
        self.seek_strikes == 0 && listen_enough(pcm_ms, self.origin_ms, self.duration_ms)
    }

    /// Turn a fresh pair into an anchor, or flag a seek. The pair's
    /// staleness (now − position_at_ms) says how many input frames ago the
    /// position was true; that input index maps onto the 16kHz grid.
    fn anchor(&mut self, np: &NowPlaying) -> Anchor {
        if np.position_at_ms <= 0 || np.position_at_ms == self.last_anchor_at || self.rate_in == 0 {
            return Anchor::Skip;
        }
        self.last_anchor_at = np.position_at_ms;
        if self.duration_ms > 0 && np.position_ms >= self.duration_ms - END_GUARD_MS {
            return Anchor::Skip;
        }
        let stale = (unix_ms() - np.position_at_ms).clamp(0, STALE_CAP_MS);
        let idx_in = (self.received as i64 - stale * self.rate_in as i64 / 1000).max(0) as u64;
        let idx = (idx_in * TARGET_HZ as u64 / self.rate_in as u64) as usize;
        if let Some(residual) = seek_residual(&self.anchors, idx, np.position_ms) {
            self.seek_strikes = self.seek_strikes.saturating_add(1);
            if self.seek_strikes >= SEEK_CONFIRM {
                return Anchor::Seek(residual, self.anchors.len(), np.position_ms);
            }
            log::info!(
                "karaoke: pair {}ms sits {:+.0}ms off the fit during {} — waiting for a second",
                np.position_ms,
                residual,
                self.title
            );
            return Anchor::Skip;
        }
        self.seek_strikes = 0;
        self.anchors.push((idx, np.position_ms));
        Anchor::Added
    }
    /// Box-decimate one input frame onto the TARGET_HZ grid.
    #[inline]
    fn push(&mut self, sample: f32) -> bool {
        let s = if sample.is_finite() { sample } else { 0.0 };
        self.peak = self.peak.max(s.abs());
        self.phase = self.phase.saturating_add(TARGET_HZ);
        self.acc += s;
        self.n += 1;
        if self.phase >= self.rate_in {
            self.phase -= self.rate_in;
            if self.samples.len() >= MAX_SAMPLES {
                return false;
            }
            self.samples.push(self.acc / self.n.max(1) as f32);
            self.acc = 0.0;
            self.n = 0;
        }
        true
    }
}

fn unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

static RECORDING: AtomicBool = AtomicBool::new(false);
static ALIGNING: AtomicBool = AtomicBool::new(false);

fn rec_slot() -> &'static Mutex<Option<Rec>> {
    static S: OnceLock<Mutex<Option<Rec>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(None))
}

fn misses() -> &'static Mutex<HashSet<String>> {
    static S: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashSet::new()))
}

fn lock_rec() -> std::sync::MutexGuard<'static, Option<Rec>> {
    rec_slot()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn lock_misses() -> std::sync::MutexGuard<'static, HashSet<String>> {
    misses()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn karaoke_dir(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("karaoke"))
}

fn lyrics_dir(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .map(|d| d.join("lyrics"))
        .unwrap_or_else(|_| std::env::temp_dir().join("pulse-lyrics"))
}

pub fn load(
    dir: &Path,
    artist: &str,
    title: &str,
    album: &str,
    duration_ms: i64,
    synced: Option<&str>,
) -> Vec<Word> {
    let key = lyrics::key_for_ms(artist, title, album, duration_ms);
    read_file(&dir.join(format!("{key}.json")), synced)
}

fn read_file(path: &Path, synced: Option<&str>) -> Vec<Word> {
    // No current source means no usable word cache; keep the file for a
    // later successful lyrics fetch instead of deleting it on a transient miss.
    let Some(synced) = synced.filter(|s| !s.trim().is_empty()) else {
        return Vec::new();
    };
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(file) = serde_json::from_str::<StoreFile>(&raw) else {
        return Vec::new();
    };
    // Only numeric caches need the revised pronunciation targets. Preserve
    // unrelated songs and their already accurate acoustic timings.
    let stale_digits = file.detail_v < 2
        && file
            .words
            .iter()
            .any(|w| w.text.chars().any(|c| c.is_ascii_digit()));
    if stale_digits
        || file.v != STORE_V
        || file.recipe != recipe()
        || file.synced.as_deref() != Some(synced)
    {
        let _ = std::fs::remove_file(path);
        return Vec::new();
    }
    file.words
}

fn write_file(dir: &Path, key: &str, synced: &str, words: &[Word]) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let json = serde_json::to_vec(&StoreFile {
        v: STORE_V,
        detail_v: 2,
        recipe: recipe().to_string(),
        synced: Some(synced.to_string()),
        words: words.to_vec(),
    })
    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    write_atomic(&dir.join(format!("{key}.json")), &json)?;
    evict_old(dir);
    Ok(())
}

fn evict_old(cache_dir: &Path) {
    let Ok(entries) = std::fs::read_dir(cache_dir) else {
        return;
    };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
        .filter_map(|e| {
            let e = e.ok()?;
            let meta = e.metadata().ok()?;
            Some((meta.modified().ok()?, e.path()))
        })
        .collect();
    if files.len() <= CACHE_MAX_FILES {
        return;
    }
    files.sort_by_key(|(t, _)| *t);
    let excess = files.len().saturating_sub(CACHE_MAX_FILES);
    for (_, path) in files.into_iter().take(excess) {
        let _ = std::fs::remove_file(path);
    }
}

fn has_file(dir: &Path, key: &str, synced: Option<&str>) -> bool {
    let path = dir.join(format!("{key}.json"));
    if read_file(&path, synced).is_empty() {
        return false;
    }
    // Old accurate word caches remain readable while the next full listen
    // upgrades acoustic detail. Mark the attempt even for non-English songs.
    crate::acoustic::assets().is_none()
        || std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<StoreFile>(&bytes).ok())
            .is_some_and(|file| file.detail_v >= 1)
}

/// Cache eligibility is stricter than the 55% diagnostic-dump threshold:
/// mapped PCM must reach within END_GUARD_MS of a known track end.
/// Incomplete listens remain retryable, regardless of alignment coverage.
fn cache_complete(pcm_end_ms: i64, duration_ms: i64) -> bool {
    duration_ms > 0 && pcm_end_ms >= duration_ms.saturating_sub(END_GUARD_MS)
}

/// Wall time alone cannot tell delayed buffered delivery from lost audio.
/// Reject either ambiguous case; never synthesize samples to cover a deficit.
fn ambiguous_gap(expected_frames: u64, received_frames: u64, sample_rate: u32) -> bool {
    expected_frames.saturating_sub(received_frames) > sample_rate as u64 * GAP_ABORT_MS / 1000
}

pub(crate) fn listen_enough(pcm_ms: i64, origin_ms: i64, duration_ms: i64) -> bool {
    let covered = pcm_ms.saturating_add(origin_ms);
    if duration_ms <= 0 {
        return pcm_ms >= 30_000;
    }
    let need = (duration_ms * 55 / 100).max(12_000);
    covered >= need
}

fn line_coverage(lines: &[align::TimedLine], words: &[Word]) -> u32 {
    let mut total = 0u32;
    let mut hit = 0u32;
    for line in lines.iter().filter(|line| !line.text.trim().is_empty()) {
        total += 1;
        // A calibrated onset can precede the LRC stamp. Membership is the
        // source line identity, never the word's position in a time window.
        if words.iter().any(|w| w.line_t == Some(line.t)) {
            hit += 1;
        }
    }
    (hit * 100).checked_div(total).unwrap_or(0)
}

pub fn observe(app: &AppHandle, np: &NowPlaying) {
    if np.player == "none" || np.status != "playing" {
        return;
    }
    if np.title.is_empty() && np.artist.is_empty() {
        return;
    }
    if np.duration_ms <= 0 {
        return;
    }
    if spotify::is_remote(app) {
        // Audio moved to a phone/speaker: local capture hears nothing from
        // here on. Finalize whatever was recording instead of leaving it to
        // collect silence until playback comes back local (review).
        RECORDING.store(false, Ordering::Relaxed);
        if let Some(rec) = lock_rec().take() {
            try_commit(app, rec);
        }
        return;
    }
    let key = lyrics::key_for_ms(&np.artist, &np.title, &np.album, np.duration_ms);
    let old = {
        let mut slot = lock_rec();
        if matches!(slot.as_ref(), Some(rec) if rec.key == key) {
            // Same track: this beat's pair is evidence for the time map —
            // unless it says the user seeked, which no single-origin or
            // fitted map can absorb: drop the recording (NOT a miss; the
            // next clean listen records it).
            if let Some(rec) = slot.as_mut() {
                if let Anchor::Seek(residual, n, pos) = rec.anchor(np) {
                    RECORDING.store(false, Ordering::Relaxed);
                    log::info!(
                        "karaoke: seek during {} — dropping this recording (pair says {}ms, fit predicted {:+.0}ms off it, {} anchors)",
                        rec.title,
                        pos,
                        residual,
                        n
                    );
                    sync_state(
                        &rec.key,
                        "waiting",
                        "Seeking interrupted learning. Play from the beginning to try again.",
                    );
                    *slot = None;
                }
            }
            return;
        }
        RECORDING.store(false, Ordering::Relaxed);
        slot.take()
    };
    if let Some(rec) = old {
        try_commit(app, rec);
    }
    if sync_states()
        .iter()
        .any(|(k, s)| k == &key && s.phase == "processing")
    {
        return;
    }
    if lock_misses().contains(&key) {
        return;
    }
    // The raw pair is the player's last push (Spotify ~every 5s, Apple
    // Music floored to whole seconds): project it to now so the origin is
    // the position the first captured sample actually belongs to. The
    // aligner's stamp-anchored offset absorbs what's left.
    let stale = if np.position_at_ms > 0 {
        (unix_ms() - np.position_at_ms).clamp(0, STALE_CAP_MS)
    } else {
        0
    };
    let origin_ms = (np.position_ms + stale).max(0);
    if origin_ms >= ARM_NEAR_START_MS {
        return;
    }
    // Only eligible near-start listens need lyrics/cache disk reads.
    let Some(dir) = karaoke_dir(app) else {
        return;
    };
    let synced = lyrics::cached_synced(
        &lyrics_dir(app),
        &np.artist,
        &np.title,
        &np.album,
        np.duration_ms,
    );
    if has_file(&dir, &key, synced.as_deref()) {
        return;
    }
    let rec = Rec {
        key,
        artist: np.artist.clone(),
        title: np.title.clone(),
        album: np.album.clone(),
        duration_ms: np.duration_ms,
        origin_ms,
        rate_in: 0,
        phase: 0,
        acc: 0.0,
        n: 0,
        samples: Vec::with_capacity(TARGET_HZ as usize * 240),
        peak: 0.0,
        started: None,
        received: 0,
        anchors: Vec::new(),
        last_anchor_at: np.position_at_ms,
        seek_strikes: 0,
    };
    *lock_rec() = Some(rec);
    RECORDING.store(true, Ordering::Relaxed);
}

pub fn on_capture_stop(app: &AppHandle) {
    RECORDING.store(false, Ordering::Relaxed);
    if let Some(rec) = lock_rec().take() {
        try_commit(app, rec);
    }
}

/// Feed one capture block of mono frames. Called from the audio thread;
/// one lock per block (a packet is ~10–20ms of audio), never per sample —
/// the per-sample version took the mutex 48k times a second on the
/// realtime thread.
pub fn push_frames(frames: &[f32], sample_rate: u32) {
    if frames.is_empty() || sample_rate == 0 || !RECORDING.load(Ordering::Relaxed) {
        return;
    }
    let mut slot = rec_slot()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(rec) = slot.as_mut() else {
        return;
    };
    if rec.rate_in == 0 {
        rec.rate_in = sample_rate;
        rec.started = Some(Instant::now());
    } else if rec.rate_in != sample_rate {
        log::info!(
            "karaoke: capture rate changed {}→{} during {} — dropping this recording",
            rec.rate_in,
            sample_rate,
            rec.title
        );
        RECORDING.store(false, Ordering::Relaxed);
        *slot = None;
        return;
    }
    // Buffered delayed packets and actual missing audio look identical
    // here. Either invalidates this recording, without poisoning retries.
    if let Some(t0) = rec.started {
        let expected = (t0.elapsed().as_secs_f64() * sample_rate as f64) as u64;
        if ambiguous_gap(expected, rec.received, sample_rate) {
            log::info!(
                "karaoke: ambiguous delivery gap during {} — dropping recording",
                rec.title
            );
            RECORDING.store(false, Ordering::Relaxed);
            *slot = None;
            return;
        }
    }
    for &s in frames {
        if !rec.push(s) {
            log::info!("karaoke: {} ran past MAX_SAMPLES — dropping", rec.title);
            RECORDING.store(false, Ordering::Relaxed);
            *slot = None;
            return;
        }
    }
    rec.received += frames.len() as u64;
}

// Writing opt-in diagnostic PCM must not contend with model inference.
static SAVING_EVIDENCE: AtomicBool = AtomicBool::new(false);
struct EvidenceGuard;
impl EvidenceGuard {
    fn acquire() -> Option<Self> {
        (!SAVING_EVIDENCE.swap(true, Ordering::SeqCst)).then(|| Self)
    }
}
impl Drop for EvidenceGuard {
    fn drop(&mut self) {
        SAVING_EVIDENCE.store(false, Ordering::SeqCst);
    }
}

struct AlignGuard;

impl Drop for AlignGuard {
    fn drop(&mut self) {
        ALIGNING.store(false, Ordering::SeqCst);
    }
}

fn try_commit(app: &AppHandle, rec: Rec) {
    if !rec.can_finalize() {
        sync_state(
            &rec.key,
            "waiting",
            "Listen interrupted. Play from the beginning to try again.",
        );
        // Opt-in diagnostics must survive a short replay or a suspect final
        // position pair. These are explicitly untrusted audio, never cached
        // timings and never a karaoke-ready event. Disk work has its own gate.
        if rec.samples.len() >= TARGET_HZ as usize * 15 {
            let Some(guard) = EvidenceGuard::acquire() else {
                log::warn!("karaoke: evidence writer busy for {}", rec.title);
                return;
            };
            let handle = app.clone();
            let result = std::thread::Builder::new().name("karaoke-evidence".into()).spawn(move || {
                let _guard = guard;
                if !settings::get_bool(&handle, DUMP_SETTING, false) { return; }
                let synced = lyrics::cached_synced(&lyrics_dir(&handle), &rec.artist, &rec.title, &rec.album, rec.duration_ms).unwrap_or_default();
                let Ok(root) = handle.path().app_data_dir() else { return; };
                let dir = root.join("karaoke-diagnostics").join(&rec.key);
                let map = TimeMap::fit(&rec.anchors, TARGET_HZ, rec.origin_ms);
                if let Err(e) = write_dump(&dir, &rec, &synced, &[], &map) {
                    log::warn!("karaoke: rejected evidence dump failed ({e})");
                } else {
                    log::info!("karaoke: retained untrusted diagnostic audio for {} at {} ({} unresolved strikes); no cache update", rec.title, dir.display(), rec.seek_strikes);
                }
            });
            if let Err(e) = result {
                log::warn!("karaoke: evidence thread spawn failed ({e})");
            }
        }
        return;
    }
    let map = TimeMap::fit(&rec.anchors, TARGET_HZ, rec.origin_ms);
    if rec.peak < PEAK_ABORT && cache_complete(map.pos_ms(rec.samples.len()), rec.duration_ms) {
        sync_state(
            &rec.key,
            "failed",
            "No usable local audio was captured. Play on this computer to learn timing.",
        );
        lock_misses().insert(rec.key);
        log::info!("karaoke: silence on {} — leaving line karaoke", rec.title);
        return;
    }
    if ALIGNING.swap(true, Ordering::SeqCst) {
        sync_state(
            &rec.key,
            "waiting",
            "Another track is finishing sync. This track needs a fresh listen afterward.",
        );
        // Neither cached nor a miss: the track re-records on its next listen.
        log::info!("karaoke: align busy — skipping {} this listen", rec.title);
        return;
    }
    let lyrics_dir = lyrics_dir(app);
    let Some(karaoke_dir) = karaoke_dir(app) else {
        ALIGNING.store(false, Ordering::SeqCst);
        return;
    };
    // Opt-in evidence dump (settings.json "karaokeDump": true) — the
    // offline scorer's input. Installed builds have no env vars.
    let dump_dir = if settings::get_bool(app, DUMP_SETTING, false) {
        app.path()
            .app_data_dir()
            .ok()
            .map(|d| d.join(DUMP_DIR).join(&rec.key))
    } else {
        None
    };
    sync_state(
        &rec.key,
        "processing",
        "Finishing sync… Your listen is captured; word timing is being aligned.",
    );
    let status_key = rec.key.clone();
    let handle = app.clone();
    // Move the guard into the closure: a failed spawn drops the closure
    // and releases ALIGNING too, even though the thread body never ran.
    let guard = AlignGuard;
    let result = std::thread::Builder::new()
        .name("karaoke-align".into())
        .spawn(move || {
            let _g = guard;
            commit_sync(&handle, rec, &lyrics_dir, &karaoke_dir, dump_dir.as_deref());
        });
    if let Err(e) = result {
        sync_state(
            &status_key,
            "failed",
            "Couldn’t start sync processing. Try again on your next listen.",
        );
        log::warn!("karaoke: align thread spawn failed ({e})");
    }
}

/// True when a fresh pair sits further from the running fit than a seek
/// residual. Needs two anchors to have a fit at all.
#[cfg(test)]
fn seek_detected(anchors: &[(usize, i64)], idx: usize, position_ms: i64) -> bool {
    seek_residual(anchors, idx, position_ms).is_some()
}

/// The signed residual (fit − reported) when it exceeds the seek band;
/// None below it or with fewer than two anchors to fit.
fn seek_residual(anchors: &[(usize, i64)], idx: usize, position_ms: i64) -> Option<f64> {
    if anchors.len() < 2 {
        return None;
    }
    let map = TimeMap::fit(anchors, TARGET_HZ, 0);
    let r = map.residual_ms(idx, position_ms);
    (r.abs() > align::SEEK_RESIDUAL_MS).then_some(r)
}

#[derive(Serialize)]
struct DumpMeta<'a> {
    artist: &'a str,
    title: &'a str,
    album: &'a str,
    duration_ms: i64,
    rate_in: u32,
    origin_ms: i64,
    anchors: &'a [(usize, i64)],
    map: &'a TimeMap,
}

/// pcm.i16 + lyrics.lrc + words.json + meta.json under `dir`; the parent
/// keeps at most DUMP_MAX dump directories (oldest evicted).
fn write_dump(
    dir: &Path,
    rec: &Rec,
    lrc: &str,
    words: &[Word],
    map: &TimeMap,
) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let mut pcm = Vec::with_capacity(rec.samples.len() * 2);
    for &s in &rec.samples {
        let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
        pcm.extend_from_slice(&v.to_le_bytes());
    }
    write_atomic(&dir.join("pcm.i16"), &pcm)?;
    write_atomic(&dir.join("lyrics.lrc"), lrc.as_bytes())?;
    let to_io = |e: serde_json::Error| std::io::Error::new(std::io::ErrorKind::InvalidData, e);
    let words_json = serde_json::to_vec(&StoreFile {
        v: STORE_V,
        detail_v: 2,
        recipe: recipe().to_string(),
        synced: Some(lrc.to_string()),
        words: words.to_vec(),
    })
    .map_err(to_io)?;
    write_atomic(&dir.join("words.json"), &words_json)?;
    let meta = serde_json::to_vec_pretty(&DumpMeta {
        artist: &rec.artist,
        title: &rec.title,
        album: &rec.album,
        duration_ms: rec.duration_ms,
        rate_in: rec.rate_in,
        origin_ms: rec.origin_ms,
        anchors: &rec.anchors,
        map,
    })
    .map_err(to_io)?;
    write_atomic(&dir.join("meta.json"), &meta)?;
    if let Some(parent) = dir.parent() {
        evict_dump_dirs(parent);
    }
    Ok(())
}

fn evict_dump_dirs(parent: &Path) {
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    let mut dirs: Vec<(std::time::SystemTime, PathBuf)> = entries
        .filter_map(|e| {
            let e = e.ok()?;
            let meta = e.metadata().ok()?;
            if !meta.is_dir() {
                return None;
            }
            Some((meta.modified().ok()?, e.path()))
        })
        .collect();
    if dirs.len() <= DUMP_MAX {
        return;
    }
    dirs.sort_by_key(|(t, _)| *t);
    let excess = dirs.len() - DUMP_MAX;
    for (_, path) in dirs.into_iter().take(excess) {
        let _ = std::fs::remove_dir_all(path);
    }
}

fn commit_sync(
    app: &AppHandle,
    rec: Rec,
    lyrics_dir: &Path,
    karaoke_dir: &Path,
    dump_dir: Option<&Path>,
) {
    commit_recording(rec, lyrics_dir, karaoke_dir, dump_dir, |ready| {
        let _ = app.emit("karaoke-ready", ready);
    });
}

struct SyncAttempt(String);
impl Drop for SyncAttempt {
    fn drop(&mut self) {
        let mut states = sync_states();
        if let Some((_, state)) = states.iter_mut().find(|(k, _)| k == &self.0) {
            if state.phase == "processing" {
                *state = SyncStatus {
                    phase: "failed",
                    detail: "Couldn’t save word sync. This listen did not produce usable timing.",
                };
            }
        }
    }
}

fn commit_recording(
    rec: Rec,
    lyrics_dir: &Path,
    karaoke_dir: &Path,
    dump_dir: Option<&Path>,
    publish: impl FnOnce(KaraokeReady),
) {
    let _status = SyncAttempt(rec.key.clone());
    let Some(synced) = lyrics::cached_synced(
        lyrics_dir,
        &rec.artist,
        &rec.title,
        &rec.album,
        rec.duration_ms,
    ) else {
        sync_state(
            &rec.key,
            "failed",
            "Timed lyrics weren’t available for this recording.",
        );
        return;
    };
    if has_file(karaoke_dir, &rec.key, Some(&synced)) {
        sync_state(&rec.key, "saved", "Word sync saved on this device.");
        return;
    }
    let lines = align::parse_lrc(&synced);
    if lines.is_empty() {
        return;
    }
    let map = TimeMap::fit(&rec.anchors, TARGET_HZ, rec.origin_ms);
    log::info!(
        "karaoke: time map for {} — {} anchors, origin {}ms, intercept {:.0}ms, rms {:.0}ms{}{}",
        rec.title,
        map.n_anchors,
        rec.origin_ms,
        map.intercept_ms,
        map.residual_rms_ms,
        if map.clamped { ", slope clamped" } else { "" },
        if map.from_origin {
            ", origin fallback"
        } else {
            ""
        },
    );
    let words = if let Some(dir) = acoustic::assets() {
        // Identical quantization to write_dump: replay and production share
        // the same PCM representation, including clipping and scale.
        let pcm: Vec<i16> = rec
            .samples
            .iter()
            .map(|s| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
            .collect();
        let started = Instant::now();
        let result = acoustic::AcousticAligner::load(
            &dir.join("mms-fa-int8.onnx"),
            &dir.join("onnxruntime.dll"),
        )
        .and_then(|mut model| model.align(&pcm, &lines, &map));
        match result {
            Ok(words) => {
                log::info!(
                    "karaoke: acoustic aligned {} words for {} in {:.2}s",
                    words.len(),
                    rec.title,
                    started.elapsed().as_secs_f64()
                );
                words
            }
            Err(e) => {
                // Never cache guessed timings under the acoustic recipe.
                // Retain diagnostic audio and let a later listen retry.
                sync_state(
                    &rec.key,
                    "failed",
                    if e.contains("unrepresentable") {
                        "Some lyric text couldn’t be aligned. Replaying alone won’t resolve this text issue."
                    } else {
                        "Couldn’t align this track’s vocals. Word sync was not saved."
                    },
                );
                log::warn!("karaoke: acoustic alignment failed for {} ({e})", rec.title);
                if let Some(dir) = dump_dir {
                    if let Err(e) = write_dump(dir, &rec, &synced, &[], &map) {
                        log::warn!("karaoke: failure dump failed ({e})");
                    }
                }
                return;
            }
        }
    } else {
        align::align(&rec.samples, TARGET_HZ, &lines, &map)
    };
    // Evidence first: a coverage miss below still leaves something to score.
    if let Some(dir) = dump_dir {
        match write_dump(dir, &rec, &synced, &words, &map) {
            Ok(()) => log::info!("karaoke: dumped {} to {}", rec.title, dir.display()),
            Err(e) => log::warn!("karaoke: dump failed ({e})"),
        }
    }
    if !cache_complete(map.pos_ms(rec.samples.len()), rec.duration_ms) {
        finish_sync_state(
            &rec.key,
            "waiting",
            "The recording ended early. Listen from the beginning through the end.",
        );
        log::info!(
            "karaoke: incomplete {} — dump only, retry next listen",
            rec.title
        );
        return;
    }
    if line_coverage(&lines, &words) < MIN_LINE_COVERAGE {
        sync_state(
            &rec.key,
            "failed",
            "Not enough vocals matched the lyrics. Keeping line sync for this track.",
        );
        lock_misses().insert(rec.key);
        log::info!("karaoke: align missed {} — leaving line karaoke", rec.title);
        return;
    }
    if let Err(e) = write_file(karaoke_dir, &rec.key, &synced, &words) {
        sync_state(
            &rec.key,
            "failed",
            "Timing was aligned but couldn’t be saved on this device.",
        );
        log::warn!("karaoke: persist failed ({e})");
        return;
    }
    sync_state(&rec.key, "saved", "Word sync saved on this device.");
    publish(KaraokeReady {
        artist: rec.artist,
        title: rec.title,
        album: rec.album,
        duration_ms: rec.duration_ms,
        words,
    });
}

// ── Word lead (docs/specs/2026-09-04-word-lead-nudge.md) ──
//
// How far BEFORE a word's aligned onset the frontend fires its wipe, on
// top of the per-player line lead. The wipe ramps over 90ms; the user's
// preferred visual lead is separate from measured acoustic onset error.
// Thien tunes it with two hotkeys; the value persists across model changes.

pub const WORD_LEAD_DEFAULT_MS: i64 = 160;
const WORD_LEAD_STEP_MS: i64 = 20;
const WORD_LEAD_RANGE_MS: (i64, i64) = (-200, 400);
const WORD_LEAD_SETTING: &str = "wordLeadMs";

fn clamp_word_lead(ms: i64) -> i64 {
    ms.clamp(WORD_LEAD_RANGE_MS.0, WORD_LEAD_RANGE_MS.1)
}

/// One nudge: `earlier` raises the lead by a step, otherwise lowers it.
fn nudged_word_lead(current: i64, earlier: bool) -> i64 {
    clamp_word_lead(if earlier {
        current + WORD_LEAD_STEP_MS
    } else {
        current - WORD_LEAD_STEP_MS
    })
}

pub fn word_lead_ms(app: &AppHandle) -> i64 {
    settings::get_value(app, WORD_LEAD_SETTING)
        .and_then(|v| v.as_i64())
        .map(clamp_word_lead)
        .unwrap_or(WORD_LEAD_DEFAULT_MS)
}

/// Hotkey action: step, persist, tell the webviews ("word-lead").
pub fn nudge_word_lead(app: &AppHandle, earlier: bool) {
    let next = nudged_word_lead(word_lead_ms(app), earlier);
    settings::set_value(app, WORD_LEAD_SETTING, serde_json::json!(next));
    log::info!("karaoke: word lead {next}ms");
    let _ = app.emit("word-lead", next);
}

/// Seed for the frontend mirror (src/lib/wordLead.ts).
#[tauri::command]
pub async fn word_lead(app: AppHandle) -> i64 {
    word_lead_ms(&app)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::align::TimedLine;
    static ALIGNMENT_FLAG_TEST_GATE: Mutex<()> = Mutex::new(());

    #[test]
    fn short_evidence_can_save_while_another_song_is_aligning() {
        let _test_gate = ALIGNMENT_FLAG_TEST_GATE.lock().unwrap();
        ALIGNING.store(true, Ordering::SeqCst);
        let _align = AlignGuard;
        let evidence = EvidenceGuard::acquire().expect("diagnostics must not share the model gate");
        assert!(
            EvidenceGuard::acquire().is_none(),
            "one disk writer at a time"
        );
        assert!(
            EvidenceGuard::acquire().is_none(),
            "failed acquisition must not release the active writer"
        );
        drop(evidence);
        assert!(
            ALIGNING.load(Ordering::SeqCst),
            "saving evidence must not unlock inference"
        );
        assert!(EvidenceGuard::acquire().is_some());
    }

    /// Explicit local integration test: real PCM/model through the production
    /// worker, source cache, word cache and karaoke-ready payload. Never writes
    /// the user's app cache. Run alone with the two environment paths below.
    #[test]
    #[ignore = "requires local licensed model assets and a complete evidence dump"]
    fn acoustic_recording_reaches_cache_and_ready_payload() {
        let evidence = PathBuf::from(
            std::env::var_os("PALETTE_KARAOKE_EVIDENCE").expect("evidence directory"),
        );
        let model =
            PathBuf::from(std::env::var_os("PALETTE_KARAOKE_MODEL").expect("model directory"));
        acoustic::configure(model);
        assert!(acoustic::assets().is_some());
        let meta: serde_json::Value =
            serde_json::from_slice(&std::fs::read(evidence.join("meta.json")).unwrap()).unwrap();
        let synced = std::fs::read_to_string(evidence.join("lyrics.lrc")).unwrap();
        let raw = std::fs::read(evidence.join("pcm.i16")).unwrap();
        let artist = meta["artist"].as_str().unwrap().to_string();
        let title = meta["title"].as_str().unwrap().to_string();
        let album = meta["album"].as_str().unwrap().to_string();
        let duration_ms = meta["duration_ms"].as_i64().unwrap();
        let key = lyrics::key_for_ms(&artist, &title, &album, duration_ms);
        let rec = Rec {
            key: key.clone(),
            artist: artist.clone(),
            title: title.clone(),
            album: album.clone(),
            duration_ms,
            origin_ms: meta["origin_ms"].as_i64().unwrap(),
            rate_in: TARGET_HZ,
            phase: 0,
            acc: 0.0,
            n: 0,
            samples: raw
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32767.0)
                .collect(),
            peak: 1.0,
            started: None,
            received: raw.len() as u64 / 2,
            anchors: serde_json::from_value(meta["anchors"].clone()).unwrap(),
            last_anchor_at: 0,
            seek_strikes: 0,
        };
        assert!(rec.can_finalize());
        let map = TimeMap::fit(&rec.anchors, TARGET_HZ, rec.origin_ms);
        assert!(cache_complete(map.pos_ms(rec.samples.len()), duration_ms));
        let root = std::env::temp_dir().join(format!(
            "palette-native-worker-{}-{}",
            std::process::id(),
            unix_ms()
        ));
        let source = root.join("lyrics");
        let cache = root.join("karaoke");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(
            source.join(format!("{key}.json")),
            serde_json::to_vec(&serde_json::json!({"synced":synced})).unwrap(),
        )
        .unwrap();
        let mut ready = None;
        commit_recording(rec, &source, &cache, None, |event| {
            ready = Some(serde_json::to_value(event).unwrap());
        });
        let words = load(&cache, &artist, &title, &album, duration_ms, Some(&synced));
        let expected = align::parse_lrc(&synced)
            .iter()
            .map(|l| align::tokenize(&l.text).len())
            .sum::<usize>();
        assert_eq!(words.len(), expected);
        assert!(words.iter().all(|w| w.line_t.is_some()
            && w.end
                .is_some_and(|end| end > w.t && end <= map.pos_ms(raw.len() / 2))));
        let event = ready.expect("ready payload after persistence");
        assert_eq!(event["title"], title);
        assert_eq!(event["words"].as_array().unwrap().len(), expected);
        let stored: StoreFile =
            serde_json::from_slice(&std::fs::read(cache.join(format!("{key}.json"))).unwrap())
                .unwrap();
        assert_eq!(stored.recipe, acoustic::RECIPE);
        assert_eq!(stored.detail_v, 2);
        let detailed = words.iter().filter(|w| !w.points.is_empty()).count();
        assert!(detailed > 0, "English evidence must retain acoustic detail");
        for word in &words {
            if let (Some(first), Some(last)) = (word.points.first(), word.points.last()) {
                assert_eq!(first.t, word.t);
                assert_eq!(Some(last.t), word.end);
                assert_eq!(first.fraction, 0.0);
                assert_eq!(last.fraction, 1.0);
                assert!(word
                    .points
                    .windows(2)
                    .all(|p| p[0].t <= p[1].t && p[0].fraction <= p[1].fraction));
            }
        }
        assert!(has_file(&cache, &key, Some(&synced)));
        // A legacy cache still displays, but allows one future detail upgrade.
        let mut legacy = serde_json::to_value(&stored).unwrap();
        legacy.as_object_mut().unwrap().remove("detail_v");
        std::fs::write(
            cache.join(format!("{key}.json")),
            serde_json::to_vec(&legacy).unwrap(),
        )
        .unwrap();
        assert_eq!(
            load(&cache, &artist, &title, &album, duration_ms, Some(&synced)),
            words
        );
        assert!(!has_file(&cache, &key, Some(&synced)));
        println!("Retained acoustic spelling detail for {detailed} words");

        assert!(load(
            &cache,
            &artist,
            &title,
            &album,
            duration_ms,
            Some("[00:01.00]different lyrics")
        )
        .is_empty());
        println!("Native worker produced {expected} cached words and a matching ready payload; temporary evidence: {}", root.display());
    }

    // Persistence regressions: a useful diagnostic recording is not
    // necessarily a trustworthy permanent word cache.
    #[test]
    fn cache_requires_pcm_near_known_track_end() {
        assert!(listen_enough(99_000, 0, 180_000));
        assert!(!cache_complete(99_000, 180_000));
        assert!(!cache_complete(178_499, 180_000));
        assert!(cache_complete(178_500, 180_000));
        assert!(cache_complete(180_100, 180_000));
        assert!(!cache_complete(180_000, 0));
        // Use mapped PCM end, including the capture origin.
        let map = TimeMap::from_origin(1_000, TARGET_HZ);
        assert!(cache_complete(map.pos_ms(177_500 * 16), 180_000));
    }

    #[test]
    fn pending_seek_rejects_finalization_until_a_good_pair() {
        let mut rec = rec_with(two_anchors(), 180_000);
        rec.samples.resize(100_000 * 16, 0.1);
        assert!(rec.can_finalize());
        rec.anchor(&pair(40_000, 180_000));
        assert_eq!(rec.seek_strikes, 1);
        assert!(!rec.can_finalize());
        // Even an end-clamped pair cannot clear the unresolved strike.
        rec.last_anchor_at = 0;
        rec.anchor(&pair(179_000, 180_000));
        assert!(!rec.can_finalize());
        rec.last_anchor_at = 0;
        rec.anchor(&pair(11_000, 180_000));
        assert!(rec.can_finalize());
    }

    #[test]
    fn delayed_delivery_is_rejected_not_padded() {
        for rate in [16_000u32, 44_100, 48_000] {
            let received = rate as u64 * 10;
            assert!(!ambiguous_gap(received, received, rate));
            assert!(!ambiguous_gap(received + rate as u64 / 20, received, rate));
            assert!(!ambiguous_gap(
                received + rate as u64 * 400 / 1000,
                received,
                rate
            ));
            assert!(ambiguous_gap(
                received + rate as u64 * 400 / 1000 + 1,
                received,
                rate
            ));
            // A one-second backlog might still arrive intact: wall time
            // cannot license inserting another second of synthetic zeros.
            assert!(ambiguous_gap(received + rate as u64, received, rate));
            assert!(!ambiguous_gap(received, received + rate as u64, rate));
        }
    }

    #[test]
    fn dropping_unstarted_alignment_job_releases_flag() {
        let _test_gate = ALIGNMENT_FLAG_TEST_GATE.lock().unwrap();
        ALIGNING.store(true, Ordering::SeqCst);
        let guard = AlignGuard;
        let job = move || {
            let _g = guard;
        };
        // Failed Builder::spawn drops its captured job without running it.
        drop(job);
        assert!(!ALIGNING.load(Ordering::SeqCst));
    }

    #[test]
    fn store_requires_exact_current_lyrics_for_load_and_eligibility() {
        let dir = std::env::temp_dir().join(format!("pulse-karaoke-source-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let key = lyrics::key_for_ms("a", "b", "c", 180_000);
        let source = "[00:01.00]one";
        let words = vec![Word {
            t: 900,
            text: "one".into(),
            end: Some(1200),
            line_t: Some(1000),
            points: Vec::new(),
        }];
        write_file(&dir, &key, source, &words).unwrap();
        assert_eq!(load(&dir, "a", "b", "c", 180_000, Some(source)), words);
        assert!(has_file(&dir, &key, Some(source)));
        for absent in [None, Some(""), Some("  ")] {
            assert!(load(&dir, "a", "b", "c", 180_000, absent).is_empty());
            assert!(!has_file(&dir, &key, absent));
        }
        // A transient missing source does not destroy an otherwise valid file.
        assert!(has_file(&dir, &key, Some(source)));
        for changed in ["[00:02.00]one", "[00:01.00]two", "[00:01.00]one\n"] {
            write_file(&dir, &key, source, &words).unwrap();
            assert!(!has_file(&dir, &key, Some(changed)));
            write_file(&dir, &key, source, &words).unwrap();
            assert!(load(&dir, "a", "b", "c", 180_000, Some(changed)).is_empty());
        }
        let path = dir.join(format!("{key}.json"));
        // Even a current version/recipe without identity is ineligible.
        write_file(&dir, &key, source, &words).unwrap();
        let mut legacy: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        legacy.as_object_mut().unwrap().remove("synced");
        std::fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
        assert!(!has_file(&dir, &key, Some(source)));
        assert!(!path.exists());
        std::fs::remove_dir(&dir).unwrap();
    }

    #[test]
    fn coverage_uses_line_identity_and_excludes_empty_markers() {
        let lines = vec![
            TimedLine {
                t: 1000,
                text: "one".into(),
            },
            TimedLine {
                t: 1500,
                text: "".into(),
            },
            TimedLine {
                t: 2000,
                text: "two".into(),
            },
            TimedLine {
                t: 2500,
                text: "  ".into(),
            },
        ];
        let words = vec![
            Word {
                t: 900,
                text: "one".into(),
                end: Some(1200),
                line_t: Some(1000),
                points: Vec::new(),
            },
            Word {
                t: 1800,
                text: "two".into(),
                end: Some(2100),
                line_t: Some(2000),
                points: Vec::new(),
            },
        ];
        assert_eq!(line_coverage(&lines, &words), 100);
        assert_eq!(line_coverage(&lines, &words[..1]), 50);
        let unrelated = vec![
            Word {
                t: 1100,
                line_t: Some(1500),
                ..Word::default()
            },
            Word {
                t: 2100,
                line_t: None,
                ..Word::default()
            },
        ];
        assert_eq!(line_coverage(&lines, &unrelated), 0);
        assert_eq!(line_coverage(&lines[1..2], &words), 0);
        assert_eq!(line_coverage(&[], &words), 0);
    }

    #[test]
    fn skip_through_is_not_enough() {
        assert!(!listen_enough(8_000, 0, 180_000));
        assert!(!listen_enough(40_000, 0, 180_000));
    }

    #[test]
    fn majority_listen_is_enough() {
        assert!(listen_enough(100_000, 0, 180_000));
        assert!(listen_enough(12_000, 0, 20_000));
    }

    #[test]
    fn store_roundtrip_keeps_word_times() {
        let dir = std::env::temp_dir().join(format!("pulse-karaoke-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let words = vec![
            Word {
                t: 1000,
                text: "one ".into(),
                end: Some(1200),
                line_t: Some(1000),
                points: Vec::new(),
            },
            Word {
                t: 1300,
                text: "two".into(),
                end: Some(1500),
                line_t: Some(1000),
                points: Vec::new(),
            },
        ];
        write_file(&dir, "abc", "[00:01.00]one two", &words).unwrap();
        let got = read_file(&dir.join("abc.json"), Some("[00:01.00]one two"));
        assert_eq!(got, words);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_words_file_is_not_a_fill() {
        let dir = std::env::temp_dir().join(format!("pulse-karaoke-empty-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let got = load(&dir, "a", "b", "c", 180_000, Some("[00:01.00]one"));
        assert!(got.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stale_store_version_is_dropped() {
        let dir = std::env::temp_dir().join(format!("pulse-karaoke-v1-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("abc.json");
        std::fs::write(&path, r#"{"v":1,"words":[{"t":1,"text":"nope","end":2}]}"#).unwrap();
        assert!(read_file(&path, Some("[00:01.00]one")).is_empty());
        assert!(!path.is_file());
        // Right version, different aligner: also dropped.
        std::fs::write(
            &path,
            format!(
                r#"{{"v":{STORE_V},"recipe":"other/9","synced":"[00:01.00]one","words":[{{"t":1,"text":"nope","end":2}}]}}"#
            ),
        )
        .unwrap();
        assert!(read_file(&path, Some("[00:01.00]one")).is_empty());
        assert!(!path.is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn coverage_ignores_silent_lines() {
        let lines = vec![
            TimedLine {
                t: 1000,
                text: "one".into(),
            },
            TimedLine {
                t: 2000,
                text: "two".into(),
            },
            TimedLine {
                t: 3000,
                text: "three".into(),
            },
        ];
        let words = vec![Word {
            t: 1000,
            text: "one".into(),
            end: Some(1400),
            line_t: Some(1000),
            points: Vec::new(),
        }];
        assert_eq!(line_coverage(&lines, &words), 33);
        assert!(line_coverage(&lines, &[]) == 0);
    }

    /// Two anchors 5s apart on a 16kHz grid, true origin 1000ms.
    fn two_anchors() -> Vec<(usize, i64)> {
        vec![(0, 1000), (80_000, 6000)]
    }

    #[test]
    fn a_jump_past_the_band_is_a_seek() {
        // 10s in, the player reports 14s: the user scrubbed forward 3s.
        assert!(seek_detected(&two_anchors(), 160_000, 14_000));
        assert!(seek_detected(&two_anchors(), 160_000, 8_000));
    }

    #[test]
    fn jitter_inside_the_band_is_not_a_seek() {
        assert!(!seek_detected(&two_anchors(), 160_000, 11_900));
        assert!(!seek_detected(&two_anchors(), 160_000, 9_600));
    }

    fn rec_with(anchors: Vec<(usize, i64)>, duration_ms: i64) -> Rec {
        Rec {
            key: "k".into(),
            artist: String::new(),
            title: "t".into(),
            album: String::new(),
            duration_ms,
            origin_ms: 0,
            rate_in: 48_000,
            phase: 0,
            acc: 0.0,
            n: 0,
            samples: Vec::new(),
            peak: 0.0,
            started: None,
            // 10s of input frames received; pairs below are stamped "now".
            received: 480_000,
            anchors,
            last_anchor_at: 0,
            seek_strikes: 0,
        }
    }

    fn pair(position_ms: i64, duration_ms: i64) -> NowPlaying {
        NowPlaying {
            position_ms,
            position_at_ms: unix_ms(),
            duration_ms,
            status: "playing".into(),
            ..Default::default()
        }
    }

    #[test]
    fn end_of_track_pair_is_not_evidence() {
        // 10s of samples but the pair says 179s of 180s: the clamp zone.
        let mut rec = rec_with(two_anchors(), 180_000);
        assert!(matches!(rec.anchor(&pair(179_000, 180_000)), Anchor::Skip));
        assert_eq!(rec.anchors.len(), 2);
        assert_eq!(rec.seek_strikes, 0);
    }

    #[test]
    fn one_glitched_pair_is_a_strike_two_are_a_seek() {
        let mut rec = rec_with(two_anchors(), 180_000);
        // Fit predicts ~11s at 10s of samples; the pair says 40s.
        assert!(matches!(rec.anchor(&pair(40_000, 180_000)), Anchor::Skip));
        assert_eq!(rec.seek_strikes, 1);
        rec.last_anchor_at = 0;
        assert!(matches!(
            rec.anchor(&pair(40_500, 180_000)),
            Anchor::Seek(..)
        ));
    }

    #[test]
    fn a_good_pair_clears_the_strike() {
        let mut rec = rec_with(two_anchors(), 180_000);
        rec.anchor(&pair(40_000, 180_000));
        rec.last_anchor_at = 0;
        assert!(matches!(rec.anchor(&pair(11_000, 180_000)), Anchor::Added));
        assert_eq!(rec.seek_strikes, 0);
        assert_eq!(rec.anchors.len(), 3);
    }

    #[test]
    fn word_lead_steps_and_clamps() {
        assert_eq!(nudged_word_lead(120, true), 140);
        assert_eq!(nudged_word_lead(120, false), 100);
        assert_eq!(nudged_word_lead(390, true), 400);
        assert_eq!(nudged_word_lead(-190, false), -200);
        assert_eq!(clamp_word_lead(9_999), 400);
    }

    #[test]
    fn no_fit_means_no_seek_verdict() {
        assert!(!seek_detected(&[(0, 1000)], 160_000, 40_000));
        assert!(!seek_detected(&[], 160_000, 40_000));
    }
}

#[cfg(test)]
mod sync_status_tests {
    use super::*;
    #[test]
    fn numeric_cache_revision_keeps_unrelated_songs() {
        let dir = std::env::temp_dir().join(format!("palette-digit-cache-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (key, text, valid) in [("numeric", "4sho", false), ("ordinary", "hello", true)] {
            let source = format!("[00:01.00]{text}");
            let word = Word {
                t: 1000,
                text: text.into(),
                end: Some(2000),
                line_t: Some(1000),
                points: Vec::new(),
            };
            write_file(&dir, key, &source, &[word]).unwrap();
            let path = dir.join(format!("{key}.json"));
            let mut old: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            old["detail_v"] = 1.into();
            std::fs::write(&path, serde_json::to_vec(&old).unwrap()).unwrap();
            assert_eq!(!read_file(&path, Some(&source)).is_empty(), valid);
            if path.exists() {
                std::fs::remove_file(path).unwrap();
            }
        }
        std::fs::remove_dir(dir).unwrap();
    }
    #[test]
    fn worker_exit_reports_failure_unless_a_terminal_result_was_recorded() {
        let failed = "status-test-failed";
        sync_state(failed, "processing", "working");
        drop(SyncAttempt(failed.into()));
        assert_eq!(
            sync_states()
                .iter()
                .find(|(k, _)| k == failed)
                .unwrap()
                .1
                .phase,
            "failed"
        );
        let active = "status-test-active";
        sync_state(active, "processing", "working");
        sync_state(active, "waiting", "interrupted replay");
        assert_eq!(
            sync_states()
                .iter()
                .find(|(k, _)| k == active)
                .unwrap()
                .1
                .phase,
            "processing"
        );
        finish_sync_state(active, "waiting", "listen ended early");
        drop(SyncAttempt(active.into()));
        assert_eq!(
            sync_states()
                .iter()
                .find(|(k, _)| k == active)
                .unwrap()
                .1
                .phase,
            "waiting"
        );
        let saved = "status-test-saved";
        sync_state(saved, "processing", "working");
        let attempt = SyncAttempt(saved.into());
        sync_state(saved, "saved", "saved");
        drop(attempt);
        let states = sync_states();
        assert_eq!(
            states.iter().find(|(k, _)| k == saved).unwrap().1.phase,
            "saved"
        );
        assert_eq!(
            states.iter().find(|(k, _)| k == failed).unwrap().1.phase,
            "failed"
        );
    }
}
