import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { shouldStartWindowDrag } from '../src/windowDrag.ts';

function makeTarget(matchesInteractive: boolean): EventTarget {
  return {
    closest: (selector: string) => {
      assert.match(selector, /button/);
      assert.match(selector, /input/);
      assert.match(selector, /select/);
      return matchesInteractive ? {} : null;
    },
  } as unknown as EventTarget;
}

function makeBoundary(insideTarget: EventTarget): EventTarget {
  return {
    contains: (target: EventTarget) => target === insideTarget,
  } as unknown as EventTarget;
}

const toolbarGap = makeTarget(false);
assert.equal(
  shouldStartWindowDrag({
    button: 0,
    target: toolbarGap,
    currentTarget: makeBoundary(toolbarGap),
  }),
  true,
  'empty toolbar space should start a window drag',
);

const toolbarButton = makeTarget(true);
assert.equal(
  shouldStartWindowDrag({
    button: 0,
    target: toolbarButton,
    currentTarget: makeBoundary(toolbarButton),
  }),
  false,
  'interactive toolbar controls should not start a window drag',
);

const closeButtonIcon = makeTarget(true);
assert.equal(
  shouldStartWindowDrag({
    button: 0,
    target: closeButtonIcon,
    currentTarget: makeBoundary(closeButtonIcon),
  }),
  false,
  'popup titlebar close controls should not start a window drag',
);

assert.equal(
  shouldStartWindowDrag({
    button: 1,
    target: toolbarGap,
    currentTarget: makeBoundary(toolbarGap),
  }),
  false,
  'non-primary pointer buttons should not start a window drag',
);

const popupTitlebarSource = readFileSync(new URL('../src/PopupTitlebar.tsx', import.meta.url), 'utf8');

assert.match(
  popupTitlebarSource,
  /const handlePointerDown = async \(event: React\.PointerEvent<HTMLDivElement>\)/,
  'popup titlebars should use pointer-down based dragging like the main window titlebar',
);

assert.match(
  popupTitlebarSource,
  /onPointerDown=\{\(event\) => void handlePointerDown\(event\)\}/,
  'popup titlebar drag handler should be wired to pointer events',
);

assert.doesNotMatch(
  popupTitlebarSource,
  /onMouseDown=\{handleMouseDown\}/,
  'popup titlebar should not rely on the previous mouse-only drag path',
);
