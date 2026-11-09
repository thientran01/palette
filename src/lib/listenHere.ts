/*
 * Listen for an event Rust sends to THIS window with emit_to.
 *
 * The global `listen()` from @tauri-apps/api/event registers with target Any,
 * and Tauri delivers every emit_to to Any listeners in EVERY webview: its
 * filter only applies to targeted listeners (tauri 2.11 event/listener.rs
 * match_any_or_filter). So `emit_to("search", "window-shown", false)` also
 * reached the widget, which then believed it was hidden and froze its karaoke
 * until something else told it otherwise (1.0.13, Thien 2026-11-09). A
 * listener targeted at this window hears emit_to for its own label and every
 * plain app.emit, nothing else.
 *
 * Tauri only: getCurrentWebviewWindow() throws in a plain browser.
 */
import { listen, type EventCallback, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

export function listenHere<T>(event: string, handler: EventCallback<T>): Promise<UnlistenFn> {
  return listen<T>(event, handler, {
    target: { kind: "WebviewWindow", label: getCurrentWebviewWindow().label },
  });
}
