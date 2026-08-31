# Mode ladder

The widget steps pill → card → expanded from one anchored bracket cluster. Each step changes the visible footprint and what is on screen; focus is a separate fullscreen room, not a fourth `pulse.mode`.

## Sub-features

- `mode-card` is the baseline garment: art, title, artist, seekable transport.
- `mode-expand-lyrics` grows the shell into the lyrics home.
- `mode-collapse-card` returns expanded to card without losing the track.
- `mode-collapse-pill` shrinks card to the glance pill (art · title · artist · time).
- `mode-expand-card` grows the pill back to card.
- `mode-nothing` shows the resting empty state when nothing is playing.
- `mode-focus-mock` opens the focus room at `/?window=focus`.

## How to get to it (user POV)

- Choose `Expand to lyrics`, `Expand to card`, `Collapse to card`, or `Collapse to pill` on the widget (hover or Tab to reveal the cluster).
- Choose `Expand to focus` on expanded — native only; the mock no-ops.
- Open `/?window=focus` to iterate the focus room in a browser.
- Open `/?nothing` to see **Nothing playing**.
- Relaunch remembers `pulse.mode` (`pill` | `card` | `expanded`).

## Driving it with the browser mock

Preconditions:

- Doctor reports `ok` at `http://localhost:1422/`.
- Baseline `/` with `pulse.mode=card` after reload.
- Now playing is **Savior** / **THE BOYZ**.

- **Card rest.** Open `/`. The widget shows **Savior**, **THE BOYZ**, `Track position`, `Pause`, `Previous track`, `Next track`. The expand control's name is `Expand to lyrics`.
- **Reveal cluster.** Run `node .cursor/skills/verify-pulse/scripts/chrome-session.mjs reveal` (or hover the bottom-right widget). The collapse control's name is `Collapse to pill`.
- **Expand to lyrics.** Run `node .cursor/skills/verify-pulse/scripts/chrome-session.mjs click Expand to lyrics`. After the 200ms glide a `Lyrics` region is present, the expand control's name is `Expand to focus`, and the collapse control's name is `Collapse to card`. Title **Savior** remains.
- **Collapse to card.** Run `node .cursor/skills/verify-pulse/scripts/chrome-session.mjs click Collapse to card`. Transport returns (`Pause`, `Back 10 seconds`). `Lyrics` is gone. Expand is `Expand to lyrics` again.
- **Collapse to pill.** Run `node .cursor/skills/verify-pulse/scripts/chrome-session.mjs reveal` then `click Collapse to pill`. The shell is the short glance: **Savior**, **THE BOYZ**, `Track position`. Expand is `Expand to card`. Collapse is `Collapse to pill` and disabled.
- **Expand to card.** Run `node .cursor/skills/verify-pulse/scripts/chrome-session.mjs reveal` then `click Expand to card`. Card transport returns. Expand is `Expand to lyrics`.
- **Resting empty.** Run `node .cursor/skills/verify-pulse/scripts/chrome-session.mjs goto http://localhost:1422/?nothing`. The widget reads **Nothing playing**. There is no `Open queue`, no `Pause`, no mode cluster.
- **Focus mock.** Run `node .cursor/skills/verify-pulse/scripts/chrome-session.mjs goto http://localhost:1422/?window=focus`. The room shows **Savior** and either a `Lyrics` region or **Finding lyrics…** / **No synced lyrics**. Reveal then look for `Show queue` and `Leave focus (Esc)`.
- **Expand to focus (native).** On expanded, activate `Expand to focus`. In the mock the widget stays expanded — report `verified-unreachable` (requires the Tauri focus window). Do not count `/?window=focus` as this entry point.
- **Proof.** From card, expand to lyrics. Run `node .cursor/skills/verify-pulse/scripts/chrome-session.mjs snapshot .cursor/skills/verify-pulse/artifacts/mode-ladder/expanded.aria.txt` and `screenshot .cursor/skills/verify-pulse/artifacts/mode-ladder/expanded.png`. Both identify Pulse via **Savior** and the `Lyrics` region. Capture `card.png` after collapsing. Capture `nothing.png` on `/?nothing` showing **Nothing playing**.

## Gotchas

- Leftover `pulse.mode` in localStorage starts you off-baseline. Reset before the first assert.
- Cluster buttons exist in the tree while `pointer-events: none`. A cursor-ide-browser click then fails with that error. Run `chrome-session.mjs reveal` or hover, then retry. `HTMLElement.click()` via chrome-session still fires the handler.
- `Collapse to pill` on pill is disabled in place. Do not treat a no-op click as a hang.
- Mode content crossfades ~200ms. Assert destination copy, not the outgoing layer.
- `/?window=focus` does not hide a widget — it is a different document. Native focus replaces the widget; the mock cannot show that pairing.
