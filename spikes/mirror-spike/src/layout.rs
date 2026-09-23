//! Dest geometry: video pane + a strip the DWM thumb is not allowed to cover.
//!
//! DWM thumbnails composite OVER the dest DC at presentation time. Anything
//! painted under `rcDestination` is invisible. The strip lives outside that
//! rect; hit-testing uses the same numbers as `DwmUpdateThumbnailProperties`.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Video,
    Drag,
    Prev,
    Play,
    Next,
    Open,
    Close,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn contains(self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }

    pub fn right(self) -> i32 {
        self.x + self.w
    }

    pub fn bottom(self) -> i32 {
        self.y + self.h
    }
}

/// Logical DIP height of the transport strip. Scaled by dest DPI at runtime.
#[cfg_attr(not(windows), allow(dead_code))]
pub const STRIP_DIP: i32 = 36;

const GRIP: i32 = 28;
const BTN: i32 = 36;
const PLAY: i32 = 40;
const OPEN: i32 = 44;

#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub video: Rect,
    pub strip: Rect,
    pub prev: Rect,
    pub play: Rect,
    pub next: Rect,
    pub open: Rect,
    pub close: Rect,
}

impl Layout {
    pub fn new(w: i32, h: i32, strip_h: i32) -> Self {
        let w = w.max(1);
        let h = h.max(1);
        let strip_h = strip_h.clamp(20, h.max(20));
        let video_h = (h - strip_h).max(0);
        let video = Rect {
            x: 0,
            y: 0,
            w,
            h: video_h,
        };
        let strip = Rect {
            x: 0,
            y: video_h,
            w,
            h: strip_h,
        };
        let y = strip.y;
        let mut x = GRIP.min(w);
        let prev = Rect {
            x,
            y,
            w: BTN,
            h: strip_h,
        };
        x += BTN;
        let play = Rect {
            x,
            y,
            w: PLAY,
            h: strip_h,
        };
        x += PLAY;
        let next = Rect {
            x,
            y,
            w: BTN,
            h: strip_h,
        };
        x += BTN;
        let open = Rect {
            x,
            y,
            w: OPEN,
            h: strip_h,
        };
        let close = Rect {
            x: (w - BTN).max(0),
            y,
            w: BTN,
            h: strip_h,
        };
        Self {
            video,
            strip,
            prev,
            play,
            next,
            open,
            close,
        }
    }

    pub fn hit(self, x: i32, y: i32) -> Hit {
        if self.video.contains(x, y) {
            return Hit::Video;
        }
        if !self.strip.contains(x, y) {
            return Hit::Drag;
        }
        // Close wins if a narrow window overlaps the cluster.
        if self.close.contains(x, y) {
            return Hit::Close;
        }
        if self.prev.contains(x, y) {
            return Hit::Prev;
        }
        if self.play.contains(x, y) {
            return Hit::Play;
        }
        if self.next.contains(x, y) {
            return Hit::Next;
        }
        if self.open.contains(x, y) && self.open.right() <= self.close.x {
            return Hit::Open;
        }
        Hit::Drag
    }
}

/// Score a GSMTC session against the cloned window. Spotify loses on purpose:
/// Windows' "current" session is whoever last played, and Thien had both
/// YouTube-in-browser and Spotify open.
pub fn smtc_score(aumid: &str, media_title: &str, src_exe: &str, src_title: &str) -> i32 {
    let a = aumid.to_ascii_lowercase();
    if a.contains("spotify") {
        return -100;
    }
    let mut score = 0;
    if is_youtube_ish(media_title) {
        score += 50;
    }
    if is_youtube_ish(src_title) && is_browser_aumid(&a) {
        score += 20;
    }
    if aumid_matches_exe(&a, src_exe) {
        score += 30;
    }
    if is_browser_aumid(&a) {
        score += 10;
    }
    let src_l = src_title.to_ascii_lowercase();
    let media_l = media_title.to_ascii_lowercase();
    if !media_l.is_empty() && src_l.contains(&media_l.chars().take(16).collect::<String>()) {
        score += 15;
    }
    score
}

pub fn is_browser_aumid(aumid: &str) -> bool {
    let a = aumid.to_ascii_lowercase();
    [
        "firefox", "chrome", "msedge", "edge", "brave", "opera", "chromium", "vivaldi",
    ]
    .iter()
    .any(|s| a.contains(s))
}

pub fn aumid_matches_exe(aumid: &str, exe: &str) -> bool {
    let a = aumid.to_ascii_lowercase();
    let stem = exe
        .to_ascii_lowercase()
        .trim_end_matches(".exe")
        .to_string();
    if stem == "msedge" {
        return a.contains("edge") || a.contains("msedge");
    }
    !stem.is_empty() && a.contains(&stem)
}

fn is_youtube_ish(s: &str) -> bool {
    s.to_ascii_lowercase().contains("youtube")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumb_rect_leaves_the_strip() {
        let l = Layout::new(560, 351, 36);
        assert_eq!(
            l.video,
            Rect {
                x: 0,
                y: 0,
                w: 560,
                h: 315
            }
        );
        assert_eq!(l.strip.y, 315);
        assert_eq!(l.strip.h, 36);
        assert!(l.video.bottom() == l.strip.y);
    }

    #[test]
    fn hits() {
        let l = Layout::new(560, 351, 36);
        assert_eq!(l.hit(10, 10), Hit::Video);
        assert_eq!(l.hit(8, 330), Hit::Drag);
        assert_eq!(l.hit(l.prev.x + 2, 330), Hit::Prev);
        assert_eq!(l.hit(l.play.x + 2, 330), Hit::Play);
        assert_eq!(l.hit(l.next.x + 2, 330), Hit::Next);
        assert_eq!(l.hit(l.open.x + 2, 330), Hit::Open);
        assert_eq!(l.hit(550, 330), Hit::Close);
        assert_eq!(l.hit(400, 330), Hit::Drag);
    }

    #[test]
    fn spotify_loses_to_firefox_youtube() {
        let yt = smtc_score(
            "Mozilla.Firefox",
            "MASSIVE Kingdom - YouTube",
            "firefox.exe",
            "MASSIVE Kingdom - YouTube — Mozilla Firefox",
        );
        let sp = smtc_score(
            "SpotifyAB.SpotifyMusic_zpdnekdrzrea0",
            "Some Song",
            "firefox.exe",
            "MASSIVE Kingdom - YouTube — Mozilla Firefox",
        );
        assert!(yt > 0);
        assert!(sp < 0);
        assert!(yt > sp);
    }
}
