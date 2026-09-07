//! Saved-sync management. Soft deletion is reversible; generations reject stale jobs.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader},
    path::Path,
    sync::{Mutex, MutexGuard},
};
use tauri::{AppHandle, Emitter, Manager};
static GATE: Mutex<()> = Mutex::new(());
pub(crate) fn lock() -> MutexGuard<'static, ()> {
    GATE.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
#[derive(Default, Clone, Serialize, Deserialize)]
struct Policy {
    revision: u64,
    deleted: bool,
    refresh: bool,
}
type Policies = BTreeMap<String, Policy>;
fn read(root: &Path) -> Result<Policies, String> {
    match std::fs::read(root.join("sync-library.json")) {
        Ok(b) => serde_json::from_slice(&b).map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Policies::new()),
        Err(e) => Err(e.to_string()),
    }
}
fn write(root: &Path, policies: &Policies) -> Result<(), String> {
    crate::settings::write_atomic(
        &root.join("sync-library.json"),
        &serde_json::to_vec(policies).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
pub(crate) fn generation(root: &Path, key: &str) -> Result<u64, String> {
    let _g = lock();
    Ok(read(root)?.get(key).map_or(0, |p| p.revision))
}
pub(crate) fn flags(root: &Path, key: &str) -> (bool, bool) {
    let _g = lock();
    match read(root) {
        Ok(p) => p
            .get(key)
            .map_or((false, false), |p| (p.deleted, p.refresh)),
        Err(_) => (true, true),
    }
}
pub(crate) fn current_locked(root: &Path, key: &str, generation: u64) -> bool {
    read(root).is_ok_and(|p| p.get(key).map_or(0, |p| p.revision) == generation)
}
pub(crate) fn complete_locked(root: &Path, key: &str) -> Result<(), String> {
    let mut p = read(root)?;
    if let Some(v) = p.get_mut(key) {
        v.deleted = false;
        v.refresh = false;
        write(root, &p)?;
    }
    Ok(())
}
fn mutate(root: &Path, key: &str, action: &str, revision: Option<u64>) -> Result<u64, String> {
    if key.is_empty() || key.len() > 16 || !key.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid song key".into());
    }
    let _g = lock();
    let mut all = read(root)?;
    let p = all.entry(key.into()).or_default();
    match action {
        "refresh" => {
            p.refresh = true;
            p.deleted = false;
        }
        "delete" => {
            p.deleted = true;
            p.refresh = false;
        }
        "undo" => {
            if revision != Some(p.revision) || !p.deleted {
                return Err("This deletion can no longer be undone".into());
            }
            p.deleted = false;
        }
        _ => return Err("Unknown sync action".into()),
    }
    p.revision = p.revision.checked_add(1).ok_or("Revision exhausted")?;
    let revision = p.revision;
    write(root, &all)?;
    Ok(revision)
}
#[derive(Serialize)]
pub struct SavedSync {
    key: String,
    title: String,
    artist: String,
    thumb_key: Option<String>,
    saved_at: u64,
    refresh: bool,
}
fn list(root: &Path) -> Result<Vec<SavedSync>, String> {
    let _g = lock();
    let policies = read(root)?;
    let mut identities = BTreeMap::new();
    if let Ok(file) = std::fs::File::open(root.join("history.jsonl")) {
        for line in BufReader::new(file).lines().map_while(Result::ok) {
            if let Ok(h) = serde_json::from_str::<crate::history::HistoryEntry>(&line) {
                let key = crate::lyrics::key_for_ms(&h.artist, &h.title, &h.album, h.duration_ms);
                identities.insert(key, (h.title, h.artist, h.key));
            }
        }
    }
    let mut found: BTreeMap<String, u64> = BTreeMap::new();
    for dir in [
        "karaoke",
        crate::vocal_preview::OLDER_CACHE_DIR,
        crate::vocal_preview::PREVIOUS_CACHE_DIR,
        crate::vocal_preview::CACHE_DIR,
    ] {
        let Ok(files) = std::fs::read_dir(root.join(dir)) else {
            continue;
        };
        for f in files.flatten() {
            let path = f.path();
            if path.extension().is_none_or(|e| e != "json") {
                continue;
            }
            let Some(key) = path.file_stem().and_then(|s| s.to_str()).map(str::to_owned) else {
                continue;
            };
            if policies.get(&key).is_some_and(|p| p.deleted) {
                continue;
            }
            let source = std::fs::read(root.join("lyrics").join(format!("{key}.json")))
                .ok()
                .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
            let synced = source.as_ref().and_then(|v| v["synced"].as_str());
            if crate::karaoke::read_file_preserving(&path, synced).is_empty() {
                continue;
            }
            let modified = f
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |t| t.as_secs());
            found
                .entry(key)
                .and_modify(|t| *t = (*t).max(modified))
                .or_insert(modified);
        }
    }
    let mut rows: Vec<_> = found
        .into_iter()
        .map(|(key, saved_at)| {
            let (title, artist, thumb_key) = identities.remove(&key).map_or(
                ("Saved song".into(), "History unavailable".into(), None),
                |(t, a, k)| (t, a, Some(k)),
            );
            let refresh = policies.get(&key).is_some_and(|p| p.refresh);
            SavedSync {
                key,
                title,
                artist,
                thumb_key,
                saved_at,
                refresh,
            }
        })
        .collect();
    rows.sort_by(|a, b| b.saved_at.cmp(&a.saved_at).then(a.key.cmp(&b.key)));
    Ok(rows)
}
#[tauri::command]
pub async fn saved_syncs(app: AppHandle) -> Result<Vec<SavedSync>, String> {
    let root = app.path().app_data_dir().map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || list(&root))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn saved_sync_action(
    app: AppHandle,
    key: String,
    action: String,
    revision: Option<u64>,
) -> Result<u64, String> {
    let root = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let changed_key = key.clone();
    let result =
        tauri::async_runtime::spawn_blocking(move || mutate(&root, &key, &action, revision))
            .await
            .map_err(|e| e.to_string())??;
    crate::karaoke::library_action_applied(&changed_key);
    let _ = app.emit("sync-library-changed", ());
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn list_validates_without_pruning_and_delete_is_reversible() {
        let root =
            std::env::temp_dir().join(format!("palette-library-list-{}", std::process::id()));
        let dir = root.join(crate::vocal_preview::CACHE_DIR);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(root.join("lyrics")).unwrap();
        let key = "abc";
        let source = "[00:01.00]hello";
        std::fs::write(
            root.join("lyrics/abc.json"),
            serde_json::to_vec(&serde_json::json!({"synced":source})).unwrap(),
        )
        .unwrap();
        let cache = serde_json::json!({"v":5,"detail_v":2,"recipe":crate::vocal_preview::RECIPE,"synced":source,"words":[{"t":1000,"end":2000,"text":"hello"}]});
        let bytes = serde_json::to_vec(&cache).unwrap();
        std::fs::write(dir.join("abc.json"), &bytes).unwrap();
        assert_eq!(list(&root).unwrap().len(), 1);
        let deleted = mutate(&root, key, "delete", None).unwrap();
        assert!(list(&root).unwrap().is_empty());
        mutate(&root, key, "undo", Some(deleted)).unwrap();
        assert_eq!(list(&root).unwrap().len(), 1);
        mutate(&root, key, "refresh", None).unwrap();
        assert!(list(&root).unwrap()[0].refresh);
        assert_eq!(std::fs::read(dir.join("abc.json")).unwrap(), bytes);
        std::fs::write(root.join("lyrics/abc.json"), br#"{"synced":"changed"}"#).unwrap();
        assert!(list(&root).unwrap().is_empty());
        assert_eq!(std::fs::read(dir.join("abc.json")).unwrap(), bytes);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn delete_undo_and_refresh_reject_old_jobs() {
        let root = std::env::temp_dir().join(format!("palette-library-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let key = "0123456789abcdef";
        let before = generation(&root, key).unwrap();
        let deleted = mutate(&root, key, "delete", None).unwrap();
        assert_eq!(flags(&root, key), (true, false));
        {
            let _g = lock();
            assert!(!current_locked(&root, key, before));
        }
        mutate(&root, key, "undo", Some(deleted)).unwrap();
        assert_eq!(flags(&root, key), (false, false));
        assert!(mutate(&root, key, "undo", Some(deleted)).is_err());
        let revision = mutate(&root, key, "refresh", None).unwrap();
        assert_eq!(flags(&root, key), (false, true));
        {
            let _g = lock();
            assert!(current_locked(&root, key, revision));
            complete_locked(&root, key).unwrap();
        }
        assert_eq!(flags(&root, key), (false, false));
        assert!(mutate(&root, "../bad", "delete", None).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
