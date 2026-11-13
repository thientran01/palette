//! Lines the listener timed by hand.
//!
//! The aligner gets most lines right, but now and then one is plainly wrong
//! (a stamp that clipped its window, a confident decode of the wrong
//! stretch) and the listener can see it at a glance. They fix it in the
//! lyrics: tap the words in as they're sung, or have the line light whole
//! on its stamp. A fix is kept here, one file per song, apart from the
//! aligner's sync, and laid over whatever sync the song has each time its
//! words are read. Relearns, extra listens and re-decodes never see it, so
//! none of them can undo it, and it carries over to every later sync of the
//! same lyrics. It applies only while its line is still in the song's
//! timed lyrics with the same stamp and text; a changed line leaves it
//! dormant, never misplaced.
//!
//! docs/specs/2026-11-13-karaoke-line-fixes.md
use super::*;

const FIXES_DIR: &str = "karaoke-fixes";
const FIXES_V: u32 = 1;
/// A tapped word must sit within this of its line's stamp: a song never
/// sings a line minutes away from it, and a runaway tap is a bug.
const REACH_MS: i64 = 60_000;
/// More words than any real lyric line.
const MAX_WORDS: usize = 200;
const MAX_TEXT: usize = 2_000;

static GATE: Mutex<()> = Mutex::new(());
fn gate() -> std::sync::MutexGuard<'static, ()> {
    GATE.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// One tapped word: where it starts and stops on the song's timeline.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TappedWord {
    pub t: i64,
    pub end: i64,
    pub text: String,
}

/// A hand fix for one line, identified by its stamp and text.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct LineFix {
    pub line_t: i64,
    pub text: String,
    /// The line's words as tapped, or none: light the whole line on its
    /// stamp.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub words: Vec<TappedWord>,
}

#[derive(Serialize, Deserialize, Default)]
struct FixFile {
    v: u32,
    lines: Vec<LineFix>,
}

pub(crate) fn path(root: &Path, key: &str) -> PathBuf {
    root.join(FIXES_DIR).join(format!("{key}.json"))
}

fn read(path: &Path) -> Vec<LineFix> {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice::<FixFile>(&b).ok())
        .filter(|f| f.v == FIXES_V)
        .map_or_else(Vec::new, |f| f.lines)
}

fn write(path: &Path, lines: Vec<LineFix>) -> std::io::Result<()> {
    if lines.is_empty() {
        return match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    let bytes = serde_json::to_vec(&FixFile { v: FIXES_V, lines })
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    write_atomic(path, &bytes)
}

/// `words` with the song's hand fixes laid over them. Words are only read
/// here, never saved: the fix file is the one place a fix lives. A song
/// with no word sync stays without one.
pub(crate) fn apply(root: &Path, key: &str, synced: Option<&str>, words: Vec<Word>) -> Vec<Word> {
    if words.is_empty() {
        return words;
    }
    let fixes = read(&path(root, key));
    if fixes.is_empty() {
        return words;
    }
    overlay(words, &fixes, synced)
}

/// The fixes whose line is in `synced` as they were made for it.
fn live<'a>(fixes: &'a [LineFix], synced: Option<&str>) -> Vec<&'a LineFix> {
    let lines = align::parse_lrc(synced.unwrap_or_default());
    fixes
        .iter()
        .filter(|fix| {
            lines
                .iter()
                .any(|line| line.t == fix.line_t && line.text == fix.text)
        })
        .collect()
}

fn overlay(words: Vec<Word>, fixes: &[LineFix], synced: Option<&str>) -> Vec<Word> {
    let live = live(fixes, synced);
    if live.is_empty() {
        return words;
    }
    let fixed: HashSet<i64> = live.iter().map(|f| f.line_t).collect();
    let mut out: Vec<Word> = words
        .into_iter()
        .filter(|w| w.line_t.is_none_or(|t| !fixed.contains(&t)))
        .collect();
    for fix in live {
        if fix.words.is_empty() {
            out.push(Word {
                t: fix.line_t,
                text: fix.text.clone(),
                line_t: Some(fix.line_t),
                fixed: Some(align::Fix::Plain),
                ..Word::default()
            });
        } else {
            out.extend(fix.words.iter().map(|w| Word {
                t: w.t,
                end: Some(w.end),
                text: w.text.clone(),
                line_t: Some(fix.line_t),
                fixed: Some(align::Fix::Timed),
                ..Word::default()
            }));
        }
    }
    // Lines in song order, each line's words as they came.
    out.sort_by_key(|w| w.line_t.unwrap_or(w.t));
    out
}

