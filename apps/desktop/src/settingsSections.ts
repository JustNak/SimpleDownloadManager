export const SETTINGS_SECTIONS = [
  {
    id: 'settings-general',
    href: '#settings-general',
    label: 'General',
    description: 'Folders and limits.',
    iconName: 'general',
  },
  {
    id: 'settings-updates',
    href: '#settings-updates',
    label: 'App Updates',
    description: 'Version controls.',
    iconName: 'updates',
  },
  {
    id: 'settings-torrenting',
    href: '#settings-torrenting',
    label: 'Torrenting',
    description: 'Seeding and peers.',
    iconName: 'torrenting',
  },
  {
    id: 'settings-appearance',
    href: '#settings-appearance',
    label: 'Appearance',
    description: 'Theme and rows.',
    iconName: 'appearance',
  },
  {
    id: 'settings-extension',
    href: '#settings-extension',
    label: 'Web Extension',
    description: 'Browser handoff.',
    iconName: 'extension',
  },
  {
    id: 'settings-native-host',
    href: '#settings-native-host',
    label: 'Native Host',
    description: 'Diagnostics tools.',
    iconName: 'native-host',
  },
] as const;

export type SettingsSectionId = (typeof SETTINGS_SECTIONS)[number]['id'];
