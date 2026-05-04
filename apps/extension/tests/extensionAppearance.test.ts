import assert from 'node:assert/strict';
import {
  DEFAULT_APPEARANCE_SETTINGS,
  applyExtensionAppearanceToElement,
  normalizeAccentColor,
  readableForegroundForHex,
  resolveExtensionThemeClasses,
} from '../src/shared/appearance.ts';

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
  };
}

assert.deepEqual(DEFAULT_APPEARANCE_SETTINGS, { theme: 'system', accentColor: '#3b82f6' });
assert.equal(normalizeAccentColor('not-a-color'), '#3b82f6');
assert.equal(normalizeAccentColor('#06B6D4'), '#06b6d4');
assert.equal(readableForegroundForHex('#ffffff'), '#0a0f14');
assert.equal(readableForegroundForHex('#111111'), '#ffffff');

assert.deepEqual(resolveExtensionThemeClasses('light', true), { dark: false, oledDark: false });
assert.deepEqual(resolveExtensionThemeClasses('dark', false), { dark: true, oledDark: false });
assert.deepEqual(resolveExtensionThemeClasses('oled_dark', false), { dark: true, oledDark: true });
assert.deepEqual(resolveExtensionThemeClasses('system', true), { dark: true, oledDark: false });

const element = makeElement();
applyExtensionAppearanceToElement(element, { theme: 'oled_dark', accentColor: '#06b6d4' }, false);

assert.equal(element.classList.contains('dark'), true);
assert.equal(element.classList.contains('oled-dark'), true);
assert.equal(element.style.getPropertyValue('--color-primary'), '#06b6d4');
assert.equal(element.style.getPropertyValue('--color-ring'), '#06b6d4');
assert.equal(element.style.getPropertyValue('--color-primary-foreground'), '#ffffff');
assert.match(element.style.getPropertyValue('--color-primary-soft'), /#06b6d4 20%/);
assert.match(element.style.getPropertyValue('--color-accent'), /#06b6d4 20%/);
assert.match(element.style.getPropertyValue('--color-selected'), /#06b6d4 24%/);
