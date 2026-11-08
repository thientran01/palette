/*
 * Second delivery path for the global hotkeys: a low-level keyboard hook
 * (WH_KEYBOARD_LL), the same mechanism Discord's keybinds use.
 *
 * Why: RegisterHotKey (tauri-plugin-global-shortcut) is silenced by any
 * foreground app that registers its keyboard through Raw Input with
 * RIDEV_NOHOTKEYS — games do this so Alt/Win combos can't yank them out of
 * fullscreen, and League of Legends is the one Thien hit (2026-11-08): every
 * Palette chord dead in game while Discord's mute bind kept working. That
 * flag only switches off the hotkey table; low-level hooks run before it and
 * still see every key. See docs/specs/2026-11-08-hotkeys-in-games.md.
 *
 * Shape: RegisterHotKey stays. It still RESERVES each chord (so no other app
 * can take it) and reports reserved-combo failures to the prefs UI, and only
 * chords it accepted are put in this hook's table. The hook runs before the
 * hotkey table, so when it matches a chord it swallows the key and fires the
 * action itself — WM_HOTKEY never arrives, nothing fires twice. Where the
 * hook can't see (an ELEVATED foreground window: UIPI hides its input from a
 * non-elevated hook) or if Windows has dropped the hook, the key falls
 * through to RegisterHotKey exactly as before. So the hook can only add
 * coverage, never remove it.
 *
 * The callback runs for every key system-wide, and Windows silently removes
 * a hook that is slow to answer (LowLevelHooksTimeout), so it does nothing
 * but a table lookup: no I/O, no logging per key, nothing stored. Actions are
 * handed to defer_main_action, the same off-main hop the WM_HOTKEY path uses.
 * The hook lives on its own thread whose only job is the message loop that
 * Windows needs to deliver hook calls.
 */
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError};

use tauri::AppHandle;
use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut};
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, PostThreadMessageW, SetWindowsHookExW,
    TranslateMessage, HC_ACTION, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_APP, WM_KEYDOWN,
    WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

pub(crate) const MOD_CTRL: u8 = 1;
pub(crate) const MOD_ALT: u8 = 2;
pub(crate) const MOD_SHIFT: u8 = 4;
pub(crate) const MOD_WIN: u8 = 8;

/// Posted to the hook thread after a fired chord that needs a menu mask.
const WM_SEND_MASK: u32 = WM_APP + 1;
/// An unassigned virtual key (AutoHotkey's default mask key).
const VK_MASK: u16 = 0xE8;

/// One chord the hook answers: a virtual key plus an exact modifier set.
#[derive(Clone, Copy)]
pub(crate) struct Binding<T> {
    pub vk: u32,
    pub mods: u8,
    pub action: T,
}

/// What the hook does with one key event.
#[derive(Debug, PartialEq)]
pub(crate) enum Verdict<T> {
    /// Not ours: hand it on untouched.
    Pass,
    /// Ours but already handled (auto-repeat, or the release of a fired key).
    Swallow,
    /// A fresh press of a bound chord: swallow it and run this action.
    Fire(T),
}

/// The pure matching logic, kept free of Win32 so it can be unit-tested.
pub(crate) struct Matcher<T> {
    bindings: Vec<Binding<T>>,
    /// Keys whose press we swallowed and whose release we haven't seen.
    /// Mirrors RegisterHotKey's MOD_NOREPEAT: holding the chord fires once.
    held: Vec<u32>,
}

impl<T: Copy> Matcher<T> {
    pub const fn new() -> Self {
        Self {
            bindings: Vec::new(),
            held: Vec::new(),
        }
    }

    /// Replace the chord table. `held` survives so a key that is down while
    /// the table changes still has its release swallowed.
    pub fn set_bindings(&mut self, bindings: Vec<Binding<T>>) {
        self.bindings = bindings;
    }

    pub fn on_key(&mut self, vk: u32, down: bool, mods: u8) -> Verdict<T> {
        if is_modifier(vk) {
            return Verdict::Pass;
        }
        let held_at = self.held.iter().position(|&h| h == vk);
        if !down {
            return match held_at {
                Some(i) => {
                    self.held.swap_remove(i);
                    Verdict::Swallow
                }
                None => Verdict::Pass,
            };
        }
        if held_at.is_some() {
            return Verdict::Swallow;
        }
        // Exact modifier match, as RegisterHotKey does: Ctrl+Alt+Shift+K is
        // not Ctrl+Alt+K.
        match self.bindings.iter().find(|b| b.vk == vk && b.mods == mods) {
            Some(b) => {
                self.held.push(vk);
                Verdict::Fire(b.action)
            }
            None => Verdict::Pass,
        }
    }
}