/// A fix as the frontend sent it, or why it can't be kept.
fn checked(fix: LineFix) -> Result<LineFix, String> {
    if fix.text.trim().is_empty() || fix.text.len() > MAX_TEXT {
        return Err("Invalid line".into());
    }
    if fix.words.len() > MAX_WORDS {
        return Err("Too many words".into());
    }
    let mut last = i64::MIN;
    for w in &fix.words {
        if w.end < w.t
            || w.t < last
            || (w.t - fix.line_t).abs() > REACH_MS
            || (w.end - fix.line_t).abs() > REACH_MS
            || w.text.len() > MAX_TEXT
        {
            return Err("Invalid word timing".into());
        }
        last = w.t;
    }
    Ok(fix)
}

fn set(root: &Path, key: &str, fix: LineFix) -> Result<(), String> {
    let fix = checked(fix)?;
    let _g = gate();
    let path = path(root, key);
    let mut lines = read(&path);
    lines.retain(|l| !(l.line_t == fix.line_t && l.text == fix.text));
    lines.push(fix);
    lines.sort_by_key(|l| l.line_t);
    write(&path, lines).map_err(|e| e.to_string())
}

fn clear(root: &Path, key: &str, line_t: i64, text: &str) -> Result<(), String> {
    let _g = gate();
    let path = path(root, key);
    let mut lines = read(&path);
    let before = lines.len();
    lines.retain(|l| !(l.line_t == line_t && l.text == text));
    if lines.len() == before {
        return Ok(());
    }
    write(&path, lines).map_err(|e| e.to_string())
}

/// After a change, every window showing the song gets its words again.
fn publish(app: &AppHandle, artist: String, title: String, album: String, duration_ms: i64) {
    let Ok(root) = app.path().app_data_dir() else {
        return;
    };
    let synced = lyrics::cached_synced(&lyrics_dir(app), &artist, &title, &album, duration_ms);
    let words = load(
        &root.join(KARAOKE_DIR),
        &artist,
        &title,
        &album,
        duration_ms,
        synced.as_deref(),
    );
    let _ = app.emit(
        "karaoke-ready",
        KaraokeReady {
            artist,
            title,
            album,
            duration_ms,
            words,
        },
    );
}

