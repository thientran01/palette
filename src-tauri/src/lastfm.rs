/*
 * Last.fm track.getSimilar — the brain behind more-like-this. Spotify's own
 * /recommendations is gated for apps registered after Nov 2024, so similarity
 * comes from Last.fm's listening graph instead.
 *
 * The API key is personal and lives in app_data/settings.json under
 * "lastfm_api_key" — the repo is public, so it must NEVER become a const.
 *
 * Spike-verified 2026-07-11 against real history (13 tracks off
 * history.jsonl): 12/13 usable, Korean catalog 9/10 — even deep cuts (XLOV,
 * tripleS) return full lists. The thin spot is RECENCY (a fresh release
 * returns 0 similars), not language, so `no_data` is a designed answer, not
 * an error. Two graph shapes the consumer must handle (similar.rs does):
 * the top of every list is "more by this artist" (match 1.0–0.9), and below
 * ~0.05 the tail degenerates into unrelated filler.
 */
use std::time::Duration;

use crate::settings::{json_capped, JSON_CAP};
use crate::spotify::urlenc;

const TIMEOUT: Duration = Duration::from_secs(15);
const UA: &str = "Palette/0.1 (https://github.com/thientran01/palette)";

pub struct SimilarTrack {
    pub title: String,
    pub artist: String,
    /// Last.fm's 0..1 similarity score.
    pub score: f64,
}

/// Cheap key validation for the prefs "Test key" button: a minimal request
/// (chart.getTopArtists, limit 1) whose only failure mode we care about is a
/// rejected key. Returns "ok" | "invalid" (Last.fm error 10/26) | "offline"
/// (transport/5xx/garbled). An empty key is "invalid" without a round-trip.
pub fn validate_key(key: &str) -> &'static str {
    if key.trim().is_empty() {
        return "invalid";
    }
    let url = format!(
        "https://ws.audioscrobbler.com/2.0/?method=chart.gettopartists&limit=1&api_key={}&format=json",
        urlenc(key),
    );
    let resp = ureq::get(&url)
        .set("User-Agent", UA)
        .timeout(TIMEOUT)
        .call();
    let v: serde_json::Value = match resp {
        Ok(r) => match json_capped(r, JSON_CAP) {
            Ok(v) => v,
            Err(_) => return "offline",
        },
        // Last.fm serves its application errors as 4xx WITH a JSON body — read
        // it so a rejected key reports as invalid, not offline.
        Err(ureq::Error::Status(_, r)) => match json_capped(r, JSON_CAP) {
            Ok(v) => v,
            Err(_) => return "offline",
        },
        Err(_) => return "offline",
    };
    if let Some(code) = v["error"].as_i64() {
        // 10 = invalid key, 26 = suspended key. Anything else (rate limit,
        // temporary service error) is not a verdict on the key itself.
        return match code {
            10 | 26 => "invalid",
            _ => "offline",
        };
    }
    "ok"
}

/// One Last.fm GET with the shared error mapping. Statuses: "bad_key" (key
/// rejected), "no_data" (Last.fm doesn't know the track/artist), "offline"
/// (transport/5xx/rate limit).
fn call(api_key: &str, params: &str) -> Result<serde_json::Value, &'static str> {
    let url = format!(
        "https://ws.audioscrobbler.com/2.0/?{params}&autocorrect=1&api_key={}&format=json",
        urlenc(api_key),
    );
    let resp = ureq::get(&url)
        .set("User-Agent", UA)
        .timeout(TIMEOUT)
        .call();
    let v: serde_json::Value = match resp {
        Ok(r) => json_capped(r, JSON_CAP).map_err(|_| "offline")?,
        // Last.fm serves its application errors as 4xx WITH a JSON body —
        // read it so an invalid key reports as bad_key, not offline.
        Err(ureq::Error::Status(_, r)) => json_capped(r, JSON_CAP).map_err(|_| "offline")?,
        Err(_) => return Err("offline"),
    };
    if let Some(code) = v["error"].as_i64() {
        // 10 = invalid key, 26 = suspended key; 6 = track/artist not found.
        // bad_key ≠ no_key: "add a key" is the wrong instruction for a key
        // that exists and was rejected.
        return Err(match code {
            10 | 26 => "bad_key",
            6 => "no_data",
            _ => "offline",
        });
    }
    Ok(v)
}

