import { invoke as tauriInvoke } from '@tauri-apps/api/core';

type InvokeArgs = Record<string, unknown> | number[] | ArrayBuffer | Uint8Array;
type TauriInvoke = <T>(command: string, args?: InvokeArgs) => Promise<T>;
type Delay = (milliseconds: number) => Promise<void>;

interface TauriCommandInvokerOptions {
  invoke?: TauriInvoke;
  delay?: Delay;
}

const STALE_WEBVIEW_REFERENCE_MESSAGE = 'failed to acquire webview reference';

export const STALE_WEBVIEW_RETRY_DELAY_MS = 75;
export const USER_SAFE_STALE_WEBVIEW_MESSAGE = 'The app window lost its connection to the backend. Reopen the window from the tray and try again.';

export const invokeTauriCommand = createTauriCommandInvoker();

export function createTauriCommandInvoker(options: TauriCommandInvokerOptions = {}) {
  const invokeCommand = options.invoke ?? tauriInvoke;
  const delay = options.delay ?? wait;

  return async function resilientInvoke<T>(command: string, args?: InvokeArgs): Promise<T> {
    try {
      return await invokeCommand<T>(command, args);
    } catch (error) {
      if (!isStaleWebviewReferenceError(error)) {
        throw error;
      }

      await delay(STALE_WEBVIEW_RETRY_DELAY_MS);

      try {
        return await invokeCommand<T>(command, args);
      } catch (retryError) {
        if (isStaleWebviewReferenceError(retryError)) {
          throw new Error(USER_SAFE_STALE_WEBVIEW_MESSAGE);
        }
        throw retryError;
      }
    }
  };
}

export function isStaleWebviewReferenceError(error: unknown): boolean {
  return errorMessage(error).toLowerCase() === STALE_WEBVIEW_REFERENCE_MESSAGE;
}

function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message.trim();
  if (typeof error === 'string') return error.trim();
  if (typeof error === 'object' && error !== null && 'message' in error) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === 'string') return message.trim();
  }
  return '';
}

function wait(milliseconds: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}
