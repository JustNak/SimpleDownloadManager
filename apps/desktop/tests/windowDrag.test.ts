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
assert.equal(shouldStartWindowDrag({ button: 0, target: toolbarGap, currentTarget: makeBoundary(toolbarGap) }), true, 'empty toolbar space should start a window drag');

const toolbarButton = makeTarget(true);
assert.equal(shouldStartWindowDrag({ button: 0, target: toolbarButton, currentTarget: makeBoundary(toolbarButton) }), false, 'interactive toolbar controls should not start a window drag');
assert.equal(shouldStartWindowDrag({ button: 1, target: toolbarGap, currentTarget: makeBoundary(toolbarGap) }), false, 'non-primary pointer buttons should not start a window drag');

const outsideToolbar = makeTarget(false);
assert.equal(shouldStartWindowDrag({ button: 0, target: outsideToolbar, currentTarget: makeBoundary(toolbarGap) }), false, 'targets outside the titlebar boundary should not start a window drag');

const popupTitlebarSource = readFileSync(new URL('../src/PopupTitlebar.svelte', import.meta.url), 'utf8');
const mainTitlebarSource = readFileSync(new URL('../src/Titlebar.svelte', import.meta.url), 'utf8');
const appSource = readFileSync(new URL('../src/App.svelte', import.meta.url), 'utf8');

assert.doesNotMatch(mainTitlebarSource, /<div class="min-w-0 flex-1" data-no-window-drag>/, 'main titlebar should not blanket-disable dragging for the entire slotted command bar');
assert.match(mainTitlebarSource, /<div class="min-w-0 flex-1 cursor-grab active:cursor-grabbing">\{@render children\(\)\}<\/div>/, 'empty slotted titlebar gaps should remain draggable and advertise grab states');
assert.match(appSource, /<label class="relative w-full min-w-0" data-no-window-drag>/, 'the search field composite should opt out of titlebar dragging without blocking adjacent empty titlebar gaps');

assert.match(popupTitlebarSource, /function startDrag\(event: PointerEvent\)/, 'popup titlebars should start dragging from the pointer-down event');
assert.match(popupTitlebarSource, /class="flex h-11 shrink-0 select-none items-center justify-between border-b border-border bg-background/, 'popup titlebars should use the same 44px drag band height as the main window titlebar');
assert.match(popupTitlebarSource, /event\.preventDefault\(\);[\s\S]*appWindow\.startDragging\(\)\.catch/, 'popup titlebars should prevent default only before firing native dragging directly');
assert.match(popupTitlebarSource, /class="flex h-full min-w-0 flex-1 cursor-grab items-center gap-2\.5 px-3 active:cursor-grabbing"/, 'the full non-button titlebar surface should advertise grab and grabbing states');
