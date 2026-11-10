//! Saved syncs re-decoded from their takes (crate::takes) when the decoder
//! changes, instead of each song waiting for its next full listen.
//!
//! Every listen a saved sync is built from keeps its take beside it, in
//! karaoke-takes/<key>.ptk. After an update that bumps
//! `acoustic::RECIPE` but not `acoustic::EMISSIONS`, a background sweep
//! decodes each saved song's takes again with the new decoder, folds them
//! together the way the listens were (`merge_listens`), and saves the
//! result in place, keeping the song's listens and detail. It runs one
//! song at a time on the sync job slot, so a listen that finishes meanwhile
//! queues behind one short decode instead of racing it.
//!
//! A song can't be re-decoded, and relearns on its next full listen as
//! before, when one of its listens kept no take (saved before takes, or on
//! the vocal trial), when the new decoder looks at a window no take holds,
//! or when the model's input changed (`EMISSIONS`).
//!
//! docs/specs/2026-11-10-karaoke-saved-emissions.md
use super::*;
use crate::takes::{lyrics_id, Take, TakeFile};
use std::time::Duration;

const TAKES_DIR: &str = "karaoke-takes";
/// The sweep waits this long after launch, out of startup's way.
const START_DELAY: Duration = Duration::from_secs(90);
/// While a listen's sync holds the job slot, the sweep looks again this
/// often.
const BUSY_RETRY: Duration = Duration::from_secs(20);

fn take_path(root: &Path, key: &str) -> PathBuf {
    root.join(TAKES_DIR).join(format!("{key}.ptk"))
}

/// After a listen saved its sync: keep its take with the takes of the
/// listens the sync was merged from. `saved_listens` is the sync it merged
/// into (None when it started afresh) and `listens` what was saved. Only
/// the main store keeps takes. A listen without one (the vocal trial, the
/// legacy aligner, or a capture of the whole device's mix, whose emissions
/// could spell out voice chat) leaves its song to relearn by listening.
pub(super) fn keep(
    root: &Path,
    rec: &Rec,
    synced: &str,
    take: Option<Take>,
    saved_listens: Option<u32>,
    listens: u32,
    main_store: bool,
) {
    let path = take_path(root, &rec.key);
    let Some(take) = take.filter(|_| main_store && !rec.device_mix) else {
        let _ = std::fs::remove_file(&path);
        return;
    };
    let lyrics = lyrics_id(synced);
    let mut takes = match saved_listens {
        None => Vec::new(),
        Some(before) => match TakeFile::read(&path) {
            Some(file)
                if file.emissions == acoustic::EMISSIONS
                    && file.lyrics == lyrics
                    && file.listens == before =>
            {
                file.takes
            }
            // An earlier listen kept no take: what's left couldn't rebuild
            // the sync, so keep none until a listen starts it afresh.
            _ => {
                let _ = std::fs::remove_file(&path);
                return;
            }
        },
    };
    takes.push(take);
    let file = TakeFile {
        emissions: acoustic::EMISSIONS.to_string(),
        lyrics,
        listens,
        artist: rec.artist.clone(),
        title: rec.title.clone(),
        album: rec.album.clone(),
        duration_ms: rec.duration_ms,
        takes,
    };
    match file.write(&path) {
        Ok(()) => evict_takes(&root.join(TAKES_DIR)),
        Err(e) => log::warn!("karaoke: take not kept for {} ({e})", rec.title),
    }
}

/// A pruned sync's take goes with it.
pub(super) fn forget(sync: &Path) {
    let (Some(dir), Some(key)) = (sync.parent(), sync.file_stem().and_then(|k| k.to_str())) else {
        return;
    };
    if dir.file_name().is_some_and(|d| d == KARAOKE_DIR) {
        if let Some(root) = dir.parent() {
            let _ = std::fs::remove_file(take_path(root, key));
        }
    }
}

/// At most one take file per sync the store can hold.
fn evict_takes(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "ptk"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    if files.len() <= CACHE_MAX_FILES {
        return;
    }
    files.sort_by_key(|(t, _)| *t);
    let excess = files.len() - CACHE_MAX_FILES;
    for (_, path) in files.into_iter().take(excess) {
        let _ = std::fs::remove_file(path);
    }
}

/// Start the sweep in the background, once per launch.
pub(crate) fn start(app: &AppHandle) {
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("karaoke-redecode".into())
        .spawn(move || {
            std::thread::sleep(START_DELAY);
            sweep(&app);
        });
    if let Err(e) = spawned {
        log::warn!("karaoke: re-decode sweep not started ({e})");
    }
}

