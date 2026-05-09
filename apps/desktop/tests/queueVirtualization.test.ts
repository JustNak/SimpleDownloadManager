import assert from 'node:assert/strict';
import {
  QUEUE_VIRTUALIZATION_OVERSCAN,
  VIRTUALIZED_QUEUE_THRESHOLD,
  getVirtualQueueWindow,
  queueRowHeightForSize,
} from '../src/queueVirtualization.ts';

assert.equal(VIRTUALIZED_QUEUE_THRESHOLD, 100);
assert.equal(QUEUE_VIRTUALIZATION_OVERSCAN, 8);
assert.equal(queueRowHeightForSize('compact'), 28);
assert.equal(queueRowHeightForSize('small'), 34);
assert.equal(queueRowHeightForSize('medium'), 42);
assert.equal(queueRowHeightForSize('large'), 54);
assert.equal(queueRowHeightForSize('damn'), 68);

assert.deepEqual(
  getVirtualQueueWindow({
    totalCount: 100,
    rowSize: 'medium',
    scrollTop: 420,
    viewportHeight: 420,
  }),
  {
    enabled: false,
    startIndex: 0,
    endIndex: 100,
    topPadding: 0,
    bottomPadding: 0,
    rowHeight: 42,
  },
  'virtualization should stay off at the threshold to avoid churn on normal queues',
);

assert.deepEqual(
  getVirtualQueueWindow({
    totalCount: 150,
    rowSize: 'medium',
    scrollTop: 420,
    viewportHeight: 420,
  }),
  {
    enabled: true,
    startIndex: 2,
    endIndex: 28,
    topPadding: 84,
    bottomPadding: 5124,
    rowHeight: 42,
  },
  'virtualization should include overscan and preserve total list height',
);

assert.deepEqual(
  getVirtualQueueWindow({
    totalCount: 150,
    rowSize: 'large',
    scrollTop: 8_000,
    viewportHeight: 540,
  }),
  {
    enabled: true,
    startIndex: 140,
    endIndex: 150,
    topPadding: 7_560,
    bottomPadding: 0,
    rowHeight: 54,
  },
  'virtualization should clamp the end of the list without adding trailing padding',
);

assert.deepEqual(
  getVirtualQueueWindow({
    totalCount: 150,
    rowSize: 'medium',
    scrollTop: 300,
    viewportHeight: 168,
    extraHeights: [{ index: 2, height: 100 }],
  }),
  {
    enabled: true,
    startIndex: 0,
    endIndex: 16,
    topPadding: 0,
    bottomPadding: 5628,
    rowHeight: 42,
  },
  'virtualization should include expanded row height in trailing padding',
);

assert.deepEqual(
  getVirtualQueueWindow({
    totalCount: 150,
    rowSize: 'compact',
    rowHeightOverride: 32,
    scrollTop: 320,
    viewportHeight: 160,
  }),
  {
    enabled: true,
    startIndex: 2,
    endIndex: 23,
    topPadding: 64,
    bottomPadding: 4064,
    rowHeight: 32,
  },
  'virtualization should support fixed-height nested member rows',
);
