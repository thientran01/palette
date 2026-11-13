//! Saved-sync management. Soft deletion is reversible; generations reject stale jobs.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
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
/// Deleted in the library (soft deletion keeps the file). Hold `lock()`.
pub(crate) fn deleted_locked(root: &Path, key: &str) -> bool {
    read(root).map_or(true, |p| p.get(key).is_some_and(|p| p.deleted))
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
    /// The next full listen relearns it (saved under an earlier recipe,
    /// before the review's support was kept, or with an unsure line and
    /// listens to spare). Its timing still shows.
    relearns: bool,
    /// What the review found, for syncs saved with its support.
    check: Option<Check>,
}

/// How sure the review is of one saved sync (`Word::support`).
#[derive(Serialize, Debug, PartialEq, Clone)]
pub struct Check {
    /// Estimated share of words that land within 300ms of the singing.
    on_time: f32,
    /// One letter per lyric line, in song order: `s` sure (every word at
    /// SURE or more), `l` loose, `u` unsure (a word under DOUBT, the line
    /// the review would doubt), `n` no word timing (line sync only), `f`
    /// fixed by the listener's hand (karaoke/fixes.rs).
    lines: String,
    /// Lines still unsure.
    unsure: u32,
    /// Lines the second ear heard again on separated vocals...
    reheard: u32,
    /// ...and those it left sure enough not to doubt.
    fixed: u32,
    /// Lines the listener fixed by hand.
    #[serde(skip_serializing_if = "is_zero")]
    hand: u32,
}
fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// `acoustic::REVIEW_BELOW`: a word under it makes its line doubted.
const DOUBT: f32 = 0.5;
const SURE: f32 = 0.9;
/// Log-odds that a word lands within 300ms, by support (support below,
/// log-odds), on a song whose first pass doubted no line.
const ON_TIME: [(f32, f32); 5] = [
    (0.1, 0.34),
    (0.3, 0.99),
    (0.5, 1.70),
    (0.7, 2.26),
    (SURE, 2.86),
];
const ON_TIME_SURE: f32 = 5.27;
/// What a hard song takes off those log-odds, per share of its words on
/// lines the first pass doubted. Support alone read high on hard songs:
/// where the model doubts many lines, its sure words are wrong more often
/// too (Embers: 81% estimated, 72% measured, 66% of its words on doubted
/// lines). The benchmark has nothing harder than that to fit on, so the
/// share stops counting at HARDEST.
const HARD_SONG: f32 = 2.59;
const HARDEST: f32 = 0.6;
/// What a late line takes off, per second its first word starts after
/// the stamp, beyond LATE_FROM_MS past the song's usual start (the median
/// line: lyric files lead or trail the vocal by a song-wide offset). A
/// path that finds a line late has often found something else: words
/// after the first were on time 97% of the time on lines that started
/// where the song's lines usually do, 77% on lines 300-700ms late, 60%
/// on lines 0.7-1.5s late, and 1 in 19 past that.
const LATE_LINE: f32 = 3.29;
const LATE_FROM_MS: f32 = 300.0;
const LATEST_S: f32 = 2.0;

/// The chance a word with this support is within 300ms of the singing, on
/// a song with this share of its words on doubted lines, in a line that
/// starts `late_s` (`Song::late_s`) late. Fitted on JamendoLyrics (79
/// songs, 21,580 words) aligned three ways: as the app does, without the
/// second ear, and with every stamp 200ms early
/// (docs/specs/2026-09-06-saved-syncs.md).
fn on_time(support: f32, doubted: f32, late_s: f32) -> f32 {
    let odds = ON_TIME
        .iter()
        .find(|(below, _)| support < *below)
        .map_or(ON_TIME_SURE, |&(_, odds)| odds);
    let off = HARD_SONG * doubted.min(HARDEST) + LATE_LINE * late_s.min(LATEST_S);
    1.0 / (1.0 + (off - odds).exp())
}

/// What the rest of a song says about each of its checked words.
struct Song {
    /// Share of the checked words on lines the first pass doubted: a line
    /// the second ear heard again, or one that still holds a word under
    /// DOUBT (no second ear, or its budget ran out).
    doubted: f32,
    /// Each line's first checked word, in ms after its stamp.
    entries: std::collections::HashMap<i64, i64>,
    /// The median of `entries`.
    usual_entry: f32,
}

