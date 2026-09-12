//! Hosted YouTube ring — a throwaway spike, not a Palette window.
//!
//! Hypothesis: frameless tao + 12px native/hit-test ring around a wry
//! WebView2 inset; close is a ring-only overlay; seat file like v1; process
//! detach so closing the launcher does not kill the surface.
//!
//! Windows-only (WebView2). `cargo run` from this crate. See README.md.

#[cfg_attr(not(windows), allow(dead_code))]
mod snap;

#[cfg(windows)]
mod surface;

fn main() {
    #[cfg(windows)]
    surface::run();

    #[cfg(not(windows))]
    {
        eprintln!(
            "youtube-ring is a Windows WebView2 spike.\n\
             This host cannot live-run it (no WebView2).\n\
             On Windows, from this crate:\n\
             \n\
               cargo run\n\
             \n\
             That detaches a 664×384 frameless ring hosting youtube.com.\n\
               cargo run -- --attached   # die with the terminal (v1 lifetime)\n\
               cargo run -- --probe      # hit-test / style leftovers → %APPDATA%\\palette-youtube-ring-spike\\probe.log"
        );
        std::process::exit(1);
    }
}
