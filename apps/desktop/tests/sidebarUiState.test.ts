import assert from 'node:assert/strict';
import {
  DEFAULT_SIDEBAR_SECTION_STATE,
  readStoredSidebarSectionState,
  SIDEBAR_SECTION_STATE_STORAGE_KEY,
  writeStoredSidebarSectionState,
} from '../src/sidebarUiState.ts';

const storage = new Map<string, string>();
const sidebarStorage = {
  getItem(key: string) {
    return storage.get(key) ?? null;
  },
  setItem(key: string, value: string) {
    storage.set(key, value);
  },
};

assert.deepEqual(
  readStoredSidebarSectionState(sidebarStorage),
  DEFAULT_SIDEBAR_SECTION_STATE,
  'fresh sessions should expand every sidebar section by default',
);

storage.set(SIDEBAR_SECTION_STATE_STORAGE_KEY, JSON.stringify({
  downloads: false,
  bulk: true,
  torrents: false,
}));

assert.deepEqual(
  readStoredSidebarSectionState(sidebarStorage),
  { downloads: false, bulk: true, torrents: false },
  'stored sidebar section expanded/collapsed state should be restored',
);

storage.set(SIDEBAR_SECTION_STATE_STORAGE_KEY, JSON.stringify({
  downloads: false,
  bulk: 'invalid',
}));

assert.deepEqual(
  readStoredSidebarSectionState(sidebarStorage),
  { downloads: false, bulk: true, torrents: true },
  'invalid or missing section values should fall back per section without discarding valid stored values',
);

storage.set(SIDEBAR_SECTION_STATE_STORAGE_KEY, 'not-json');

assert.deepEqual(
  readStoredSidebarSectionState(sidebarStorage),
  DEFAULT_SIDEBAR_SECTION_STATE,
  'invalid stored sidebar JSON should fall back to the default expanded state',
);

writeStoredSidebarSectionState({ downloads: false, bulk: false, torrents: true }, sidebarStorage);

assert.equal(
  storage.get(SIDEBAR_SECTION_STATE_STORAGE_KEY),
  '{"downloads":false,"bulk":false,"torrents":true}',
  'sidebar section toggles should be written back to local storage',
);
