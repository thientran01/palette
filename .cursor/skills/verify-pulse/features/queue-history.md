# Queue and history

Queue is one open/closed bit with two garments: a popover above pill/card, and a peer layer inside expanded. **Up next** is Pulse's list; **Earlier** is play history. Play-now and add are uri-gated on a live Spotify session.

## Sub-features

- `queue-open-card` opens the popover from card (and pill).
- `queue-open-expanded` shows the queue layer under the now-playing header.
- `queue-up-next` lists the seeded upcoming rows.
- `queue-remove` drops a row from Up next.
- `queue-add-from-history` queues an Earlier row and toasts.
- `queue-play-now` jumps now-playing to an Earlier track.
- `queue-gate-off` narrates when Spotify is disconnected.

## How to get to it (user POV)

- Choose `Open queue` (bottom-left on card/expanded; pill hover scrim next to play/pause).
- Choose `Close queue` on the same control (`aria-pressed` true while open).
- On a history row, choose `Play now` or `Add to queue`, or press Enter to add.
- On a queue row, choose `Remove from queue`, or press Delete; drag to reorder.
- Choose `More like Savior` to seed similar tracks.
- Open `/?spotify=off` to read the gate caption.

## Driving it with the browser mock

Preconditions:

- Doctor reports `ok` at `http://localhost:1422/`.
- Baseline `/` in card mode, playing **Savior**, Spotify connected.
- Up next starts as **Instagram — DEAN** then **WHERE YOU AT? — STAYC**.
- Earlier is a long seeded list of the same three titles.

- **Open popover.** Tab until `Open queue` is focused and activate it. The name becomes `Close queue`. A list named `Up next` contains `Instagram — DEAN` and `WHERE YOU AT? — STAYC`. A list named `Earlier` is present. Header copy includes `Up next · 2`.
- **Remove.** Hover or focus `Instagram — DEAN` and activate `Remove from queue`. `Up next` no longer contains Instagram. Count copy is `Up next · 1`.
- **Add from history.** On an Earlier row named `Savior — THE BOYZ`, activate `Add to queue`. A polite live region reads `Queued · Savior` and `Up next` gains `Savior — THE BOYZ`.
- **Play now.** On an Earlier row named `Instagram — DEAN`, activate `Play now`. Now playing becomes **Instagram**. The live region reads `Playing · Instagram` (or a failure toast). Reopen or keep queue open: the played ring track is no longer the Up next front if it was queued.
- **Expanded garment.** Close the popover, expand to lyrics, activate `Open queue`. The `Lyrics` region is replaced by the same `Up next` / `Earlier` lists under the **Savior** (or current) header. `Show lyrics` (or `Show album cover`) exits the queue layer.
- **Disconnected gate.** Open `/?spotify=off`, reset to card, open queue. Caption **Queue works with Spotify — connect from the tray** is visible. Up next rows still render if the mock list is non-empty; play/add on Earlier are not offered.
- **Proof.** Capture `.cursor/skills/verify-pulse/artifacts/queue-history/open.aria.txt` and `open.png` with the popover open on card, showing `Up next` and both seeded titles. Capture `after-add.aria.txt` after queuing Savior.

## Gotchas

- While the popover is open the mode cluster stays revealed. Closing queue hides it again until hover/Tab.
- History action buttons are `tabIndex=-1`. Use the row (`Instagram — DEAN`) then the named button, or Enter on the row to add.
- Play-now suppresses the pill announcement for intermediates. Prove the landing title, not a flicker.
- `/?jump=partial` makes play-now toast `Played — some items couldn't re-queue`.
- More-like-this toasts `Added N similar tracks` on `/`, or the `/?similar=` copy. Rows arrive over a few hundred ms — wait for the toast, not the first flash.
- `/?queue=empty` gates Spotify's own queue read (`no_playback`), not Pulse's Up next list.
