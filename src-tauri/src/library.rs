//! Spotify library reads for the Search window's Library section: the user's
//! playlists, their Liked Songs, a playlist's songs, and public playlists by
//! search. Every call is a plain GET over spotify.rs's api_call (same 401
//! refresh, 429 honor and token policy); nothing here writes to Spotify.
//! Playback of a playlist goes through spotify::play_context.
//!
//! What Spotify lets a Development Mode app read (February 2026 changes,
//! checked against developer.spotify.com 2026-11-06):
//! - `GET /me/playlists` and `GET /me/tracks` stay open. The playlist
//!   object's `tracks` field is renamed `items` (`items.total` is the count).
//! - `GET /playlists/{id}/items` (was `/tracks`) answers ONLY for playlists
//!   the user owns or collaborates on. Followed and public playlists come back
//!   as metadata only, so their songs can't be listed — but `context_uri`
//!   playback still works for them. Each item's track moved from `track` to
//!   `item` (`track` may now be a boolean), so both spellings are read.
//! - `GET /search` caps `limit` at 10.
//! - Spotify-owned editorial and algorithmic playlists can't be opened by new
//!   apps at all, so they are dropped everywhere (owner id "spotify").
//!
//! Scopes: a token consented before the library scopes were added lacks
//! them. That is checked up front from the stored grant string and answered
//! as "needs_reconnect" (one re-consent fixes it) instead of a 403.
//!
//! Caching lives in the search window's realm (Search.tsx): the lists load
//! once per app session and filter locally on every keystroke.

use serde::Serialize;
use tauri::AppHandle;

use crate::spotify::{self, QueueTrack};