/// The entries of a `{outer: {inner: [...]}}` list. A single result
/// serializes as an object, not a 1-array.
fn entries(v: &serde_json::Value, outer: &str, inner: &str) -> Vec<serde_json::Value> {
    let node = &v[outer][inner];
    match node.as_array() {
        Some(a) => a.clone(),
        None if node.is_object() => vec![node.clone()],
        None => Vec::new(),
    }
}

/// Number or numeric string, per the API's mood. An absent/unparseable match
/// collapses to 0.0 and dies at the consumer's floor — deliberate: an unscored
/// similar is indistinguishable from mush, and dropping it is the safe read.
fn score(t: &serde_json::Value) -> f64 {
    t["match"]
        .as_f64()
        .or_else(|| t["match"].as_str().and_then(|s| s.parse().ok()))
        .unwrap_or(0.0)
}

fn parse_similar_tracks(v: &serde_json::Value) -> Vec<SimilarTrack> {
    entries(v, "similartracks", "track")
        .iter()
        .filter_map(|t| {
            Some(SimilarTrack {
                title: t["name"].as_str()?.to_string(),
                artist: t["artist"]["name"].as_str()?.to_string(),
                score: score(t),
            })
        })
        .collect()
}

/// (artist name, match score), best first.
fn parse_similar_artists(v: &serde_json::Value) -> Vec<(String, f64)> {
    entries(v, "similarartists", "artist")
        .iter()
        .filter_map(|a| Some((a["name"].as_str()?.to_string(), score(a))))
        .collect()
}

fn parse_top_titles(v: &serde_json::Value) -> Vec<String> {
    entries(v, "toptracks", "track")
        .iter()
        .filter_map(|t| t["name"].as_str().map(str::to_string))
        .collect()
}

/// track.getSimilar alone. Statuses as `call`; an empty Ok list also means
/// no_data — the caller collapses them.
pub fn get_similar(
    api_key: &str,
    title: &str,
    artist: &str,
    limit: u32,
) -> Result<Vec<SimilarTrack>, &'static str> {
    let v = call(
        api_key,
        &format!(
            "method=track.getSimilar&artist={}&track={}&limit={limit}",
            urlenc(artist),
            urlenc(title),
        ),
    )?;
    Ok(parse_similar_tracks(&v))
}

/// Similar artists the artist fallback walks, and top tracks taken from each.
/// Every artist costs one extra Last.fm call, so this bounds the fallback at
/// 1 + FALLBACK_ARTISTS calls per seed.
const FALLBACK_ARTISTS: usize = 4;
const FALLBACK_TOP_TRACKS: u32 = 3;

/*
 * Track similars, falling back to similar ARTISTS' top tracks when Last.fm's
 * track graph comes back empty. In Oct 2026 track.getSimilar started
 * answering an accepted key with zero similars for every seed tried (the
 * discovery log showed 0 similars across four seeds, run after run), which
 * left both "Something different" and more-like-this with nothing to offer.
 * artist.getSimilar is the coarser graph, but it's still Last.fm's
 * listening data, and genre discovery is the point anyway.
 *
 * The fallback list is interleaved by rank (every artist's #1 track, then
 * every #2, ...), so a consumer walking it front to back meets different
 * artists first. The score is the ARTIST's match, so the consumer's match
 * floor still trims the mush tail.
 */
