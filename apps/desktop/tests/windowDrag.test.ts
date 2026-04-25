import assert from 'node:assert/strict';
import { shouldStartWindowDrag } from '../src/windowDrag';

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
