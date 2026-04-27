export const PROTOCOL_VERSION = 1;
export const HOST_NAME = 'com.myapp.download_manager';
export const PIPE_NAME = '\\\\.\\pipe\\myapp.downloads.v1';
export const MAX_URL_LENGTH = 2048;
export const MAX_METADATA_LENGTH = 512;
export const MAX_HANDOFF_AUTH_HEADERS = 16;
export const MAX_HANDOFF_AUTH_HEADER_NAME_LENGTH = 64;
export const MAX_HANDOFF_AUTH_HEADER_VALUE_LENGTH = 16 * 1024;
export const ALLOWED_URL_PROTOCOLS = ['http:', 'https:', 'magnet:'] as const;
export const DEFAULT_EXTENSION_LISTEN_PORT = 1420;

export type BrowserKind = 'chrome' | 'edge' | 'firefox';
export type ExtensionEntryPoint = 'context_menu' | 'popup' | 'browser_download';
export type ExtensionRequestType =
  | 'ping'
  | 'enqueue_download'
  | 'prompt_download'
  | 'open_app'
  | 'get_status'
  | 'save_extension_settings';
export type HostResponseType =
  | 'pong'
  | 'accepted'
  | 'rejected'
  | 'app_not_installed'
  | 'app_unreachable'
  | 'invalid_payload';
export type AppRequestType =
  | 'ping'
  | 'get_status'
  | 'enqueue_download'
  | 'prompt_download'
  | 'show_window'
  | 'save_extension_settings';
export type AppResponseType =
  | 'queued'
  | 'duplicate_existing_job'
  | 'prompt_canceled'
  | 'invalid_url'
  | 'blocked_by_policy'
  | 'ready';

export type DesktopConnectionState = 'checking' | 'connected' | 'host_missing' | 'app_missing' | 'app_unreachable' | 'error';
export type DownloadHandoffMode = 'off' | 'ask' | 'auto';

export type ErrorCode =
  | 'INVALID_PAYLOAD'
  | 'INVALID_URL'
  | 'UNSUPPORTED_SCHEME'
  | 'URL_TOO_LONG'
  | 'METADATA_TOO_LARGE'
  | 'HOST_NOT_AVAILABLE'
  | 'HOST_REGISTRATION_MISSING'
  | 'HOST_PROTOCOL_MISMATCH'
  | 'APP_NOT_INSTALLED'
  | 'APP_UNREACHABLE'
  | 'APP_TIMEOUT'
  | 'DESTINATION_NOT_CONFIGURED'
  | 'DESTINATION_INVALID'
  | 'DUPLICATE_JOB'
  | 'PERMISSION_DENIED'
  | 'PROTECTED_DOWNLOAD_AUTH_REQUIRED'
  | 'RATE_LIMITED'
  | 'DOWNLOAD_FAILED'
  | 'INTERNAL_ERROR';

export interface RequestSource {
  entryPoint: ExtensionEntryPoint;
  browser: BrowserKind;
  extensionVersion: string;
  pageUrl?: string;
  pageTitle?: string;
  referrer?: string;
  incognito?: boolean;
}

export interface HandoffAuthHeader {
  name: string;
  value: string;
}

export interface HandoffAuth {
  headers: HandoffAuthHeader[];
}

export interface EnqueueDownloadPayload {
  url: string;
  source: RequestSource;
  handoffAuth?: HandoffAuth;
}

export interface PromptDownloadPayload {
  url: string;
  source: RequestSource;
  suggestedFilename?: string;
  totalBytes?: number;
  handoffAuth?: HandoffAuth;
}

export interface OpenAppPayload {
  reason: 'user_request' | 'reconnect';
}

export interface EmptyPayload {
  [key: string]: never;
}

export interface RequestEnvelope<TType extends string, TPayload> {
  protocolVersion: number;
  requestId: string;
  type: TType;
  payload: TPayload;
}

export interface SuccessResponse<TType extends HostResponseType, TPayload> {
  ok: true;
  requestId: string;
  type: TType;
  payload: TPayload;
}

export interface ErrorResponse<TType extends HostResponseType = HostResponseType> {
  ok: false;
  requestId: string;
  type: TType;
  code: ErrorCode;
  message: string;
}

