export type PopupActionResult =
  | { ok: true }
  | { ok: false; message: string };

export async function runPopupAction({
  action,
  close,
  fallbackMessage = 'Action failed.',
}: {
  action: () => Promise<void>;
  close?: () => Promise<void> | void;
  fallbackMessage?: string;
}): Promise<PopupActionResult> {
  try {
    await action();
    await close?.();
    return { ok: true };
  } catch (error) {
    return { ok: false, message: getErrorMessage(error, fallbackMessage) };
  }
}

function getErrorMessage(error: unknown, fallback: string): string {
  if (error instanceof Error && error.message.trim()) {
    return error.message;
  }

  if (typeof error === 'string' && error.trim()) {
    return error;
  }

  if (typeof error === 'object' && error !== null && 'message' in error) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === 'string' && message.trim()) {
      return message;
    }
  }

  return fallback;
}
