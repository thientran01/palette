//! GSMTC media core: watches the current Windows media session (change events
//! plus a heartbeat poll) and exposes transport commands. Capability quirks
//! per player are documented in docs/smtc-support-matrix.md — notably Apple
//! Music ignores seek and packs "artist — album" into the artist field.

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use serde::Serialize;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use windows::Foundation::TypedEventHandler;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as PlaybackStatus,
};
use windows::Media::MediaPlaybackType;
use windows::Storage::Streams::{Buffer, DataReader, InputStreamOptions};
use windows_future::{
    AsyncOperationCompletedHandler, AsyncOperationWithProgressCompletedHandler, IAsyncOperation,
    IAsyncOperationWithProgress,
};

const TICKS_PER_MS: i64 = 10_000; // WinRT TimeSpan tick = 100ns
/// Offset between the Windows FILETIME epoch (1601) and the unix epoch, in ms.
const FILETIME_EPOCH_OFFSET_MS: i64 = 11_644_473_600_000;

#[derive(Serialize, Clone, Default, PartialEq)]
pub struct NowPlaying {
    pub app_id: String,
    /// "apple_music" | "spotify" | "other" | "none"
    pub player: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    /// "playing" | "paused" | "stopped" | "none"
    pub status: String,
    /// RAW player-reported position — no staleness projection applied. The
    /// frontend owns the one clock that turns (position, reported-at) pairs
    /// into a display position; Rust never projects a UI-visible position.
    pub position_ms: i64,
    pub duration_ms: i64,
    /// Unix ms when the player last stamped its timeline (GSMTC
    /// LastUpdatedTime). 0 = the player never stamped it.
    pub position_at_ms: i64,
    pub can_seek: bool,
    pub art_id: Option<String>,
    /// GSMTC MediaPlaybackType bucket: "music" | "video" | "image" |
    /// "unknown". Copied onto history rows; the history allowlist (Apple
    /// Music / Spotify, dropping video/image) is what actually keeps Search
    /// and the Earlier feed music-only. "" only ever appears on pre-feature
    /// history.jsonl rows (see history::is_music).
    pub media_kind: String,
}

/// Album art for the current track, a per-key state machine (the slot's None
/// = no key claimed). snapshot() only STAMPS art_id from this cache and
/// claims a new key as Pending — it never touches the thumbnail stream, so
/// no emit waits on art (the skip's title paint used to sit behind a
/// 3s-bounded stream read + base64 encode). Every stream read and every
/// Pending→Cached / rev transition happens in art_pump on the media loop
/// thread; other threads' snapshots only insert Pending, which keeps the
/// byte-level state single-writer and the compare-and-set on re-lock small.
pub struct ArtCache(pub Mutex<Option<ArtState>>);

/// How long after a key change we keep distrusting the cached thumbnail.
/// GSMTC updates title/artist before the player attaches the new thumbnail
/// (worst on Apple Music), so the first read after a track change can capture
/// the PREVIOUS track's image. While unsettled inside this window the media
/// loop re-reads and fingerprints the stream each tick; a byte change bumps
/// `rev` so the emitted art_id changes and the frontend re-fetches. Two
/// consecutive reads with identical bytes settle the entry (see
/// apply_art_read) and stop the re-reads early.
const ART_PROBE_WINDOW_MS: i64 = 10_000;

pub enum ArtState {
    /// Key claimed by a metadata-first snapshot; no read has finished yet.
    /// The emitted art_id is None until the media loop's art_pump runs the
    /// deferred first read.
    Pending { key: String, first_seen_ms: i64 },
    /// At least one read finished. url None = the read failed — cached too,
    /// so a bad thumbnail isn't retried outside the probe window.
    Cached {
        key: String,
        /// Bumped when probing catches the thumbnail bytes changing under
        /// the same key. Part of the emitted art_id (`"{key}:{rev}"`).
        rev: u32,
        url: Option<String>,
        /// Cheap content fingerprint (len + first/last 1KB). None = read failed.
        fingerprint: Option<u64>,
        /// When this key first appeared — anchors the probe window.
        first_seen_ms: i64,
        /// True once a probe read confirmed the bytes unchanged — probing
        /// stops early instead of running out the window.
        settled: bool,
    },
}

impl ArtState {
    fn key(&self) -> &str {
        match self {
            ArtState::Pending { key, .. } | ArtState::Cached { key, .. } => key,
        }
    }

    /// The id emitted as NowPlaying.art_id and matched by the media_art IPC.
    /// None while Pending or after a failed read — the follow-up emit carries
    /// the real id once bytes land.
    fn art_id(&self) -> Option<String> {
        match self {
            ArtState::Pending { .. } => None,
            ArtState::Cached { key, rev, url, .. } => url.is_some().then(|| format!("{key}:{rev}")),
        }
    }
}

/// Resolve an emitted art_id (`"{key}:{rev}"`) to the cached data URL.
/// Unknown ids (Pending, a different key, an outdated rev) resolve to None —
/// the frontend treats that as "a newer payload is coming" and retries.
pub fn art_url(cache: &ArtCache, art_id: &str) -> Option<String> {
    match lock_art(cache).as_ref() {
        Some(ArtState::Cached { key, rev, url, .. }) if format!("{key}:{rev}") == art_id => {
            url.clone()
        }
        _ => None,
    }
}

fn lock_art(cache: &ArtCache) -> std::sync::MutexGuard<'_, Option<ArtState>> {
    cache
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ── Stage telemetry + bounded WinRT waits ───────────────────────────────────
// The 2026-07-15 freeze: a WinRT call into the source player never completed,
// wedging the media loop thread forever with no log line and no clue WHICH
// call hung. Two guards live here:
//   1. wait_op/wait_op_progress — every async op gets a timeout, so a player
//      that never answers becomes a logged failure on the call's existing
//      None/false path instead of a hung thread.
//   2. MEDIA_STAGE — names the call the MEDIA LOOP is currently inside; the
//      media-loop watchdog (lib.rs) reads it on a stall so the residual
//      un-timeout-able surface (sync property reads, which windows-rs offers
//      no bounded wait for) self-identifies in pulse.log. Only the loop
//      thread writes stages: transport commands and the now_playing seed run
//      these same functions on other threads and must not scribble over a
//      wedged loop's marker.

#[derive(Clone, Copy)]
#[repr(u8)]
pub enum Stage {
    Idle = 0,
    ManagerRequest,
    GetSession,
    SessionId,
    MediaProps,
    Thumbnail,
    ArtOpen,
    ArtRead,
    Timeline,
    PlaybackInfo,
    Transport,
    SimWedge,
}

const STAGE_NAMES: [&str; 12] = [
    "idle",
    "manager_request",
    "get_session",
    "session_id",
    "media_props",
    "thumbnail",
    "art_open",
    "art_read",
    "timeline",
    "playback_info",
    "transport",
    "sim_wedge",
];

static MEDIA_STAGE: AtomicU8 = AtomicU8::new(0);
/// Last stage TRANSITION on the media loop thread (unix ms; 0 = loop not
/// started). The watchdog measures staleness of this, not iteration
/// completion: one legitimate slow-art iteration can chain dozens of
/// succeeding 3s-bounded chunk reads and take >10s while making real
/// progress — each transition refreshes the beat, so only a single blocking
/// call that never returns goes stale.
static MEDIA_BEAT_MS: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