const LIBRARY_SCOPES: [&str; 4] = [
    "playlist-read-private",
    "playlist-read-collaborative",
    "user-library-read",
    "user-read-recently-played",
];
/// Recently-played pages walked for the Recents-style sort (50 tracks each,
/// Spotify's maximum page).
const MAX_RECENT_PAGES: usize = 4;
/// Spotify's page maximum for the library and playlist-item endpoints.
const PAGE: u32 = 50;
/// Page caps per load, so one call can't walk an enormous library forever:
/// 1000 playlists, 2000 liked songs, 1000 songs in one playlist.
const MAX_PLAYLIST_PAGES: usize = 20;
const MAX_LIKED_PAGES: usize = 40;
const MAX_ITEM_PAGES: usize = 20;
/// Spotify's post-February-2026 ceiling for `GET /search`.
const SEARCH_MAX: u32 = 10;
/// Owner id of Spotify's own editorial and algorithmic playlists.
const EDITORIAL_OWNER: &str = "spotify";

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct PlaylistSummary {
    pub uri: String,
    pub id: String,
    pub name: String,
    /// Owner display name (falls back to the owner id).
    pub owner: String,
    /// Owned by the user or collaborative: the only playlists whose songs
    /// Spotify will list. Everything else is play-only.
    pub mine: bool,
    /// Song count when Spotify reports one.
    pub total: Option<i64>,
    pub art_url: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct LibraryResult {
    /// "ok" | "needs_reconnect" | "disconnected" | "offline"
    pub status: String,
    /// The account id — builds the Liked Songs context
    /// (`spotify:user:<id>:collection`).
    pub user_id: String,
    pub playlists: Vec<PlaylistSummary>,
    /// Context uris from the recently-played log, most recent first, each
    /// once — the Library sorts playlists by it (Thien's pick, 2026-11-06:
    /// "Recently played first", like Spotify's Recents). Empty when the
    /// read fails; the sort then falls back to A–Z.
    pub recent_contexts: Vec<String>,
}

#[derive(Serialize, Clone)]
pub struct TracksResult {
    /// "ok" | "needs_reconnect" | "restricted" (Spotify won't list this
    /// playlist's songs) | "disconnected" | "offline"
    pub status: String,
    pub tracks: Vec<QueueTrack>,
    /// Spotify's own count, which can exceed `tracks` when the page cap hit.
    pub total: Option<i64>,
}

#[derive(Serialize, Clone)]
pub struct PlaylistSearchResult {
    /// "ok" | "disconnected" | "offline"
    pub status: String,
    pub playlists: Vec<PlaylistSummary>,
}

impl LibraryResult {
    fn bare(status: &str) -> Self {
        Self {
            status: status.into(),
            user_id: String::new(),
            playlists: Vec::new(),
            recent_contexts: Vec::new(),
        }
    }
}

impl TracksResult {
    fn bare(status: &str) -> Self {
        Self {
            status: status.into(),
            tracks: Vec::new(),
            total: None,
        }
    }
}

/// One simplified playlist object. None for a null slot (Spotify's search
/// pads its playlist results with nulls), a missing uri, or an editorial
/// playlist. `me` = the account id, for ownership.
pub(crate) fn parse_playlist(v: &serde_json::Value, me: &str) -> Option<PlaylistSummary> {
    let uri = v["uri"].as_str()?.to_string();
    let id = v["id"].as_str()?.to_string();
    let owner_id = v["owner"]["id"].as_str().unwrap_or_default();
    if owner_id == EDITORIAL_OWNER {
        return None;
    }
    let owner = v["owner"]["display_name"]
        .as_str()
        .filter(|s| !s.is_empty())
        .unwrap_or(owner_id)
        .to_string();
    let collaborative = v["collaborative"].as_bool().unwrap_or(false);
    let total = v["items"]["total"]
        .as_i64()
        .or_else(|| v["tracks"]["total"].as_i64());
    Some(PlaylistSummary {
        uri,
        id,
        name: v["name"].as_str().unwrap_or_default().to_string(),
        owner,
        mine: (!me.is_empty() && owner_id == me) || collaborative,
        total,
        art_url: spotify::small_image(&v["images"]),
    })
}

/// One playlist-item or saved-track row → a playable track. Reads the new
/// `item` field first, then the old `track` field, and skips anything that
/// isn't a Spotify track (local files, episodes, removed songs).
pub(crate) fn parse_item(row: &serde_json::Value) -> Option<QueueTrack> {
    let t = [&row["item"], &row["track"]]
        .into_iter()
        .find(|v| v.is_object())?;
    let track = spotify::parse_track(t)?;
    track.uri.starts_with("spotify:track:").then_some(track)
}

/// Walk a paged endpoint by its `next` links, up to `max_pages`. Returns the
/// first page's `total` and every row from `field` (usually "items").
fn walk(
    app: &AppHandle,
    first_url: String,
    max_pages: usize,
    on_403: &'static str,
) -> Result<(Option<i64>, Vec<serde_json::Value>), &'static str> {
    let mut rows = Vec::new();
    let mut total = None;
    let mut url = Some(first_url);
    for _ in 0..max_pages {
        let Some(u) = url.take() else { break };
        let Some(v) = spotify::api_call_impl(app, "GET", &u, None, on_403)? else {
            break;
        };
        if total.is_none() {
            total = v["total"].as_i64();
        }
        match v["items"].as_array() {
            Some(items) => rows.extend(items.iter().cloned()),
            // No `items` array on the first page = Spotify answered with
            // metadata only (a playlist the user doesn't own).
            None if rows.is_empty() => return Err("restricted"),
            None => break,
        }
        url = v["next"].as_str().map(String::from);
    }
    Ok((total, rows))
}

fn me_id(app: &AppHandle) -> Result<String, &'static str> {
    let v = spotify::api_call(app, "GET", "https://api.spotify.com/v1/me")?;
    Ok(v.and_then(|v| v["id"].as_str().map(String::from))
        .unwrap_or_default())
}

