import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const appSource = await readFile(new URL('../src/App.tsx', import.meta.url), 'utf8');
const toastAreaSource = await readFile(new URL('../src/ToastArea.tsx', import.meta.url), 'utf8');

assert.match(
  appSource,
  /const removeToast = useCallback\(\(id: string\) => \{/,
  'toast dismissal should be stable across unrelated App renders',
);

assert.doesNotMatch(
  toastAreaSource,
  /onDismiss=\{\(\) => onDismiss\(toast\.id\)\}/,
  'ToastArea should not create a fresh dismiss closure for each toast on every render',
);

assert.match(
  toastAreaSource,
  /setTimeout\(\(\) => onDismiss\(toast\.id\), TOAST_AUTO_CLOSE_MS\)/,
  'toast auto-close should dismiss by toast id from inside the timer',
);

assert.match(
  toastAreaSource,
  /\[toast\.id, toast\.autoClose, onDismiss\]/,
  'toast auto-close effect should not reset when unrelated toast object fields are re-rendered',
);