impl Song {
    fn new(words: &[crate::align::Word]) -> Self {
        // Per line: checked words, and whether the first pass doubted it.
        let mut lines: std::collections::HashMap<Option<i64>, (u32, bool)> = Default::default();
        let mut entries = std::collections::HashMap::new();
        for w in words {
            let (checked, doubted) = lines.entry(w.line_t).or_default();
            *checked += u32::from(w.support.is_some());
            *doubted |= w.reheard || w.support.is_some_and(|p| p < DOUBT);
            if let (Some(line_t), Some(_)) = (w.line_t, w.support) {
                entries.entry(line_t).or_insert(w.t - line_t);
            }
        }
        let (mut all, mut hard) = (0, 0);
        for (checked, doubted) in lines.into_values() {
            all += checked;
            hard += if doubted { checked } else { 0 };
        }
        let mut sorted: Vec<i64> = entries.values().copied().collect();
        sorted.sort_unstable();
        let usual_entry = match sorted.len() {
            0 => 0.0,
            n => (sorted[(n - 1) / 2] + sorted[n / 2]) as f32 / 2.0,
        };
        Self {
            doubted: hard as f32 / all.max(1) as f32,
            entries,
            usual_entry,
        }
    }

    /// Seconds the word's line starts later than the song's lines usually
    /// do, past the slack every line gets.
    fn late_s(&self, word: &crate::align::Word) -> f32 {
        let entry = word.line_t.and_then(|t| self.entries.get(&t));
        entry.map_or(0.0, |&ms| {
            ((ms as f32 - self.usual_entry - LATE_FROM_MS) / 1000.0).max(0.0)
        })
    }
}

/// None when no word carries support: saved before the review kept it.
fn check(lines: &[crate::align::TimedLine], words: &[crate::align::Word]) -> Option<Check> {
    let song = Song::new(words);
    let shares: Vec<f32> = words
        .iter()
        .filter_map(|w| Some(on_time(w.support?, song.doubted, song.late_s(w))))
        .collect();
    if shares.is_empty() {
        return None;
    }
    let on_time = shares.iter().sum::<f32>() / shares.len() as f32;
    let (mut unsure, mut reheard, mut fixed, mut hand) = (0, 0, 0, 0);
    let mut seen = std::collections::HashSet::new();
    let map = lines
        .iter()
        // Blank stamps mark instrumental breaks; a repeated stamp is one row.
        .filter(|l| !l.text.trim().is_empty() && seen.insert(l.t))
        .map(|line| {
            let own = words.iter().filter(|w| w.line_t == Some(line.t));
            let (mut any, mut heard, mut least, mut by_hand) = (false, false, None::<f32>, false);
            for w in own {
                any = true;
                heard |= w.reheard;
                by_hand |= w.fixed.is_some();
                if let Some(p) = w.support {
                    least = Some(least.map_or(p, |l| l.min(p)));
                }
            }
            let state = match least {
                _ if by_hand => 'f',
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
            hand += u32::from(by_hand);
            state
        })
        .collect();
    Some(Check {
        on_time: (on_time * 1000.0).round() / 1000.0,
        lines: map,
        unsure,
        reheard,
        fixed,
        hand,
    })
}
/// A file's size and modified time: what tells a memo it went stale.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Stamp {
    len: u64,
    modified: Option<std::time::SystemTime>,
}
impl Stamp {
    fn of(meta: &std::fs::Metadata) -> Self {
        Self {
            len: meta.len(),
            modified: meta.modified().ok(),
        }
    }
    fn at(path: &Path) -> Option<Self> {
        std::fs::metadata(path).ok().map(|m| Self::of(&m))
    }
}

/// One saved file's row, kept while neither it nor its lyrics change.
struct Summary {
    saved: Stamp,
    source: Option<Stamp>,
    /// The song's hand fixes (karaoke/fixes.rs).
    fixes: Option<Stamp>,
    assets: bool,
    /// None when the file isn't a usable sync (`read_saved` refused it).
    row: Option<(Option<Check>, bool)>,
}

