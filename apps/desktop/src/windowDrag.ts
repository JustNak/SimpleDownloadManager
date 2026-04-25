const INTERACTIVE_WINDOW_DRAG_SELECTOR = [
  'a[href]',
  'button',
  'input',
  'label',
  'select',
  'textarea',
  '[contenteditable="true"]',
  '[data-no-window-drag]',
  '[role="button"]',
  '[role="menuitem"]',
].join(',');

type WindowDragEventTarget = EventTarget & {
  closest?: (selector: string) => Element | null;
};

type WindowDragBoundary = EventTarget & {
  contains?: (target: EventTarget) => boolean;
};

export function shouldStartWindowDrag({
  button,
  target,
  currentTarget,
}: {
  button: number;
  target: EventTarget | null;
  currentTarget: EventTarget | null;
}): boolean {
  if (button !== 0 || !target || !currentTarget) {
    return false;
  }

  const dragTarget = target as WindowDragEventTarget;
  const dragBoundary = currentTarget as WindowDragBoundary;

  if (typeof dragTarget.closest !== 'function' || typeof dragBoundary.contains !== 'function') {
    return false;
  }

  if (!dragBoundary.contains(target)) {
    return false;
  }

  return !dragTarget.closest(INTERACTIVE_WINDOW_DRAG_SELECTOR);
}
