# Mirror spike matrix — DWM thumbnail live-clone

What `DwmRegisterThumbnail` actually does when the destination is a small
always-on-top tao window cloning an already-open YouTube browser tab — the
source of truth for the Surfaces **MIRROR** tier (not the hosted WebView2
path; that is `spikes/web-surface`).

Instrument: [`spikes/mirror-spike`](../spikes/mirror-spike). Standalone
`cargo run` crate, own `[workspace]`, not part of `src-tauri/`. Windows-only
(MSVC). Authoring agent is on Linux. **One live stamp so far:** Thien,
2026-08-29 — opaque dest + DWM clone over Roblox ("it works now"). Frameless
+ GSMTC rows below are new and unchecked.

Feel target (not a pixel spec, do not restyle Palette): a compact floating
clone over YouTube theater and over a fullscreen game (Roblox / Frontlines).
This spike is a bare native window + a DWM thumbnail + a thin strip of GSMTC
transport. No Palette chrome, no input forwarding.

```
cd spikes/mirror-spike
cargo run                  # pick / auto YouTube
cargo run -- list
cargo run -- youtube
cargo run -- --crop youtube
cargo run -- --layered youtube
```

Started 2026-08-29. Pull + rebuild after the frameless/GSMTC slice.
Behavior is OS- and GPU-dependent — re-run after major Windows updates
and after a browser GPU-pipeline change.

## Hypothesis (non-binding)

Opaque tao window + `DwmRegisterThumbnail` is enough to clone a YouTube
Chrome window over a borderless-fullscreen game. Transparent / `WS_EX_LAYERED`
is the thing that may fail (the known Tauri/tao risk). If DWM refuses the dest
at the API level, the next spike is `Windows.Graphics.Capture` — do not
implement it here unless this crate can *prove* DWM is impossible.

## API facts (MSDN / composition model — not a live run)

Filled from the DWM thumbnail docs and the composition-engine writeups. These
are constraints the crate is written against; they are not "DWM works on this
machine."

