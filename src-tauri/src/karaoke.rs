//! Local word-karaoke store, capture, and align.

use crate::align::{self, Word};
use crate::lyrics;
use crate::media::NowPlaying;
use crate::settings::write_atomic;
use crate::spotify;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};

const CACHE_MAX_FILES: usize = 500;
const TARGET_HZ: u32 = 16_000;
const MAX_SAMPLES: usize = 16_000 * 60 * 8;
const ARM_NEAR_START_MS: i64 = 8_000;
const PEAK_ABORT: f32 = 1e-3;
const MIN_LINE_COVERAGE: u32 = 30;
/// v2 = the energy-rise aligner. A spectral-flux + DP aligner shipped
/// briefly as v3 (2026-09-04) and regressed live — it latched onto beat
/// onsets and its pre-stamp window merged lines; any v3 file on disk is
/// dropped on read and the track re-records under v2.
const STORE_V: u32 = 2;
/// Wall-clock deficit past which the capture is judged to have delivered
/// nothing for a stretch (process loopback goes quiet with its target) —
/// the gap is padded with silence so later word times don't drift early.
/// Well above normal packet jitter (~10–50ms).
const GAP_PAD_MS: u64 = 400;
/// Staleness projection cap: a position stamped longer ago than this is
/// not extrapolated further (the pair is the player's, not the clock's).
const STALE_CAP_MS: i64 = 5_000;

#[derive(Serialize, Deserialize)]
struct StoreFile {
    v: u32,
    words: Vec<Word>,
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
    /// since: the pair that detects a delivery gap (see GAP_PAD_MS).
    started: Option<Instant>,
    received: u64,
}

impl Rec {
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

pub fn load(dir: &Path, artist: &str, title: &str, album: &str, duration_ms: i64) -> Vec<Word> {
    let key = lyrics::key_for_ms(artist, title, album, duration_ms);
    read_file(&dir.join(format!("{key}.json")))
}

fn read_file(path: &Path) -> Vec<Word> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(file) = serde_json::from_str::<StoreFile>(&raw) else {
        return Vec::new();
    };
    if file.v != STORE_V {
        let _ = std::fs::remove_file(path);
        return Vec::new();
    }
    file.words
}

fn write_file(dir: &Path, key: &str, words: &[Word]) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let json = serde_json::to_vec(&StoreFile {
        v: STORE_V,
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

fn has_file(dir: &Path, key: &str) -> bool {
    !read_file(&dir.join(format!("{key}.json"))).is_empty()
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
    if lines.is_empty() {
        return 0;
    }
    let mut hit = 0u32;
    for (i, line) in lines.iter().enumerate() {
        let next_t = lines.get(i + 1).map(|n| n.t).unwrap_or(i64::MAX);
        if words.iter().any(|w| w.t >= line.t && w.t < next_t) {
            hit += 1;
        }
    }
    hit * 100 / lines.len() as u32
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
        return;
    }
    let key = lyrics::key_for_ms(&np.artist, &np.title, &np.album, np.duration_ms);
    let old = {
        let mut slot = lock_rec();
        match slot.as_ref() {
            Some(rec) if rec.key == key => return,
            Some(_) => {
                RECORDING.store(false, Ordering::Relaxed);
                slot.take()
            }
            None => None,
        }
    };
    if let Some(rec) = old {
        try_commit(app, rec);
    }
    let Some(dir) = karaoke_dir(app) else {
        return;
    };
    if has_file(&dir, &key) {
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
        RECORDING.store(false, Ordering::Relaxed);
        *slot = None;
        return;
    }
    // Delivery gap: the wall clock says far more audio has elapsed than
    // arrived. Process loopback delivers nothing while its target renders
    // nothing, and a recording that silently skips that stretch puts
    // every later word early by its length — pad it as silence instead.
    if let Some(t0) = rec.started {
        let expected = (t0.elapsed().as_secs_f64() * sample_rate as f64) as u64;
        let deficit = expected.saturating_sub(rec.received);
        if deficit > sample_rate as u64 * GAP_PAD_MS / 1000 {
            for _ in 0..deficit {
                if !rec.push(0.0) {
                    RECORDING.store(false, Ordering::Relaxed);
                    *slot = None;
                    return;
                }
            }
            rec.received += deficit;
        }
    }
    for &s in frames {
        if !rec.push(s) {
            RECORDING.store(false, Ordering::Relaxed);
            *slot = None;
            return;
        }
    }
    rec.received += frames.len() as u64;
}

struct AlignGuard;

impl Drop for AlignGuard {
    fn drop(&mut self) {
        ALIGNING.store(false, Ordering::SeqCst);
    }
}

fn try_commit(app: &AppHandle, rec: Rec) {
    let pcm_ms = if rec.samples.is_empty() {
        0
    } else {
        rec.samples.len() as i64 * 1000 / TARGET_HZ as i64
    };
    if !listen_enough(pcm_ms, rec.origin_ms, rec.duration_ms) {
        return;
    }
    if rec.peak < PEAK_ABORT {
        lock_misses().insert(rec.key);
        log::info!("karaoke: silence on {} — leaving line karaoke", rec.title);
        return;
    }
    if ALIGNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let lyrics_dir = lyrics_dir(app);
    let Some(karaoke_dir) = karaoke_dir(app) else {
        ALIGNING.store(false, Ordering::SeqCst);
        return;
    };
    let handle = app.clone();
    let _ = std::thread::Builder::new()
        .name("karaoke-align".into())
        .spawn(move || {
            let _g = AlignGuard;
            commit_sync(&handle, rec, &lyrics_dir, &karaoke_dir);
        });
}

fn commit_sync(app: &AppHandle, rec: Rec, lyrics_dir: &Path, karaoke_dir: &Path) {
    if has_file(karaoke_dir, &rec.key) {
        return;
    }
    let Some(synced) = lyrics::cached_synced(
        lyrics_dir,
        &rec.artist,
        &rec.title,
        &rec.album,
        rec.duration_ms,
    ) else {
        return;
    };
    let lines = align::parse_lrc(&synced);
    if lines.is_empty() {
        return;
    }
    let words = align::align(&rec.samples, TARGET_HZ, &lines, rec.origin_ms);
    if line_coverage(&lines, &words) < MIN_LINE_COVERAGE {
        lock_misses().insert(rec.key);
        log::info!("karaoke: align missed {} — leaving line karaoke", rec.title);
        return;
    }
    if let Err(e) = write_file(karaoke_dir, &rec.key, &words) {
        log::warn!("karaoke: persist failed ({e})");
        return;
    }
    let _ = app.emit(
        "karaoke-ready",
        KaraokeReady {
            artist: rec.artist,
            title: rec.title,
            album: rec.album,
            duration_ms: rec.duration_ms,
            words,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::align::TimedLine;

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
            },
            Word {
                t: 1300,
                text: "two".into(),
                end: Some(1500),
            },
        ];
        write_file(&dir, "abc", &words).unwrap();
        let got = read_file(&dir.join("abc.json"));
        assert_eq!(got, words);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_words_file_is_not_a_fill() {
        let dir = std::env::temp_dir().join(format!("pulse-karaoke-empty-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let got = load(&dir, "a", "b", "c", 180_000);
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
        assert!(read_file(&path).is_empty());
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
        }];
        assert_eq!(line_coverage(&lines, &words), 33);
        assert!(line_coverage(&lines, &[]) == 0);
    }
}
