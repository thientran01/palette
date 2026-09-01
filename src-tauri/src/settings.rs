/*
 * app_data/settings.json — the shared key-value settings file, read-modify-
 * write. Grew out of lib.rs's companion-only helpers: save_companion used to
 * write `{"companion": on}` WHOLESALE, which was fine while companion was the
 * only key and a silent clobber the moment a second one existed (the Last.fm
 * key is the second). All writers go through set_value now.
 *
 * Writes are serialized by a module mutex — two tray toggles can't interleave
 * their read-modify-write. The parsed root lives behind that same mutex:
 * first read populates the cache, later get_value calls serve it, set_value
 * merges then write_atomic's the file. A hand-edit to settings.json while
 * the app is running is not picked up by later reads. It never was reliable
 * (a live set_value already clobbered the file wholesale). Writers are rare
 * (tray clicks, hand edits while the app is closed), and a hand-edit during
 * a live write still loses politely (last writer wins whole-file).
 */
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::Value;
use tauri::{AppHandle, Manager};

/// In-memory settings.json object. `from_disk` runs at most once per
/// instance — the test seam that proves a set/get round-trip is cache-only.
struct Cache {
    root: Option<Value>,
}

impl Cache {
    const fn new() -> Self {
        Self { root: None }
    }

    fn load(&mut self, from_disk: impl FnOnce() -> Value) -> &mut Value {
        self.root.get_or_insert_with(from_disk)
    }

    fn get(&mut self, key: &str, from_disk: impl FnOnce() -> Value) -> Option<Value> {
        self.load(from_disk).get(key).cloned()
    }

    fn set(&mut self, key: &str, value: Value, from_disk: impl FnOnce() -> Value) {
        self.load(from_disk)
            .as_object_mut()
            .expect("cached root is always an object")
            .insert(key.to_string(), value);
    }
}

static GATE: Mutex<Cache> = Mutex::new(Cache::new());

fn lock() -> std::sync::MutexGuard<'static, Cache> {
    GATE.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|d| d.join("settings.json"))
}

fn read_disk(app: &AppHandle) -> Value {
    path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| Value::Object(Default::default()))
}

pub fn get_value(app: &AppHandle, key: &str) -> Option<Value> {
    lock().get(key, || read_disk(app))
}

pub fn get_string(app: &AppHandle, key: &str) -> Option<String> {
    get_value(app, key)?.as_str().map(str::to_string)
}

pub fn get_bool(app: &AppHandle, key: &str, default: bool) -> bool {
    get_value(app, key)
        .and_then(|v| v.as_bool())
        .unwrap_or(default)
}

pub fn set_value(app: &AppHandle, key: &str, value: Value) {
    let mut cache = lock();
    cache.set(key, value, || read_disk(app));
    let Some(p) = path(app) else { return };
    let serialized = cache
        .root
        .as_ref()
        .expect("set populates the root")
        .to_string();
    // In-session behavior rides the cache either way; a failed write
    // only surfaces at next launch — say so instead of diverging silently.
    if let Err(e) = write_atomic(&p, serialized.as_bytes()) {
        log::warn!("settings: {key} not persisted: {e}");
    }
}

/// Crash-safe file replace shared by every config writer (settings.json here,
/// upnext.json, spotify_tokens.json). Write to a sibling temp file in the SAME
/// directory, then `rename` it over the target: on NTFS a same-volume rename is
/// atomic (Rust's `fs::rename` maps to MoveFileExW with MOVEFILE_REPLACE_EXISTING
/// on Windows), so a crash mid-write leaves EITHER the old file or the fully
/// written new one — never the truncated/empty file that plain `fs::write`
/// produces when it opens the target with O_TRUNC before writing. Creates the
/// parent dir. Callers already serialize their own writes (module mutexes), so
/// the fixed per-target temp name never collides; a temp left by a prior crash
/// is simply overwritten on the next write.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "target path has no parent dir",
        )
    })?;
    std::fs::create_dir_all(dir)?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("pulse");
    let tmp = dir.join(format!(".{name}.tmp"));
    // fsync the temp BEFORE the rename. `fs::write` only lands the bytes in the
    // page cache; the rename is metadata-ordered but the DATA is not, so a power
    // loss after the rename can still commit a torn/empty file (the exact
    // failure this atomic-replace exists to prevent, just moved one step later).
    // sync_all() forces the bytes durable first, so the rename only ever exposes
    // a fully-written temp. The handle is dropped (closed) before the rename —
    // Windows won't MoveFileExW over/from an open handle.
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        // Don't leak the temp if the swap failed.
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// Shared hard cap for reading a JSON HTTP response body. ureq's `into_json`
/// has NO size limit, so a hostile or compromised endpoint could stream a
/// multi-GB body and OOM the parse; every JSON payload the app reads (lyrics,
/// Last.fm similars, Spotify player/token state) is a small document, so 2 MiB
/// clears them all with headroom.
pub(crate) const JSON_CAP: u64 = 2 * 1024 * 1024;

/// Capped replacement for `ureq::Response::into_json`: parse the body reading
/// at most `cap` bytes. A body that would exceed the cap fails to parse, which
/// every caller already treats as offline/None — the safe read for a payload
/// that large. Lives here beside `write_atomic` as the crate's shared
/// robust-I/O helpers.
pub(crate) fn json_capped<T: serde::de::DeserializeOwned>(
    resp: ureq::Response,
    cap: u64,
) -> serde_json::Result<T> {
    serde_json::from_reader(resp.into_reader().take(cap))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::Cell;

    #[test]
    fn set_then_get_does_not_reload() {
        let mut cache = Cache::new();
        let loads = Cell::new(0);
        cache.set("a", json!(1), || {
            loads.set(loads.get() + 1);
            json!({})
        });
        assert_eq!(loads.get(), 1);
        let got = cache.get("a", || {
            loads.set(loads.get() + 1);
            panic!("disk re-read");
        });
        assert_eq!(got, Some(json!(1)));
        assert_eq!(loads.get(), 1);
    }

    #[test]
    fn merge_keeps_both_keys() {
        let mut cache = Cache::new();
        let loads = Cell::new(0);
        cache.set("a", json!(1), || {
            loads.set(loads.get() + 1);
            json!({})
        });
        cache.set("b", json!(2), || {
            loads.set(loads.get() + 1);
            json!({"a": 0})
        });
        assert_eq!(
            loads.get(),
            1,
            "second set must merge the cache, not reload"
        );
        assert_eq!(cache.get("a", || panic!("disk re-read")), Some(json!(1)));
        assert_eq!(cache.get("b", || panic!("disk re-read")), Some(json!(2)));
    }
}
