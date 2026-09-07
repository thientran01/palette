import type { SyncStatus } from './backend';
export const savedSyncStatus: SyncStatus = {phase:'saved', detail:'Available on this device for future listens.'};
export function resolveSyncStatus(saved: boolean, preview: boolean, remote: SyncStatus | null): SyncStatus {
  if (saved && !preview) return savedSyncStatus;
  return remote ?? {phase:'waiting', detail:preview ? 'Play from the beginning to try vocal timing. Your saved timing stays available.' : 'Checking word sync…'};
}