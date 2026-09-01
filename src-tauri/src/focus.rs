/*
 * Focus mode — the fullscreen now-playing takeover (design "B1"). A
 * user-invoked, TRANSIENT second window: the expanded view's expand bracket
 * (labeled "Expand to focus") opens it, Esc / its collapse control /
 * Alt-F4 close it, and closing returns the main widget to its EXACT prior
 * intent (VisIntent.focus_open composes into effective_visible; the flag is
 * memory-only, so a relaunch can never boot into focus).
 *
 * WARM ON INTENT, destroy-on-close (2026-09-01). The original create-on-open
 * priced the WebView2 cold-create as affordable for a deliberate takeover;
 * Thien's live verdict overruled it — open "takes really long": controller
 * create + chunk load + React mount + seeds, all serialized behind the
 * click, on a blank HWND until React paints. The expand-to-focus bracket
 * only exists in the expanded view, so ENTERING expanded is the intent
 * signal: App.tsx warms the window (focus_warm → ensure, the search.rs
 * single-flight create — hidden, NOT fullscreen, VisIntent untouched) and
 * open becomes position + fullscreen + show + focus on an already-painted
 * webview. Leaving expanded cools a warmed-but-never-opened window
 * (focus_cool, gated on focus_open so an OPEN takeover is never destroyed
 * out from under the user), and use stays destroy-on-close — so there is
 * still no resident third webview at rest. Open with no warm window (a
 * failed or raced warm) is exactly the old cold create: every path degrades
 * to it, and the open→show ms logs on both paths so the win is measurable.
 * The frontend's arrival choreography re-keys on the "focus-shown" event +
 * focus_shown seed pair (Focus.tsx) so a warmed room still fades in AT
 * show, never invisibly at mount.
 *
 * The label-filtered Destroyed handler in lib.rs stays the SINGLE cleanup
 * point: it clears focus_open and re-applies visibility, so Esc, the
 * collapse control, OS teardown — and a cool — all converge on one path (a
 * cooled window clears an already-false flag into an unchanged intent, a
 * no-op by construction).
 *
 * This is the doctrine-legal reincarnation of what the removed P3
 * ambient-grow was groping toward: the same "bigger surface while I'm
 * around but not driving" want, INVOKED deliberately instead of guessed
 * from idle timers. It inherits the search window's multi-window seams
 * (capabilities label, window-state denylist, Moved guard, per-window
 * reactive votes) and adds the two gates that windows born after "main"
 * need: the media loop's `visible` and the audio capture switch both widen
 * to "main OR focus" (lib.rs) — a warmed HIDDEN window reads is_visible
 * false there, so warming never widens either gate.
 *
 * The window is fullscreen on the main widget's monitor before it is ever
 * shown, and never resized while visible (house rule; a DIFFERENT window
 * born at size is legal — the never-resize doctrine is about animating a
 * live window's bounds). The cold path has ALWAYS run set_fullscreen on a
 * hidden window (visible(false) at build, fullscreen before show); warming
 * only lengthens the hidden lifetime before that same call.
 */
use std::sync::atomic::Ordering;
use std::sync::Mutex;
use std::time::Instant;

use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::{apply_visibility, VisIntent};

pub const LABEL: &str = "focus";

/// Single-flight create/destroy gate (search.rs's CREATE discipline): warm
/// and open double-check window existence under it, cool destroys under it —
/// so rapid expanded enter/exit can interleave warms and cools without ever
/// double-creating or overlapping a create with a destroy.
static CREATE: Mutex<()> = Mutex::new(());

/// Build the window hidden and NOT fullscreen. Geometry (position +
/// fullscreen, both while still hidden) belongs to open(), which knows the
/// widget's monitor at OPEN time — a warm-time monitor pick would go stale
/// the moment the widget is dragged elsewhere.
fn create(app: &AppHandle) {
    let result = WebviewWindowBuilder::new(
        app,
        LABEL,
        WebviewUrl::App("index.html?window=focus".into()),
    )
    .title("Palette Focus")
    .decorations(false)
    // UNLIKE the widget/search window (chromeless floating surfaces that skip the
    // taskbar), focus mode is a fullscreen view the user actively works in
    // and Alt+Tabs away from — it MUST stay in the taskbar + Alt+Tab
    // switcher. With skip_taskbar it dropped behind whatever you switched to
    // with no way back (not in the switcher, not on the taskbar), leaving
    // only the Pulse hotkey — which closes focus rather than restoring it
    // (Thien, 2026-07-12). It is not always-on-top on purpose: switching to
    // another app should surface that app, and Alt+Tab brings focus back.
    .resizable(false)
    .maximizable(false)
    .minimizable(false)
    // Born hidden so open()'s position → fullscreen → show sequence is
    // deterministic (fullscreen takes the window's CURRENT monitor) — and so
    // a warmed window stays invisible until it is actually opened.
    .visible(false)
    .build();
    if let Err(e) = result {
        log::error!("focus: window create failed: {e}");
    }
}

/// Single-flight create-if-missing (the search.rs ensure pattern): two
/// callers (a warm and a racing open) converge on one window. A failure
/// downgrades gracefully — open() finds no window and gives up, warm simply
/// leaves the next open on the cold path.
fn ensure(app: &AppHandle) {
    if app.get_webview_window(LABEL).is_some() {
        return;
    }
    let _gate = CREATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if app.get_webview_window(LABEL).is_some() {
        return;
    }
    create(app);
}

