//! Pure per-axis edge snap. No corner magnet, no reserved margin rail.
//!
//! A drop within `rail` of an edge *line* settles that axis onto the line;
//! otherwise the coordinate stands. Lines are the work-area edges and the
//! full monitor edges (the flush-with-the-screen line that retired fsSeat).
//! Presence never calls this.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

pub fn snap_axis(v: i32, lines: &[i32], rail: i32) -> i32 {
    let mut best: Option<(i32, i32)> = None; // (distance, line)
    for &line in lines {
        let d = (v - line).abs();
        if d <= rail && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, line));
        }
    }
    best.map_or(v, |(_, line)| line)
}

pub fn clamp_origin(x: i32, y: i32, w: i32, h: i32, bounds: Rect) -> (i32, i32) {
    (
        x.clamp(bounds.x, (bounds.x + bounds.w - w).max(bounds.x)),
        y.clamp(bounds.y, (bounds.y + bounds.h - h).max(bounds.y)),
    )
}

/// Clamp onto the monitor, then snap each axis independently.
pub fn settle(x: i32, y: i32, w: i32, h: i32, mon: Rect, work: Rect, rail: i32) -> (i32, i32) {
    let (x, y) = clamp_origin(x, y, w, h, mon);
    let x = snap_axis(
        x,
        &[work.x, work.x + work.w - w, mon.x, mon.x + mon.w - w],
        rail,
    );
    let y = snap_axis(
        y,
        &[work.y, work.y + work.h - h, mon.y, mon.y + mon.h - h],
        rail,
    );
    (x, y)
}

/// 12px frame around a w×h client. Used by the hit-test probe.
pub fn in_ring(x: i32, y: i32, w: i32, h: i32, ring: i32) -> bool {
    x >= 0 && y >= 0 && x < w && y < h && (x < ring || y < ring || x >= w - ring || y >= h - ring)
}

/// Top-right 20px hit, clipped to the ring L so it never covers the video.
pub fn in_close_l(x: i32, y: i32, w: i32, _h: i32, ring: i32, hit: i32) -> bool {
    if hit <= 0 || ring <= 0 {
        return false;
    }
    if x < w - hit || x >= w || y < 0 || y >= hit {
        return false;
    }
    let local_x = x - (w - hit);
    y < ring || local_x >= hit - ring
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn near_left_snaps_x_holds_y() {
        let mon = Rect {
            x: 0,
            y: 0,
            w: 1920,
            h: 1080,
        };
        let work = Rect {
            x: 0,
            y: 0,
            w: 1920,
            h: 1040,
        };
        let (x, y) = settle(18, 400, 664, 384, mon, work, 24);
        assert_eq!(x, 0);
        assert_eq!(y, 400);
    }

    #[test]
    fn mid_screen_stands() {
        let mon = Rect {
            x: 0,
            y: 0,
            w: 1920,
            h: 1080,
        };
        assert_eq!(settle(800, 300, 664, 384, mon, mon, 24), (800, 300));
    }

    #[test]
    fn bottom_has_work_and_flush_lines() {
        let mon = Rect {
            x: 0,
            y: 0,
            w: 1920,
            h: 1080,
        };
        let work = Rect {
            x: 0,
            y: 0,
            w: 1920,
            h: 1040,
        };
        // Near the work-area bottom (above the taskbar).
        let y_work = 1040 - 384; // 656
        let (_, y) = settle(100, y_work + 10, 664, 384, mon, work, 24);
        assert_eq!(y, y_work);
        // Near the true screen bottom — the flush line, not fsSeat.
        let y_flush = 1080 - 384; // 696
        let (_, y) = settle(100, y_flush + 8, 664, 384, mon, work, 24);
        assert_eq!(y, y_flush);
    }

    #[test]
    fn no_corner_magnet() {
        let mon = Rect {
            x: 0,
            y: 0,
            w: 1920,
            h: 1080,
        };
        // Near left AND near top — each axis snaps; that is the edge rule,
        // not a magnet that yanks a near-corner drop into the corner from
        // further away than the rail.
        let (x, y) = settle(10, 10, 664, 384, mon, mon, 24);
        assert_eq!((x, y), (0, 0));
        let (x, y) = settle(10, 80, 664, 384, mon, mon, 24);
        assert_eq!((x, y), (0, 80));
    }

    #[test]
    fn close_l_stays_off_the_video() {
        let (w, h, ring, hit) = (664, 384, 12, 20);
        // Corner of the ring: yes.
        assert!(in_close_l(663, 0, w, h, ring, hit));
        assert!(in_close_l(650, 5, w, h, ring, hit));
        // 8×8 overlap of a naive 20×20 onto the webview: no.
        assert!(!in_close_l(644, 12, w, h, ring, hit));
        assert!(!in_close_l(650, 16, w, h, ring, hit));
        // Inset video: no.
        assert!(!in_ring(12, 12, w, h, ring));
        assert!(in_ring(6, 200, w, h, ring));
    }
}
