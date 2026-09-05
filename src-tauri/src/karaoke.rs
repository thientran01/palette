//! Local word-karaoke store, capture, and align.

use crate::align::{self, TimeMap, Word};
use crate::lyrics;
use crate::media::NowPlaying;
use crate::settings::{self, write_atomic};
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
/// v3 = the prior + refinement-ladder aligner (docs/specs/2026-09-04-
/// karaoke-aligner-ladder.md). v2 files came from the energy-rise aligner
/// that measured 498ms median against tap truth; they're dropped on read
/// and re-record on the next full listen. (A spectral-flux DP aligner also
/// wore v3 for a few hours on 2026-09-04; those files were already purged
/// by the version check, so the number is safe to reuse.)
const STORE_V: u32 = 3;
/// Wall-clock deficit past which the capture is judged to have delivered
/// nothing for a stretch (process loopback goes quiet with its target) —
/// the gap is padded with silence so later word times don't drift early.
/// Well above normal packet jitter (~10–50ms).
const GAP_PAD_MS: u64 = 400;
/// Staleness projection cap: a position stamped longer ago than this is
/// not extrapolated further (the pair is the player's, not the clock's).
const STALE_CAP_MS: i64 = 5_000;
/// settings.json switch for the evidence dump (docs/specs/2026-09-04).
const DUMP_SETTING: &str = "karaokeDump";
const DUMP_DIR: &str = "karaoke-dumps";
/// ~8MB of i16 PCM per four-minute dump; keep the last few only.
const DUMP_MAX: usize = 5;

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
    /// (output sample index, position_ms) from every fresh pair the media
    /// loop saw while recording — the TimeMap's evidence.
    anchors: Vec<(usize, i64)>,
    /// position_at_ms of the last pair consumed (the arming pair counts),
    /// so a beat that merely re-emits the same pair adds nothing.
    last_anchor_at: i64,
}

enum Anchor {
    Skip,
    Added,
    /// (residual ms, anchors so far, reported position) — logged so a
    /// false positive is diagnosable from the log alone.
    Seek(f64, usize, i64),
}

impl Rec {
    /// Turn a fresh pair into an anchor, or flag a seek. The pair's
    /// staleness (now − position_at_ms) says how many input frames ago the
    /// position was true; that input index maps onto the 16kHz grid.
    fn anchor(&mut self, np: &NowPlaying) -> Anchor {
        if np.position_at_ms <= 0 || np.position_at_ms == self.last_anchor_at || self.rate_in == 0 {
            return Anchor::Skip;
        }
        self.last_anchor_at = np.position_at_ms;
        let stale = (unix_ms() - np.position_at_ms).clamp(0, STALE_CAP_MS);
        let idx_in = (self.received as i64 - stale * self.rate_in as i64 / 1000).max(0) as u64;
        let idx = (idx_in * TARGET_HZ as u64 / self.rate_in as u64) as usize;
        if let Some(residual) = seek_residual(&self.anchors, idx, np.position_ms) {
            return Anchor::Seek(residual, self.anchors.len(), np.position_ms);
        }
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
        anchors: Vec::new(),
        last_anchor_at: np.position_at_ms,
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
    let handle = app.clone();
    let _ = std::thread::Builder::new()
        .name("karaoke-align".into())
        .spawn(move || {
            let _g = AlignGuard;
            commit_sync(&handle, rec, &lyrics_dir, &karaoke_dir, dump_dir.as_deref());
        });
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
    let words = align::align(&rec.samples, TARGET_HZ, &lines, &map);
    // Evidence first: a coverage miss below still leaves something to score.
    if let Some(dir) = dump_dir {
        match write_dump(dir, &rec, &synced, &words, &map) {
            Ok(()) => log::info!("karaoke: dumped {} to {}", rec.title, dir.display()),
            Err(e) => log::warn!("karaoke: dump failed ({e})"),
        }
    }
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

    #[test]
    fn no_fit_means_no_seek_verdict() {
        assert!(!seek_detected(&[(0, 1000)], 160_000, 40_000));
        assert!(!seek_detected(&[], 160_000, 40_000));
    }
}
