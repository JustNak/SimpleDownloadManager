export type SidebarSectionKey = 'downloads' | 'bulk' | 'torrents';

export interface SidebarSectionState {
  downloads: boolean;
  bulk: boolean;
  torrents: boolean;
}

export interface SidebarSectionStateStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

export const DEFAULT_SIDEBAR_SECTION_STATE: SidebarSectionState = {
  downloads: true,
  bulk: true,
  torrents: true,
};

export const SIDEBAR_SECTION_STATE_STORAGE_KEY = 'simple-download-manager.sidebarSections';

export function readStoredSidebarSectionState(
  storage: SidebarSectionStateStorage | null = getBrowserSidebarStorage(),
): SidebarSectionState {
  if (!storage) return { ...DEFAULT_SIDEBAR_SECTION_STATE };

  try {
    const storedState = storage.getItem(SIDEBAR_SECTION_STATE_STORAGE_KEY);
    if (!storedState) return { ...DEFAULT_SIDEBAR_SECTION_STATE };

    const parsed: unknown = JSON.parse(storedState);
    if (!parsed || typeof parsed !== 'object') return { ...DEFAULT_SIDEBAR_SECTION_STATE };

    return {
      downloads: booleanOrDefault(parsed, 'downloads'),
      bulk: booleanOrDefault(parsed, 'bulk'),
      torrents: booleanOrDefault(parsed, 'torrents'),
    };
  } catch {
    return { ...DEFAULT_SIDEBAR_SECTION_STATE };
  }
}

export function writeStoredSidebarSectionState(
  state: SidebarSectionState,
  storage: SidebarSectionStateStorage | null = getBrowserSidebarStorage(),
): void {
  if (!storage) return;

  try {
    storage.setItem(SIDEBAR_SECTION_STATE_STORAGE_KEY, JSON.stringify(state));
  } catch {
    // Non-critical preference persistence can fail in restricted browser storage modes.
  }
}

function booleanOrDefault(value: object, key: SidebarSectionKey): boolean {
  const candidate = (value as Partial<Record<SidebarSectionKey, unknown>>)[key];
  return typeof candidate === 'boolean' ? candidate : DEFAULT_SIDEBAR_SECTION_STATE[key];
}

function getBrowserSidebarStorage(): SidebarSectionStateStorage | null {
  if (typeof globalThis.localStorage === 'undefined') return null;
  return globalThis.localStorage;
}