fn sweep(app: &AppHandle) {
    let Some(model_dir) = acoustic::assets() else {
        return;
    };
    let Ok(root) = app.path().app_data_dir() else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(root.join(TAKES_DIR)) else {
        return;
    };
    let mut keys: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            (path.extension()? == "ptk").then(|| path.file_stem()?.to_str().map(str::to_string))?
        })
        .collect();
    keys.sort();
    let dir = root.join(KARAOKE_DIR);
    let mut decoder = None;
    let (mut redone, mut failed) = (0, 0);
    for key in keys {
        if due(&root, &dir, &key).is_none() {
            continue;
        }
        let guard = loop {
            if let Some(guard) = claim() {
                break guard;
            }
            std::thread::sleep(BUSY_RETRY);
        };
        // Again on the slot: a listen may have relearned the song meanwhile.
        if let Some(job) = due(&root, &dir, &key) {
            let decoder =
                decoder.get_or_insert_with(|| acoustic::AcousticAligner::decoder_only(model_dir));
            match redecode(app, &root, &dir, decoder, job) {
                Ok(()) => redone += 1,
                Err(e) => {
                    failed += 1;
                    let _ = std::fs::remove_file(take_path(&root, &key));
                    log::info!("karaoke: {key} relearns by listening ({e})");
                }
            }
        }
        run_queue(app, guard);
    }
    if redone + failed > 0 {
        log::info!(
            "karaoke: re-decoded {redone} saved syncs under {}; {failed} wait for a listen",
            acoustic::RECIPE
        );
    }
}

/// The sync job slot (ALIGNING), when no listen holds it.
fn claim() -> Option<AlignGuard> {
    let _queue = lock_queue();
    (!ALIGNING.swap(true, Ordering::SeqCst)).then(AlignGuard::new)
}

struct Due {
    key: String,
    store: StoreFile,
    synced: String,
    file: TakeFile,
    generation: u64,
}

/// A saved sync an earlier recipe decoded, with a take for every listen it
/// was built from. A take that can never serve it again is removed.
fn due(root: &Path, dir: &Path, key: &str) -> Option<Due> {
    let path = take_path(root, key);
    let store = std::fs::read_to_string(dir.join(format!("{key}.json")))
        .ok()
        .and_then(|raw| serde_json::from_str::<StoreFile>(&raw).ok());
    let Some(store) = store else {
        // Its sync was evicted or dropped.
        let _ = std::fs::remove_file(&path);
        return None;
    };
    let synced = store.synced.clone()?;
    if store.v != STORE_V
        || !superseded(&store.recipe, recipe())
        || store.words.is_empty()
        || collapsed(&store.words)
    {
        return None;
    }
    let (deleted, refresh) = crate::sync_library::flags(root, key);
    if deleted || refresh {
        return None;
    }
    let generation = crate::sync_library::generation(root, key).ok()?;
    let file = TakeFile::read(&path)
        .filter(|f| {
            f.emissions == acoustic::EMISSIONS
                && f.lyrics == lyrics_id(&synced)
                && f.listens == store.listens
                && !f.takes.is_empty()
        })
        .or_else(|| {
            let _ = std::fs::remove_file(&path);
            None
        })?;
    Some(Due {
        key: key.to_string(),
        store,
        synced,
        file,
        generation,
    })
}

fn redecode(
    app: &AppHandle,
    root: &Path,
    dir: &Path,
    decoder: &mut acoustic::AcousticAligner,
    job: Due,
) -> std::result::Result<(), String> {
    let started = Instant::now();
    let lines = align::parse_lrc(&job.synced);
    let words = decode_listens(decoder, &job.file.takes, &lines)?;
    let mut words = words.ok_or("no takes")?;
    if line_coverage(&lines, &words) < MIN_LINE_COVERAGE || collapsed(&words) {
        return Err("the new decode found too little".into());
    }
    carry_voice(&job.store.words, &mut words);
    let moved = moved_words(&job.store.words, &words);
    {
        let _library = crate::sync_library::lock();
        if !crate::sync_library::current_locked(root, &job.key, job.generation) {
            return Err("changed in the library meanwhile".into());
        }
        write_store(
            dir,
            &job.key,
            &job.synced,
            &words,
            job.store.listens,
            job.store.detail_v,
        )
        .map_err(|e| e.to_string())?;
    }
    log::info!(
        "karaoke: re-decoded {} from {} saved listens in {:.2}s ({moved} of {} words moved)",
        job.file.title,
        job.file.takes.len(),
        started.elapsed().as_secs_f64(),
        words.len(),
    );
    let _ = app.emit(
        "karaoke-ready",
        KaraokeReady {
            artist: job.file.artist,
            title: job.file.title,
            album: job.file.album,
            duration_ms: job.file.duration_ms,
            words,
        },
    );
    Ok(())
}

/// Each take decoded again, oldest first, folded the way its listens were
/// saved.
fn decode_listens(
    decoder: &mut acoustic::AcousticAligner,
    takes: &[Take],
    lines: &[align::TimedLine],
) -> std::result::Result<Option<Vec<Word>>, String> {
    let mut words: Option<Vec<Word>> = None;
    for take in takes {
        let fresh = decoder.redecode(take.clone(), lines)?;
        words = Some(match words {
            None => fresh,
            Some(saved) => merge_listens(&saved, &fresh).0,
        });
    }
    Ok(words)
}

/// Words whose onset or end moved.
fn moved_words(old: &[Word], new: &[Word]) -> usize {
    if old.len() != new.len() {
        return new.len();
    }
    old.iter()
        .zip(new)
        .filter(|(a, b)| a.t != b.t || a.end != b.end)
        .count()
}