/// Identity (title, artist, thumb key) per lyrics key, read from the
/// append-only history log. Only the bytes past `end` are read again.
#[derive(Default)]
struct Identities {
    path: std::path::PathBuf,
    /// The log's first bytes when last read. `clear_history` deletes the log
    /// and new plays restart it, so a changed head means read it all again.
    head: Vec<u8>,
    /// Just past the last whole line read.
    end: u64,
    by_key: std::collections::HashMap<String, (String, String, String)>,
}
/// Past the first line's start time, which a cleared log never repeats.
const HEAD_BYTES: usize = 512;
impl Identities {
    fn update(&mut self, path: &Path) {
        use std::io::{Read, Seek, SeekFrom};
        let Ok(mut file) = std::fs::File::open(path) else {
            *self = Self::default();
            self.path = path.to_owned();
            return;
        };
        let len = file.metadata().map_or(0, |m| m.len());
        let mut head = Vec::with_capacity(HEAD_BYTES);
        if (&mut file)
            .take(HEAD_BYTES as u64)
            .read_to_end(&mut head)
            .is_err()
        {
            return;
        }
        if self.path != path || len < self.end || !head.starts_with(&self.head) {
            *self = Self::default();
            self.path = path.to_owned();
        }
        self.head = head;
        let mut tail = Vec::new();
        if file.seek(SeekFrom::Start(self.end)).is_err() || file.read_to_end(&mut tail).is_err() {
            return;
        }
        // A line still being appended has no newline yet: leave it for later.
        let Some(last) = tail.iter().rposition(|&b| b == b'\n') else {
            return;
        };
        for line in tail[..=last].split(|&b| b == b'\n') {
            if let Ok(h) = serde_json::from_slice::<crate::history::HistoryEntry>(line) {
                let key = crate::lyrics::key_for_ms(&h.artist, &h.title, &h.album, h.duration_ms);
                self.by_key.insert(key, (h.title, h.artist, h.key));
            }
        }
        self.end += last as u64 + 1;
    }
}

/// The panel lists every saved sync, and building a row means parsing its
/// word file, its lyrics and the whole play history: tens of megabytes of
/// JSON on a long library, which made every open of the panel wait. Rows are
/// kept here between calls and rebuilt only for files that changed.
#[derive(Default)]
struct Memo {
    files: std::collections::HashMap<std::path::PathBuf, Summary>,
    identities: Identities,
}
static MEMO: std::sync::LazyLock<Mutex<Memo>> = std::sync::LazyLock::new(Default::default);

