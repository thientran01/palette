/** One preference mirrored across widget, focus, and settings. Live changes
 * take precedence over an older startup snapshot. Regular line sync stays on. */
import { useEffect, useSyncExternalStore } from "react";
import { commands, listenSettingsChanged } from "./backend";

// Withhold word wipes until the persisted preference is known.
let enabled = false;
let seeded = false;
let listening = false;
let revision = 0;
let initializing: Promise<void> | undefined;
const subscribers = new Set<() => void>();

function apply(value: boolean): void {
  enabled = value;
  subscribers.forEach(cb => cb());
}

export function karaokeEnabled(): boolean { return enabled; }

export function initKaraokePreference(): Promise<void> {
  if (seeded) return Promise.resolve();
  if (initializing) return initializing;
  initializing = (async () => {
    const before = revision;
    if (!listening) {
      await listenSettingsChanged(({ key, value }) => {
        if (key !== "karaoke_lyrics" || typeof value !== "boolean") return;
        revision++;
        apply(value);
      });
      listening = true;
    }
    const seed = await commands.prefsSeed();
    if (revision === before) apply(seed.karaoke_lyrics);
    seeded = true;
  })().catch(() => {
    // A later consumer can retry; keep the last known preference meanwhile.
  }).finally(() => { initializing = undefined; });
  return initializing;
}

function subscribe(cb: () => void): () => void {
  subscribers.add(cb);
  return () => { subscribers.delete(cb); };
}

export function useKaraokePreference(): boolean {
  const value = useSyncExternalStore(subscribe, karaokeEnabled);
  useEffect(() => { void initKaraokePreference(); }, []);
  return value;
}

export async function setKaraokePreference(enabled: boolean): Promise<void> {
  await initKaraokePreference();
  await commands.setKaraokeLyrics(enabled);
  // Read the source of truth even if the cross-window event is delayed.
  const before = revision;
  const seed = await commands.prefsSeed();
  if (revision === before) apply(seed.karaoke_lyrics);
}
