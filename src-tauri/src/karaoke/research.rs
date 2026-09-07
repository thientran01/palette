//! Explicit one-song evidence capture. Never writes a sync cache or runs a model.
use super::*;
use tauri_plugin_opener::OpenerExt;

const RESEARCH_DIR: &str = "karaoke-research";
#[derive(Clone, Default, Serialize)]
pub struct ResearchStatus {
    id: u64,
    key: String,
    title: String,
    phase: String,
    detail: String,
    elapsed_ms: i64,
    progress: u8,
}
impl ResearchStatus {
    fn busy(&self) -> bool {
        matches!(self.phase.as_str(), "armed" | "recording" | "saving")
    }
    fn arm(&mut self, key: String, title: String) -> Result<(), String> {
        if self.busy() {
            return Err("Finish or cancel the current research recording first.".into());
        }
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        *self = Self {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            key,
            title,
            phase: "armed".into(),
            detail: "Replay from the beginning with Palette visible.".into(),
            ..Self::default()
        };
        Ok(())
    }
    fn begin(&mut self, key: &str) -> Option<u64> {
        if self.phase != "armed" || self.key != key {
            return None;
        }
        self.phase = "recording".into();
        self.detail = "Local audio only. Stops when the track changes.".into();
        Some(self.id)
    }
    fn finish(&mut self, id: u64, phase: &str, detail: &str) -> bool {
        if self.id == id && id != 0 {
            self.phase = phase.into();
            self.detail = detail.into();
            return true;
        }
        false
    }
}
fn state() -> std::sync::MutexGuard<'static, ResearchStatus> {
    static STATE: OnceLock<Mutex<ResearchStatus>> = OnceLock::new();
    STATE
        .get_or_init(|| Mutex::new(ResearchStatus::default()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
pub(super) fn requested(key: &str) -> bool {
    let s = state();
    s.key == key && s.phase == "armed"
}
pub(super) fn begin(key: &str) -> Option<u64> {
    state().begin(key)
}
pub(super) fn interrupted(rec: &Rec, reason: &str) {
    if let Some(id) = rec.research {
        state().finish(id, "error", reason);
    }
}
#[tauri::command]
pub async fn research_status() -> ResearchStatus {
    // The recorder lock is always acquired before the status lock.
    let rec = lock_rec();
    let mut s = state().clone();
    if let Some(rec) = rec.as_ref().filter(|r| r.research == Some(s.id)) {
        s.elapsed_ms = rec.samples.len() as i64 * 1000 / TARGET_HZ as i64;
        s.progress = recording_progress(rec.samples.len(), rec.duration_ms, rec.origin_ms);
    }
    s
}
#[tauri::command]
pub async fn research_action(
    app: AppHandle,
    action: String,
    artist: String,
    title: String,
    album: String,
    duration_ms: i64,
) -> Result<(), String> {
    if action == "open" {
        let root = app
            .path()
            .app_data_dir()
            .map_err(|e| e.to_string())?
            .join(RESEARCH_DIR);
        return tauri::async_runtime::spawn_blocking(move || {
            std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
            app.opener()
                .open_path(root.to_string_lossy(), None::<&str>)
                .map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())?;
    }
    let mut slot = lock_rec();
    let mut s = state();
    match action.as_str() {
        "start" => {
            if title.trim().is_empty() || duration_ms <= 0 {
                return Err("Choose a song in Spotify or Apple Music first.".into());
            }
            let key = lyrics::key_for_ms(&artist, &title, &album, duration_ms);
            s.arm(key.clone(), title)?;
            if let Some(rec) = slot.as_mut().filter(|r| r.key == key) {
                rec.research = s.begin(&key);
            }
        }
        "cancel" => {
            if s.phase == "saving" {
                return Err("The recording is already saving.".into());
            }
            // Cancel only this request, never an unrelated ordinary recording.
            if slot.as_ref().is_some_and(|r| r.research == Some(s.id)) {
                RECORDING.store(false, Ordering::Relaxed);
                *slot = None;
            }
            *s = ResearchStatus::default();
        }
        "stop" => {
            if s.phase != "recording" {
                return Err("No research recording is active.".into());
            }
            if slot.as_ref().is_none_or(|r| r.research != Some(s.id)) {
                return Err("The recording has already stopped.".into());
            }
            RECORDING.store(false, Ordering::Relaxed);
            let rec = slot.take().expect("checked recorder");
            drop(s);
            drop(slot);
            save(&app, rec);
        }
        _ => return Err("Unknown recording action.".into()),
    }
    Ok(())
}

pub(super) fn save(app: &AppHandle, rec: Rec) {
    let id = rec.research.expect("research recording");
    // Cancellation can win between recorder removal and save. In that case
    // neither write audio nor allow this worker to finish a newer request.
    if !state().finish(id, "saving", "Saving audio and timing evidence…") {
        return;
    }
    let handle = app.clone();
    let result = std::thread::Builder::new()
        .name("karaoke-research".into())
        .spawn(move || {
            let result = save_inner(&handle, &rec);
            match result {
                Ok(detail) => {
                    state().finish(id, "saved", detail);
                }
                Err(e) => {
                    log::warn!("karaoke research: {}: {e}", rec.title);
                    state().finish(id, "error", &format!("Couldn’t save recording: {e}"));
                }
            }
        });
    if let Err(e) = result {
        state().finish(id, "error", &format!("Couldn’t start saving: {e}"));
    }
}
fn save_inner(app: &AppHandle, rec: &Rec) -> Result<&'static str, String> {
    if rec.samples.len() < TARGET_HZ as usize {
        return Err("Less than one second of audio was captured. Replay and try again.".into());
    }
    let root = app.path().app_data_dir().map_err(|e| e.to_string())?;
    save_to(&root, rec, crate::vocal_preview::enabled())
}
fn save_to(root: &Path, rec: &Rec, preview_enabled: bool) -> Result<&'static str, String> {
    let synced = lyrics::cached_synced(
        &root.join("lyrics"),
        &rec.artist,
        &rec.title,
        &rec.album,
        rec.duration_ms,
    )
    .unwrap_or_default();
    // Snapshot the timings being investigated without altering cache/status state.
    let mut words = Vec::new();
    let mut timing_source = None;
    for cache in [
        crate::vocal_preview::CACHE_DIR,
        crate::vocal_preview::PREVIOUS_CACHE_DIR,
        crate::vocal_preview::OLDER_CACHE_DIR,
        "karaoke",
    ] {
        if cache != "karaoke" && !preview_enabled {
            continue;
        }
        if crate::sync_library::flags(root, &rec.key).0 {
            break;
        }
        words = read_file_preserving(
            &root.join(cache).join(format!("{}.json", rec.key)),
            Some(&synced),
        );
        if !words.is_empty() {
            timing_source = Some(cache);
            break;
        }
    }
    let dir = root
        .join(RESEARCH_DIR)
        .join(format!("{}-{}", unix_ms(), rec.key));
    let map = TimeMap::fit(&rec.anchors, TARGET_HZ, rec.origin_ms);
    write_dump(&dir, rec, &synced, &words, &map).map_err(|e| e.to_string())?;
    // Playable without importing raw PCM. No extra work on the audio callback.
    let pcm = std::fs::read(dir.join("pcm.i16")).map_err(|e| e.to_string())?;
    write_atomic(&dir.join("audio.wav"), &wav(&pcm)).map_err(|e| e.to_string())?;
    let complete =
        rec.can_finalize() && cache_complete(map.pos_ms(rec.samples.len()), rec.duration_ms);
    let manifest = serde_json::json!({
        "purpose":"research-only", "complete":complete, "peak":rec.peak,
        "timing_cache":timing_source, "vocal_preview_enabled":preview_enabled,
        "seek_strikes":rec.seek_strikes, "words_origin":"existing saved timing; not generated by this recording",
        "audio":"16 kHz mono signed 16-bit little-endian PCM", "saved_at_ms":unix_ms()
    });
    write_atomic(
        &dir.join("research.json"),
        &serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(if complete {
        "Recording saved locally. Saved lyric timing is unchanged."
    } else {
        "Partial recording saved for research. Saved lyric timing is unchanged."
    })
}

fn wav(pcm: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(44 + pcm.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&TARGET_HZ.to_le_bytes());
    out.extend_from_slice(&(TARGET_HZ * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    out.extend_from_slice(pcm);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn partial_research_capture_saves_audio_and_never_overwrites_existing_sync() {
        let root = std::env::temp_dir().join(format!(
            "palette-research-test-{}-{}",
            std::process::id(),
            unix_ms()
        ));
        let mut rec = super::super::tests::rec_with(vec![(0, 0), (16_000, 1000)], 180_000);
        rec.research = Some(1);
        rec.key = lyrics::key_for_ms(&rec.artist, &rec.title, &rec.album, rec.duration_ms);
        std::fs::create_dir_all(root.join("lyrics")).unwrap();
        std::fs::write(
            root.join("lyrics").join(format!("{}.json", rec.key)),
            br#"{"synced":"[00:00.00]t"}"#,
        )
        .unwrap();
        rec.samples = vec![0.25; 32_000];
        let cache = root.join("karaoke");
        write_file(
            &cache,
            &rec.key,
            "[00:00.00]t",
            &[Word {
                text: "t".into(),
                t: 250,
                end: Some(1500),
                ..Word::default()
            }],
        )
        .unwrap();
        write_file(
            &root.join(crate::vocal_preview::CACHE_DIR),
            &rec.key,
            "[00:00.00]t",
            &[Word {
                text: "t".into(),
                t: 750,
                end: Some(1500),
                ..Word::default()
            }],
        )
        .unwrap();
        let before = std::fs::read(cache.join(format!("{}.json", rec.key))).unwrap();
        assert!(save_to(&root, &rec, false).unwrap().starts_with("Partial"));
        assert_eq!(
            std::fs::read(cache.join(format!("{}.json", rec.key))).unwrap(),
            before
        );
        let dir = std::fs::read_dir(root.join(RESEARCH_DIR))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let pcm = std::fs::read(dir.join("pcm.i16")).unwrap();
        let audio = std::fs::read(dir.join("audio.wav")).unwrap();
        assert_eq!(&audio[..4], b"RIFF");
        assert_eq!(&audio[8..12], b"WAVE");
        assert_eq!(
            u32::from_le_bytes(audio[24..28].try_into().unwrap()),
            16_000
        );
        assert_eq!(&audio[44..], pcm);
        assert_eq!(pcm.len(), 64_000);
        let stored: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.join("words.json")).unwrap()).unwrap();
        assert_eq!(stored["words"][0]["t"], 250);
        let meta: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.join("research.json")).unwrap()).unwrap();
        assert_eq!(meta["complete"], false);
        assert_eq!(meta["purpose"], "research-only");
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn cached_song_can_be_requested_without_cache_mutation() {
        let mut s = ResearchStatus::default();
        s.arm("cached-song".into(), "Baby Flower".into()).unwrap();
        assert!(s.begin("next-song").is_none());
        assert_eq!(s.phase, "armed");
        assert!(s.begin("cached-song").is_some());
        assert_eq!(s.phase, "recording");
    }
    #[test]
    fn busy_request_cannot_be_replaced_or_started_twice() {
        let mut s = ResearchStatus::default();
        s.arm("one".into(), "Song".into()).unwrap();
        assert!(s.arm("two".into(), "Other".into()).is_err());
        assert!(s.begin("one").is_some());
        assert!(s.begin("one").is_none());
        s.finish(s.id, "saving", "Saving");
        assert!(s.arm("two".into(), "Other".into()).is_err());
        s.finish(s.id, "saved", "Done");
        assert!(s.arm("two".into(), "Other".into()).is_ok());
    }
    #[test]
    fn cancelled_same_song_cannot_be_saved_by_detached_old_recording() {
        let mut s = ResearchStatus::default();
        s.arm("same".into(), "Song".into()).unwrap();
        let old = s.begin("same").unwrap();
        s = ResearchStatus::default(); // Cancel after the recorder was detached.
        assert!(!s.finish(old, "saving", "Saving"));
        s.arm("same".into(), "Song".into()).unwrap();
        assert_ne!(s.id, old);
        assert!(!s.finish(old, "saved", "Done"));
        assert_eq!(s.phase, "armed");
    }
    #[test]
    fn stale_completion_does_not_change_new_request() {
        let mut s = ResearchStatus::default();
        s.arm("new".into(), "Song".into()).unwrap();
        s.finish(s.id + 1, "error", "Interrupted");
        assert_eq!(s.phase, "armed");
        assert_eq!(s.key, "new");
    }
}
