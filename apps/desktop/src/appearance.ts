import type { Settings } from './types';

export const DEFAULT_ACCENT_COLOR = '#3b82f6';

export type AppearanceSettings = Pick<Settings, 'theme' | 'accentColor'>;

export interface ThemeClassState {
  dark: boolean;
  oledDark: boolean;
}

interface AppearanceElement {
  classList: {
    toggle(name: string, force?: boolean): boolean;
  };
  style: {
    setProperty(name: string, value: string): void;
  };
}

export function normalizeAccentColor(rawColor: string | undefined): string {
  const color = rawColor?.trim() ?? '';
  return /^#[0-9a-f]{6}$/i.test(color) ? color.toLowerCase() : DEFAULT_ACCENT_COLOR;
}

export function readableForegroundForHex(hex: string): string {
  const normalized = normalizeAccentColor(hex);
  const red = Number.parseInt(normalized.slice(1, 3), 16);
  const green = Number.parseInt(normalized.slice(3, 5), 16);
  const blue = Number.parseInt(normalized.slice(5, 7), 16);
  const luminance = (0.2126 * red + 0.7152 * green + 0.0722 * blue) / 255;
  return luminance > 0.58 ? '#0a0f14' : '#ffffff';
}

export function resolveThemeClasses(
  theme: Settings['theme'],
  systemPrefersDark: boolean,
): ThemeClassState {
  const oledDark = theme === 'oled_dark';
  const dark = oledDark || theme === 'dark' || (theme === 'system' && systemPrefersDark);
  return { dark, oledDark };
}

export function applyAppearanceToElement(
  element: AppearanceElement,
  settings: AppearanceSettings,
  systemPrefersDark: boolean,
): ThemeClassState {
  const themeClasses = resolveThemeClasses(settings.theme, systemPrefersDark);
  const accent = normalizeAccentColor(settings.accentColor);
  const foreground = readableForegroundForHex(accent);

  element.classList.toggle('dark', themeClasses.dark);
  element.classList.toggle('oled-dark', themeClasses.oledDark);
  element.style.setProperty('--color-primary', accent);
  element.style.setProperty('--color-ring', accent);
  element.style.setProperty('--color-primary-foreground', foreground);
  element.style.setProperty('--color-primary-soft', `color-mix(in oklch, ${accent} 20%, var(--color-background))`);
  element.style.setProperty('--color-accent', `color-mix(in oklch, ${accent} 20%, var(--color-background))`);
  element.style.setProperty('--color-accent-foreground', accent);
  element.style.setProperty('--color-selected', `color-mix(in oklch, ${accent} 24%, var(--color-background))`);

  return themeClasses;
}

export function getSystemPrefersDark(): boolean {
  if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') {
    return false;
  }
  return window.matchMedia('(prefers-color-scheme: dark)').matches;
}

export function applyAppearance(
  settings: AppearanceSettings,
  options: { root?: AppearanceElement; systemPrefersDark?: boolean } = {},
): ThemeClassState {
  const root = options.root ?? document.documentElement;
  const systemPrefersDark = options.systemPrefersDark ?? getSystemPrefersDark();
  return applyAppearanceToElement(root, settings, systemPrefersDark);
}