export interface AcceptedPayload {
  status: 'queued' | 'duplicate_existing_job' | 'canceled';
  jobId?: string;
  filename?: string;
  appState: 'running' | 'launched';
}

export interface QueueSummary {
  total: number;
  active: number;
  attention: number;
  queued: number;
  downloading: number;
  completed: number;
  failed: number;
}

export interface ExtensionIntegrationSettings {
  enabled: boolean;
  downloadHandoffMode: DownloadHandoffMode;
  listenPort: number;
  contextMenuEnabled: boolean;
  showProgressAfterHandoff: boolean;
  showBadgeStatus: boolean;
  excludedHosts: string[];
  ignoredFileExtensions: string[];
  authenticatedHandoffEnabled: boolean;
  authenticatedHandoffHosts: string[];
}

export function normalizeExcludedHostPattern(value: string): string {
  let pattern = value
    .trim()
    .replace(/^https?:\/\//i, '')
    .replace(/^[^@/]+@/, '')
    .replace(/[/?#].*$/, '')
    .toLowerCase();

  pattern = pattern.replace(/:\d+$/, '');
  pattern = pattern.replace(/^\.+|\.+$/g, '');

  if (
    !pattern
    || pattern.includes('/')
    || pattern.includes('\\')
    || /\s/.test(pattern)
    || !/[a-z0-9]/.test(pattern)
    || !/^[a-z0-9.*-]+$/.test(pattern)
  ) {
    return '';
  }

  return pattern;
}

export function isHostnameExcludedByPatterns(hostname: string, patterns: string[]): boolean {
  const normalizedHostname = normalizeExcludedHostPattern(hostname);
  if (!normalizedHostname) return false;

  return patterns.some((pattern) => {
    const normalizedPattern = normalizeExcludedHostPattern(pattern);
    if (!normalizedPattern) return false;

    if (normalizedPattern.includes('*')) {
      return wildcardHostPatternRegex(normalizedPattern).test(normalizedHostname);
    }

    return normalizedHostname === normalizedPattern || normalizedHostname.endsWith(`.${normalizedPattern}`);
  });
}

export function isUrlHostExcludedByPatterns(url: string, patterns: string[]): boolean {
  try {
    return isHostnameExcludedByPatterns(new URL(url).hostname, patterns);
  } catch {
    return false;
  }
}

function wildcardHostPatternRegex(pattern: string): RegExp {
  const escaped = pattern
    .split('*')
    .map(escapeRegExp)
    .join('[^.]*');
  return new RegExp(`^${escaped}$`);
}

function escapeRegExp(value: string): string {
  return value.replace(/[\\^$.*+?()[\]{}|]/g, '\\$&');
}

export interface PongPayload {
  appState: 'running' | 'launched';
  extensionVersion?: string;
  connectionState?: DesktopConnectionState;
  queueSummary?: QueueSummary;
  extensionSettings?: ExtensionIntegrationSettings;
}

export interface AppRequestEnvelope<TType extends AppRequestType, TPayload> {
  protocolVersion: number;
  requestId: string;
  type: TType;
  payload: TPayload;
}

export interface AppSuccessResponse<TType extends AppResponseType, TPayload> {
  ok: true;
  requestId: string;
  type: TType;
  payload: TPayload;
}

export interface AppErrorResponse {
  ok: false;
  requestId: string;
  type: AppResponseType;
  code: ErrorCode;
  message: string;
}

export type ExtensionToHostRequest =
  | RequestEnvelope<'ping', EmptyPayload>
  | RequestEnvelope<'enqueue_download', EnqueueDownloadPayload>
  | RequestEnvelope<'prompt_download', PromptDownloadPayload>
  | RequestEnvelope<'open_app', OpenAppPayload>
  | RequestEnvelope<'get_status', EmptyPayload>
  | RequestEnvelope<'save_extension_settings', ExtensionIntegrationSettings>;

export type HostToExtensionResponse =
  | SuccessResponse<'pong', PongPayload>
  | SuccessResponse<'accepted', AcceptedPayload>
  | ErrorResponse;

export type AppRequest =
  | AppRequestEnvelope<'ping', EmptyPayload>
  | AppRequestEnvelope<'get_status', EmptyPayload>
  | AppRequestEnvelope<'enqueue_download', EnqueueDownloadPayload>
  | AppRequestEnvelope<'prompt_download', PromptDownloadPayload>
  | AppRequestEnvelope<'show_window', OpenAppPayload>
  | AppRequestEnvelope<'save_extension_settings', ExtensionIntegrationSettings>;

export type AppResponse =
  | AppSuccessResponse<
      'ready',
      {
        appState: 'running' | 'launched';
        connectionState?: DesktopConnectionState;
        queueSummary?: QueueSummary;
        extensionSettings?: ExtensionIntegrationSettings;
      }
    >
  | AppSuccessResponse<'queued', { jobId: string; filename?: string; status: 'queued' }>
  | AppSuccessResponse<
      'duplicate_existing_job',
      { jobId: string; filename?: string; status: 'duplicate_existing_job' }
    >
  | AppSuccessResponse<'prompt_canceled', { status: 'canceled' }>
  | AppErrorResponse;

export type ValidationResult<T> =
  | { ok: true; value: T }
  | { ok: false; code: ErrorCode; message: string };

export function createRequestId(): string {
  return crypto.randomUUID();
}

export function validateHttpUrl(input: string): ValidationResult<string> {
  if (!input.trim()) {
    return { ok: false, code: 'INVALID_URL', message: 'URL is required.' };
  }

  if (input.length > MAX_URL_LENGTH) {
    return { ok: false, code: 'URL_TOO_LONG', message: `URL exceeds ${MAX_URL_LENGTH} characters.` };
  }

  let parsed: URL;

  try {
    parsed = new URL(input);
  } catch {
    return { ok: false, code: 'INVALID_URL', message: 'URL is not valid.' };
  }

  if (!ALLOWED_URL_PROTOCOLS.includes(parsed.protocol as (typeof ALLOWED_URL_PROTOCOLS)[number])) {
    return { ok: false, code: 'UNSUPPORTED_SCHEME', message: 'Only http, https, and magnet URLs are supported.' };
  }

  return { ok: true, value: parsed.toString() };
}

export function trimMetadata(value: string | undefined): string | undefined {
  if (!value) {
    return undefined;
  }

  return value.slice(0, MAX_METADATA_LENGTH);
}

export function sanitizeSource(source: RequestSource): RequestSource {
  return {
    entryPoint: source.entryPoint,
    browser: source.browser,
    extensionVersion: trimMetadata(source.extensionVersion) ?? '0.0.0',
    pageUrl: trimMetadata(source.pageUrl),
    pageTitle: trimMetadata(source.pageTitle),
    referrer: trimMetadata(source.referrer),
    incognito: source.incognito ?? false,
  };
}

export function isAllowedHandoffAuthHeaderName(name: string): boolean {
  const normalized = name.trim().toLowerCase();
  return normalized === 'cookie'
    || normalized === 'authorization'
    || normalized === 'referer'
    || normalized === 'origin'
    || normalized === 'user-agent'
    || normalized === 'accept'
    || normalized === 'accept-language'
    || normalized.startsWith('sec-fetch-')
    || normalized.startsWith('sec-ch-ua');
}

export function sanitizeHandoffAuth(auth: HandoffAuth | undefined): HandoffAuth | undefined {
  if (!auth?.headers?.length) {
    return undefined;
  }

  const headers: HandoffAuthHeader[] = [];
  for (const header of auth.headers) {
    if (headers.length >= MAX_HANDOFF_AUTH_HEADERS) {
      break;
    }

    const name = header.name.trim();
    const value = header.value;
    if (
      !name
      || name.length > MAX_HANDOFF_AUTH_HEADER_NAME_LENGTH
      || value.length > MAX_HANDOFF_AUTH_HEADER_VALUE_LENGTH
      || /[\r\n:]/.test(name)
      || /[\r\n]/.test(value)
      || !isAllowedHandoffAuthHeaderName(name)
    ) {
      continue;
    }

    headers.push({ name, value });
  }

  return headers.length > 0 ? { headers } : undefined;
}

export function createPingRequest(requestId = createRequestId()): RequestEnvelope<'ping', EmptyPayload> {
  return {
    protocolVersion: PROTOCOL_VERSION,
    requestId,
    type: 'ping',
    payload: {},
  };
}

export function createGetStatusRequest(requestId = createRequestId()): RequestEnvelope<'get_status', EmptyPayload> {
  return {
    protocolVersion: PROTOCOL_VERSION,
    requestId,
    type: 'get_status',
    payload: {},
  };
}

export function createOpenAppRequest(
  payload: OpenAppPayload,
  requestId = createRequestId(),
): RequestEnvelope<'open_app', OpenAppPayload> {
  return {
    protocolVersion: PROTOCOL_VERSION,
    requestId,
    type: 'open_app',
    payload,
  };
}

export function createEnqueueDownloadRequest(
  url: string,
  source: RequestSource,
  requestId = createRequestId(),
  handoffAuth?: HandoffAuth,
): ValidationResult<RequestEnvelope<'enqueue_download', EnqueueDownloadPayload>> {
  const validatedUrl = validateHttpUrl(url);
  if (!validatedUrl.ok) {
    return validatedUrl;
  }

  const sanitizedAuth = sanitizeHandoffAuth(handoffAuth);
  return {
    ok: true,
    value: {
      protocolVersion: PROTOCOL_VERSION,
      requestId,
      type: 'enqueue_download',
      payload: {
        url: validatedUrl.value,
        source: sanitizeSource(source),
        ...(sanitizedAuth ? { handoffAuth: sanitizedAuth } : {}),
      },
    },
  };
}

export function createPromptDownloadRequest(
  url: string,
  source: RequestSource,
  metadata: { suggestedFilename?: string; totalBytes?: number; handoffAuth?: HandoffAuth } = {},
  requestId = createRequestId(),
): ValidationResult<RequestEnvelope<'prompt_download', PromptDownloadPayload>> {
  const validatedUrl = validateHttpUrl(url);
  if (!validatedUrl.ok) {
    return validatedUrl;
  }

  const totalBytes =
    typeof metadata.totalBytes === 'number' && Number.isFinite(metadata.totalBytes) && metadata.totalBytes > 0
      ? Math.floor(metadata.totalBytes)
      : undefined;

  const sanitizedAuth = sanitizeHandoffAuth(metadata.handoffAuth);
  return {
    ok: true,
    value: {
      protocolVersion: PROTOCOL_VERSION,
      requestId,
      type: 'prompt_download',
      payload: {
        url: validatedUrl.value,
        source: sanitizeSource(source),
        suggestedFilename: trimMetadata(metadata.suggestedFilename),
        totalBytes,
        ...(sanitizedAuth ? { handoffAuth: sanitizedAuth } : {}),
      },
    },
  };
}

export function createSaveExtensionSettingsRequest(
  settings: ExtensionIntegrationSettings,
  requestId = createRequestId(),
): RequestEnvelope<'save_extension_settings', ExtensionIntegrationSettings> {
  return {
    protocolVersion: PROTOCOL_VERSION,
    requestId,
    type: 'save_extension_settings',
    payload: settings,
  };
}

export function isErrorResponse(response: HostToExtensionResponse): response is ErrorResponse {
  return response.ok === false;
}

export function toUserFacingMessage(code: ErrorCode, fallback: string): string {
  switch (code) {
    case 'HOST_NOT_AVAILABLE':
    case 'HOST_REGISTRATION_MISSING':
      return 'The native host is not available. Reinstall the desktop app integration.';
    case 'APP_NOT_INSTALLED':
      return 'The desktop app is not installed.';
    case 'APP_UNREACHABLE':
      return 'The desktop app could not be reached.';
    case 'HOST_PROTOCOL_MISMATCH':
      return 'The browser extension and desktop app are out of date with each other. Update both components.';
    case 'INVALID_URL':
    case 'UNSUPPORTED_SCHEME':
      return 'Enter a valid http, https, or magnet URL.';
    case 'PROTECTED_DOWNLOAD_AUTH_REQUIRED':
      return 'This site requires your browser session. Enable Protected Downloads or let the browser handle this download.';
    default:
      return fallback;
  }
}
