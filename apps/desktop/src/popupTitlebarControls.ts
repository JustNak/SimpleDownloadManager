export type PopupTitlebarControlKind = 'minimize' | 'close';

export interface PopupTitlebarControl {
  kind: PopupTitlebarControlKind;
  label: string;
}

export function popupTitlebarControls(): PopupTitlebarControl[] {
  return [
    { kind: 'minimize', label: 'Minimize' },
    { kind: 'close', label: 'Close' },
  ];
}
