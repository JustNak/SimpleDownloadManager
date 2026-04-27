import assert from 'node:assert/strict';
import { runPopupAction } from '../src/popupActions.ts';

{
  const events: string[] = [];

  const result = await runPopupAction({
    action: async () => {
      events.push('action');
    },
    close: async () => {
      events.push('close');
    },
  });

  assert.deepEqual(events, ['action', 'close'], 'successful popup actions should run before closing');
  assert.deepEqual(result, { ok: true }, 'successful popup actions should report success');
}

{
  const events: string[] = [];

  const result = await runPopupAction({
    action: async () => {
      events.push('action');
      throw new Error('Could not reveal file');
    },
    close: async () => {
      events.push('close');
    },
    fallbackMessage: 'Action failed.',
  });

  assert.deepEqual(events, ['action'], 'failed popup actions should leave the popup open');
  assert.deepEqual(
    result,
    { ok: false, message: 'Could not reveal file' },
    'failed popup actions should expose the backend error message',
  );
}