pub fn similar_or_artist_fallback(
    api_key: &str,
    title: &str,
    artist: &str,
    limit: u32,
) -> Result<Vec<SimilarTrack>, &'static str> {
    match get_similar(api_key, title, artist, limit) {
        Ok(v) if !v.is_empty() => return Ok(v),
        Ok(_) | Err("no_data") => {}
        Err(e) => return Err(e),
    }
    let v = call(
        api_key,
        &format!(
            "method=artist.getSimilar&artist={}&limit=10",
            urlenc(artist)
        ),
    )?;
    let seed = artist.trim().to_lowercase();
    let artists: Vec<(String, f64)> = parse_similar_artists(&v)
        .into_iter()
        .filter(|(name, _)| name.trim().to_lowercase() != seed)
        .take(FALLBACK_ARTISTS)
        .collect();
    let mut per_artist: Vec<(String, f64, Vec<String>)> = Vec::new();
    for (name, match_score) in artists {
        match call(
            api_key,
            &format!(
                "method=artist.getTopTracks&artist={}&limit={FALLBACK_TOP_TRACKS}",
                urlenc(&name),
            ),
        ) {
            Ok(v) => per_artist.push((name, match_score, parse_top_titles(&v))),
            // A dead connection would stack 15s timeouts across the rest.
            Err("offline") => break,
            Err(_) => continue,
        }
    }
    let out = interleave(per_artist);
    log::info!(
        "lastfm: no track similars, artist fallback gave {}",
        out.len()
    );
    Ok(out)
}

/// Rank-interleave each artist's top titles into one candidate list.
fn interleave(per_artist: Vec<(String, f64, Vec<String>)>) -> Vec<SimilarTrack> {
    let depth = per_artist.iter().map(|a| a.2.len()).max().unwrap_or(0);
    let mut out = Vec::new();
    for rank in 0..depth {
        for (name, match_score, titles) in &per_artist {
            if let Some(title) = titles.get(rank) {
                out.push(SimilarTrack {
                    title: title.clone(),
                    artist: name.clone(),
                    score: *match_score,
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn similar_tracks_parse_array_object_and_string_match() {
        let v = json!({"similartracks": {"track": [
            {"name": "A", "artist": {"name": "X"}, "match": 0.5},
            {"name": "B", "artist": {"name": "Y"}, "match": "0.25"},
            {"name": "C"}
        ]}});
        let got = parse_similar_tracks(&v);
        assert_eq!(got.len(), 2);
        assert_eq!((got[0].title.as_str(), got[0].score), ("A", 0.5));
        assert_eq!((got[1].artist.as_str(), got[1].score), ("Y", 0.25));

        let one = json!({"similartracks": {"track": {"name": "A", "artist": {"name": "X"}}}});
        assert_eq!(parse_similar_tracks(&one).len(), 1);
        let empty = json!({"similartracks": {"track": [], "@attr": {"artist": "X"}}});
        assert!(parse_similar_tracks(&empty).is_empty());
    }

    #[test]
    fn similar_artists_and_top_titles_parse() {
        let v = json!({"similarartists": {"artist": [
            {"name": "X", "match": "1"},
            {"name": "Y", "match": "0.4"}
        ]}});
        assert_eq!(
            parse_similar_artists(&v),
            vec![("X".to_string(), 1.0), ("Y".to_string(), 0.4)]
        );
        let t = json!({"toptracks": {"track": [{"name": "T1"}, {"name": "T2"}]}});
        assert_eq!(parse_top_titles(&t), vec!["T1", "T2"]);
    }

    #[test]
    fn interleave_walks_rank_first() {
        let got = interleave(vec![
            ("X".into(), 0.9, vec!["x1".into(), "x2".into()]),
            ("Y".into(), 0.5, vec!["y1".into()]),
        ]);
        let order: Vec<_> = got
            .iter()
            .map(|t| (t.artist.as_str(), t.title.as_str()))
            .collect();
        assert_eq!(order, vec![("X", "x1"), ("Y", "y1"), ("X", "x2")]);
        assert_eq!(got[1].score, 0.5);
    }
}