thread_local! {
    static IS_MEDIA_LOOP: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Called once at the top of the media loop thread (lib.rs). Stage writes and
/// the dev sim-wedge apply only on that thread.
pub fn mark_media_loop_thread() {
    IS_MEDIA_LOOP.with(|f| f.set(true));
    beat();
}

/// Refresh the liveness beat without touching the stage — the media loop
/// calls this once per iteration (covers stage-free hidden beats).
pub fn beat() {
    if IS_MEDIA_LOOP.with(|f| f.get()) {
        MEDIA_BEAT_MS.store(now_ms(), Ordering::Relaxed);
    }
}

fn set_stage(stage: Stage) {
    if IS_MEDIA_LOOP.with(|f| f.get()) {
        MEDIA_STAGE.store(stage as u8, Ordering::Relaxed);
        MEDIA_BEAT_MS.store(now_ms(), Ordering::Relaxed);
    }
}

/// The stage the media loop last entered — read by the watchdog on a stall.
pub fn stage_name() -> &'static str {
    STAGE_NAMES[(MEDIA_STAGE.load(Ordering::Relaxed) as usize) % STAGE_NAMES.len()]
}

/// Ms since the media loop last made progress (a stage transition or an
/// iteration beat); 0 while the loop hasn't started. Read by the watchdog.
pub fn beat_age_ms() -> i64 {
    let beat = MEDIA_BEAT_MS.load(Ordering::Relaxed);
    if beat == 0 {
        0
    } else {
        now_ms() - beat
    }
}

/// Async-op timeout for metadata/manager/transport calls — normally <10ms.
const OP_TIMEOUT_MS: u64 = 2_000;
/// Art timeout, per open/chunk — thumbnail streams marshal from the source app.
const ART_TIMEOUT_MS: u64 = 3_000;

/// `.get()` with a deadline: hook the op's Completed handler (what `.get()`
/// does internally), wait bounded, best-effort Cancel on timeout. Zero cost on
/// the healthy path. Completions arrive on the COM threadpool — every caller
/// thread is MTA (the media loop initializes explicitly; tauri's async workers
/// ride the implicit MTA), so no pump is needed.
fn wait_op<T: windows::core::RuntimeType + 'static>(
    op: IAsyncOperation<T>,
    timeout_ms: u64,
    stage: Stage,
) -> Option<T> {
    set_stage(stage);
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let hooked = op
        .SetCompleted(&AsyncOperationCompletedHandler::new(move |_, _| {
            let _ = tx.send(());
            Ok(())
        }))
        .is_ok();
    let out = if hooked && rx.recv_timeout(Duration::from_millis(timeout_ms)).is_ok() {
        op.GetResults().ok()
    } else {
        if hooked {
            log::warn!(
                "media: WinRT op timed out after {timeout_ms}ms at stage '{}'",
                STAGE_NAMES[stage as u8 as usize]
            );
        } else {
            log::warn!(
                "media: SetCompleted failed at stage '{}' (no wait attempted)",
                STAGE_NAMES[stage as u8 as usize]
            );
        }
        let _ = op.Cancel();
        None
    };
    set_stage(Stage::Idle);
    out
}

/// wait_op for IAsyncOperationWithProgress (thumbnail ReadAsync).
fn wait_op_progress<T, P>(
    op: IAsyncOperationWithProgress<T, P>,
    timeout_ms: u64,
    stage: Stage,
) -> Option<T>
where
    T: windows::core::RuntimeType + 'static,
    P: windows::core::RuntimeType + 'static,
{
    set_stage(stage);
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let hooked = op
        .SetCompleted(&AsyncOperationWithProgressCompletedHandler::new(
            move |_, _| {
                let _ = tx.send(());
                Ok(())
            },
        ))
        .is_ok();
    let out = if hooked && rx.recv_timeout(Duration::from_millis(timeout_ms)).is_ok() {
        op.GetResults().ok()
    } else {
        if hooked {
            log::warn!(
                "media: WinRT op timed out after {timeout_ms}ms at stage '{}'",
                STAGE_NAMES[stage as u8 as usize]
            );
        } else {
            log::warn!(
                "media: SetCompleted failed at stage '{}' (no wait attempted)",
                STAGE_NAMES[stage as u8 as usize]
            );
        }
        let _ = op.Cancel();
        None
    };
    set_stage(Stage::Idle);
    out
}

/// Dev tray "Simulate media wedge": the media loop's next snapshot sleeps
/// under Stage::SimWedge until the deadline, faking the un-reproducible
/// wedged-WinRT-call state. Loop-thread only — seed/transport snapshots stay
/// live, which is exactly the incident's shape (and what the lock narrowing
/// in lib.rs must keep working).
#[cfg(debug_assertions)]
static SIM_WEDGE_UNTIL_MS: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

#[cfg(debug_assertions)]
pub fn simulate_wedge(ms: i64) {
    SIM_WEDGE_UNTIL_MS.store(now_ms() + ms, Ordering::Relaxed);
}

/// GSMTC manager, cached for the app lifetime: RequestAsync is a blocking
/// cross-process round-trip and was being paid on every snapshot and every
/// transport command. The manager is a stable singleton connection — sessions
/// come and go underneath it. A failed request retries on the next call.
static MANAGER: Mutex<Option<Manager>> = Mutex::new(None);

fn manager() -> Option<Manager> {
    // Fast path: hand back the cached singleton without touching RequestAsync.
    {
        let cached = MANAGER
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(m) = cached.as_ref() {
            return Some(m.clone());
        }
    }
    // Cold/failed: RequestAsync UNLOCKED. Holding MANAGER across RequestAsync +
    // wait_op (up to OP_TIMEOUT_MS) serialized EVERY GSMTC caller — transport
    // commands and hotkeys included — behind a ~2s re-request during a GSMTC
    // restart. The manager is a singleton (sessions come and go underneath it),
    // so a racing second requester is harmless: fill-if-still-empty on re-lock,
    // last-writer-wins either way. A failed request stays uncached and retries
    // on the next call (unchanged).
    let requested = Manager::RequestAsync()
        .ok()
        .and_then(|op| wait_op(op, OP_TIMEOUT_MS, Stage::ManagerRequest))?;
    let mut cached = MANAGER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if cached.is_none() {
        *cached = Some(requested.clone());
        Some(requested)
    } else {
        // Another caller filled it while we requested — prefer the winner (both
        // are valid singletons); our just-made one drops.
        cached.clone()
    }
}

pub fn current_session() -> Option<Session> {
    let mgr = manager()?;
    set_stage(Stage::GetSession);
    let current = mgr.GetCurrentSession();
    set_stage(Stage::Idle);
    match current {
        Ok(s) => Some(s),
        // windows-rs maps a null return ("no current session" — a normal
        // state, e.g. Apple Music stopped) to an Err carrying S_OK. Any real
        // failure code means the cached manager's connection died (service
        // restart, sleep/resume) — drop it so the next call re-requests,
        // otherwise the app would stay dark until restart.
        Err(e) => {
            if !e.code().is_ok() {
                *MANAGER
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
            }
            None
        }
    }
}

