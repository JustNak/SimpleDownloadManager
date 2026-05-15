import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import {
  APPEARANCE_CACHE_KEY,
  DEFAULT_APPEARANCE_SETTINGS,
  DEFAULT_ACCENT_COLOR,
  applyAppearanceToElement,
  appearanceSettingsFromSearchParams,
  cachedAppearanceSettingsFromJson,
  normalizeAccentColor,
  readableForegroundForHex,
  resolveThemeClasses,
  serializeAppearanceSettings,
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
assert.equal(APPEARANCE_CACHE_KEY, 'simple-download-manager-appearance');
assert.deepEqual(DEFAULT_APPEARANCE_SETTINGS, { theme: 'system', accentColor: DEFAULT_ACCENT_COLOR });
assert.equal(
  serializeAppearanceSettings({ theme: 'dark', accentColor: '#06B6D4' }),
  '{"theme":"dark","accentColor":"#06b6d4"}',
);
assert.deepEqual(
  cachedAppearanceSettingsFromJson('{"theme":"oled_dark","accentColor":"#14B8A6"}'),
  { theme: 'oled_dark', accentColor: '#14b8a6' },
);
assert.deepEqual(cachedAppearanceSettingsFromJson('not-json'), DEFAULT_APPEARANCE_SETTINGS);
assert.deepEqual(
  appearanceSettingsFromSearchParams('?window=download-progress&theme=dark&accentColor=%23f97316'),
  { theme: 'dark', accentColor: '#f97316' },
);
assert.deepEqual(
  appearanceSettingsFromSearchParams('?window=download-progress&theme=nope&accentColor=bad'),
  DEFAULT_APPEARANCE_SETTINGS,
);
assert.equal(appearanceSettingsFromSearchParams('?window=download-progress'), null);

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
  '../src/DownloadPromptWindow.svelte',
  '../src/DownloadProgressWindow.svelte',
  '../src/BatchProgressWindow.svelte',
]) {
  const source = readFileSync(new URL(file, import.meta.url), 'utf8');
  assert.equal(source.includes("classList.add('dark')"), false, `${file} should use shared appearance settings instead of forcing dark mode`);
}

const indexHtml = readFileSync(new URL('../index.html', import.meta.url), 'utf8');
assert.match(indexHtml, /<style>[\s\S]*--color-background[\s\S]*body[\s\S]*background-color:\s*var\(--color-background\)/, 'desktop shell should include boot theme CSS before Svelte loads');
assert.match(indexHtml, /src="\/appearance-preload\.js"[\s\S]*src="\/src\/main\.ts"/, 'desktop shell should run appearance preload before the Svelte entry module');

const appCss = readFileSync(new URL('../src/app.css', import.meta.url), 'utf8');
assert.match(appCss, /@keyframes popup-window-enter[\s\S]*opacity[\s\S]*transform/, 'popup shells should have a lightweight themed entrance animation');
assert.match(appCss, /@media \(prefers-reduced-motion: reduce\)[\s\S]*html\.popup-window\.popup-ready \.app-window[\s\S]*animation:\s*none/, 'popup entrance animation should respect reduced-motion preferences');