fn is_modifier(vk: u32) -> bool {
    [
        VK_CONTROL,
        VK_LCONTROL,
        VK_RCONTROL,
        VK_MENU,
        VK_LMENU,
        VK_RMENU,
        VK_SHIFT,
        VK_LSHIFT,
        VK_RSHIFT,
        VK_LWIN,
        VK_RWIN,
    ]
    .iter()
    .any(|k| u32::from(k.0) == vk)
}

/// Releasing Alt or Win with nothing in between opens the window menu bar or
/// the Start menu, and the chord's key never reached the app, so to it the
/// modifier WAS alone. A tap of an unassigned key breaks that up (the
/// AutoHotkey "menu mask" trick). With Ctrl held too, neither menu opens, so
/// a chord that includes Ctrl (every default) never injects anything.
pub(crate) fn needs_mask(mods: u8) -> bool {
    mods & (MOD_ALT | MOD_WIN) != 0 && mods & MOD_CTRL == 0
}

pub(crate) type Action = fn(&AppHandle);

static MATCHER: Mutex<Matcher<Action>> = Mutex::new(Matcher::new());
static APP: OnceLock<AppHandle> = OnceLock::new();
static HOOK_THREAD: AtomicU32 = AtomicU32::new(0);

/// Install the hook on its own thread. Failure only logs: RegisterHotKey
/// keeps working without it.
pub(crate) fn install(app: &AppHandle) {
    if APP.set(app.clone()).is_err() {
        return; // already installed
    }
    let spawned = std::thread::Builder::new()
        .name("keyhook".into())
        .spawn(|| {
            // SAFETY: plain Win32 calls; the hook proc is a 'static fn and
            // the module handle is this process's own image.
            unsafe {
                HOOK_THREAD.store(GetCurrentThreadId(), Ordering::Relaxed);
                let module = GetModuleHandleW(None).ok().map(Into::into);
                if let Err(e) = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), module, 0) {
                    log::warn!("keyboard hook not installed, hotkeys use RegisterHotKey only: {e}");
                    return;
                }
                log::info!("keyboard hook installed");
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    if msg.message == WM_SEND_MASK {
                        send_mask();
                        continue;
                    }
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        });
    if let Err(e) = spawned {
        log::warn!("keyboard hook thread failed to start: {e}");
    }
}

/// Point the hook at the chords RegisterHotKey accepted (lib.rs's
/// register_all), or at nothing while the prefs window captures a new chord.
pub(crate) fn set_chords(chords: &[(String, Action)]) {
    let bindings = chords
        .iter()
        .filter_map(|(chord, action)| {
            let parsed = chord.parse::<Shortcut>().ok();
            let binding = parsed.and_then(|s| {
                Some(Binding {
                    vk: u32::from(code_to_vk(s.key)?.0),
                    mods: mods_of(s.mods),
                    action: *action,
                })
            });
            if binding.is_none() {
                log::warn!("keyboard hook can't map chord {chord}; RegisterHotKey only");
            }
            binding
        })
        .collect();
    MATCHER
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .set_bindings(bindings);
}

fn mods_of(m: Modifiers) -> u8 {
    let mut out = 0;
    if m.contains(Modifiers::CONTROL) {
        out |= MOD_CTRL;
    }
    if m.contains(Modifiers::ALT) {
        out |= MOD_ALT;
    }
    if m.contains(Modifiers::SHIFT) {
        out |= MOD_SHIFT;
    }
    if m.intersects(Modifiers::SUPER | Modifiers::META) {
        out |= MOD_WIN;
    }
    out
}

/// Modifiers physically down right now. Read from the system rather than
/// tracked from the event stream: a release that happens on the secure
/// desktop (Ctrl+Alt+Del) never reaches the hook and would leave a tracked
/// modifier stuck down.
fn current_mods() -> u8 {
    // SAFETY: GetAsyncKeyState has no preconditions.
    let down = |vk: VIRTUAL_KEY| unsafe { GetAsyncKeyState(i32::from(vk.0)) } as u16 & 0x8000 != 0;
    let mut out = 0;
    if down(VK_CONTROL) {
        out |= MOD_CTRL;
    }
    if down(VK_MENU) {
        out |= MOD_ALT;
    }
    if down(VK_SHIFT) {
        out |= MOD_SHIFT;
    }
    if down(VK_LWIN) || down(VK_RWIN) {
        out |= MOD_WIN;
    }
    out
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        // SAFETY: for HC_ACTION, lparam points at a KBDLLHOOKSTRUCT.
        let kb = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let msg = wparam.0 as u32;
        let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
        let up = msg == WM_KEYUP || msg == WM_SYSKEYUP;
        if down || up {
            let mods = if down { current_mods() } else { 0 };
            let verdict = MATCHER
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .on_key(kb.vkCode, down, mods);
            match verdict {
                Verdict::Pass => {}
                Verdict::Swallow => return LRESULT(1),
                Verdict::Fire(action) => {
                    if let Some(app) = APP.get() {
                        crate::defer_main_action(app, action);
                    }
                    if needs_mask(mods) {
                        // Not from inside the hook: SendInput re-enters it.
                        let tid = HOOK_THREAD.load(Ordering::Relaxed);
                        let _ =
                            unsafe { PostThreadMessageW(tid, WM_SEND_MASK, WPARAM(0), LPARAM(0)) };
                    }
                    return LRESULT(1);
                }
            }
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

fn send_mask() {
    let key = |flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(VK_MASK),
                dwFlags: flags,
                ..Default::default()
            },
        },
    };
    let inputs = [key(KEYBD_EVENT_FLAGS(0)), key(KEYEVENTF_KEYUP)];
    // SAFETY: a valid slice of fully initialised INPUTs.
    unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
}