/// Wake signals sent from WinRT event handlers to the media loop.
pub enum Wake {
    /// Session content changed (metadata/thumbnail/playback) — snapshot now.
    Event,
    /// The OS swapped the current session — re-subscribe, then snapshot.
    SessionChanged,
}

/// Subscribes to GSMTC change events and forwards them to the media loop as
/// [`Wake`]s. The loop stays the only place that snapshots and emits — the
/// WinRT handlers (which fire on the OS threadpool) only ever poke the
/// channel, so going event-driven adds no new concurrency into snapshot().
pub struct SessionWatch {
    manager: Manager,
    tx: Sender<Wake>,
    /// (session, app_id, [media_props_token, playback_info_token])
    watched: Option<(Session, String, [i64; 2])>,
}

impl SessionWatch {
    /// None when GSMTC is unavailable — the caller falls back to pure polling.
    pub fn new(tx: Sender<Wake>) -> Option<Self> {
        let manager = manager()?;
        let session_tx = tx.clone();
        manager
            .CurrentSessionChanged(&TypedEventHandler::new(move |_, _| {
                let _ = session_tx.send(Wake::SessionChanged);
                Ok(())
            }))
            .ok()?;
        Some(Self {
            manager,
            tx,
            watched: None,
        })
    }

    /// Attach change handlers to the CURRENT session if it isn't the watched
    /// one. `force` re-attaches even when the app id matches — a player can
    /// re-register a fresh session under the same id (Apple Music does, on
    /// every stop/start), leaving handlers on the dead one. Missed events are
    /// never fatal: the heartbeat poll still covers everything within 500ms.
    pub fn resubscribe(&mut self, force: bool) {
        set_stage(Stage::GetSession);
        let current = self.manager.GetCurrentSession().ok();
        set_stage(Stage::SessionId);
        let current_id = current
            .as_ref()
            .and_then(|s| s.SourceAppUserModelId().ok())
            .map(|h| h.to_string());
        set_stage(Stage::Idle);
        let watched_id = self.watched.as_ref().map(|(_, id, _)| id.as_str());
        if !force && current_id.as_deref() == watched_id {
            return;
        }
        if let Some((old, _, [t_props, t_play])) = self.watched.take() {
            // The old session may already be deregistered — best effort.
            let _ = old.RemoveMediaPropertiesChanged(t_props);
            let _ = old.RemovePlaybackInfoChanged(t_play);
        }
        let (Some(session), Some(id)) = (current, current_id) else {
            return;
        };
        let props_tx = self.tx.clone();
        let t_props = session.MediaPropertiesChanged(&TypedEventHandler::new(move |_, _| {
            let _ = props_tx.send(Wake::Event);
            Ok(())
        }));
        let play_tx = self.tx.clone();
        let t_play = session.PlaybackInfoChanged(&TypedEventHandler::new(move |_, _| {
            let _ = play_tx.send(Wake::Event);
            Ok(())
        }));
        // Timeline events are deliberately NOT subscribed: Apple Music fires
        // one per second for position, and the heartbeat already bounds
        // position staleness at the same interval the frontend interpolates
        // over. Events buy latency only where polling is visibly slow —
        // track/art/status changes. Position-only ticks don't marshal
        // metadata either (plan_beat's reuse), so the pair stays: events
        // for the fields that announce themselves, the tick for the one
        // that doesn't.
        match (t_props, t_play) {
            (Ok(t_props), Ok(t_play)) => {
                self.watched = Some((session, id, [t_props, t_play]));
            }
            // Partial registration: remove the half that landed, or the next
            // iteration (watched stayed None) re-registers on this same
            // session and the orphaned handler compounds every heartbeat.
            (Ok(t_props), Err(_)) => {
                let _ = session.RemoveMediaPropertiesChanged(t_props);
            }
            (Err(_), Ok(t_play)) => {
                let _ = session.RemovePlaybackInfoChanged(t_play);
            }
            (Err(_), Err(_)) => {}
        }
    }

    /// The app_id whose change handlers are currently attached. plan_beat's
    /// position-only reuse is licensed by this: "metadata cannot change
    /// without MediaPropertiesChanged waking us" only holds while the
    /// CURRENT session's handlers are live — degraded to pure polling (no
    /// SessionWatch) or inside a failed-attach window, the heartbeat's full
    /// marshal is back to being the only thing that catches a track change.
    pub fn watching(&self) -> Option<&str> {
        self.watched.as_ref().map(|(_, id, _)| id.as_str())
    }
}

fn player_kind(app_id: &str) -> &'static str {
    let id = app_id.to_lowercase();
    if id.contains("applemusic") {
        "apple_music"
    } else if id.contains("spotify") {
        "spotify"
    } else {
        "other"
    }
}

/// The track identity hash — shared vocabulary across the app: the art cache
/// key, the emitted art_id's key prefix, AND history.rs's entry/thumb key
/// (posClock keys on the same raw tuple frontend-side). Album and duration
/// are deliberately NOT included (that's the lyrics key).
///
/// DefaultHasher's algorithm is not guaranteed stable across Rust releases,
/// and history.rs persists these keys (entry `key`, thumb filenames). A
/// toolchain bump that changes it costs only cosmetics — old thumbs stop
/// resolving (glyph fallback, eviction cleans them up) while entry data stays
/// intact — so we accept it; switch to a fixed hasher if keys ever gain a
/// load-bearing cross-version meaning.
pub(crate) fn ident_key(app_id: &str, title: &str, artist: &str) -> String {
    let mut h = DefaultHasher::new();
    (app_id, title, artist).hash(&mut h);
    format!("{:x}", h.finish())
}

/// Cheap content fingerprint for probe comparisons: length + first/last ~1KB.
/// Deliberately NOT a full-image hash — probing runs every poll for a few
/// seconds and must not re-encode or re-hash whole covers.
fn art_fingerprint(bytes: &[u8]) -> u64 {
    let mut h = DefaultHasher::new();
    bytes.len().hash(&mut h);
    let k = bytes.len().min(1024);
    bytes[..k].hash(&mut h);
    bytes[bytes.len() - k..].hash(&mut h);
    h.finish()
}