/// The singer's voice (voice.rs) needs audio a re-decode doesn't have, so
/// held words keep the track their listen measured, matched by line, place
/// in the line and text, and re-cut to the word's new onset and length:
/// the track is the voice at absolute song times, so a moved onset reads
/// the frames that are now under it, and frames outside what was measured
/// are unvoiced.
fn carry_voice(old: &[Word], new: &mut [Word]) {
    use crate::voice::{is_held, VoiceTrack, HOP_MS, UNVOICED};
    let mut tracks: HashMap<(i64, usize), &Word> = HashMap::new();
    let mut place: HashMap<i64, usize> = HashMap::new();
    for w in old {
        let Some(line_t) = w.line_t else { continue };
        let i = place.entry(line_t).or_default();
        if w.voice.is_some() {
            tracks.insert((line_t, *i), w);
        }
        *i += 1;
    }
    place.clear();
    for w in new.iter_mut() {
        let Some(line_t) = w.line_t else { continue };
        let i = place.entry(line_t).or_default();
        let at = *i;
        *i += 1;
        let Some(before) = tracks.get(&(line_t, at)).filter(|b| b.text == w.text) else {
            continue;
        };
        let Some(track) = before.voice.as_ref() else {
            continue;
        };
        if !is_held(w) {
            continue;
        }
        let frames = (w.end.unwrap_or(w.t) + crate::voice::TAIL_MS - w.t) / HOP_MS;
        let shift = (w.t - before.t) as f64 / HOP_MS as f64;
        let mut c = Vec::with_capacity(frames.max(1) as usize);
        let mut l = Vec::with_capacity(frames.max(1) as usize);
        for k in 0..frames.max(1) {
            let j = (k as f64 + shift).round();
            let j = (j >= 0.0).then_some(j as usize);
            c.push(j.and_then(|j| track.c.get(j).copied()).unwrap_or(UNVOICED));
            l.push(j.and_then(|j| track.l.get(j).copied()).unwrap_or(0));
        }
        if c.iter().any(|&x| x != UNVOICED) {
            w.voice = Some(VoiceTrack { c, l });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voice::{VoiceTrack, UNVOICED};

    fn word(line_t: i64, text: &str, t: i64, end: i64) -> Word {
        Word {
            text: text.into(),
            t,
            end: Some(end),
            line_t: Some(line_t),
            ..Word::default()
        }
    }

    #[test]
    fn held_words_keep_their_voice_at_the_same_song_times() {
        let mut held = word(1_000, "love", 1_000, 1_600);
        held.voice = Some(VoiceTrack {
            c: (0..72).map(|k| k as i16).collect(),
            l: vec![50; 72],
        });
        let old = vec![word(1_000, "my", 800, 1_000), held];
        // The onset moved 30 ms later and the note got shorter.
        let mut new = vec![
            word(1_000, "my", 800, 1_030),
            word(1_000, "love", 1_030, 1_530),
        ];
        carry_voice(&old, &mut new);
        let voice = new[1].voice.as_ref().unwrap();
        assert_eq!(voice.c.len(), (500 + crate::voice::TAIL_MS) as usize / 10);
        assert_eq!(voice.c[0], 3, "frame 0 is what was measured 30 ms in");
        assert_eq!(*voice.c.last().unwrap(), 64);
        assert!(new[0].voice.is_none());
        // Moved earlier: the frames before what was measured are unvoiced.
        let mut earlier = vec![word(1_000, "my", 800, 950), word(1_000, "love", 950, 1_600)];
        carry_voice(&old, &mut earlier);
        let voice = earlier[1].voice.as_ref().unwrap();
        assert_eq!(
            &voice.c[..6],
            &[UNVOICED, UNVOICED, UNVOICED, UNVOICED, UNVOICED, 0]
        );
        assert_eq!(voice.l[0], 0);
    }

    #[test]
    fn voice_stays_off_words_that_changed_or_stopped_being_held() {
        let mut held = word(1_000, "love", 1_000, 1_600);
        held.voice = Some(VoiceTrack {
            c: vec![100; 72],
            l: vec![50; 72],
        });
        let old = vec![held];
        let mut short = vec![word(1_000, "love", 1_000, 1_200)];
        carry_voice(&old, &mut short);
        assert!(short[0].voice.is_none());
        let mut other = vec![word(1_000, "dove", 1_000, 1_600)];
        carry_voice(&old, &mut other);
        assert!(other[0].voice.is_none());
        let mut other_line = vec![word(2_000, "love", 1_000, 1_600)];
        carry_voice(&old, &mut other_line);
        assert!(other_line[0].voice.is_none());
    }

    #[test]
    fn moved_words_counts_onsets_and_ends() {
        let old = vec![word(0, "a", 0, 100), word(0, "b", 100, 200)];
        let mut new = old.clone();
        assert_eq!(moved_words(&old, &new), 0);
        new[1].end = Some(210);
        assert_eq!(moved_words(&old, &new), 1);
        assert_eq!(moved_words(&old, &new[..1]), 1);
    }
}
