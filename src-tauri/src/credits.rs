/*
 * Song credits (producers, writers, engineers) from the Genius API, shown
 * under the last lyric line.
 *
 * Why Genius: none of Palette's other sources carry credits (GSMTC, the
 * Spotify Web API, LRCLIB). A test on Thien's own library (2026-11-07, 29
 * songs: new and old K-pop plus Western) matched 29/29 on Genius, with
 * writers on all 29, mixing or mastering on 22 and a producer on 14.
 * MusicBrainz had writers on 10, mixing or mastering on 3 and a producer on
 * 2, so it isn't worth a fallback. SM-style K-pop pages leave Genius's
 * Producer field empty and list the arranger and engineers instead, and
 * pre-2016 K-pop is often writers only, so a short credits list is normal.
 *
 * The client access token is personal and lives in app_data/settings.json
 * under "genius_token". The repo is public, so it must NEVER become a const.
 * It travels in the Authorization header, never the URL, so request logs
 * can't leak it.
 *
 * Matching: Genius search is free text, and K-pop results are crowded with
 * "Genius English Translations" / "Genius Romanizations" pages. A hit counts
 * only when it isn't one of those accounts AND its title and artist both
 * contain ours (token-wise, so "RM" can't match "Storm"). Spotify-style
 * suffixes like "(prod. HONNE)" or "(Wumuti & Hyun)" break Genius search, so
 * the first query drops bracketed parts and the full title is the retry.
 * No confident match means no credits: wrong credits are worse than none.
 *
 * Answers cache in app_data/credits_cache.json. Credits rarely change, so a
 * hit lasts 30 days; a miss lasts 3 days because new songs gain credits
 * soon after release. Transport failures and token problems aren't cached.
 */
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager};

use crate::settings::{self, json_capped, JSON_CAP};
use crate::spotify::urlenc;

const TIMEOUT: Duration = Duration::from_secs(8);
const UA: &str = "Palette/0.1 (https://github.com/thientran01/palette)";
const API: &str = "https://api.genius.com";
pub const TOKEN_KEY: &str = "genius_token";

const CACHE_FILE: &str = "credits_cache.json";
const HIT_TTL_MS: u64 = 30 * 24 * 60 * 60 * 1000;
const MISS_TTL_MS: u64 = 3 * 24 * 60 * 60 * 1000;
const CACHE_CAP: usize = 2000;
/// A page with dozens of instrument rows would bury the lyrics' ending.
const MAX_ROWS: usize = 10;
const MAX_NAMES: usize = 8;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CreditRow {
    pub role: String,
    pub names: Vec<String>,
}

/// IPC payload. `status`: "ok" | "none" (no confident match, or a page with
/// no credits) | "no_key" | "bad_key" | "offline".
#[derive(Serialize)]
pub struct CreditsOut {
    status: &'static str,
    rows: Vec<CreditRow>,
    url: Option<String>,
}

