# Mirror spike matrix — DWM thumbnail live-clone

What `DwmRegisterThumbnail` actually does when the destination is a small
always-on-top tao window cloning an already-open YouTube browser tab — the
source of truth for the Surfaces **MIRROR** tier (not the hosted WebView2
path; that is `spikes/web-surface`).

Instrument: [`spikes/mirror-spike`](../spikes/mirror-spike). Standalone
`cargo run` crate, own `[workspace]`, not part of `src-tauri/`. Windows-only
(MSVC). **No cell below is a live Windows measurement from this PR** — the
authoring agent is on Linux and cannot call DWM. Prior-art / MSDN cells are
filled; every Windows-live cell is a checkbox for Thien.

Feel target (not a pixel spec, do not restyle Palette): a compact floating
clone over YouTube theater and over a fullscreen game (Roblox / Frontlines).
This spike is a bare native window + a DWM thumbnail. No Palette chrome, no
GSMTC transport, no input forwarding.

```
cd spikes/mirror-spike
cargo run                  # pick / auto YouTube
cargo run -- list
cargo run -- youtube
cargo run -- --crop youtube
cargo run -- --layered youtube
```

Started 2026-08-29. Behavior is OS- and GPU-dependent — re-run after major
Windows updates and after a browser GPU-pipeline change.

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
| Thumbnail is composited **at presentation time**, not blitted into the dest DC | Greg Schechter / DWM API notes; [SO 2196268](https://stackoverflow.com/questions/2196268/is-it-possible-to-capture-a-window-with-windows-7-dwm-thumbnail-in-it) | We cannot screenshot the thumb. Overlays painted in our DC sit **under** it. Empty-state = hide the thumb (`fVisible=false`) then paint. |
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

- [ ] **Borderless / windowed-fullscreen Roblox (or Frontlines)** — dest
      stays visible and keeps compositing over the game. This is the
      screenshot case. Exclusive-fullscreen D3D is a different row.
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

### 6. Click = raise source (view-only)

- [ ] **Left-click the dest** while the source is in the background. Source
      is restored if minimized and becomes foreground. The crate prints
      `click → raise source … foreground_now=…`. No clicks are synthesized
      into the YouTube client area.
- [ ] **A game is foreground, then click the dest, then click again** — does
      the second click still raise Chrome, or does the game eat the
      foreground lock? Stamp it; don't add `AttachThreadInput` unless a
      finding says the simple path is dead.

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

*None yet — Linux authoring run. Stamp below, dated, like the presence
matrix ("measured 2026-07-09").*

1. —
2. —

## How to fill the live cells

On the Windows machine (MSVC toolchain, same as `npm run tauri dev`):

```
cd spikes/mirror-spike
cargo run -- list                 # picker sanity
cargo run -- youtube              # opaque path — leave it up
# play a YouTube video; look at the dest
# alt-tab a game under it; click the dest; minimize Chrome; restore
cargo run -- --crop youtube       # one-look crop verdict
cargo run -- --layered youtube    # does the thumb survive WS_EX_LAYERED?
```

Tick the boxes. Paste any `DwmRegisterThumbnail` / `exstyle` lines into
Findings. Deleting `%APPDATA%\palette-mirror-spike\` resets the seat.
