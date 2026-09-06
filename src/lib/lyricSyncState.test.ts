import { describe, expect, it } from 'vitest';
import { resolveSyncStatus } from './lyricSyncState';
describe('vocal preview status', () => {
  it('does not let existing original words hide experimental processing', () => {
    expect(resolveSyncStatus(true, true, {phase:'processing',detail:'working'}).phase).toBe('processing');
    expect(resolveSyncStatus(true, true, null).phase).toBe('waiting');
  });
  it('shows experimental saved state and retains normal saved behavior', () => {
    expect(resolveSyncStatus(true, true, {phase:'saved',detail:'ready'}).phase).toBe('saved');
    expect(resolveSyncStatus(true, false, {phase:'processing',detail:'working'}).phase).toBe('saved');
  });
});