fn list(root: &Path) -> Result<Vec<SavedSync>, String> {
    // The gate guards the policy file only. A sync saved while this reads
    // emits karaoke-ready, and the panel reads again.
    let policies = {
        let _g = lock();
        read(root)?
    };
    let mut memo = MEMO
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let memo = &mut *memo;
    memo.identities.update(&root.join("history.jsonl"));
    let assets = crate::acoustic::assets().is_some();
    let mut seen = std::collections::HashSet::new();
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
            seen.insert(path.clone());
            if policies.get(&key).is_some_and(|p| p.deleted) {
                continue;
            }
            let Ok(meta) = f.metadata() else {
                continue;
            };
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |t| t.as_secs());
            if found.get(&key).is_some_and(|(t, ..)| *t >= modified) {
                continue;
            }
            let source_path = root.join("lyrics").join(format!("{key}.json"));
            let (saved, source) = (Stamp::of(&meta), Stamp::at(&source_path));
            let fixes = Stamp::at(&crate::karaoke::fixes::path(root, &key));
            let fresh = memo.files.get(&path).is_some_and(|s| {
                s.saved == saved && s.source == source && s.fixes == fixes && s.assets == assets
            });
            if !fresh {
                let row = summarize(&path, &source_path);
                memo.files.insert(
                    path.clone(),
                    Summary {
                        saved,
                        source,
                        fixes,
                        assets,
                        row,
                    },
                );
            }
            let Some((check, relearns)) = memo.files.get(&path).and_then(|s| s.row.clone()) else {
                continue;
            };
            found.insert(key, (modified, check, relearns));
        }
    }
    memo.files.retain(|path, _| seen.contains(path));
    let mut rows: Vec<_> = found
        .into_iter()
        .map(|(key, (saved_at, check, relearns))| {
            let (title, artist, thumb_key) = memo.identities.by_key.get(&key).map_or(
                ("Saved song".into(), "History unavailable".into(), None),
                |(t, a, k)| (t.clone(), a.clone(), Some(k.clone())),
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

/// The song key a saved file is named by.
fn key_of(path: &Path) -> Option<&str> {
    path.file_stem().and_then(|s| s.to_str())
}

/// A saved file's check and relearn flag, or None when it isn't usable.
fn summarize(path: &Path, source_path: &Path) -> Option<(Option<Check>, bool)> {
    let source = std::fs::read(source_path)
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
    let synced = source.as_ref().and_then(|v| v["synced"].as_str());
    let saved = crate::karaoke::read_saved(path, synced)?;
    let lines = crate::align::parse_lrc(synced.unwrap_or_default());
    // Lines the listener fixed by hand show as fixed, whatever the aligner
    // saved under them.
    let words = match (path.parent().and_then(Path::parent), key_of(path)) {
        (Some(root), Some(key)) => crate::karaoke::fixes::apply(root, key, synced, saved.words),
        _ => saved.words,
    };
    Some((check(&lines, &words), saved.relearns))
}
/// Seconds after launch to build the memo, clear of startup work.
const WARM_AFTER_S: u64 = 20;
/// Builds the memo in the background so the first open is quick as well.
pub fn warm(app: &AppHandle) {
    let Ok(root) = app.path().app_data_dir() else {
        return;
    };
    let _ = std::thread::Builder::new()
        .name("sync-library-warm".into())
        .spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(WARM_AFTER_S));
            let _ = list(&root);
        });
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
        // Two of the six checked words sit on lines the first pass doubted
        // (the reheard line and the still unsure one), and every line
        // starts on its stamp. With that share: 4 sure words, one at 0.6,
        // one at 0.2.
        let at = |support| on_time(support, 1.0 / 3.0, 0.0);
        let expected = (4.0 * at(0.95) + at(0.6) + at(0.2)) / 6.0;
        assert!((c.on_time - expected).abs() < 0.001, "{}", c.on_time);
        // Saved before the review kept support: nothing to judge.
        assert_eq!(check(&lines, &[word(1000, None, false)]), None);
    }
    #[test]
    fn a_hard_song_lowers_every_word() {
        // Sure words on a song the first pass never doubted: nearly all on time.
        assert!(on_time(1.0, 0.0, 0.0) > 0.99);
        let supports = [0.05, 0.2, 0.4, 0.6, 0.8, 0.95];
        // The same support counts for less where many lines were doubted...
        for support in supports {
            assert!(on_time(support, 0.5, 0.0) < on_time(support, 0.1, 0.0));
        }
        // ...surer words still score higher on the same song...
        for doubted in [0.0, 0.3, 0.6] {
            let shares = supports.map(|p| on_time(p, doubted, 0.0));
            assert!(shares.windows(2).all(|w| w[0] < w[1]), "{shares:?}");
        }
        // ...and past the hardest benchmark songs the share stops counting.
        assert_eq!(on_time(0.95, HARDEST, 0.0), on_time(0.95, 1.0, 0.0));
        // A song of sure words still reads lower when its lines were doubted.
        let line = |t: i64| TimedLine {
            t,
            text: "la la".into(),
        };
        let lines = [line(1000), line(2000)];
        let easy = [word(1000, Some(0.95), false), word(2000, Some(0.95), false)];
        let hard = [word(1000, Some(0.95), true), word(2000, Some(0.95), true)];
        let (easy, hard) = (check(&lines, &easy).unwrap(), check(&lines, &hard).unwrap());
        assert!(hard.on_time < easy.on_time - 0.01);
    }
    #[test]
    fn the_song_counts_checked_words_on_doubted_lines() {
        let words = [
            word(1000, Some(0.97), false),
            word(1000, Some(0.95), false),
            // Still under DOUBT: its whole line counts.
            word(2000, Some(0.95), false),
            word(2000, Some(0.3), false),
            // Reheard and fixed: the first pass doubted it.
            word(3000, Some(0.92), true),
            // Unchecked words don't count either way.
            word(3000, None, true),
            word(4000, None, false),
        ];
        assert!((Song::new(&words).doubted - 3.0 / 5.0).abs() < 1e-6);
        assert_eq!(Song::new(&[word(1000, None, false)]).doubted, 0.0);
    }
    #[test]
    fn a_line_that_starts_late_for_its_song_counts_for_less() {
        // Sure words throughout; `entry` is how long after its stamp each
        // line's first word starts.
        let song = |entries: [i64; 5]| -> Vec<Word> {
            let lines = entries.iter().enumerate();
            lines
                .flat_map(|(i, &entry)| {
                    let line_t = 10_000 * (i as i64 + 1);
                    [entry, entry + 400].map(|after| Word {
                        t: line_t + after,
                        ..word(line_t, Some(0.97), false)
                    })
                })
                .collect()
        };
        let score = |words: &[Word]| {
            let song = Song::new(words);
            let shares = words.iter().map(|w| on_time(0.97, 0.0, song.late_s(w)));
            shares.sum::<f32>() / words.len() as f32
        };
        let on_stamp = score(&song([0, 40, -20, 60, 20]));
        assert!(on_stamp > 0.99);
        // A lyric file that leads the vocal moves every line alike: no line
        // is late for its song.
        assert_eq!(score(&song([500, 540, 480, 560, 520])), on_stamp);
        // The slack every line gets.
        assert_eq!(score(&song([0, 40, -20, 60, 300])), on_stamp);
        // One line of five found 1.8 seconds late: its words are probably
        // off, so the song loses part of that line's share.
        let late = score(&song([0, 40, -20, 60, 1800]));
        assert!(late < on_stamp - 0.05 && late > 0.85, "{late}");
        // Later than LATEST_S past the slack counts no further.
        let words = song([0, 40, -20, 60, 9000]);
        let last = Song::new(&words).late_s(&words[9]);
        assert_eq!(on_time(0.97, 0.0, last), on_time(0.97, 0.0, LATEST_S));
        // A line's first checked word sets its start, not one the review
        // never judged.
        let mut words = song([0, 40, -20, 60, 20]);
        words.insert(0, word(10_000, None, false));
        words[0].t = 15_000;
        assert_eq!(Song::new(&words).late_s(&words[1]), 0.0);
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
    fn identities_read_only_new_lines_and_restart_after_a_clear() {
        let root = std::env::temp_dir().join(format!("palette-identities-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let log = root.join("history.jsonl");
        let line = |title: &str, started: i64| {
            let mut v = serde_json::json!({"v":2,"key":format!("thumb-{title}"),"app_id":"a","player":"spotify","title":title,"artist":"Artist","album":"","started_at_ms":started,"ended_at_ms":0,"ms_listened":1,"duration_ms":180_000}).to_string();
            v.push('\n');
            v
        };
        let key = |title: &str| crate::lyrics::key_for_ms("Artist", title, "", 180_000);
        let mut ids = Identities::default();
        std::fs::write(&log, line("One", 1)).unwrap();
        ids.update(&log);
        assert_eq!(ids.by_key.len(), 1);
        // A torn append waits for its newline.
        let mut grown = line("One", 1) + &line("Two", 2);
        grown.push_str(r#"{"v":2,"key":"half"#);
        std::fs::write(&log, &grown).unwrap();
        ids.update(&log);
        assert_eq!(ids.by_key[&key("Two")].2, "thumb-Two");
        let end = ids.end;
        assert_eq!(end as usize, line("One", 1).len() + line("Two", 2).len());
        // Unchanged log: nothing more is read.
        ids.update(&log);
        assert_eq!(ids.end, end);
        // Cleared, then new plays grew it past the old end: read from the top.
        std::fs::write(
            &log,
            line("Three", 9) + &line("Four", 10) + &line("Five", 11),
        )
        .unwrap();
        ids.update(&log);
        assert!(!ids.by_key.contains_key(&key("One")));
        assert_eq!(ids.by_key.len(), 3);
        std::fs::remove_file(&log).unwrap();
        ids.update(&log);
        assert!(ids.by_key.is_empty());
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