/// The accelerator key → virtual key table, matching what
/// global-hotkey 0.8 hands RegisterHotKey (its key_to_vk is private), so the
/// hook answers exactly the key RegisterHotKey reserved.
fn code_to_vk(code: Code) -> Option<VIRTUAL_KEY> {
    Some(match code {
        Code::KeyA => VK_A,
        Code::KeyB => VK_B,
        Code::KeyC => VK_C,
        Code::KeyD => VK_D,
        Code::KeyE => VK_E,
        Code::KeyF => VK_F,
        Code::KeyG => VK_G,
        Code::KeyH => VK_H,
        Code::KeyI => VK_I,
        Code::KeyJ => VK_J,
        Code::KeyK => VK_K,
        Code::KeyL => VK_L,
        Code::KeyM => VK_M,
        Code::KeyN => VK_N,
        Code::KeyO => VK_O,
        Code::KeyP => VK_P,
        Code::KeyQ => VK_Q,
        Code::KeyR => VK_R,
        Code::KeyS => VK_S,
        Code::KeyT => VK_T,
        Code::KeyU => VK_U,
        Code::KeyV => VK_V,
        Code::KeyW => VK_W,
        Code::KeyX => VK_X,
        Code::KeyY => VK_Y,
        Code::KeyZ => VK_Z,
        Code::Digit0 => VK_0,
        Code::Digit1 => VK_1,
        Code::Digit2 => VK_2,
        Code::Digit3 => VK_3,
        Code::Digit4 => VK_4,
        Code::Digit5 => VK_5,
        Code::Digit6 => VK_6,
        Code::Digit7 => VK_7,
        Code::Digit8 => VK_8,
        Code::Digit9 => VK_9,
        Code::Equal => VK_OEM_PLUS,
        Code::Comma => VK_OEM_COMMA,
        Code::Minus => VK_OEM_MINUS,
        Code::Period => VK_OEM_PERIOD,
        Code::Semicolon => VK_OEM_1,
        Code::Slash => VK_OEM_2,
        Code::Backquote => VK_OEM_3,
        Code::BracketLeft => VK_OEM_4,
        Code::Backslash => VK_OEM_5,
        Code::BracketRight => VK_OEM_6,
        Code::Quote => VK_OEM_7,
        Code::Backspace => VK_BACK,
        Code::Tab => VK_TAB,
        Code::Space => VK_SPACE,
        Code::Enter | Code::NumpadEnter => VK_RETURN,
        Code::CapsLock => VK_CAPITAL,
        Code::Escape => VK_ESCAPE,
        Code::PageUp => VK_PRIOR,
        Code::PageDown => VK_NEXT,
        Code::End => VK_END,
        Code::Home => VK_HOME,
        Code::ArrowLeft => VK_LEFT,
        Code::ArrowUp => VK_UP,
        Code::ArrowRight => VK_RIGHT,
        Code::ArrowDown => VK_DOWN,
        Code::PrintScreen => VK_SNAPSHOT,
        Code::Insert => VK_INSERT,
        Code::Delete => VK_DELETE,
        Code::F1 => VK_F1,
        Code::F2 => VK_F2,
        Code::F3 => VK_F3,
        Code::F4 => VK_F4,
        Code::F5 => VK_F5,
        Code::F6 => VK_F6,
        Code::F7 => VK_F7,
        Code::F8 => VK_F8,
        Code::F9 => VK_F9,
        Code::F10 => VK_F10,
        Code::F11 => VK_F11,
        Code::F12 => VK_F12,
        Code::F13 => VK_F13,
        Code::F14 => VK_F14,
        Code::F15 => VK_F15,
        Code::F16 => VK_F16,
        Code::F17 => VK_F17,
        Code::F18 => VK_F18,
        Code::F19 => VK_F19,
        Code::F20 => VK_F20,
        Code::F21 => VK_F21,
        Code::F22 => VK_F22,
        Code::F23 => VK_F23,
        Code::F24 => VK_F24,
        Code::NumLock => VK_NUMLOCK,
        Code::Numpad0 => VK_NUMPAD0,
        Code::Numpad1 => VK_NUMPAD1,
        Code::Numpad2 => VK_NUMPAD2,
        Code::Numpad3 => VK_NUMPAD3,
        Code::Numpad4 => VK_NUMPAD4,
        Code::Numpad5 => VK_NUMPAD5,
        Code::Numpad6 => VK_NUMPAD6,
        Code::Numpad7 => VK_NUMPAD7,
        Code::Numpad8 => VK_NUMPAD8,
        Code::Numpad9 => VK_NUMPAD9,
        Code::NumpadAdd => VK_ADD,
        Code::NumpadDecimal => VK_DECIMAL,
        Code::NumpadDivide => VK_DIVIDE,
        Code::NumpadMultiply => VK_MULTIPLY,
        Code::NumpadSubtract => VK_SUBTRACT,
        Code::ScrollLock => VK_SCROLL,
        Code::AudioVolumeDown => VK_VOLUME_DOWN,
        Code::AudioVolumeUp => VK_VOLUME_UP,
        Code::AudioVolumeMute => VK_VOLUME_MUTE,
        Code::MediaPlay => VK_PLAY,
        Code::MediaPause | Code::Pause => VK_PAUSE,
        Code::MediaPlayPause => VK_MEDIA_PLAY_PAUSE,
        Code::MediaStop => VK_MEDIA_STOP,
        Code::MediaTrackNext => VK_MEDIA_NEXT_TRACK,
        Code::MediaTrackPrevious => VK_MEDIA_PREV_TRACK,
        // Anything else RegisterHotKey couldn't register either.
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const K: u32 = VK_K.0 as u32;
    const N: u32 = VK_N.0 as u32;
    const CA: u8 = MOD_CTRL | MOD_ALT;

    fn matcher() -> Matcher<&'static str> {
        let mut m = Matcher::new();
        m.set_bindings(vec![
            Binding {
                vk: K,
                mods: CA,
                action: "playpause",
            },
            Binding {
                vk: N,
                mods: CA,
                action: "next",
            },
        ]);
        m
    }

    #[test]
    fn fires_once_per_press_and_swallows_repeat_and_release() {
        let mut m = matcher();
        assert_eq!(m.on_key(K, true, CA), Verdict::Fire("playpause"));
        // Auto-repeat while held: MOD_NOREPEAT parity.
        assert_eq!(m.on_key(K, true, CA), Verdict::Swallow);
        assert_eq!(m.on_key(K, false, 0), Verdict::Swallow);
        // Released, so the next press fires again.
        assert_eq!(m.on_key(K, true, CA), Verdict::Fire("playpause"));
    }

    #[test]
    fn modifiers_must_match_exactly() {
        let mut m = matcher();
        assert_eq!(m.on_key(K, true, 0), Verdict::Pass);
        assert_eq!(m.on_key(K, true, MOD_CTRL), Verdict::Pass);
        assert_eq!(m.on_key(K, true, CA | MOD_SHIFT), Verdict::Pass);
        // Unswallowed presses leave their releases alone too.
        assert_eq!(m.on_key(K, false, 0), Verdict::Pass);
    }

    #[test]
    fn modifier_keys_always_pass() {
        let mut m = matcher();
        let ctrl = u32::from(VK_LCONTROL.0);
        assert_eq!(m.on_key(ctrl, true, MOD_CTRL), Verdict::Pass);
        assert_eq!(m.on_key(ctrl, false, 0), Verdict::Pass);
    }

    #[test]
    fn empty_table_passes_but_still_swallows_a_held_release() {
        let mut m = matcher();
        assert_eq!(m.on_key(N, true, CA), Verdict::Fire("next"));
        // Prefs capture starts while N is still down.
        m.set_bindings(Vec::new());
        assert_eq!(m.on_key(K, true, CA), Verdict::Pass);
        assert_eq!(m.on_key(N, false, 0), Verdict::Swallow);
        assert_eq!(m.on_key(N, true, CA), Verdict::Pass);
    }

    #[test]
    fn mask_only_without_ctrl() {
        assert!(!needs_mask(CA));
        assert!(!needs_mask(MOD_CTRL | MOD_WIN));
        assert!(needs_mask(MOD_ALT));
        assert!(needs_mask(MOD_WIN | MOD_SHIFT));
        assert!(!needs_mask(MOD_SHIFT));
    }

    #[test]
    fn default_chords_map_to_keys() {
        for chord in ["ctrl+alt+k", "ctrl+alt+left", "ctrl+alt+[", "ctrl+alt+]"] {
            let s: Shortcut = chord.parse().expect(chord);
            assert!(code_to_vk(s.key).is_some(), "{chord}");
            assert_eq!(mods_of(s.mods), CA, "{chord}");
        }
    }
}