/// Read the session thumbnail into raw bytes + mime. Best-effort: any failure
/// → None. ReadAsync may return FEWER bytes than requested (that was shipping
/// truncated images that failed to decode) — loop until the stream is drained.
fn read_art_bytes(session: &Session) -> Option<(Vec<u8>, String)> {
    const CHUNK: u32 = 262_144;
    let props = session
        .TryGetMediaPropertiesAsync()
        .ok()
        .and_then(|op| wait_op(op, OP_TIMEOUT_MS, Stage::MediaProps))?;
    set_stage(Stage::Thumbnail);
    let thumb = props.Thumbnail();
    set_stage(Stage::Idle);
    let stream = thumb
        .ok()?
        .OpenReadAsync()
        .ok()
        .and_then(|op| wait_op(op, ART_TIMEOUT_MS, Stage::ArtOpen))?;
    set_stage(Stage::Thumbnail);
    let size = stream.Size();
    set_stage(Stage::Idle);
    let size = size.ok()?;
    if size == 0 || size > 8_000_000 {
        return None;
    }
    // Apple Music reports a comma-separated LIST ("image/jpeg,image/jpe,image/jpg");
    // commas are invalid inside a data: URL mime — take the first entry only.
    let mime = stream
        .ContentType()
        .map(|h| h.to_string())
        .ok()
        .and_then(|m| m.split(',').next().map(|s| s.trim().to_string()))
        .filter(|m| m.starts_with("image/"))
        .unwrap_or_else(|| "image/jpeg".into());
    let mut bytes: Vec<u8> = Vec::with_capacity(size as usize);
    while (bytes.len() as u64) < size {
        // Cap the final request to the declared remainder — some streams are
        // views into a larger backing store and would return trailing garbage.
        let want = CHUNK.min((size - bytes.len() as u64) as u32);
        let chunk = Buffer::Create(want).ok()?;
        let chunk = stream
            .ReadAsync(&chunk, want, InputStreamOptions::ReadAhead)
            .ok()
            .and_then(|op| wait_op_progress(op, ART_TIMEOUT_MS, Stage::ArtRead))?;
        let len = chunk.Length().ok()? as usize;
        if len == 0 {
            break; // stream ended early — bail below if incomplete
        }
        let reader = DataReader::FromBuffer(&chunk).ok()?;
        let mut part = vec![0u8; len];
        reader.ReadBytes(&mut part).ok()?;
        bytes.extend_from_slice(&part);
    }
    if (bytes.len() as u64) < size {
        return None;
    }
    bytes.truncate(size as usize);
    Some((bytes, mime))
}

fn art_data_url(bytes: &[u8], mime: &str) -> String {
    format!("data:{};base64,{}", mime, B64.encode(bytes))
}

fn none_now() -> NowPlaying {
    NowPlaying {
        player: "none".into(),
        status: "none".into(),
        ..Default::default()
    }
}

/// Metadata + status + raw timeline for a session — everything but art.
/// Shared by snapshot() (which adds the art pipeline) and history_probe()
/// (the hidden-window history feed, which must do zero art work). Returns
/// the payload (art_id None) plus whether a thumbnail stream exists.
fn base_snapshot(session: &Session) -> (NowPlaying, bool) {
    set_stage(Stage::SessionId);
    let app_id = session.SourceAppUserModelId();
    set_stage(Stage::Idle);
    let app_id = app_id.map(|h| h.to_string()).unwrap_or_default();
    let player = player_kind(&app_id).to_string();

    let props = session
        .TryGetMediaPropertiesAsync()
        .ok()
        .and_then(|op| wait_op(op, OP_TIMEOUT_MS, Stage::MediaProps));
    // Property getters marshal off the fetched proxy too — same stage.
    set_stage(Stage::MediaProps);
    let (mut title, mut artist, mut album, media_kind, has_thumb) = match props {
        Some(p) => (
            p.Title().map(|h| h.to_string()).unwrap_or_default(),
            p.Artist().map(|h| h.to_string()).unwrap_or_default(),
            p.AlbumTitle().map(|h| h.to_string()).unwrap_or_default(),
            // MediaPlaybackType — informational. History's music gate is
            // the Apple Music / Spotify player allowlist; this kind only
            // drops video/image from those apps. Browsers/YouTube often
            // report Music(1) for anime and other video, so the kind alone
            // is not a music signal. Nullable: a null IReference, read
            // error, or Unknown(0) all fall to "unknown". Match the
            // numeric tuple like playback_info's PlaybackStatus(4).
            match p.PlaybackType().ok().and_then(|r| r.Value().ok()) {
                Some(MediaPlaybackType(1)) => "music",
                Some(MediaPlaybackType(2)) => "video",
                Some(MediaPlaybackType(3)) => "image",
                _ => "unknown",
            }
            .to_string(),
            p.Thumbnail().is_ok(),
        ),
        // Non-empty here too, so "" stays the pre-feature legacy sentinel.
        None => (
            String::new(),
            String::new(),
            String::new(),
            "unknown".into(),
            false,
        ),
    };
    set_stage(Stage::Idle);
    // Apple Music packs "artist — album" into the artist field (matrix quirk #5).
    if player == "apple_music" && album.is_empty() {
        if let Some((a, b)) = artist.split_once(" — ") {
            album = b.trim().to_string();
            artist = a.trim().to_string();
        }
    }
    if title.is_empty() && artist.is_empty() {
        title = "Unknown".into();
    }

    let (status, can_seek) = playback_info(session);
    let (mut position_ms, duration_ms, position_at_ms) = raw_timeline(session);
    if duration_ms > 0 {
        position_ms = position_ms.clamp(0, duration_ms);
    }

    (
        NowPlaying {
            app_id,
            player,
            title,
            artist,
            album,
            status: status.to_string(),
            position_ms,
            duration_ms,
            position_at_ms,
            can_seek,
            art_id: None,
            media_kind,
        },
        has_thumb,
    )
}

/// Art-free observation for the history tracker while the window is hidden —
/// the ONE narrow exception to "the media loop does no work while hidden"
/// (~5s cadence, no art marshal, no emit; see lib.rs's media loop).
pub fn history_probe() -> NowPlaying {
    match current_session() {
        Some(session) => base_snapshot(&session).0,
        None => none_now(),
    }
}

/// Snapshot the current session into a NowPlaying payload, stamping art_id
/// from the art cache. Never blocks on a thumbnail read: a new (app, title,
/// artist) key is claimed as Pending (art_id None) and the media loop's
/// art_pump runs the read off the emit path — every emit_now caller gets the
/// metadata-first behavior with no special cases.
pub fn snapshot(art_cache: &ArtCache) -> NowPlaying {
    // Dev sim-wedge: fake a hung WinRT call, media-loop thread only (see
    // simulate_wedge — seed/transport snapshots must stay live, like the
    // real incident).
    #[cfg(debug_assertions)]
    if IS_MEDIA_LOOP.with(|f| f.get()) {
        let remaining = SIM_WEDGE_UNTIL_MS.load(Ordering::Relaxed) - now_ms();
        if remaining > 0 {
            set_stage(Stage::SimWedge);
            std::thread::sleep(Duration::from_millis(remaining as u64));
            set_stage(Stage::Idle);
        }
    }
    let Some(session) = current_session() else {
        return none_now();
    };
    let (mut np, has_thumb) = base_snapshot(&session);

    np.art_id = if has_thumb {
        let key = ident_key(&np.app_id, &np.title, &np.artist);
        let mut cache = lock_art(art_cache);
        match cache.as_ref() {
            Some(state) if state.key() == key => state.art_id(),
            // New key: claim it as Pending and emit metadata-first. Racing
            // snapshots (poll thread + hotkey/command emit_now) writing the
            // same Pending is idempotent; art_pump resolves it.
            _ => {
                *cache = Some(ArtState::Pending {
                    key,
                    first_seen_ms: now_ms(),
                });
                None
            }
        }
    } else {
        None
    };
    np
}

