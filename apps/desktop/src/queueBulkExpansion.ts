export const BULK_MEMBER_ROW_HEIGHT = 32;
export const BULK_MEMBER_PANEL_MAX_HEIGHT = 256;
export const BULK_MEMBER_PANEL_VERTICAL_CHROME = 8;

export function bulkMemberPanelHeight(memberCount: number): number {
  return Math.min(BULK_MEMBER_PANEL_MAX_HEIGHT, Math.max(1, memberCount) * BULK_MEMBER_ROW_HEIGHT);
}

export function bulkExpansionHeight(memberCount: number): number {
  if (memberCount <= 0) return 0;
  return bulkMemberPanelHeight(memberCount) + BULK_MEMBER_PANEL_VERTICAL_CHROME;
}

export function pruneRecordKeys<T>(record: Record<string, T>, allowedKeys: Set<string>): Record<string, T> {
  let changed = false;
  const next: Record<string, T> = {};
  for (const [key, value] of Object.entries(record)) {
    if (allowedKeys.has(key)) {
      next[key] = value;
    } else {
      changed = true;
    }
  }
  return changed ? next : record;
}
