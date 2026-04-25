export interface DeletePromptContent {
  title: string;
  description: string;
  checkboxLabel: string;
  confirmLabel: string;
  contextMenuLabel: string;
  selectedSummary: string;
  missingPathLabel: string;
}

export function getDeletePromptContent(selectedCount: number): DeletePromptContent {
  const count = normalizeSelectedCount(selectedCount);
  if (count === 1) {
    return {
      title: 'Delete Download',
      description: 'Remove this download from the list. Disk deletion requires explicit confirmation below.',
      checkboxLabel: 'Delete file from disk',
      confirmLabel: 'Delete',
      contextMenuLabel: 'Delete',
      selectedSummary: '1 download selected',
      missingPathLabel: 'No file path is recorded for this download.',
    };
  }

  return {
    title: `Delete ${count} Downloads`,
    description: 'Remove these downloads from the list. Disk deletion requires explicit confirmation below.',
    checkboxLabel: 'Delete selected files from disk',
    confirmLabel: 'Delete All',
    contextMenuLabel: 'Delete All',
    selectedSummary: `${count} downloads selected`,
    missingPathLabel: 'No file path is recorded for this download.',
  };
}

export function getDeleteContextMenuLabel(selectedCount: number): string {
  return getDeletePromptContent(selectedCount).contextMenuLabel;
}

function normalizeSelectedCount(selectedCount: number): number {
  if (!Number.isFinite(selectedCount) || selectedCount < 1) return 1;
  return Math.floor(selectedCount);
}
