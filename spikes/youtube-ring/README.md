# youtube-ring

Hosted YouTube spike — a real WebView2 `youtube.com` inside a 12px drag ring.
Sibling of [`../web-surface`](../web-surface) (v1: decorated Netflix window,
dies with its parent). Not the DWM mirror. Not wired into Palette. Not a 1.0
merge.

Windows-only (WebView2). This crate is the instrument; the verdict is whether
the ring can host YouTube, drag by the ring only, and outlive the launcher.

## Run (Windows)

```sh
cd spikes/youtube-ring
cargo run
```

`cargo run` **detaches**: the cargo/launcher process exits, the ring stays
(v1's named fail). The surface is born 664×384, always-on-top, no stock
title bar. Clicks on the 640×360 inset hit YouTube. The 12px ring is the
only drag.

```sh
cargo run -- --attached   # in-process; dies with the terminal (v1 lifetime)
cargo run -- --probe      # hit-test + style leftovers → %APPDATA%\palette-youtube-ring-spike\probe.log
```

`--probe` can combine with `--attached`. Deleting
`%APPDATA%\palette-youtube-ring-spike\` is the full reset (profile + seat +
probe log). Seat file is `x y` only.

## Linux / this VM

`cargo test` and `cargo build` work (stub + snap math). `cargo run` exits 1
with the Windows command — it does not fake a WebView2 pass.

## Stamp (locked)

- 12px ring around 640×360 16:9. Outer 664×384. Ring is the only drag.
- Neutral shell (house surface), 1px fg/10 hairline, radius 12. No accent
  on the frame. Idle: shell + hairline + × at opacity 0. Reveal on
  `data-hot` (cursor in the RING) plus `:has(:focus-visible)` — opacity
  only, EASE.inOut / DUR 3 (200ms). The 12px HTCAPTION hit stays always.
  Never CSS `:hover`. Never `:focus-within`.
- Close: × in the ring top-right, 20px hit (L-clipped so it never sits
  on the video), same reveal as the shell.
- Persist x,y. One born size. Edge snap 24px per axis. Always-on-top.
- Presence never moves it. No corner magnet, no fsSeat, no reserved rail,
  no Palette transport, no URL bar, no stock title bar, no Win11 accent
  border, no chrome on the video.
