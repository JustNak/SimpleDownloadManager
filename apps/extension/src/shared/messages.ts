import type { ErrorCode, ExtensionIntegrationSettings, HostToExtensionResponse, QueueSummary } from '@myapp/protocol';

export type PopupRequest =
  | { type: 'popup_ping' }
  | { type: 'popup_get_state' }
  | { type: 'popup_open_app' }
  | { type: 'popup_open_settings' }
  | { type: 'popup_open_options' }
  | { type: 'popup_enqueue'; url: string }
  | { type: 'extension_settings_update'; settings: ExtensionIntegrationSettings };

export interface PopupStateResponse {
  connection: 'checking' | 'connected' | 'host_missing' | 'app_missing' | 'app_unreachable' | 'error';
  isSubmitting: boolean;
  queueSummary?: QueueSummary;
  extensionSettings?: ExtensionIntegrationSettings;
  lastResult?: HostToExtensionResponse;
  lastError?: { code: ErrorCode; message: string };
}
