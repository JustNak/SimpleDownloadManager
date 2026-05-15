import { tick } from 'svelte';
import { markPopupReady } from './backend';

let readyPromise: Promise<void> | null = null;

export function revealPopupWhenReady(): Promise<void> {
  readyPromise ??= revealPopupWhenReadyOnce();
  return readyPromise;
}

async function revealPopupWhenReadyOnce(): Promise<void> {
  await tick();
  await new Promise<void>((resolve) => {
    if (typeof requestAnimationFrame !== 'function') {
      resolve();
      return;
    }
    requestAnimationFrame(() => resolve());
  });

  try {
    await markPopupReady();
  } catch (error) {
    console.error('Failed to mark popup window ready.', error);
  } finally {
    document.documentElement.classList.add('popup-ready');
  }
}
