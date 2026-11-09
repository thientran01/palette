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
    /// The next full listen relearns it (saved under an earlier recipe or
    /// before the review's support was kept). Its timing still shows.
    relearns: bool,
    /// What the review found, for syncs saved with its support.
    check: Option<Check>,
}

/// How sure the review is of one saved sync (`Word::support`).
#[derive(Serialize, Debug, PartialEq)]
pub struct Check {
    /// Estimated share of words that land within 300ms of the singing.
    on_time: f32,
    /// One letter per lyric line, in song order: `s` sure (every word at
    /// SURE or more), `l` loose, `u` unsure (a word under DOUBT, the line
    /// the review would doubt), `n` no word timing (line sync only).
    lines: String,
    /// Lines still unsure.
    unsure: u32,
    /// Lines the second ear heard again on separated vocals...
    reheard: u32,
    /// ...and those it left sure enough not to doubt.
    fixed: u32,
}

/// `acoustic::REVIEW_BELOW`: a word under it makes its line doubted.
const DOUBT: f32 = 0.5;
const SURE: f32 = 0.9;
/// Share of words within 300ms by support: (support below, share on time).
/// Measured on JamendoLyrics (79 songs, 21,580 words) with the second ear,
/// as the app aligns. Against each song's measured share the estimate was
/// within 1.8 points on average (Pearson 0.91), and it ran a few points
/// high on the hardest songs (docs/specs/2026-09-06-saved-syncs.md).
const ON_TIME: [(f32, f32); 5] = [
    (0.1, 0.30),
    (0.3, 0.45),
    (0.5, 0.57),
    (0.7, 0.81),
    (SURE, 0.895),
];
const ON_TIME_SURE: f32 = 0.993;

fn on_time(support: f32) -> f32 {
    ON_TIME
        .iter()
        .find(|(below, _)| support < *below)
        .map_or(ON_TIME_SURE, |&(_, share)| share)
}

/// None when no word carries support: saved before the review kept it.
fn check(lines: &[crate::align::TimedLine], words: &[crate::align::Word]) -> Option<Check> {
    let supported: Vec<f32> = words.iter().filter_map(|w| w.support).collect();
    if supported.is_empty() {
        return None;
    }
    let on_time = supported.iter().map(|&p| on_time(p)).sum::<f32>() / supported.len() as f32;
    let (mut unsure, mut reheard, mut fixed) = (0, 0, 0);
    let mut seen = std::collections::HashSet::new();
    let map = lines
        .iter()
        // Blank stamps mark instrumental breaks; a repeated stamp is one row.
        .filter(|l| !l.text.trim().is_empty() && seen.insert(l.t))
        .map(|line| {
            let own = words.iter().filter(|w| w.line_t == Some(line.t));
            let (mut any, mut heard, mut least) = (false, false, None::<f32>);
            for w in own {
                any = true;
                heard |= w.reheard;
                if let Some(p) = w.support {
                    least = Some(least.map_or(p, |l| l.min(p)));
                }
            }
            let state = match least {
                None if !any => 'n',
                // Words the review couldn't judge: not sure, not doubted.
                None => 'l',
                Some(p) if p < DOUBT => 'u',
                Some(p) if p < SURE => 'l',
                Some(_) => 's',
            };
            unsure += u32::from(state == 'u');
            reheard += u32::from(heard);
            fixed += u32::from(heard && state != 'u');
            state
        })
        .collect();
    Some(Check {
        on_time: (on_time * 1000.0).round() / 1000.0,
        lines: map,
        unsure,
        reheard,
        fixed,
    })
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
    // key -> (newest save, the newest file's check, relearns)
    let mut found: BTreeMap<String, (u64, Option<Check>, bool)> = BTreeMap::new();
    for dir in std::iter::once("karaoke")
        .chain(crate::vocal_preview::EARLIER_CACHE_DIRS)
        .chain([crate::vocal_preview::CACHE_DIR])
    {
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
            let Some(saved) = crate::karaoke::read_saved(&path, synced) else {
                continue;
            };
            let modified = f
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |t| t.as_secs());
            if found.get(&key).is_some_and(|(t, ..)| *t >= modified) {
                continue;
            }
            let lines = crate::align::parse_lrc(synced.unwrap_or_default());
            found.insert(key, (modified, check(&lines, &saved.words), saved.relearns));
        }
    }
    let mut rows: Vec<_> = found
        .into_iter()
        .map(|(key, (saved_at, check, relearns))| {
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
                relearns,
                check,
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
    use crate::align::{TimedLine, Word};
    fn word(line_t: i64, support: Option<f32>, reheard: bool) -> Word {
        Word {
            t: line_t,
            text: "la".into(),
            line_t: Some(line_t),
            support,
            reheard,
            ..Default::default()
        }
    }
    #[test]
    fn check_maps_lines_and_estimates_words_on_time() {
        let line = |t: i64, text: &str| TimedLine {
            t,
            text: text.into(),
        };
        let lines = [
            line(1000, "sure line"),
            line(2000, "loose line"),
            line(3000, ""),
            line(4000, "fixed on the second listen"),
            line(5000, "still unsure"),
            line(6000, "line timing only"),
        ];
        let words = [
            word(1000, Some(0.97), false),
            word(1000, Some(0.95), false),
            word(2000, Some(0.95), false),
            word(2000, Some(0.6), false),
            word(4000, Some(0.92), true),
            word(5000, Some(0.2), true),
            word(5000, None, false),
        ];
        let c = check(&lines, &words).unwrap();
        assert_eq!(c.lines, "slsun");
        assert_eq!((c.unsure, c.reheard, c.fixed), (1, 2, 1));
        // 4 sure words, one at 0.6 (81%), one at 0.2 (45%).
        let expected = (4.0 * 0.993 + 0.81 + 0.45) / 6.0;
        assert!((c.on_time - expected).abs() < 0.001, "{}", c.on_time);
        // Saved before the review kept support: nothing to judge.
        assert_eq!(check(&lines, &[word(1000, None, false)]), None);
    }
    #[test]
    fn support_round_trips_and_stays_out_of_files_without_it() {
        let plain = serde_json::to_value(word(1000, None, false)).unwrap();
        assert!(plain.get("support").is_none() && plain.get("reheard").is_none());
        let checked = word(1000, Some(0.42), true);
        let back: Word = serde_json::from_value(serde_json::to_value(&checked).unwrap()).unwrap();
        assert_eq!(back, checked);
    }
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