/// A finished thumbnail read, reduced to what the cache stores.
struct ArtRead {
    fingerprint: u64,
    url: String,
}

/// The cache stage art_pump sampled under the lock before its unlocked
/// stream read — the compare-and-set token for the re-lock. A read whose
/// sample no longer matches the slot (the key changed in flight, or the
/// stage advanced) is dropped: the newer key's own pipeline owns the cache.
#[derive(Clone, Copy)]
enum ArtSample {
    Pending,
    Probe { rev: u32, fingerprint: Option<u64> },
}

/// Pure state transition for a completed art read (read None = the read
/// failed). Returns true when the emitted art_id changed, i.e. the caller
/// owes the frontend a follow-up emit.
fn apply_art_read(
    slot: &mut Option<ArtState>,
    key: &str,
    sampled: ArtSample,
    read: Option<ArtRead>,
) -> bool {
    match (slot.as_mut(), sampled) {
        // Deferred first read: Pending → Cached at rev 0. A failed read is
        // cached too (url None, art_id stays None — nothing to emit); probe
        // re-reads inside the window get to upgrade it.
        (
            Some(ArtState::Pending {
                key: k,
                first_seen_ms,
            }),
            ArtSample::Pending,
        ) if k == key => {
            let first_seen_ms = *first_seen_ms;
            let present = read.is_some();
            let (url, fingerprint) = match read {
                Some(r) => (Some(r.url), Some(r.fingerprint)),
                None => (None, None),
            };
            *slot = Some(ArtState::Cached {
                key: key.to_string(),
                rev: 0,
                url,
                fingerprint,
                first_seen_ms,
                settled: false,
            });
            present
        }
        (
            Some(ArtState::Cached {
                key: k,
                rev,
                url,
                fingerprint,
                settled,
                ..
            }),
            ArtSample::Probe {
                rev: sampled_rev,
                fingerprint: sampled_fp,
            },
        ) if k == key && *rev == sampled_rev => {
            match read {
                // Failed probe read = no information — keep the cached entry
                // (don't drop a good image on a transient failure), keep probing.
                None => false,
                // Two consecutive reads with identical bytes: settled, at any
                // rev — a correct first read stops after this confirming read
                // instead of running out the 10s window.
                Some(r) if sampled_fp == Some(r.fingerprint) => {
                    *settled = true;
                    false
                }
                // Bytes changed under the same key — the pinned image was
                // stale (or this change IS the stale-to-fresh transition).
                // Bump rev so the emitted art_id changes and the frontend
                // re-fetches; stay unsettled so the next read must confirm
                // stability. An id is never re-associated with different
                // bytes — the frontend latches an id on first fetch.
                Some(r) => {
                    *rev += 1;
                    *url = Some(r.url);
                    *fingerprint = Some(r.fingerprint);
                    *settled = false;
                    true
                }
            }
        }
        // Key or stage moved on while the read was in flight — drop the
        // result; stamping it would file one track's art under another's id.
        _ => false,
    }
}

/// Media-loop art pump: runs the deferred first read for a Pending key and
/// the distrust-window probe re-reads, entirely off the emit path. Called
/// right after emit_now with the payload it returned (`now` names the key
/// the read is for). Returns true when the cached art_id changed — the
/// caller re-emits so the frontend gets the same payload with art_id set.
///
/// Runs ONLY on the media loop thread, right after the emit: single-flight
/// per key for free (one thread, sequential reads), byte-level cache writes
/// stay single-writer, and the loop's stage telemetry covers the read. The
/// cost is that a slow read delays the next heartbeat tick — bounded by
/// wait_op's 3s-per-chunk timeout, the same bound snapshot() itself carried
/// when the read sat on the emit path.
///
/// The lock is never held across the stream read (media_art shares it):
/// sample under the lock, read unlocked, re-lock and compare-and-set.
pub fn art_pump(cache: &ArtCache, now: &NowPlaying) -> bool {
    let key = ident_key(&now.app_id, &now.title, &now.artist);
    let sampled = {
        let slot = lock_art(cache);
        match slot.as_ref() {
            // A Pending first read runs regardless of window age (a key
            // claimed while the loop was hidden must still get its art).
            Some(ArtState::Pending { key: k, .. }) if *k == key => Some(ArtSample::Pending),
            Some(ArtState::Cached {
                key: k,
                rev,
                fingerprint,
                first_seen_ms,
                settled,
                ..
            }) if *k == key && !settled && now_ms() - first_seen_ms < ART_PROBE_WINDOW_MS => {
                Some(ArtSample::Probe {
                    rev: *rev,
                    fingerprint: *fingerprint,
                })
            }
            _ => None,
        }
    };
    let Some(sampled) = sampled else {
        return false;
    };
    let read = current_session()
        .and_then(|s| read_art_bytes(&s))
        .map(|(bytes, mime)| ArtRead {
            fingerprint: art_fingerprint(&bytes),
            url: art_data_url(&bytes, &mime),
        });
    apply_art_read(&mut lock_art(cache), &key, sampled, read)
}

/// Re-open art distrust on a media-properties wake inside the probe window.
/// The settle rule can latch a stale-but-stable first capture: Apple Music
/// attaches the new thumbnail seconds after the metadata, so two early reads
/// both see the OLD cover and settle on it — and the attach announces itself
/// as exactly the MediaPropertiesChanged wake this rides. Un-settling lets
/// the next art_pump catch the byte change (rev bump → follow-up emit).
/// Bounded: outside the window this is a no-op, so wake spam (pause/play)
/// costs at most a few extra reads per track.
pub fn art_rearm(cache: &ArtCache) {
    if let Some(ArtState::Cached {
        settled,
        first_seen_ms,
        ..
    }) = lock_art(cache).as_mut()
    {
        if *settled && now_ms() - *first_seen_ms < ART_PROBE_WINDOW_MS {
            *settled = false;
        }
    }
}

/// Convert the raw timeline ticks (Position TimeSpan, LastUpdatedTime
/// FILETIME) to the emitted (position_ms, position_at_ms) pair. THE one
/// conversion, shared by raw_timeline (every full snapshot) and the media
/// loop's position-only reuse path (which derives the pair from tick_key's
/// raw ticks) — two copies would let the reused pair drift from what a full
/// snapshot would have emitted this beat. Updated ticks 0 covers both "read
/// failed" and "never stamped"; it converts to the documented position_at_ms
/// sentinel 0, never the negative FILETIME-epoch artifact.
pub fn position_pair_from_ticks(position_ticks: i64, updated_ticks: i64) -> (i64, i64) {
    let position_at_ms = if updated_ticks == 0 {
        0
    } else {
        updated_ticks / TICKS_PER_MS - FILETIME_EPOCH_OFFSET_MS
    };
    (position_ticks / TICKS_PER_MS, position_at_ms)
}

