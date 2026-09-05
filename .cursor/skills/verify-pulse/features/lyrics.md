# Lyrics

Expanded is the karaoke home. Synced lines follow the clock; a click seeks so the chosen line is the one that highlights. A hover toggle can show the album cover instead. A miss keeps the art as the identity.

## Sub-features

- `lyrics-show` renders the `Lyrics` region after expand on a synced track.
- `lyrics-current` marks the line for the current position (mock starts near 63s).
- `lyrics-words` on `/?lyrics=words` wipes sung words to foreground on the current line; untimed lines stay whole-line highlight.
- `lyrics-seek-line` seeks to a line on a seekable session.
- `lyrics-toggle-art` swaps to the big cover without leaving expanded.
- `lyrics-miss` shows **No synced lyrics** when the fetch misses.

## How to get to it (user POV)

- Choose `Expand to lyrics` from card (or expand from pill to card first).
- Choose a line named `Seek to …` to seek there.
- Choose `Show album cover` / `Show lyrics` (top-right, hover or Tab).
- From the queue layer, the same control exits to lyrics (or art if none are synced).
- Open `/?lyrics=none` then expand to see the miss.
- Open `/?lyrics=words` then expand to see in-line word karaoke.
- Open `/?window=focus` for the focus-scale lyrics room.

## Driving it with the browser mock

Preconditions:

- Doctor reports `ok` at `http://localhost:1422/`.
- Baseline `/` in card mode, playing **Savior**, seekable.
- Default mock lyrics are synced lines `Mock lyric line N — la la la (Savior)` on a 4s cadence after a 12s intro.

- **Open lyrics.** Activate `Expand to lyrics`. A region named `Lyrics` appears. Visible lines include `Mock lyric line` and **Savior**. Transport stays (`Pause`).
- **Current line.** A line near the 63s start is current (the mock parks just before a break at 64s). The current line is not muted relative to its neighbors.
- **Click to seek.** Activate `Seek to Mock lyric line 1 — la la la (Savior)`. `Track position` valuetext moves near `0:12` (vocal lead may land slightly earlier). That line becomes current.
- **Album toggle.** Tab until `Show album cover` is focused and activate it. The `Lyrics` region is gone; **Savior** / **THE BOYZ** sit under a large cover. The control's name becomes `Show lyrics`.
- **Back to lyrics.** Activate `Show lyrics`. The `Lyrics` region returns.
- **Miss.** Open `/?lyrics=none`, reset to card, expand. Copy **No synced lyrics** appears. The view toggle is named `No synced lyrics` and is disabled. The cover remains the identity.
- **Word wipe.** Open `/?lyrics=words`, reset to card, expand. Seek to `Seek to Mock lyric line 1 — la la la (Savior)`. The current line keeps the accent marker; sung words are foreground and not-yet-sung words stay muted. Accent is not on the glyphs. Untimed `/?` (no words param) still highlights the whole current line.
- **Focus room.** Open `/?window=focus` on the default mock. A `Lyrics` region is present at the larger type scale, with **Savior** still identified.
- **Proof.** Capture `.cursor/skills/verify-pulse/artifacts/lyrics/synced.aria.txt` and `synced.png` on default expanded `/` showing the `Lyrics` region and **Savior**. Capture `miss.png` on `/?lyrics=none` expanded showing **No synced lyrics**.

## Gotchas

- `/?am` cannot seek: lines are not buttons (`Seek to …` is absent). Karaoke still highlights.
- Click-to-seek lands `VOCAL_LEAD_MS` before the line. Assert the highlight and an earlier `Track position`, not an exact stamp.
- `?lyrics=<ms>` delays the fetch. **Finding lyrics…** waits 400ms before it is allowed to appear — do not treat a blank caption in the first 400ms as a miss.
- The note/mic seat is the only lyrics entry from the queue layer. Closing queue via `Close queue` also returns you; prove the `Lyrics` region, not just `aria-pressed` false.
- Track changes refetch lyrics. Do not assert line 1 of Savior after a skip to Instagram.
