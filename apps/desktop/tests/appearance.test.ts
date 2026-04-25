import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import {
  DEFAULT_ACCENT_COLOR,
  applyAppearanceToElement,
  normalizeAccentColor,
  readableForegroundForHex,
  resolveThemeClasses,
} from '../src/appearance.ts';

class FakeClassList {
  values = new Set<string>();

  toggle(name: string, force?: boolean): boolean {
    const shouldAdd = force ?? !this.values.has(name);
    if (shouldAdd) {
      this.values.add(name);
      return true;
    }
    this.values.delete(name);
    return false;
  }

  contains(name: string): boolean {
    return this.values.has(name);
  }
}

class FakeStyle {
  values = new Map<string, string>();

  setProperty(name: string, value: string) {
    this.values.set(name, value);
  }

  getPropertyValue(name: string): string {
    return this.values.get(name) ?? '';
  }
}

function makeElement() {
  return {
    classList: new FakeClassList(),
    style: new FakeStyle(),
  } as unknown as HTMLElement;
}

assert.equal(normalizeAccentColor('not-a-color'), DEFAULT_ACCENT_COLOR);
assert.equal(normalizeAccentColor('#F43F5E'), '#f43f5e');
assert.equal(readableForegroundForHex('#ffffff'), '#0a0f14');
assert.equal(readableForegroundForHex('#111111'), '#ffffff');

assert.deepEqual(resolveThemeClasses('light', true), { dark: false, oledDark: false });
assert.deepEqual(resolveThemeClasses('dark', false), { dark: true, oledDark: false });
assert.deepEqual(resolveThemeClasses('oled_dark', false), { dark: true, oledDark: true });
assert.deepEqual(resolveThemeClasses('system', true), { dark: true, oledDark: false });
assert.deepEqual(resolveThemeClasses('system', false), { dark: false, oledDark: false });

const element = makeElement();
applyAppearanceToElement(element, { theme: 'oled_dark', accentColor: '#06b6d4' }, false);

assert.equal(element.classList.contains('dark'), true);
assert.equal(element.classList.contains('oled-dark'), true);
assert.equal(element.style.getPropertyValue('--color-primary'), '#06b6d4');
assert.equal(element.style.getPropertyValue('--color-ring'), '#06b6d4');
assert.equal(element.style.getPropertyValue('--color-primary-foreground'), readableForegroundForHex('#06b6d4'));
assert.match(element.style.getPropertyValue('--color-primary-soft'), /#06b6d4 20%/);
assert.match(element.style.getPropertyValue('--color-accent'), /#06b6d4 20%/);
assert.match(element.style.getPropertyValue('--color-selected'), /#06b6d4 24%/);

for (const file of [
  '../src/DownloadPromptWindow.tsx',
  '../src/DownloadProgressWindow.tsx',
  '../src/BatchProgressWindow.tsx',
]) {
  const source = readFileSync(new URL(file, import.meta.url), 'utf8');
  assert.equal(
    source.includes("classList.add('dark')"),
    false,
    `${file} should use shared appearance settings instead of forcing dark mode`,
  );
}