/// Raw GSMTC timeline triple: (position_ms, duration_ms, last_updated_unix_ms).
/// Deliberately NO staleness projection — re-projecting each snapshot is what
/// let Apple Music's 1s-quantized pushes land behind the previous projection
/// (the lyric flash-back), and the old out-of-range fallback to the raw
/// position was a second regression source. last_updated is 0 when the player
/// never stamped the timeline.
fn raw_timeline(session: &Session) -> (i64, i64, i64) {
    set_stage(Stage::Timeline);
    let t = session.GetTimelineProperties();
    set_stage(Stage::Idle);
    let Ok(t) = t else {
        return (0, 0, 0);
    };
    let pos_ticks = t.Position().map(|d| d.Duration).unwrap_or(0);
    let end = t.EndTime().map(|d| d.Duration / TICKS_PER_MS).unwrap_or(0);
    let updated_ticks = t.LastUpdatedTime().map(|d| d.UniversalTime).unwrap_or(0);
    let (pos, updated) = position_pair_from_ticks(pos_ticks, updated_ticks);
    (pos, end, updated)
}

/// (status, can_seek) from ONE GetPlaybackInfo read — snapshot used to fetch
/// it twice (status, then capability).
fn playback_info(session: &Session) -> (&'static str, bool) {
    set_stage(Stage::PlaybackInfo);
    let info = session.GetPlaybackInfo();
    let out = match info {
        Err(_) => ("stopped", false),
        Ok(info) => {
            let status = match info.PlaybackStatus() {
                Ok(PlaybackStatus(4)) => "playing",
                Ok(PlaybackStatus(5)) => "paused",
                _ => "stopped",
            };
            let can_seek = info
                .Controls()
                .and_then(|c| c.IsPlaybackPositionEnabled())
                .unwrap_or(false);
            (status, can_seek)
        }
    };
    set_stage(Stage::Idle);
    out
}

fn playback_status(session: &Session) -> &'static str {
    playback_info(session).0
}

/// The CURRENT session's player bucket ("apple_music"|"spotify"|"other"|
/// "none") — the queue-aware skip's cheap gate (one session read, no
/// metadata marshal).
pub fn current_player() -> &'static str {
    match current_session() {
        Some(s) => s
            .SourceAppUserModelId()
            .map(|h| player_kind(&h.to_string()))
            .unwrap_or("other"),
        None => "none",
    }
}

