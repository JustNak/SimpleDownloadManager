import assert from 'node:assert/strict';
import {
  STALE_WEBVIEW_RETRY_DELAY_MS,
  USER_SAFE_STALE_WEBVIEW_MESSAGE,
  createTauriCommandInvoker,
  isStaleWebviewReferenceError,
} from '../src/tauriInvoke.ts';

assert.equal(isStaleWebviewReferenceError('failed to acquire webview reference'), true);
assert.equal(
  isStaleWebviewReferenceError(new Error('FAILED TO ACQUIRE WEBVIEW REFERENCE')),
  true,
);
assert.equal(isStaleWebviewReferenceError(new Error('failed to resolve download')), false);

{
  let attempts = 0;
  const delays: number[] = [];
  const invokeCommand = createTauriCommandInvoker({
    invoke: async (command, args) => {
      attempts += 1;
      if (attempts === 1) {
        throw new Error('failed to acquire webview reference');
      }
      return { command, args, ok: true };
    },
    delay: async (milliseconds) => {
      delays.push(milliseconds);
    },
  });

  const result = await invokeCommand('add_jobs', { urls: ['https://example.com/file.zip'] });

  assert.deepEqual(result, {
    command: 'add_jobs',
    args: { urls: ['https://example.com/file.zip'] },
    ok: true,
  });
  assert.equal(attempts, 2);
  assert.deepEqual(delays, [STALE_WEBVIEW_RETRY_DELAY_MS]);
}

{
  let attempts = 0;
  const invokeCommand = createTauriCommandInvoker({
    invoke: async () => {
      attempts += 1;
      throw attempts === 1
        ? 'failed to acquire webview reference'
        : new Error('FAILED TO ACQUIRE WEBVIEW REFERENCE');
    },
    delay: async () => undefined,
  });

  await assert.rejects(
    () => invokeCommand('add_jobs'),
    new RegExp(USER_SAFE_STALE_WEBVIEW_MESSAGE.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')),
  );
  assert.equal(attempts, 2);
}

{
  let attempts = 0;
  const invokeCommand = createTauriCommandInvoker({
    invoke: async () => {
      attempts += 1;
      throw new Error('download resolver failed');
    },
    delay: async () => {
      throw new Error('unrelated errors should not be delayed');
    },
  });

  await assert.rejects(() => invokeCommand('add_jobs'), /download resolver failed/);
  assert.equal(attempts, 1);
}
