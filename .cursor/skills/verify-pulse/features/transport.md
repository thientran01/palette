# Transport

Transport plays, pauses, skips, and seeks the current session. The card and expanded views show the full control row at rest; the pill hides play/pause until hover. Seek is capability-gated.

## Sub-features

- `transport-pause` freezes playback and the control becomes `Play`.
- `transport-play` resumes and the control becomes `Pause`.
- `transport-seek-back` moves ~10s earlier on a seekable session.
- `transport-seek-fwd` moves ~10s later on a seekable session.
- `transport-next` advances the track (queue-aware when Up next has a ring match).
- `transport-prev` walks to the previous ring track.
- `transport-am-no-seek` disables seek on the Apple Music mock.

## How to get to it (user POV)

- Choose `Pause` / `Play`, `Back 10 seconds`, `Forward 10 seconds`, `Previous track`, or `Next track` on card or expanded.
- On the pill, hover or Tab to reveal play/pause, then choose it.
- Drag or key the `Track position` slider (ArrowLeft/ArrowRight 5s, Home, End) when seekable.
- Open `/?am` to see seek refused.

## Driving it with the browser mock

Preconditions:

- Doctor reports `ok` at `http://localhost:1422/`.
- Baseline `/` in card mode, playing **Savior**.
- Default Up next is **Instagram** then **WHERE YOU AT?** — `Next track` lands on Instagram and pops it from Up next.

- **Visible row.** On `/` the card shows `Previous track`, `Back 10 seconds`, `Pause`, `Forward 10 seconds`, `Next track`, and `Track position` with valuetext like `1:03 of 3:24` (start is ~63s; it advances).
- **Pause.** Activate `Pause`. The same control's name becomes `Play`. `Track position` valuetext stops advancing across two snapshots ~1s apart.
- **Play.** Activate `Play`. The name returns to `Pause`. Valuetext advances again.
- **Seek back.** While playing, read `aria-valuenow` on `Track position`, activate `Back 10 seconds`, and read again. The new value is about 10s smaller (tolerance 2s).
- **Seek forward.** Activate `Forward 10 seconds`. `aria-valuenow` rises about 10s.
- **Next (queue-aware).** Activate `Next track`. Now playing becomes **Instagram** / **DEAN**. Open queue: `Up next` contains **WHERE YOU AT?** and does not contain **Instagram**.
- **Previous.** Activate `Previous track`. Now playing returns to **Savior**.
- **Apple Music no-seek.** Open `/?am` (reset mode to card). Seek controls are named `Seeking not supported by Apple Music` and are disabled. `Track position` is a progressbar, not a slider. `Pause` still works.
- **Pill play/pause.** Set mode to pill, Tab until `Pause` is focused, activate it. The name becomes `Play`.
- **Proof.** Capture `.cursor/skills/verify-pulse/artifacts/transport/paused.aria.txt` and `paused.png` after pause on card (identity **Savior**, control `Play`). Capture `after-next.aria.txt` / `after-next.png` after next (identity **Instagram**).

## Gotchas

- Default `/` next is queue-aware. Restore the Instagram/WHERE YOU AT? list (reload `/`) before proving a plain ring skip, or empty Up next first.
- Play/pause is optimistic: the label flips on press, then reconciles. Assert the label after the click lands, not mid-pointerdown.
- `/?am` resume re-sends the pause-era stamp. The displayed clock must not leap by the pause length — if you pause for a few seconds and play, position stays near the pause point.
- Pill seek is display-only (hairline progressbar). Do not look for Back/Forward there.
- Do not use `__mockNext()` for this feature; the Next button is the user path on card/expanded.