/// Cheap heartbeat probe: (app_id, timeline Position + LastUpdatedTime ticks,
/// status). The media loop skips the full snapshot (metadata marshal + art
/// work + emit) when this hasn't moved since the previous tick, and when ONLY
/// the timeline pair moved it republishes the last emitted payload with the
/// fresh pair instead of marshaling (see plan_beat) — Apple Music floors
/// position to 1s, so a full marshal per tick meant waking the player process
/// every second for the length of every song. Position is included on its own
/// because a player may move it without re-stamping LastUpdatedTime
/// (unverified for programmatic Spotify seeks) — the post-seek UI bound must
/// stay one heartbeat, not one push cadence. Metadata-only changes still
/// snapshot: MediaPropertiesChanged wakes force one.
pub type TickKey = (String, i64, i64, &'static str);

pub fn tick_key() -> Option<TickKey> {
    let session = current_session()?;
    set_stage(Stage::SessionId);
    let app_id = session.SourceAppUserModelId();
    set_stage(Stage::Idle);
    let app_id = app_id.map(|h| h.to_string()).unwrap_or_default();
    set_stage(Stage::Timeline);
    let timeline = session.GetTimelineProperties();
    set_stage(Stage::Idle);
    let (position, updated) = timeline
        .map(|t| {
            (
                t.Position().map(|d| d.Duration).unwrap_or(0),
                t.LastUpdatedTime().map(|d| d.UniversalTime).unwrap_or(0),
            )
        })
        .unwrap_or((0, 0));
    Some((app_id, position, updated, playback_status(&session)))
}

/// True while the art pipeline needs the media loop to run full snapshots
/// (where art_pump lives): a Pending key waiting for its deferred first
/// read, or an unsettled Cached entry inside the distrust window (art_pump
/// re-reads each tick then). Settled or window-expired Cached entries stop
/// forcing snapshots. Pending probes REGARDLESS of age, matching art_pump's
/// own sampling rule: a key claimed by an off-loop snapshot (hotkey emit
/// while hidden) can outlive the window before the loop ever pumps it, and
/// the position-only reuse path would otherwise starve that first read
/// forever — pre-reuse, the position-forced full snapshots healed it by
/// accident.
pub fn art_probing(cache: &ArtCache) -> bool {
    lock_art(cache).as_ref().is_some_and(|e| match e {
        ArtState::Pending { .. } => true,
        ArtState::Cached {
            settled,
            first_seen_ms,
            ..
        } => !settled && now_ms() - first_seen_ms < ART_PROBE_WINDOW_MS,
    })
}

/// What the media loop's heartbeat does this beat (see plan_beat).
#[derive(Debug, PartialEq)]
pub enum BeatAction {
    /// Nothing moved — no snapshot, no emit.
    Skip,
    /// Only the raw timeline pair moved and the last emitted payload is
    /// reusable: republish it with a fresh position pair (lib.rs
    /// emit_position_refresh), no metadata marshal, no art_pump.
    ReusePosition,
    /// Full snapshot: metadata marshal, emit, art_pump.
    Snapshot,
}

/// The heartbeat escalation decision, pure so the branch is testable.
/// `last_payload` summarizes the last payload actually emitted, as
/// (app_id, status); None = nothing emitted yet. `subscribed` = the tick's
/// session is the one SessionWatch currently holds handlers on.
///
/// Reusing on a position-only delta is sound because metadata cannot change
/// without MediaPropertiesChanged firing, and that wake sets `force` before
/// the next beat; status is part of the tick itself. Every other delta
/// escalates to the full marshal:
/// - `force` (event wakes, first beat after launch/show) and `probing` (the
///   art pipeline needs the full path — art_pump only runs there).
/// - Session appear/vanish (either tick None), or an app_id/status delta.
/// - A missing or mismatched last payload: a session that vanished and came
///   back must re-marshal, and an empty app_id is never trusted as identity.
/// - `subscribed` false: without live handlers the reuse premise is gone —
///   a track auto-advance would republish stale metadata under fresh
///   positions for the rest of playback. The heartbeat marshal is the
///   pre-reuse safety net ("missed events are never fatal"); keep it.
pub fn plan_beat(
    tick: Option<&TickKey>,
    last_tick: Option<&TickKey>,
    force: bool,
    probing: bool,
    subscribed: bool,
    last_payload: Option<(&str, &str)>,
) -> BeatAction {
    if force || probing {
        return BeatAction::Snapshot;
    }
    if tick == last_tick {
        return BeatAction::Skip;
    }
    let (Some((app_id, _, _, status)), Some((last_app, _, _, last_status))) = (tick, last_tick)
    else {
        return BeatAction::Snapshot;
    };
    let position_only = !app_id.is_empty() && app_id == last_app && status == last_status;
    let payload_matches =
        last_payload.is_some_and(|(p_app, p_status)| p_app == app_id && p_status == *status);
    if position_only && payload_matches && subscribed {
        BeatAction::ReusePosition
    } else {
        BeatAction::Snapshot
    }
}

pub fn play_pause() -> bool {
    current_session()
        .and_then(|s| s.TryTogglePlayPauseAsync().ok())
        .and_then(|op| wait_op(op, OP_TIMEOUT_MS, Stage::Transport))
        .unwrap_or(false)
}

pub fn next() -> bool {
    current_session()
        .and_then(|s| s.TrySkipNextAsync().ok())
        .and_then(|op| wait_op(op, OP_TIMEOUT_MS, Stage::Transport))
        .unwrap_or(false)
}

pub fn prev() -> bool {
    current_session()
        .and_then(|s| s.TrySkipPreviousAsync().ok())
        .and_then(|op| wait_op(op, OP_TIMEOUT_MS, Stage::Transport))
        .unwrap_or(false)
}

/// Absolute seek. Returns false when the session is gone or refuses the call.
/// NOTE: Apple Music returns true and does nothing — callers should treat the
/// bool as "command delivered", not "seek happened" (matrix finding #3).
pub fn seek_abs_ms(target_ms: i64) -> bool {
    let Some(session) = current_session() else {
        return false;
    };
    let target = target_ms.max(0) * TICKS_PER_MS;
    session
        .TryChangePlaybackPositionAsync(target)
        .ok()
        .and_then(|op| wait_op(op, OP_TIMEOUT_MS, Stage::Transport))
        .unwrap_or(false)
}

/// Relative seek anchored on a locally projected position (Spotify's raw
/// reported position can be ~5s behind what the user hears), clamped to the
/// track bounds. This projection is the ONE left in Rust and it never reaches
/// the UI — global hotkeys land here without passing through the frontend, so
/// the ±10s anchor must be computed on this side.
pub fn seek_rel_ms(delta_ms: i64) -> bool {
    let Some(session) = current_session() else {
        return false;
    };
    let (pos, end, updated) = raw_timeline(&session);
    let staleness = now_ms() - updated;
    let pos = if playback_status(&session) == "playing" && (0..30_000).contains(&staleness) {
        pos + staleness
    } else {
        pos
    };
    let end = if end > 0 { end } else { i64::MAX };
    seek_abs_ms((pos + delta_ms).clamp(0, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(fp: u64) -> Option<ArtRead> {
        Some(ArtRead {
            fingerprint: fp,
            url: format!("data:image/jpeg;base64,fp{fp}"),
        })
    }

    fn pending(key: &str) -> Option<ArtState> {
        Some(ArtState::Pending {
            key: key.into(),
            first_seen_ms: now_ms(),
        })
    }

    fn cached(key: &str, rev: u32, fp: Option<u64>, settled: bool) -> Option<ArtState> {
        Some(ArtState::Cached {
            key: key.into(),
            rev,
            url: fp.map(|f| format!("data:image/jpeg;base64,fp{f}")),
            fingerprint: fp,
            first_seen_ms: now_ms(),
            settled,
        })
    }

    fn state(slot: &Option<ArtState>) -> (u32, Option<u64>, bool) {
        match slot.as_ref().expect("cached state") {
            ArtState::Cached {
                rev,
                fingerprint,
                settled,
                ..
            } => (*rev, *fingerprint, *settled),
            ArtState::Pending { .. } => panic!("expected Cached"),
        }
    }

    #[test]
    fn pending_first_read_lands_at_rev_zero_and_emits() {
        let mut slot = pending("a");
        assert!(apply_art_read(&mut slot, "a", ArtSample::Pending, read(7)));
        assert_eq!(state(&slot), (0, Some(7), false));
        assert_eq!(
            slot.as_ref().unwrap().art_id().as_deref(),
            Some("a:0"),
            "first landed read emits the rev-0 id"
        );
    }

    #[test]
    fn identical_probe_reads_settle_at_rev_zero_and_stop_probing() {
        let mut slot = pending("a");
        apply_art_read(&mut slot, "a", ArtSample::Pending, read(7));
        let emit = apply_art_read(
            &mut slot,
            "a",
            ArtSample::Probe {
                rev: 0,
                fingerprint: Some(7),
            },
            read(7),
        );
        assert!(!emit, "settling changes no art_id");
        assert_eq!(state(&slot), (0, Some(7), true));
        let cache = ArtCache(Mutex::new(slot));
        assert!(!art_probing(&cache), "settled stops the forced snapshots");
    }

    #[test]
    fn byte_change_bumps_rev_and_requires_one_more_stable_read() {
        let mut slot = cached("a", 0, Some(7), false);
        let emit = apply_art_read(
            &mut slot,
            "a",
            ArtSample::Probe {
                rev: 0,
                fingerprint: Some(7),
            },
            read(9),
        );
        assert!(emit, "a rev bump owes the frontend a follow-up emit");
        assert_eq!(state(&slot), (1, Some(9), false));
        assert_eq!(slot.as_ref().unwrap().art_id().as_deref(), Some("a:1"));
        {
            let cache = ArtCache(Mutex::new(slot.take()));
            assert!(art_probing(&cache), "re-armed after the change");
            slot = cache.0.into_inner().unwrap();
        }
        let emit = apply_art_read(
            &mut slot,
            "a",
            ArtSample::Probe {
                rev: 1,
                fingerprint: Some(9),
            },
            read(9),
        );
        assert!(!emit);
        assert_eq!(state(&slot), (1, Some(9), true));
    }

    #[test]
    fn failed_probe_read_keeps_state_and_keeps_probing() {
        let mut slot = cached("a", 0, Some(7), false);
        let emit = apply_art_read(
            &mut slot,
            "a",
            ArtSample::Probe {
                rev: 0,
                fingerprint: Some(7),
            },
            None,
        );
        assert!(!emit);
        assert_eq!(
            state(&slot),
            (0, Some(7), false),
            "no information, no change"
        );
    }

    #[test]
    fn pending_failed_read_caches_miss_then_a_success_upgrades() {
        let mut slot = pending("a");
        let emit = apply_art_read(&mut slot, "a", ArtSample::Pending, None);
        assert!(!emit, "nothing to show yet, nothing to emit");
        assert_eq!(state(&slot), (0, None, false));
        assert_eq!(slot.as_ref().unwrap().art_id(), None);
        let emit = apply_art_read(
            &mut slot,
            "a",
            ArtSample::Probe {
                rev: 0,
                fingerprint: None,
            },
            read(7),
        );
        assert!(emit, "None→Some fingerprint counts as a byte change");
        assert_eq!(state(&slot), (1, Some(7), false));
    }

    #[test]
    fn in_flight_key_change_drops_the_read() {
        // Read for key "a" sampled Pending, but "b" claimed the slot while
        // the stream read ran — the result must not stamp a's art onto b.
        let mut slot = pending("b");
        assert!(!apply_art_read(&mut slot, "a", ArtSample::Pending, read(7)));
        match slot.as_ref().unwrap() {
            ArtState::Pending { key, .. } => assert_eq!(key, "b"),
            ArtState::Cached { .. } => panic!("b's Pending claim must survive"),
        }
        // Same for a probe read raced by a rev advance under the same key.
        let mut slot = cached("a", 2, Some(9), false);
        let emit = apply_art_read(
            &mut slot,
            "a",
            ArtSample::Probe {
                rev: 1,
                fingerprint: Some(7),
            },
            read(8),
        );
        assert!(!emit);
        assert_eq!(state(&slot), (2, Some(9), false), "stale-rev read dropped");
    }

    #[test]
    fn pending_forces_probing_and_rearm_reopens_the_window() {
        let cache = ArtCache(Mutex::new(pending("a")));
        assert!(
            art_probing(&cache),
            "a pending key must keep the loop ticking"
        );
        let cache = ArtCache(Mutex::new(cached("a", 0, Some(7), true)));
        assert!(!art_probing(&cache));
        art_rearm(&cache);
        assert!(
            art_probing(&cache),
            "a wake inside the window re-opens distrust"
        );
    }

    #[test]
    fn art_url_resolves_only_the_exact_cached_id() {
        let cache = ArtCache(Mutex::new(cached("a", 1, Some(7), true)));
        assert!(art_url(&cache, "a:1").is_some());
        assert_eq!(art_url(&cache, "a:0"), None, "outdated rev");
        assert_eq!(art_url(&cache, "b:1"), None, "different key");
        let cache = ArtCache(Mutex::new(pending("a")));
        assert_eq!(art_url(&cache, "a:0"), None, "pending has no bytes yet");
    }

    #[test]
    fn pending_probes_regardless_of_window_age() {
        // A key claimed off-loop (hotkey emit while hidden) can outlive the
        // probe window before the loop ever pumps it — position-only reuse
        // must not starve that first read.
        let cache = ArtCache(Mutex::new(Some(ArtState::Pending {
            key: "a".into(),
            first_seen_ms: now_ms() - ART_PROBE_WINDOW_MS - 60_000,
        })));
        assert!(
            art_probing(&cache),
            "aged Pending still owes its first read"
        );
    }

    fn tick(app: &str, pos: i64, updated: i64, status: &'static str) -> TickKey {
        (app.into(), pos, updated, status)
    }

    #[test]
    fn position_only_delta_reuses_the_last_payload() {
        let last = tick("am", 10_000_000, 500, "playing");
        let now = tick("am", 20_000_000, 600, "playing");
        assert_eq!(
            plan_beat(
                Some(&now),
                Some(&last),
                false,
                false,
                true,
                Some(("am", "playing")),
            ),
            BeatAction::ReusePosition
        );
        // LastUpdatedTime re-stamped without the position moving is still a
        // timeline-pair-only delta.
        let restamped = tick("am", 10_000_000, 900, "playing");
        assert_eq!(
            plan_beat(
                Some(&restamped),
                Some(&last),
                false,
                false,
                true,
                Some(("am", "playing")),
            ),
            BeatAction::ReusePosition
        );
    }

    #[test]
    fn app_id_or_status_delta_escalates() {
        let last = tick("am", 10, 500, "playing");
        let switched = tick("spotify", 20, 600, "playing");
        assert_eq!(
            plan_beat(
                Some(&switched),
                Some(&last),
                false,
                false,
                true,
                Some(("am", "playing")),
            ),
            BeatAction::Snapshot
        );
        let paused = tick("am", 20, 600, "paused");
        assert_eq!(
            plan_beat(
                Some(&paused),
                Some(&last),
                false,
                false,
                true,
                Some(("am", "playing")),
            ),
            BeatAction::Snapshot
        );
    }

    #[test]
    fn force_or_probing_escalates_even_unchanged() {
        let t = tick("am", 10, 500, "playing");
        let payload = Some(("am", "playing"));
        assert_eq!(
            plan_beat(Some(&t), Some(&t), true, false, true, payload),
            BeatAction::Snapshot
        );
        assert_eq!(
            plan_beat(Some(&t), Some(&t), false, true, true, payload),
            BeatAction::Snapshot
        );
    }

    #[test]
    fn unsubscribed_session_escalates() {
        // No live MediaPropertiesChanged handler on this session (degraded
        // polling-only mode, or a failed-attach window) — the reuse premise
        // is gone, so a position-only delta still marshals.
        let last = tick("am", 10, 500, "playing");
        let now = tick("am", 20, 600, "playing");
        assert_eq!(
            plan_beat(
                Some(&now),
                Some(&last),
                false,
                false,
                false,
                Some(("am", "playing")),
            ),
            BeatAction::Snapshot
        );
    }

    #[test]
    fn missing_or_mismatched_last_payload_escalates() {
        let last = tick("am", 10, 500, "playing");
        let now = tick("am", 20, 600, "playing");
        assert_eq!(
            plan_beat(Some(&now), Some(&last), false, false, true, None),
            BeatAction::Snapshot,
            "nothing emitted yet"
        );
        assert_eq!(
            plan_beat(
                Some(&now),
                Some(&last),
                false,
                false,
                true,
                Some(("spotify", "playing")),
            ),
            BeatAction::Snapshot,
            "payload from a different app"
        );
        assert_eq!(
            plan_beat(
                Some(&now),
                Some(&last),
                false,
                false,
                true,
                Some(("am", "paused")),
            ),
            BeatAction::Snapshot,
            "payload's playing-shape out of step with the tick"
        );
    }

    #[test]
    fn session_edges_and_empty_identity_escalate_or_skip() {
        let t = tick("am", 10, 500, "playing");
        let payload = Some(("am", "playing"));
        assert_eq!(
            plan_beat(None, Some(&t), false, false, true, payload),
            BeatAction::Snapshot,
            "session vanished"
        );
        assert_eq!(
            plan_beat(Some(&t), None, false, false, true, payload),
            BeatAction::Snapshot,
            "session appeared"
        );
        assert_eq!(
            plan_beat(None, None, false, false, true, None),
            BeatAction::Skip,
            "no session on both beats is an unchanged tick"
        );
        let anon_last = tick("", 10, 500, "playing");
        let anon_now = tick("", 20, 600, "playing");
        assert_eq!(
            plan_beat(
                Some(&anon_now),
                Some(&anon_last),
                false,
                false,
                true,
                Some(("", "playing")),
            ),
            BeatAction::Snapshot,
            "an empty app_id is never trusted as identity"
        );
    }

    #[test]
    fn unchanged_tick_skips() {
        let t = tick("am", 10, 500, "playing");
        assert_eq!(
            plan_beat(
                Some(&t),
                Some(&t),
                false,
                false,
                true,
                Some(("am", "playing"))
            ),
            BeatAction::Skip
        );
    }

    #[test]
    fn position_pair_conversion_matches_raw_timeline() {
        // 3_000 ms of 100ns ticks; a known unix stamp through the FILETIME
        // epoch offset — the same math raw_timeline applies.
        let unix_ms = 1_756_700_000_123i64;
        let updated_ticks = (unix_ms + FILETIME_EPOCH_OFFSET_MS) * TICKS_PER_MS;
        assert_eq!(
            position_pair_from_ticks(3_000 * TICKS_PER_MS, updated_ticks),
            (3_000, unix_ms)
        );
        assert_eq!(
            position_pair_from_ticks(0, 0),
            (0, 0),
            "updated 0 = never stamped, the documented sentinel"
        );
    }
}
