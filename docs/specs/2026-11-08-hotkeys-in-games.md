# Hotkeys inside games (2026-11-08)

Thien reported that none of Palette's hotkeys (show/hide, play/pause, skip,
seek) work while League of Legends is focused, the only game where that
happens, while Discord's mute keybind keeps working there.

## Why

Palette registers its chords with `RegisterHotKey` (through
tauri-plugin-global-shortcut). Windows lets the focused app switch that
whole table off: a program that reads the keyboard through Raw Input with
the `RIDEV_NOHOTKEYS` flag (or DirectInput in exclusive mode, which sets
it) gets every key, and registered hotkeys stop firing until it loses
focus. Games use it so Alt and Win combinations can't pull them out of
fullscreen.

Discord's keybinds don't use `RegisterHotKey`. They use a low-level
keyboard hook (`WH_KEYBOARD_LL`), which Windows calls before it checks the
hotkey table, so that flag doesn't affect it.

We haven't confirmed that League sets the flag (that would mean inspecting
the game process). It is the explanation that fits both facts. If League
were running elevated instead, Discord's hook would fail there too, and
`RegisterHotKey` would keep working.

## What changed

`src-tauri/src/keyhook.rs` installs a `WH_KEYBOARD_LL` hook on its own
thread. `register_all` gives it every chord that `RegisterHotKey` accepted.
When the hook sees one of those chords, it swallows the key and fires the
action itself. The hook runs first, so `WM_HOTKEY` never arrives and
nothing fires twice.

`RegisterHotKey` stays for three reasons. It reserves the chord so no other
app takes it. It reports reserved-combo failures to the prefs UI. It also
still delivers the chord when the hook can't: when the focused window is
elevated (UIPI hides its input from a non-elevated hook), or when Windows
has removed a hook that answered too slowly. Chords that failed to
register are not hooked, so a chord another app owns stays that app's.

Details:

- Modifiers must match exactly, and a held chord fires once, the same as
  `MOD_NOREPEAT`. The release of a fired key is swallowed too.
- Modifiers are read with `GetAsyncKeyState` instead of being tracked from
  events. A release on the secure desktop (Ctrl+Alt+Del) never reaches the
  hook and would leave a tracked modifier stuck down.
- A chord with Alt or Win but no Ctrl sends one tap of the unassigned key
  0xE8 after it fires. The focused app never saw the chord's key, so
  without the tap, releasing Alt opens its menu bar and releasing Win opens
  Start. All default chords include Ctrl, so the defaults never inject
  anything.
- Prefs hotkey capture empties the hook's table along with unregistering
  the chords, or capture could never see a bound chord.
- The callback only looks up the key in the table and hands actions to
  `defer_main_action`. It doesn't log or store keys.

## Not done

- Running Palette elevated or with `uiAccess`. Those would cover elevated
  games too, but they change how Palette is installed and launched, and no
  game Thien plays needs them so far.