| Fact | Source | Implication for this spike |
|---|---|---|
| Dest and source must be **top-level** HWNDs; a child dest returns `E_INVALIDARG` | [DwmRegisterThumbnail](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/nf-dwmapi-dwmregisterthumbnail) | tao's window HWND is the dest. No child/WebView dest. |
| Dest must be **this process** (or the desktop HWND) | same remarks | We never register into someone else's window. |
| `DwmRegisterThumbnail` does **not** draw; nothing appears until `DwmUpdateThumbnailProperties` | [thumbnail overview](https://learn.microsoft.com/en-us/windows/win32/dwm/thumbnail-ovw) | Spike always updates dest rect + `fVisible` after register and on resize. |
| Thumbnail is composited **at presentation time**, not blitted into the dest DC | Greg Schechter / DWM API notes; [SO 2196268](https://stackoverflow.com/questions/2196268/is-it-possible-to-capture-a-window-with-windows-7-dwm-thumbnail-in-it) | We cannot screenshot the thumb. Overlays painted in our DC sit **under** it. **The strip is outside `rcDestination`** — that is the only way transport stays visible. Empty-state = hide the thumb (`fVisible=false`) then paint the video pane. |
| `fSourceClientAreaOnly` drops the Win32 non-client frame only | [DWM_THUMBNAIL_PROPERTIES](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ns-dwmapi-dwm_thumbnail_properties) | Default path uses this. Browser tab/toolbar still show. `--crop` adds a ~88 DIP top `rcSource` inset — a guess, not a video-element finder. |
| Minimized sources stop being composited; the thumb freezes on the last frame | OnTopReplica [#71](https://github.com/LorenzCK/OnTopReplica/issues/71) (wontfix, 2018); same as the taskbar preview | Honest frozen/empty state. Do not fake video. Click-to-raise restores the source. |
| `UpdateLayeredWindow` dests have no DWM composition surface for thumbs | Alt-Tabby / community (ULW bypasses normal composition) | `--layered` uses `WS_EX_LAYERED` + `SetLayeredWindowAttributes` (SLWA), which is the *possible* layered path. ULW is not this experiment. |
| Thumb handle is process-local; unregister in the same process | MSDN remarks | Close / source-died paths call `DwmUnregisterThumbnail`. |

## Questions this spike exists to answer

Windows-live cells stay unchecked until a real run stamps them. Each row names
the exact command.

### 1. Opaque dest — does the thumb even appear?

- [ ] **Chrome + YouTube, windowed / theater.** `cargo run -- youtube` with a
      YouTube tab already open in Chrome. Dest is the default opaque tao
      window (~560×315, always-on-top, native decorations, remembered seat).
      Expect: live clone of that HWND, including the page (player + YouTube
      chrome), not a still.
- [ ] **Edge + YouTube.** Same command; Edge is `Chrome_WidgetWin_*` /
      `msedge.exe` and should enumerate the same way.
- [ ] **Brave + YouTube.** Same.
- [ ] **Firefox + YouTube.** Class `MozillaWindowClass`. Picker treats it as
      a browser. Does DWM clone it as live as Chromium?
- [ ] **`DwmRegisterThumbnail` HRESULT is S_OK** on the opaque dest (the
      crate prints `DwmRegisterThumbnail ok`). If this fails, copy the line
      into a finding — that is the "DWM is impossible" gate, not a shrug.

### 2. Is it actually live video?

- [ ] **Playing YouTube motion** (a music video, not a still thumbnail)
      updates in the dest in something that feels like "watchable," not a
      1 Hz slideshow. DWM thumbs are composition-live on paper; GPU overlays
      have historically dropped out of thumbs on some pipelines.
- [ ] **Audio** stays in the *source* browser (DWM is pixels only). Confirm
      this is fine for the Roblox case — you hear YouTube from Chrome, you
      watch the clone. Not a bug if the dest is silent.

### 3. Always-on-top over a game (the load-bearing case)

- [x] **Borderless / windowed-fullscreen Roblox (or Frontlines)** —
      **measured 2026-08-29 (Thien):** "Holy shit it works now." Shot:
      YouTube livestream clone, stock Windows title bar, bottom-left over
      Roblox (Opera GX visible in the cloned chrome). Opaque dest + DWM
      is real. The remaining gimmick was the dest itself (title bar +
      whole-window click-to-raise) — that is the frameless/GSMTC slice.
- [ ] **Exclusive fullscreen D3D game** (if one is handy). Prior art: many
      always-on-top windows lose to exclusive fullscreen. Stamp what
      actually happens; do not "fix" it in this crate.

### 4. Layered / transparent dest (second experiment — documented, not ship)

```
cargo run -- --layered youtube
```

The crate ORs `WS_EX_LAYERED` onto the dest and calls
`SetLayeredWindowAttributes(..., alpha=230, LWA_ALPHA)`, then prints the
exstyle before/after. Watch the dest with your eyes.

- [ ] **`DwmRegisterThumbnail` still returns S_OK** on the layered dest.
- [ ] **The thumbnail draws** (live / frozen / blank / tinted-with-alpha).
      Write what you see. This is THE known risk for a future Tauri
      transparent dest.
- [ ] If it fails: opaque remains the Surfaces MIRROR path. Do not chase a
      layered dest in this crate.

### 5. Source lifecycle

- [ ] **Minimize the YouTube window.** Dest title becomes
      `mirror — frozen (source minimized)` and the thumb is hidden (honest
      empty, not a fake player). OnTopReplica says the last frame would
      otherwise sit there forever.
- [ ] **Restore the YouTube window** (or click the mirror). Thumb comes back
      live.
- [ ] **Close the YouTube window.** Dest stays up, empty, titled
      `mirror — source closed`. No crash, no re-register onto a random HWND.

### 6. Raise source (view-only) — superseded path

Whole-window click-to-raise was the gimmick on the 2026-08-29 Roblox run
("i have to fullscreen into the video and then alt tab"). That path is
gone. Raise is **only** the strip `open` hit or a double-click on the
video pane (same `raise_source`). Play/pause must not raise.

- [ ] **`open` / video-pane double-click** raises the source. Crate prints
      `open → raise source …` / `double-click video → raise source`.
- [ ] **Play/pause does not raise** Firefox/Chrome/Opera. Game can stay
      under the dest. Confirm the console says `smtc cmd accepted=… (no raise)`
      and the source window does not come to the front.

### 7. Crop / resize (nice-to-have, not a research project)

- [ ] **Resize the dest** — thumb dest rect follows the client area. No
      stretch-to-wrong-letterbox so bad you can't watch. (Some stretch is
      fine; we do not letterbox.)
- [ ] **`--crop`** (`cargo run -- --crop youtube`) insets ~88 DIP from the
      top of the client (tabs + address bar at 96 DPI, scaled by
      `GetDpiForWindow`). Usable, too much, or too little? One sentence.
      Do not chase a perfect video-only crop in this crate.

### 8. Picker

```
cargo run -- list
```

- [ ] **List** shows the YouTube Chrome/Edge/Brave window, tagged
      `[youtube]`, and skips tool windows / empty titles / cloaked /
      `Progman`/`WorkerW`/`Shell_TrayWnd`.
- [ ] **One YouTube window + bare `cargo run`** auto-picks it.
- [ ] **Several YouTube windows + bare `cargo run`** prints the list and
      waits for an index (TTY).

### 9. Frameless dest + strip (2026-08-29 slice 2)

```
cd spikes/mirror-spike
git pull
cargo run -- youtube
```

- [ ] **No native title bar.** Dest is frameless; the only chrome is the
      bottom strip (`::` / prev / play / next / open / x).
- [ ] **Thumb vs strip z-order.** The DWM clone fills the video pane and
      **does not cover** the strip. If the thumb paints over the buttons,
      `rcDestination` leaked — that is a bug, not a "hope."
- [ ] **Strip-drag** moves the dest over Roblox without raising the source.
      Video-pane single click does nothing (no raise, no drag).
- [ ] **Frameless over Roblox.** Same load-bearing case as the 2026-08-29
      shot, without the stock title bar. Can you keep playing and still
      see / move the clone?

### 10. GSMTC transport (does not raise the source)

The crate binds a session by score (YouTube title + browser AUMID matching
the cloned exe). **Spotify is scored −100** so Windows' "current" session
does not steal the buttons. Console prints every session + the bind line.

- [ ] **Bind hits the cloned browser**, not Spotify, when both are open.
      Look for `smtc bound: AUMID=… title=…` after `smtc sessions:`.
      YouTube-in-Firefox / Opera GX / Chrome should win.
- [ ] **Play/pause** toggles the livestream without alt-tabbing to the
      browser. Audio stays in the source. Buttons disable honestly when
      there is no session (`smtc: no session — transport disabled`).
- [ ] **Next/prev** only fire when the session says they are enabled
      (livestreams often disable them — muted label, click prints
      `next disabled` / `prev disabled`).
- [ ] **Seek is not faked.** `can_seek=` is printed; there is no seek
      scrubber on the strip. If a later slice adds one, it must honor
      `IsPlaybackPositionEnabled`.

## Fallback (only if DWM is impossible)

If `DwmRegisterThumbnail` fails on an opaque, top-level, same-process tao
HWND — i.e. a code-level refusal you can quote, not "I didn't like the
crop" — the next spike is **`Windows.Graphics.Capture`**
(`GraphicsCaptureItem` from a window, frame pool → dest). That API has its
own costs (capture permission, GPU copy, still fails on some minimized /
protected paths). **Do not implement it in `mirror-spike`.**

DRM / HW-protected video (Netflix) is out of scope. YouTube is the target;
a black thumb on a protected surface would be a finding, not a fix.

## Findings (fill on the live run)

1. **Opaque dest over Roblox works** (Thien, 2026-08-29): "Holy shit it
   works now. its just really gimicky because i can't control the mini
   window itself. i have to fullscreen into the video and then alt tab to
   roblox. the top chrome is bothering too." Shot: YouTube livestream
   clone, stock title bar, bottom-left over Roblox. That finding closed
   the "does DWM even draw" question and opened slice 2 (frameless +
   GSMTC, this commit).
2. —

## How to fill the live cells

On the Windows machine (MSVC toolchain, same as `npm run tauri dev`):

```
cd spikes/mirror-spike
git pull && cargo run -- youtube  # frameless + strip + GSMTC
# Roblox under it; strip-drag; play/pause; confirm Firefox/Opera not raised
# leave Spotify playing in the background — bind line must not be Spotify
cargo run -- --crop youtube       # one-look crop verdict
cargo run -- --layered youtube    # does the thumb survive WS_EX_LAYERED?
```

Tick the boxes. Paste any `DwmRegisterThumbnail` / `exstyle` lines into
Findings. Deleting `%APPDATA%\palette-mirror-spike\` resets the seat.
