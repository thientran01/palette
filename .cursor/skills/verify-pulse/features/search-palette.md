# Search palette

The palette is a second window for getting to a song: type to search Spotify, Enter plays now, Shift+Enter queues and stays open. Empty state resurfaces a few history rows.

## Sub-features

- `palette-empty` shows **From your history** picks on a connected session.
- `palette-search` filters the mock catalog as the user types.
- `palette-empty-query` shows **No matches on Spotify** for a miss.
- `palette-queue` queues the selected row and stays on the palette.
- `palette-play` starts play-now (the mock palette does not dismiss).
- `palette-disconnected` shows the tray-connect gate.

## How to get to it (user POV)

- Open `/?window=palette` in the browser mock.
- Native: `Ctrl+Alt+S` summons the real palette window — `verified-unreachable` here.
- Type in `Search Spotify`. Press Enter to play, Shift+Enter to queue, Escape to dismiss (native hide is a no-op in the mock).

## Driving it with the browser mock

Preconditions:

- Doctor reports `ok` at `http://localhost:1422/`.
- Palette URL is `http://localhost:1422/?window=palette` (same port, different document).
- Connected mock (`/?window=palette` without `spotify=off`).
- Search pool: Savior, Instagram, WHERE YOU AT?, plus **Happy Ending** / Kep1er, **About Love** / Red Velvet, **Euphoria** / keshi.

- **History empty state.** Open `/?window=palette`. A combobox named `Search Spotify` is focused, placeholder **Play something…**. A listbox named `From your history` has one to three rows (reasons such as **Most played**, **Haven't heard in a while**, **Today's pick**, **On this day**). Hint copy `↵ play · ⇧↵ queue` is visible.
- **Search match.** Fill `Search Spotify` with `dean`. After ~250ms debounce + ~180ms mock latency, the listbox name is `Search results` and contains **Instagram**.
- **Search extra.** Replace the query with `kep`. Results contain **Happy Ending**.
- **Search miss.** Replace the query with `volcano`. Status **No matches on Spotify** appears. The results list is empty.
- **Queue.** Clear to `dean` so **Instagram** is selected. Activate `Queue Instagram` (or Shift+Enter). The polite live region reads `Queued · Instagram`. The palette stays open with the query intact.
- **Play extra (gone).** Search `euphoria` and activate `Play Euphoria now` (or Enter). The mock play-now of a non-ring uri returns `gone`. The palette stays open in the mock (`paletteHide` is a no-op). Confirm via a second tab on `/` only if you already had that page mounted in the same mock process — otherwise prove the palette did not crash and the combobox still exists. Ring tracks (`Savior`, `Instagram`, `WHERE YOU AT?`) are the play-now successes; extras are the gone path.
- **Disconnected.** Open `/?window=palette&spotify=off`. Copy **Connect Spotify from the tray to search and play** is visible. There is no `Search results` list.
- **Proof.** Capture `.cursor/skills/verify-pulse/artifacts/search-palette/history.aria.txt` and `history.png` on the empty state (identity **Play something…** plus a history listbox). Capture `search-dean.aria.txt` / `search-dean.png` showing **Instagram** in `Search results`.

## Gotchas

- Native Enter-to-play dismisses the window. The mock cannot hide a second webview — do not treat a still-visible palette as a failed play.
- Search extras (Happy Ending, About Love, Euphoria) are not in the now-playing ring. Playing them is the deliberate `gone` path.
- Debounce is 250ms and the mock adds 180ms. Wait for **Searching…** to clear or for the result row, not a fixed 100ms.
- `/?window=palette` is a different JS realm from `/`. Queuing here updates the shared mock module in that document only — a second browser tab on `/` does not share the in-memory up-next list. Prove queue from the palette toast, or by opening queue in the **same** page after navigating back to `/` (that remounts a fresh mock). Treat cross-tab list equality as out of scope.
- Hover must move more than 3px before it steals the highlighted row from keyboard selection.
