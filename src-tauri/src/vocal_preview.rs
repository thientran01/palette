//! Opt-in development-only vocal trial: native MDX separation
//! (`separation.rs`) before acoustic alignment. Never enabled in release builds.
use std::path::Path;

pub const CACHE_DIR: &str = "karaoke-vocals-preview-v4";
pub const RECIPE: &str = "mms-int8/mdx-kim2-4";
/// Earlier trial generations, newest first, with the recipe each wrote.
/// Still readable while the trial relearns; never deleted on mismatch.
pub const EARLIER_CACHES: [(&str, &str); 3] = [
    ("karaoke-vocals-preview-v3", "mms-int8/demucs-source-3"),
    ("karaoke-vocals-preview-v2", "mms-int8/demucs-entry-2"),
    ("karaoke-vocals-preview-v1", "mms-int8/demucs-preview-1"),
];
pub const EARLIER_CACHE_DIRS: [&str; 3] = [
    EARLIER_CACHES[0].0,
    EARLIER_CACHES[1].0,
    EARLIER_CACHES[2].0,
];

pub fn enabled() -> bool {
    cfg!(debug_assertions) && std::env::var("PALETTE_VOCAL_PREVIEW").is_ok_and(|v| v == "1")
}

fn cache_is_preview(path: &Path) -> bool {
    path.parent()
        .and_then(Path::file_name)
        .is_some_and(|n| n == CACHE_DIR)
}

pub fn cache_recipe<'a>(path: &Path, baseline: &'a str) -> &'a str {
    let dir = path.parent().and_then(Path::file_name);
    if cache_is_preview(path) {
        return RECIPE;
    }
    EARLIER_CACHES
        .iter()
        .find(|(name, _)| dir.is_some_and(|d| d == *name))
        .map_or(baseline, |(_, recipe)| recipe)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cache_identity_is_separate_from_original() {
        assert_eq!(
            cache_recipe(Path::new("root/karaoke/song.json"), "original"),
            "original"
        );
        assert_eq!(
            cache_recipe(
                &Path::new("root").join(CACHE_DIR).join("song.json"),
                "original"
            ),
            RECIPE
        );
    }
    #[test]
    fn earlier_generations_keep_their_recipes() {
        for (dir, recipe) in EARLIER_CACHES {
            assert_eq!(
                cache_recipe(&Path::new("root").join(dir).join("song.json"), "original"),
                recipe
            );
        }
    }
}
