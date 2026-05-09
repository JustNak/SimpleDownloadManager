import type { QueueRowSize } from './types';

export const VIRTUALIZED_QUEUE_THRESHOLD = 100;
export const QUEUE_VIRTUALIZATION_OVERSCAN = 8;

export interface VirtualQueueWindowInput {
  totalCount: number;
  rowSize: QueueRowSize;
  scrollTop: number;
  viewportHeight: number;
  rowHeightOverride?: number;
  extraHeights?: readonly VirtualQueueExtraHeight[];
}

export interface VirtualQueueWindow {
  enabled: boolean;
  startIndex: number;
  endIndex: number;
  topPadding: number;
  bottomPadding: number;
  rowHeight: number;
}

export interface VirtualQueueExtraHeight {
  index: number;
  height: number;
}

export function queueRowHeightForSize(size: QueueRowSize): number {
  switch (size) {
    case 'compact':
      return 28;
    case 'small':
      return 34;
    case 'large':
      return 54;
    case 'damn':
      return 68;
    case 'medium':
    default:
      return 42;
  }
}

export function getVirtualQueueWindow({
  totalCount,
  rowSize,
  scrollTop,
  viewportHeight,
  rowHeightOverride,
  extraHeights = [],
}: VirtualQueueWindowInput): VirtualQueueWindow {
  const rowHeight = rowHeightOverride && rowHeightOverride > 0
    ? rowHeightOverride
    : queueRowHeightForSize(rowSize);
  if (totalCount <= VIRTUALIZED_QUEUE_THRESHOLD) {
    return {
      enabled: false,
      startIndex: 0,
      endIndex: Math.max(0, totalCount),
      topPadding: 0,
      bottomPadding: 0,
      rowHeight,
    };
  }

  const normalizedExtraHeights = normalizeExtraHeights(extraHeights, totalCount);
  const safeScrollTop = Math.max(0, scrollTop);
  const safeViewportHeight = Math.max(rowHeight, viewportHeight);
  const firstVisibleIndex = firstVisibleItemIndex({
    totalCount,
    rowHeight,
    scrollTop: safeScrollTop,
    extraHeights: normalizedExtraHeights,
  });
  const visibleCount = Math.ceil(safeViewportHeight / rowHeight);
  const startIndex = Math.max(0, firstVisibleIndex - QUEUE_VIRTUALIZATION_OVERSCAN);
  const endIndex = Math.min(
    totalCount,
    firstVisibleIndex + visibleCount + QUEUE_VIRTUALIZATION_OVERSCAN,
  );
  const totalHeight = totalVirtualHeight(totalCount, rowHeight, normalizedExtraHeights);

  return {
    enabled: true,
    startIndex,
    endIndex,
    topPadding: virtualOffsetForIndex(startIndex, rowHeight, normalizedExtraHeights),
    bottomPadding: Math.max(0, totalHeight - virtualOffsetForIndex(endIndex, rowHeight, normalizedExtraHeights)),
    rowHeight,
  };
}

function normalizeExtraHeights(
  extraHeights: readonly VirtualQueueExtraHeight[],
  totalCount: number,
): VirtualQueueExtraHeight[] {
  return extraHeights
    .filter((item) => Number.isInteger(item.index) && item.index >= 0 && item.index < totalCount && item.height > 0)
    .sort((left, right) => left.index - right.index);
}

function firstVisibleItemIndex({
  totalCount,
  rowHeight,
  scrollTop,
  extraHeights,
}: {
  totalCount: number;
  rowHeight: number;
  scrollTop: number;
  extraHeights: readonly VirtualQueueExtraHeight[];
}): number {
  let low = 0;
  let high = Math.max(0, totalCount - 1);

  while (low < high) {
    const mid = Math.floor((low + high) / 2);
    const itemEnd = virtualOffsetForIndex(mid + 1, rowHeight, extraHeights);
    if (itemEnd > scrollTop) {
      high = mid;
    } else {
      low = mid + 1;
    }
  }

  return low;
}

function totalVirtualHeight(
  totalCount: number,
  rowHeight: number,
  extraHeights: readonly VirtualQueueExtraHeight[],
): number {
  return virtualOffsetForIndex(totalCount, rowHeight, extraHeights);
}

function virtualOffsetForIndex(
  index: number,
  rowHeight: number,
  extraHeights: readonly VirtualQueueExtraHeight[],
): number {
  const baseOffset = Math.max(0, index) * rowHeight;
  const extraOffset = extraHeights
    .filter((item) => item.index < index)
    .reduce((total, item) => total + item.height, 0);
  return baseOffset + extraOffset;
}