fn library(app: &AppHandle) -> LibraryResult {
    if !spotify::connected(app) {
        return LibraryResult::bare("disconnected");
    }
    if !spotify::has_scopes(app, &LIBRARY_SCOPES) {
        return LibraryResult::bare("needs_reconnect");
    }
    let me = match me_id(app) {
        Ok(id) => id,
        Err(e) => return LibraryResult::bare(e),
    };
    let first = format!("https://api.spotify.com/v1/me/playlists?limit={PAGE}");
    match walk(app, first, MAX_PLAYLIST_PAGES, "needs_reconnect") {
        Ok((_, rows)) => LibraryResult {
            status: "ok".into(),
            playlists: rows.iter().filter_map(|r| parse_playlist(r, &me)).collect(),
            user_id: me,
            recent_contexts: recent_contexts(app),
        },
        Err("restricted") => LibraryResult::bare("offline"),
        Err(e) => LibraryResult::bare(e),
    }
}

/// The play log's contexts, newest first. Best-effort: any failure is an
/// empty list (the list still loads, sorted A–Z).
fn recent_contexts(app: &AppHandle) -> Vec<String> {
    let first = format!("https://api.spotify.com/v1/me/player/recently-played?limit={PAGE}");
    match walk(app, first, MAX_RECENT_PAGES, "offline") {
        Ok((_, rows)) => contexts_newest_first(&rows),
        Err(e) => {
            log::warn!("spotify recently-played unavailable: {e}");
            Vec::new()
        }
    }
}

/// Recently-played rows → their context uris, newest first, deduped. Rows
/// arrive newest first; `played_at` (ISO-8601 UTC, so it sorts as text)
/// re-sorts them anyway in case pages overlap.
pub(crate) fn contexts_newest_first(rows: &[serde_json::Value]) -> Vec<String> {
    let mut plays: Vec<(&str, &str)> = rows
        .iter()
        .filter_map(|r| Some((r["played_at"].as_str()?, r["context"]["uri"].as_str()?)))
        .collect();
    plays.sort_by(|a, b| b.0.cmp(a.0));
    let mut out: Vec<String> = Vec::new();
    for (_, uri) in plays {
        if !out.iter().any(|u| u == uri) {
            out.push(uri.to_string());
        }
    }
    out
}

fn liked(app: &AppHandle) -> TracksResult {
    if !spotify::connected(app) {
        return TracksResult::bare("disconnected");
    }
    if !spotify::has_scopes(app, &LIBRARY_SCOPES) {
        return TracksResult::bare("needs_reconnect");
    }
    let first = format!("https://api.spotify.com/v1/me/tracks?limit={PAGE}");
    match walk(app, first, MAX_LIKED_PAGES, "needs_reconnect") {
        Ok((total, rows)) => TracksResult {
            status: "ok".into(),
            tracks: rows.iter().filter_map(parse_item).collect(),
            total,
        },
        Err("restricted") => TracksResult::bare("offline"),
        Err(e) => TracksResult::bare(e),
    }
}

fn playlist_tracks(app: &AppHandle, id: &str) -> TracksResult {
    if !spotify::connected(app) {
        return TracksResult::bare("disconnected");
    }
    if !spotify::has_scopes(app, &LIBRARY_SCOPES) {
        return TracksResult::bare("needs_reconnect");
    }
    let first = format!(
        "https://api.spotify.com/v1/playlists/{}/items?limit={PAGE}",
        spotify::urlenc(id)
    );
    // A 403 here is Spotify refusing a playlist the user doesn't own.
    match walk(app, first, MAX_ITEM_PAGES, "restricted") {
        Ok((total, rows)) => TracksResult {
            status: "ok".into(),
            tracks: rows.iter().filter_map(parse_item).collect(),
            total,
        },
        Err(e) => TracksResult::bare(e),
    }
}