/// The open sequence: ensure a window (warm hit or cold create), then
/// position on the widget's monitor, fullscreen, show, focus — and only for
/// a takeover that actually appeared, flag the intent and yield the widget.
fn open(app: &AppHandle) {
    let t0 = Instant::now();
    let vis = app.state::<VisIntent>();
    if vis.focus_open.load(Ordering::Relaxed) {
        // Already open (double-click on the bracket) — front it. If it's a
        // mid-teardown corpse (Esc→reopen race, the destroy hasn't drained),
        // this no-ops and the click is lost; rare, self-heals, accepted.
        if let Some(win) = app.get_webview_window(LABEL) {
            let _ = win.set_focus();
        }
        return;
    }
    // Warm hit = App.tsx pre-created the window on entering expanded, so the
    // webview is already navigated, mounted, and seeded; open is just
    // geometry + show. Miss = the old cold create, the fallback. Read before
    // ensure purely for the log's honesty.
    let path = if app.get_webview_window(LABEL).is_some() {
        "warm"
    } else {
        "cold"
    };
    ensure(app);
    let Some(win) = app.get_webview_window(LABEL) else {
        // Create failed (logged in create()) — no takeover, intent untouched.
        return;
    };
    // Same monitor as the widget (derive, don't assume — even on
    // today's single-monitor machine).
    if let Some(pos) = app
        .get_webview_window("main")
        .and_then(|m| m.outer_position().ok())
    {
        let _ = win.set_position(pos);
    }
    let _ = win.set_fullscreen(true);
    // The widget yields only for a takeover that actually appeared:
    // a failed show() with the flag set would leave NOTHING on
    // screen, and only the focus window's Destroyed event clears
    // the flag (quick-review catch; Ctrl+Alt+M carries a recovery
    // path regardless).
    if win.show().is_err() {
        log::error!("focus: show failed — aborting the takeover");
        let _ = win.destroy();
        return;
    }
    // The measurable win: click-to-show on both paths, in pulse.log.
    log::info!(
        "focus: open → show in {}ms ({path})",
        t0.elapsed().as_millis()
    );
    let _ = win.set_focus();
    // The arrival signal (search.rs's "search-shown" twin): a warmed realm
    // re-keys its arrival choreography on it. Paired with the focus_shown
    // seed below, because a COLD open's emit fires while the webview is
    // still loading — before any listener exists.
    let _ = app.emit_to(LABEL, "focus-shown", ());
    vis.focus_open.store(true, Ordering::Relaxed);
    apply_visibility(app);
}

/// Destroy a warmed-but-never-opened window — the widget left expanded, so
/// the intent signal lapsed. Gated on focus_open (an OPEN takeover is never
/// destroyed out from under the user; the widget sits in expanded BEHIND it,
/// so its mode never "leaves") and serialized under CREATE so a destroy can
/// never overlap a concurrent warm's create. The Destroyed event this fires
/// clears an already-false flag and re-applies an unchanged intent — a no-op
/// on the main widget by construction.
fn cool(app: &AppHandle) {
    let _gate = CREATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if app.state::<VisIntent>().focus_open.load(Ordering::Relaxed) {
        return;
    }
    if let Some(win) = app.get_webview_window(LABEL) {
        let _ = win.destroy();
    }
}

/// Open the takeover. Deferred to the blocking pool: the body can wait on
/// CREATE across a WebView2 build (a warm still in flight), which must never
/// park a core worker (the off_core discipline) — and the frontend
/// fire-and-forgets this anyway.
#[tauri::command]
pub async fn focus_open(app: AppHandle) {
    crate::defer_main_action(&app, open);
}

/// Warm the takeover window (App.tsx, on the widget entering expanded):
/// create it hidden so the eventual open is geometry + show on an
/// already-painted webview. Never touches VisIntent.
#[tauri::command]
pub async fn focus_warm(app: AppHandle) {
    crate::defer_main_action(&app, ensure);
}

/// Cool a warmed-but-never-opened takeover window (App.tsx, on the widget
/// leaving expanded). No-ops when the takeover is open or no window exists.
#[tauri::command]
pub async fn focus_cool(app: AppHandle) {
    crate::defer_main_action(&app, cool);
}

/// Seed for the "focus-shown" event pair: is THIS window already visible?
/// A webview that mounts into an already-shown window (the cold path — the
/// open's emit fired before React registered a listener) reads true and
/// runs its arrival immediately; a warmed hidden mount reads false and
/// waits for the event.
#[tauri::command]
pub async fn focus_shown(window: WebviewWindow) -> bool {
    window.is_visible().unwrap_or(false)
}

/// Close = destroy; the Destroyed handler (lib.rs) clears focus_open and
/// restores the widget — one cleanup path for Esc, collapse, and Alt-F4.
#[tauri::command]
pub async fn focus_close(app: AppHandle) {
    if let Some(win) = app.get_webview_window(LABEL) {
        let _ = win.destroy();
    }
}

/// The Destroyed-event cleanup (called from lib.rs's window-event handler,
/// which runs on the main thread). Clear focus_open on the spot, but run the
/// restoring apply_visibility OFF the main thread — it does emit_now → GSMTC
/// `.get()`, which must never block the message pump (the Application-Hang
/// class). See lib.rs defer_main_action.
pub fn on_destroyed(app: &AppHandle) {
    app.state::<VisIntent>()
        .focus_open
        .store(false, Ordering::Relaxed);
    crate::defer_main_action(app, apply_visibility);
}