/// Keep a hand fix for one line: `words` as tapped, or none to light the
/// line whole on its stamp.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn karaoke_fix_line(
    app: AppHandle,
    artist: String,
    title: String,
    album: String,
    duration_ms: i64,
    line_t: i64,
    text: String,
    words: Vec<TappedWord>,
) -> Result<(), String> {
    let root = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let key = lyrics::key_for_ms(&artist, &title, &album, duration_ms);
    let fix = LineFix {
        line_t,
        text,
        words,
    };
    let kind = if fix.words.is_empty() {
        "plain"
    } else {
        "tapped"
    };
    tauri::async_runtime::spawn_blocking(move || {
        set(&root, &key, fix)?;
        log::info!("karaoke: line at {line_t}ms of {title} fixed by hand ({kind})");
        publish(&app, artist, title, album, duration_ms);
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Drop the hand fix for one line: the aligner's timing shows again.
#[tauri::command]
pub async fn karaoke_unfix_line(
    app: AppHandle,
    artist: String,
    title: String,
    album: String,
    duration_ms: i64,
    line_t: i64,
    text: String,
) -> Result<(), String> {
    let root = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let key = lyrics::key_for_ms(&artist, &title, &album, duration_ms);
    tauri::async_runtime::spawn_blocking(move || {
        clear(&root, &key, line_t, &text)?;
        log::info!("karaoke: hand fix for the line at {line_t}ms of {title} removed");
        publish(&app, artist, title, album, duration_ms);
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    const LRC: &str = "[00:01.00]one two\n[00:05.00]three four\n[00:09.00]five\n";

    fn word(line_t: i64, text: &str, t: i64) -> Word {
        Word {
            t,
            end: Some(t + 200),
            text: text.into(),
            line_t: Some(line_t),
            support: Some(0.4),
            ..Word::default()
        }
    }

    fn sync() -> Vec<Word> {
        vec![
            word(1000, "one ", 1000),
            word(1000, "two", 1400),
            word(5000, "three ", 8100),
            word(5000, "four", 8150),
            word(9000, "five", 9000),
        ]
    }

    fn tapped(t: i64, end: i64, text: &str) -> TappedWord {
        TappedWord {
            t,
            end,
            text: text.into(),
        }
    }

    #[test]
    fn a_tapped_line_replaces_the_aligners_words_for_that_line_only() {
        let fixes = [LineFix {
            line_t: 5000,
            text: "three four".into(),
            words: vec![tapped(5100, 5500, "three "), tapped(5600, 6400, "four")],
        }];
        let out = overlay(sync(), &fixes, Some(LRC));
        let texts: Vec<_> = out.iter().map(|w| (w.text.as_str(), w.t)).collect();
        assert_eq!(
            texts,
            [
                ("one ", 1000),
                ("two", 1400),
                ("three ", 5100),
                ("four", 5600),
                ("five", 9000)
            ]
        );
        let fixed: Vec<_> = out.iter().filter(|w| w.fixed.is_some()).collect();
        assert_eq!(fixed.len(), 2);
        assert!(fixed
            .iter()
            .all(|w| w.fixed == Some(align::Fix::Timed) && w.support.is_none()));
        assert_eq!(fixed[1].end, Some(6400));
    }

    #[test]
    fn a_plain_fix_leaves_one_untimed_word_on_the_stamp() {
        let fixes = [LineFix {
            line_t: 5000,
            text: "three four".into(),
            words: vec![],
        }];
        let out = overlay(sync(), &fixes, Some(LRC));
        let line: Vec<_> = out.iter().filter(|w| w.line_t == Some(5000)).collect();
        assert_eq!(line.len(), 1);
        assert_eq!(line[0].fixed, Some(align::Fix::Plain));
        assert_eq!((line[0].t, line[0].end), (5000, None));
        assert_eq!(line[0].text, "three four");
    }

    #[test]
    fn a_fix_for_a_line_the_lyrics_no_longer_have_stays_dormant() {
        let moved = LineFix {
            line_t: 5200,
            text: "three four".into(),
            words: vec![],
        };
        let reworded = LineFix {
            line_t: 5000,
            text: "three for".into(),
            words: vec![],
        };
        assert_eq!(overlay(sync(), &[moved, reworded], Some(LRC)), sync());
        assert_eq!(
            overlay(
                sync(),
                &[LineFix {
                    line_t: 5000,
                    text: "three four".into(),
                    words: vec![]
                }],
                None
            ),
            sync()
        );
    }

    #[test]
    fn fixes_survive_a_new_sync_and_never_reach_the_saved_file() {
        let root = std::env::temp_dir().join(format!(
            "palette-fixes-{}-{}",
            std::process::id(),
            unix_ms()
        ));
        let key = "00112233aabbccdd";
        set(
            &root,
            key,
            LineFix {
                line_t: 1000,
                text: "one two".into(),
                words: vec![tapped(900, 1300, "one "), tapped(1300, 2000, "two")],
            },
        )
        .unwrap();
        // A relearn saves different words; the fix still lays over them.
        let relearned: Vec<Word> = sync()
            .into_iter()
            .map(|mut w| {
                w.t += 30;
                w
            })
            .collect();
        let out = apply(&root, key, Some(LRC), relearned.clone());
        assert_eq!(out[0].t, 900);
        assert_eq!(out[2].t, relearned[2].t);
        // Taking it back shows the aligner's line again, and the last fix
        // gone removes the file.
        clear(&root, key, 1000, "one two").unwrap();
        assert_eq!(apply(&root, key, Some(LRC), relearned.clone()), relearned);
        assert!(!path(&root, key).exists());
        // No word sync: nothing to lay a fix over.
        set(
            &root,
            key,
            LineFix {
                line_t: 1000,
                text: "one two".into(),
                words: vec![],
            },
        )
        .unwrap();
        assert!(apply(&root, key, Some(LRC), Vec::new()).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_refit_of_the_same_line_replaces_the_earlier_fix() {
        let root = std::env::temp_dir().join(format!(
            "palette-refix-{}-{}",
            std::process::id(),
            unix_ms()
        ));
        let key = "ffeeddccbbaa0011";
        let plain = LineFix {
            line_t: 9000,
            text: "five".into(),
            words: vec![],
        };
        set(&root, key, plain).unwrap();
        set(
            &root,
            key,
            LineFix {
                line_t: 9000,
                text: "five".into(),
                words: vec![tapped(9100, 9900, "five")],
            },
        )
        .unwrap();
        let kept = read(&path(&root, key));
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].words.len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn taps_out_of_order_or_far_from_the_line_are_refused() {
        let fix = |words| LineFix {
            line_t: 5000,
            text: "three four".into(),
            words,
        };
        assert!(checked(fix(vec![
            tapped(5600, 6000, "three "),
            tapped(5100, 5500, "four")
        ]))
        .is_err());
        assert!(checked(fix(vec![tapped(5600, 5500, "three four")])).is_err());
        assert!(checked(fix(vec![tapped(500_000, 500_100, "three four")])).is_err());
        assert!(checked(fix(vec![tapped(5100, 5500, "three four")])).is_ok());
        assert!(checked(LineFix {
            line_t: 0,
            text: " ".into(),
            words: vec![]
        })
        .is_err());
    }
}
