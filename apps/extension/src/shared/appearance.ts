import type { AppearanceSettings, AppearanceTheme } from '@myapp/protocol';

export const DEFAULT_ACCENT_COLOR = '#3b82f6';
export const APPEARANCE_CACHE_KEY = 'simple-download-manager-appearance';
export const DEFAULT_APPEARANCE_SETTINGS: AppearanceSettings = {
  theme: 'system',
  accentColor: DEFAULT_ACCENT_COLOR,
};

export interface ExtensionThemeClassState {
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

export function normalizeAppearanceSettings(settings?: Partial<AppearanceSettings>): AppearanceSettings {
  return {
    theme: normalizeTheme(settings?.theme),
    accentColor: normalizeAccentColor(settings?.accentColor),
  };
}

export function serializeAppearanceSettings(settings?: Partial<AppearanceSettings>): string {
  return JSON.stringify(normalizeAppearanceSettings(settings));
}

export function cachedAppearanceSettingsFromJson(rawValue: string | null | undefined): AppearanceSettings {
  if (!rawValue) {
    return DEFAULT_APPEARANCE_SETTINGS;
  }

  try {
    return normalizeAppearanceSettings(JSON.parse(rawValue) as Partial<AppearanceSettings>);
  } catch {
    return DEFAULT_APPEARANCE_SETTINGS;
  }
}

export function readableForegroundForHex(hex: string): string {
  const normalized = normalizeAccentColor(hex);
  const red = Number.parseInt(normalized.slice(1, 3), 16);
  const green = Number.parseInt(normalized.slice(3, 5), 16);
  const blue = Number.parseInt(normalized.slice(5, 7), 16);
  const luminance = (0.2126 * red + 0.7152 * green + 0.0722 * blue) / 255;
  return luminance > 0.58 ? '#0a0f14' : '#ffffff';
}

export function resolveExtensionThemeClasses(
  theme: AppearanceTheme,
  systemPrefersDark: boolean,
): ExtensionThemeClassState {
  const oledDark = theme === 'oled_dark';
  const dark = oledDark || theme === 'dark' || (theme === 'system' && systemPrefersDark);
  return { dark, oledDark };
}

export function applyExtensionAppearanceToElement(
  element: AppearanceElement,
  settings: Partial<AppearanceSettings> | undefined,
  systemPrefersDark: boolean,
): ExtensionThemeClassState {
  const normalized = normalizeAppearanceSettings(settings);
  const themeClasses = resolveExtensionThemeClasses(normalized.theme, systemPrefersDark);
  const accent = normalizeAccentColor(normalized.accentColor);
  const foreground = readableForegroundForHex(accent);

  element.classList.toggle('light', normalized.theme === 'light');
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

export function applyExtensionAppearance(
  settings: Partial<AppearanceSettings> | undefined,
  options: { root?: AppearanceElement; systemPrefersDark?: boolean; cache?: boolean } = {},
): ExtensionThemeClassState {
  const root = options.root ?? document.documentElement;
  const systemPrefersDark = options.systemPrefersDark ?? getSystemPrefersDark();
  const normalized = normalizeAppearanceSettings(settings);
  if (options.cache !== false) {
    cacheAppearanceSettings(normalized);
  }
  return applyExtensionAppearanceToElement(root, normalized, systemPrefersDark);
}

function normalizeTheme(theme: AppearanceTheme | undefined): AppearanceTheme {
  return theme === 'light' || theme === 'dark' || theme === 'oled_dark' || theme === 'system'
    ? theme
    : DEFAULT_APPEARANCE_SETTINGS.theme;
}

function getSystemPrefersDark(): boolean {
  if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') {
    return false;
  }
  return window.matchMedia('(prefers-color-scheme: dark)').matches;
}

function cacheAppearanceSettings(settings: AppearanceSettings): void {
  if (typeof window === 'undefined') {
    return;
  }

  try {
    window.localStorage?.setItem(APPEARANCE_CACHE_KEY, serializeAppearanceSettings(settings));
  } catch {
    // Browser extension pages can still render correctly if localStorage is unavailable.
  }
}