impl CreditsOut {
    fn status(status: &'static str) -> Self {
        Self {
            status,
            rows: Vec::new(),
            url: None,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct Entry {
    at: u64,
    rows: Vec<CreditRow>,
    url: Option<String>,
}

#[tauri::command]
pub async fn song_credits(app: AppHandle, artist: String, title: String) -> CreditsOut {
    tauri::async_runtime::spawn_blocking(move || lookup(&app, &artist, &title))
        .await
        .unwrap_or_else(|e| {
            log::error!("credits task panicked: {e}");
            CreditsOut::status("offline")
        })
}

/// Validate a token for the prefs "Test token" button: "ok" | "invalid" |
/// "offline".
#[tauri::command]
pub async fn test_genius_token(token: String) -> String {
    tauri::async_runtime::spawn_blocking(move || {
        if token.trim().is_empty() {
            return "invalid".to_string();
        }
        match get(token.trim(), &format!("{API}/search?q=palette")) {
            Ok(_) => "ok",
            Err(Fail::BadKey) => "invalid",
            Err(Fail::Offline) => "offline",
        }
        .to_string()
    })
    .await
    .unwrap_or_else(|_| "offline".to_string())
}

fn lookup(app: &AppHandle, artist: &str, title: &str) -> CreditsOut {
    let key = cache_key(artist, title);
    if let Some(e) = cache_get(app, &key) {
        return out_of(e.rows, e.url);
    }
    let Some(token) = settings::get_string(app, TOKEN_KEY)
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
    else {
        return CreditsOut::status("no_key");
    };
    match fetch(&token, artist, title) {
        Ok((rows, url)) => {
            log::info!("credits: {} rows for {artist} - {title}", rows.len());
            cache_put(app, key, rows.clone(), url.clone());
            out_of(rows, url)
        }
        Err(Fail::BadKey) => {
            log::warn!("credits: Genius rejected the token");
            CreditsOut::status("bad_key")
        }
        Err(Fail::Offline) => CreditsOut::status("offline"),
    }
}

fn out_of(rows: Vec<CreditRow>, url: Option<String>) -> CreditsOut {
    CreditsOut {
        status: if rows.is_empty() { "none" } else { "ok" },
        rows,
        url,
    }
}

enum Fail {
    BadKey,
    Offline,
}

fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| ureq::AgentBuilder::new().timeout(TIMEOUT).build())
}

fn get(token: &str, url: &str) -> Result<Value, Fail> {
    let resp = agent()
        .get(url)
        .set("User-Agent", UA)
        .set("Authorization", &format!("Bearer {token}"))
        .call();
    match resp {
        Ok(r) => json_capped(r, JSON_CAP).map_err(|_| Fail::Offline),
        Err(ureq::Error::Status(401 | 403, _)) => Err(Fail::BadKey),
        Err(_) => Err(Fail::Offline),
    }
}

/// Search, pick a confident hit, read its credits. `Ok` with no rows is a
/// served miss.
fn fetch(token: &str, artist: &str, title: &str) -> Result<(Vec<CreditRow>, Option<String>), Fail> {
    let lead_artist = artist.split(',').next().unwrap_or(artist).trim();
    let short = strip_brackets(title);
    let mut queries = vec![format!("{short} {lead_artist}")];
    if short != title.trim() {
        queries.push(format!("{} {lead_artist}", title.trim()));
    }
    for q in queries {
        let v = get(token, &format!("{API}/search?q={}", urlenc(&q)))?;
        let Some(id) = pick_hit(&v["response"]["hits"], artist, title) else {
            continue;
        };
        let song = get(token, &format!("{API}/songs/{id}?text_format=plain"))?;
        let song = &song["response"]["song"];
        let url = song["url"].as_str().map(str::to_string);
        return Ok((credit_rows(song), url));
    }
    Ok((Vec::new(), None))
}

/// Lowercase, letters and digits only (any script), single-spaced tokens.
fn tokens(s: &str) -> Vec<String> {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

/// Whether `needle`'s tokens appear as a contiguous run in `hay`'s.
fn has_run(hay: &[String], needle: &[String]) -> bool {
    !needle.is_empty() && hay.windows(needle.len()).any(|w| w == needle)
}

/// Drop "(...)" and "[...]" segments and a " - Remastered"-style suffix.
fn strip_brackets(title: &str) -> String {
    let mut out = String::new();
    let mut depth = 0usize;
    for c in title.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    let out = out.split(" - ").next().unwrap_or("").trim().to_string();
    if out.is_empty() {
        title.trim().to_string()
    } else {
        out
    }
}

/// The parts of an artist string worth matching on: "Red Velvet - IRENE &
/// SEULGI" → [red velvet, irene, seulgi]; "K/DA, i-dle" → [k da, i dle].
fn artist_parts(artist: &str) -> Vec<Vec<String>> {
    let mut s = artist.to_lowercase();
    for sep in [
        " - ", " & ", ",", " x ", " feat. ", " feat ", " ft. ", " with ",
    ] {
        s = s.replace(sep, "\u{1}");
    }
    s.split('\u{1}')
        .map(tokens)
        .filter(|t| !t.is_empty())
        .collect()
}

fn pick_hit(hits: &Value, artist: &str, title: &str) -> Option<u64> {
    let want_title = tokens(&strip_brackets(title));
    let parts = artist_parts(artist);
    for hit in hits.as_array()? {
        let r = &hit["result"];
        let primary = r["primary_artist"]["name"].as_str().unwrap_or("");
        // Genius's own translation/romanization accounts.
        if primary.starts_with("Genius ") || primary == "Genius" {
            continue;
        }
        let names = format!("{} {}", r["artist_names"].as_str().unwrap_or(""), primary);
        let hit_artist = tokens(&names);
        if !parts.iter().any(|p| has_run(&hit_artist, p)) {
            continue;
        }
        let hit_title = tokens(r["title"].as_str().unwrap_or(""));
        let hit_short = tokens(&strip_brackets(r["title"].as_str().unwrap_or("")));
        // Korean-titled pages keep the English title in brackets
        // ("짧은 치마 (Miniskirt)"), so check the full title too.
        if has_run(&hit_title, &want_title) || has_run(&want_title, &hit_short) {
            if let Some(id) = r["id"].as_u64() {
                return Some(id);
            }
        }
    }
    None
}

/// How a credit role is shown, and where it sorts. `None` hides the role.
fn role_display(label: &str) -> Option<(String, u8)> {
    let l = label.trim().to_lowercase();
    const HIDDEN: [&str; 16] = [
        "label",
        "publisher",
        "copyright",
        "distributor",
        "a&r",
        "manage",
        "video",
        "photo",
        "artwork",
        "art direction",
        "creative direct",
        "choreograph",
        "marketing",
        "assistant",
        "executive",
        "stylist",
    ];
    if l.is_empty() || HIDDEN.iter().any(|h| l.contains(h)) {
        return None;
    }
    let mapped = match l.as_str() {
        "producer" | "produced by" => ("Produced by", 0),
        "co-producer" => ("Co-produced by", 1),
        "lyricist" | "lyrics" => ("Lyrics", 3),
        "composer" | "composition" => ("Composition", 4),
        "arranger" | "arrangement" => ("Arrangement", 5),
        "vocals" | "lead vocals" => ("Vocals", 6),
        "background vocals" | "backing vocals" | "chorus" => ("Background vocals", 7),
        "vocal director" | "vocal directing" | "vocal producer" => ("Vocal direction", 8),
        "recording engineer" | "recorded by" => ("Recorded by", 60),
        "mixing engineer" | "mix engineer" | "mixed by" => ("Mixed by", 62),
        "mastering engineer" | "mastered by" => ("Mastered by", 63),
        _ => return Some((sentence_case(label.trim()), 30)),
    };
    Some((mapped.0.to_string(), mapped.1))
}

/// "Digital Editor" → "Digital editor".
fn sentence_case(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first
            .to_uppercase()
            .chain(chars.flat_map(char::to_lowercase))
            .collect(),
        None => String::new(),
    }
}

fn names_of(list: &Value) -> Vec<String> {
    list.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x["name"].as_str())
                .map(|n| n.trim().to_string())
                .filter(|n| !n.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

type Ranked = Vec<(u8, usize, CreditRow)>;

/// Merge `names` into the row for `role`, creating it at `rank` if new.
fn add_names(rows: &mut Ranked, role: &str, rank: u8, names: Vec<String>) {
    if names.is_empty() {
        return;
    }
    let i = match rows.iter().position(|(_, _, r)| r.role == role) {
        Some(i) => i,
        None => {
            let n = rows.len();
            rows.push((
                rank,
                n,
                CreditRow {
                    role: role.to_string(),
                    names: Vec::new(),
                },
            ));
            n
        }
    };
    let row = &mut rows[i].2;
    for name in names {
        if !row
            .names
            .iter()
            .any(|n| n.to_lowercase() == name.to_lowercase())
        {
            row.names.push(name);
        }
    }
}

/// Genius song object → ordered, merged, de-duplicated rows. Ties in rank
/// keep Genius's own order.
fn credit_rows(song: &Value) -> Vec<CreditRow> {
    let mut rows: Ranked = Vec::new();
    add_names(
        &mut rows,
        "Produced by",
        0,
        names_of(&song["producer_artists"]),
    );
    if let Some(perfs) = song["custom_performances"].as_array() {
        for p in perfs {
            if let Some((role, rank)) = role_display(p["label"].as_str().unwrap_or("")) {
                add_names(&mut rows, &role, rank, names_of(&p["artists"]));
            }
        }
    }
    // writer_artists is usually lyricists and composers together, so it
    // only earns a row when the page doesn't split them.
    let split = rows
        .iter()
        .any(|(_, _, r)| r.role == "Lyrics" || r.role == "Composition");
    if !split {
        add_names(
            &mut rows,
            "Written by",
            2,
            names_of(&song["writer_artists"]),
        );
    }
    rows.sort_by_key(|(rank, n, _)| (*rank, *n));
    rows.into_iter()
        .take(MAX_ROWS)
        .map(|(_, _, mut r)| {
            r.names.truncate(MAX_NAMES);
            r
        })
        .collect()
}

// ---- disk cache ----

fn cache_key(artist: &str, title: &str) -> String {
    format!(
        "{}\u{1}{}",
        tokens(artist).join(" "),
        tokens(title).join(" ")
    )
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

struct Cache {
    path: Option<PathBuf>,
    map: HashMap<String, Entry>,
}

static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

fn with_cache<R>(app: &AppHandle, f: impl FnOnce(&mut Cache) -> R) -> R {
    let mut guard = CACHE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let cache = guard.get_or_insert_with(|| {
        let path = app.path().app_data_dir().ok().map(|d| d.join(CACHE_FILE));
        let map = path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
        Cache { path, map }
    });
    f(cache)
}

fn fresh(e: &Entry, now: u64) -> bool {
    let ttl = if e.rows.is_empty() {
        MISS_TTL_MS
    } else {
        HIT_TTL_MS
    };
    now.saturating_sub(e.at) < ttl
}

fn cache_get(app: &AppHandle, key: &str) -> Option<Entry> {
    let now = unix_ms();
    with_cache(app, |c| c.map.get(key).filter(|e| fresh(e, now)).cloned())
}

fn cache_put(app: &AppHandle, key: String, rows: Vec<CreditRow>, url: Option<String>) {
    let now = unix_ms();
    with_cache(app, |c| {
        c.map.insert(key, Entry { at: now, rows, url });
        prune(&mut c.map, now);
        let Some(path) = &c.path else { return };
        match serde_json::to_string(&c.map) {
            Ok(json) => {
                if let Err(e) = settings::write_atomic(path, json.as_bytes()) {
                    log::warn!("credits: cache write failed: {e}");
                }
            }
            Err(e) => log::warn!("credits: cache encode failed: {e}"),
        }
    });
}

/// Drop expired entries, then the oldest past the cap.
fn prune(map: &mut HashMap<String, Entry>, now: u64) {
    map.retain(|_, e| fresh(e, now));
    if map.len() <= CACHE_CAP {
        return;
    }
    let mut ages: Vec<(u64, String)> = map.iter().map(|(k, e)| (e.at, k.clone())).collect();
    ages.sort();
    for (_, k) in ages.into_iter().take(map.len() - CACHE_CAP) {
        map.remove(&k);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn hit(id: u64, title: &str, artist_names: &str, primary: &str) -> Value {
        json!({ "type": "song", "result": {
            "id": id, "title": title, "artist_names": artist_names,
            "primary_artist": { "name": primary }
        }})
    }

    #[test]
    fn strips_spotify_suffixes() {
        assert_eq!(strip_brackets("seoul (prod. HONNE)"), "seoul");
        assert_eq!(strip_brackets("Silence (Wumuti & Hyun)"), "Silence");
        assert_eq!(strip_brackets("Song - Remastered 2011"), "Song");
        assert_eq!(strip_brackets("(Intro)"), "(Intro)");
    }

    #[test]
    fn skips_translation_pages_and_wrong_artists() {
        let hits = json!([
            hit(
                1,
                "AOA - 짧은 치마 (Miniskirt) (English Translation)",
                "Genius English Translations",
                "Genius English Translations"
            ),
            hit(2, "Miniskirt", "Someone Else", "Someone Else"),
            hit(3, "짧은 치마 (Miniskirt)", "AOA", "AOA"),
        ]);
        assert_eq!(pick_hit(&hits, "AOA", "Miniskirt"), Some(3));
    }

    #[test]
    fn matches_sub_unit_and_short_artist_names_by_token() {
        let hits = json!([hit(7, "Fall 4 U", "IRENE & SEULGI", "IRENE & SEULGI")]);
        assert_eq!(
            pick_hit(&hits, "Red Velvet - IRENE & SEULGI", "Fall 4 U"),
            Some(7)
        );
        let storm = json!([hit(8, "seoul", "Storm", "Storm")]);
        assert_eq!(pick_hit(&storm, "RM", "seoul (prod. HONNE)"), None);
        let rm = json!([hit(9, "seoul", "RM", "RM")]);
        assert_eq!(pick_hit(&rm, "RM", "seoul (prod. HONNE)"), Some(9));
    }

    #[test]
    fn no_confident_hit_means_no_credits() {
        let hits = json!([hit(1, "Teddy Bear", "Elvis Presley", "Elvis Presley")]);
        assert_eq!(pick_hit(&hits, "STAYC", "Teddy Bear"), None);
    }

    #[test]
    fn rows_are_ordered_merged_and_filtered() {
        let song = json!({
            "producer_artists": [],
            "writer_artists": [{ "name": "Kenzie" }, { "name": "Dem Jointz" }],
            "custom_performances": [
                { "label": "Mastering Engineer", "artists": [{ "name": "Kwon Nam-woo" }] },
                { "label": "Label", "artists": [{ "name": "SM Entertainment" }] },
                { "label": "Composer", "artists": [{ "name": "Kenzie" }, { "name": "Dem Jointz" }] },
                { "label": "Lyricist", "artists": [{ "name": "Kenzie" }] },
                { "label": "Mixing Engineer", "artists": [{ "name": "Kim Cheol-sun" }] },
                { "label": "Digital Editor", "artists": [{ "name": "Noh Min-ji" }] },
                { "label": "Assistant Mixing Engineer", "artists": [{ "name": "X" }] },
                { "label": "Arranger", "artists": [{ "name": "Dem Jointz" }] }
            ]
        });
        let roles: Vec<String> = credit_rows(&song).into_iter().map(|r| r.role).collect();
        assert_eq!(
            roles,
            [
                "Lyrics",
                "Composition",
                "Arrangement",
                "Digital editor",
                "Mixed by",
                "Mastered by"
            ]
        );
    }

    #[test]
    fn writers_show_when_the_page_does_not_split_them() {
        let song = json!({
            "producer_artists": [{ "name": "HONNE" }],
            "writer_artists": [{ "name": "RM" }, { "name": "HONNE" }, { "name": "rm" }],
            "custom_performances": []
        });
        let rows = credit_rows(&song);
        assert_eq!(
            rows[0],
            CreditRow {
                role: "Produced by".into(),
                names: vec!["HONNE".into()]
            }
        );
        assert_eq!(
            rows[1],
            CreditRow {
                role: "Written by".into(),
                names: vec!["RM".into(), "HONNE".into()]
            }
        );
    }

    #[test]
    fn misses_expire_sooner_than_hits() {
        let mut map = HashMap::new();
        map.insert(
            "miss".into(),
            Entry {
                at: 0,
                rows: vec![],
                url: None,
            },
        );
        map.insert(
            "hit".into(),
            Entry {
                at: 0,
                rows: vec![CreditRow {
                    role: "Mixed by".into(),
                    names: vec!["A".into()],
                }],
                url: None,
            },
        );
        prune(&mut map, MISS_TTL_MS + 1);
        assert!(map.contains_key("hit"));
        assert!(!map.contains_key("miss"));
    }
}