fn search_playlists(app: &AppHandle, query: &str, limit: u32) -> PlaylistSearchResult {
    let url = format!(
        "https://api.spotify.com/v1/search?type=playlist&limit={}&q={}",
        limit.clamp(1, SEARCH_MAX),
        spotify::urlenc(query)
    );
    match spotify::api_call(app, "GET", &url) {
        Ok(v) => PlaylistSearchResult {
            status: "ok".into(),
            // Ownership is resolved in the webview against the cached
            // library (a search hit the user owns opens like a library row).
            playlists: v
                .and_then(|v| {
                    v["playlists"]["items"]
                        .as_array()
                        .map(|items| items.iter().filter_map(|p| parse_playlist(p, "")).collect())
                })
                .unwrap_or_default(),
        },
        Err(e) => PlaylistSearchResult {
            status: e.into(),
            playlists: Vec::new(),
        },
    }
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct NowContext {
    /// The playing context (playlist, `…:collection` for Liked Songs, an
    /// album…); None when Spotify plays loose tracks.
    pub context_uri: Option<String>,
    pub track_uri: Option<String>,
}

/// What Spotify is playing right now, for the Library's "Playing" labels.
/// None = nothing playing, disconnected or offline (labels just don't show).
fn now_context(app: &AppHandle) -> Option<NowContext> {
    let v = spotify::api_call(
        app,
        "GET",
        "https://api.spotify.com/v1/me/player/currently-playing",
    )
    .ok()??;
    Some(parse_now_context(&v))
}

pub(crate) fn parse_now_context(v: &serde_json::Value) -> NowContext {
    NowContext {
        context_uri: v["context"]["uri"].as_str().map(String::from),
        track_uri: v["item"]["uri"].as_str().map(String::from),
    }
}

// ---- commands (blocking HTTP on the dedicated pool, like spotify.rs) ----

/// A fresh currently-playing read (one GET) — the Library marks the playlist
/// and song that are playing. Read fresh on each summon rather than cached,
/// because the context can change from the Spotify app at any time.
#[tauri::command]
pub async fn spotify_now_context(app: AppHandle) -> Option<NowContext> {
    tauri::async_runtime::spawn_blocking(move || now_context(&app))
        .await
        .unwrap_or(None)
}

/// The user's playlists (owned, collaborative and followed), editorial ones
/// dropped, in Spotify's own order.
#[tauri::command]
pub async fn spotify_library(app: AppHandle) -> LibraryResult {
    tauri::async_runtime::spawn_blocking(move || library(&app))
        .await
        .unwrap_or_else(|e| {
            log::error!("spotify library task panicked: {e}");
            LibraryResult::bare("offline")
        })
}

/// Liked Songs, newest first (Spotify's saved-at order).
#[tauri::command]
pub async fn spotify_liked(app: AppHandle) -> TracksResult {
    tauri::async_runtime::spawn_blocking(move || liked(&app))
        .await
        .unwrap_or_else(|e| {
            log::error!("spotify liked task panicked: {e}");
            TracksResult::bare("offline")
        })
}

/// One playlist's songs. "restricted" when Spotify won't list them.
#[tauri::command]
pub async fn spotify_playlist_tracks(app: AppHandle, id: String) -> TracksResult {
    tauri::async_runtime::spawn_blocking(move || playlist_tracks(&app, &id))
        .await
        .unwrap_or_else(|e| {
            log::error!("spotify playlist task panicked: {e}");
            TracksResult::bare("offline")
        })
}

/// Public playlists matching `query` ("Playlists on Spotify").
#[tauri::command]
pub async fn spotify_search_playlists(
    app: AppHandle,
    query: String,
    limit: Option<u32>,
) -> PlaylistSearchResult {
    tauri::async_runtime::spawn_blocking(move || search_playlists(&app, &query, limit.unwrap_or(5)))
        .await
        .unwrap_or_else(|e| {
            log::error!("spotify playlist search task panicked: {e}");
            PlaylistSearchResult {
                status: "offline".into(),
                playlists: Vec::new(),
            }
        })
}

/// Play a playlist (or Liked Songs) as the new context, optionally from one
/// of its songs. See spotify::play_context for the statuses.
#[tauri::command]
pub async fn spotify_play_context(
    app: AppHandle,
    context_uri: String,
    offset_uri: Option<String>,
) -> String {
    tauri::async_runtime::spawn_blocking(move || {
        spotify::play_context(&app, &context_uri, offset_uri.as_deref()).to_string()
    })
    .await
    .unwrap_or_else(|e| {
        log::error!("spotify play context task panicked: {e}");
        "offline".into()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn playlist_reads_new_items_total_and_old_tracks_total() {
        let new = json!({"uri":"spotify:playlist:a","id":"a","name":"Late drives",
            "owner":{"id":"me","display_name":"Thien"},"items":{"total":53},"images":null});
        let p = parse_playlist(&new, "me").unwrap();
        assert_eq!(p.total, Some(53));
        assert!(p.mine);
        assert_eq!(p.owner, "Thien");
        let old = json!({"uri":"spotify:playlist:b","id":"b","name":"x",
            "owner":{"id":"andy"},"tracks":{"total":88}});
        let p = parse_playlist(&old, "me").unwrap();
        assert_eq!(p.total, Some(88));
        assert!(!p.mine);
        assert_eq!(p.owner, "andy");
    }

    #[test]
    fn collaborative_counts_as_mine_and_editorial_is_dropped() {
        let collab = json!({"uri":"spotify:playlist:c","id":"c","name":"x",
            "owner":{"id":"jun"},"collaborative":true});
        assert!(parse_playlist(&collab, "me").unwrap().mine);
        let editorial = json!({"uri":"spotify:playlist:d","id":"d","name":"Today's Top Hits",
            "owner":{"id":"spotify"}});
        assert_eq!(parse_playlist(&editorial, "me"), None);
        assert_eq!(parse_playlist(&serde_json::Value::Null, "me"), None);
    }

    #[test]
    fn recent_contexts_are_newest_first_and_unique() {
        let rows = vec![
            json!({"played_at":"2026-11-06T02:40:00Z","context":{"uri":"spotify:playlist:b"}}),
            json!({"played_at":"2026-11-06T02:50:00Z","context":{"uri":"spotify:playlist:a"}}),
            json!({"played_at":"2026-11-06T02:30:00Z","context":{"uri":"spotify:playlist:a"}}),
            json!({"played_at":"2026-11-06T02:20:00Z","context":null}),
        ];
        assert_eq!(
            contexts_newest_first(&rows),
            vec![
                "spotify:playlist:a".to_string(),
                "spotify:playlist:b".to_string()
            ]
        );
    }

    #[test]
    fn now_context_reads_context_and_item() {
        let v = json!({"context":{"uri":"spotify:playlist:a"},"item":{"uri":"spotify:track:1"}});
        assert_eq!(
            parse_now_context(&v),
            NowContext {
                context_uri: Some("spotify:playlist:a".into()),
                track_uri: Some("spotify:track:1".into()),
            }
        );
        let loose = json!({"context":null,"item":{"uri":"spotify:track:1"}});
        assert_eq!(parse_now_context(&loose).context_uri, None);
    }

    #[test]
    fn item_reads_new_field_then_old_and_skips_non_tracks() {
        let track = json!({"uri":"spotify:track:1","name":"Alone","artists":[{"name":"SISTAR"}],
            "album":{"name":"Alone","images":[]},"duration_ms":206000});
        let new = json!({"item": track.clone(), "track": true});
        assert_eq!(parse_item(&new).unwrap().title, "Alone");
        let old = json!({"track": track});
        assert_eq!(parse_item(&old).unwrap().artist, "SISTAR");
        let local = json!({"item":{"uri":"spotify:local:a:b:c:1","name":"x"}});
        assert!(parse_item(&local).is_none());
        assert!(parse_item(&json!({"item": null, "track": false})).is_none());
    }
}
