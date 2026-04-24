export const PROTOCOL_VERSION = 1;
export const HOST_NAME = 'com.myapp.download_manager';
export const PIPE_NAME = '\\\\.\\pipe\\myapp.downloads.v1';
export const MAX_URL_LENGTH = 2048;
export const MAX_METADATA_LENGTH = 512;
export const ALLOWED_URL_PROTOCOLS = ['http:', 'https:'] as const;

export type BrowserKind = 'chrome' | 'edge' | 'firefox';
export type ExtensionEntryPoint = 'context_menu' | 'popup';
export type ExtensionRequestType = 'ping' | 'enqueue_download' | 'open_app' | 'get_status';
export type HostResponseType =
  | 'pong'
  | 'accepted'
  | 'rejected'
  | 'app_not_installed'
  | 'app_unreachable'
  | 'invalid_payload';
export type AppRequestType = 'ping' | 'get_status' | 'enqueue_download' | 'show_window';
export type AppResponseType = 'queued' | 'duplicate_existing_job' | 'invalid_url' | 'blocked_by_policy' | 'ready';

export type DesktopConnectionState = 'checking' | 'connected' | 'host_missing' | 'app_missing' | 'app_unreachable' | 'error';

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

export interface EnqueueDownloadPayload {
  url: string;
  source: RequestSource;
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
  status: 'queued';
  jobId: string;
  appState: 'running' | 'launched';
}

export interface QueueSummary {
  total: number;
  active: number;
  queued: number;
  downloading: number;
  completed: number;
  failed: number;
}

export interface PongPayload {
  appState: 'running' | 'launched';
  extensionVersion?: string;
  connectionState?: DesktopConnectionState;
  queueSummary?: QueueSummary;
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
  | RequestEnvelope<'open_app', OpenAppPayload>
  | RequestEnvelope<'get_status', EmptyPayload>;

export type HostToExtensionResponse =
  | SuccessResponse<'pong', PongPayload>
  | SuccessResponse<'accepted', AcceptedPayload>
  | ErrorResponse;

export type AppRequest =
  | AppRequestEnvelope<'ping', EmptyPayload>
  | AppRequestEnvelope<'get_status', EmptyPayload>
  | AppRequestEnvelope<'enqueue_download', EnqueueDownloadPayload>
  | AppRequestEnvelope<'show_window', OpenAppPayload>;

export type AppResponse =
  | AppSuccessResponse<'ready', { appState: 'running' | 'launched' }>
  | AppSuccessResponse<'queued', { jobId: string; status: 'queued' }>
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
    return { ok: false, code: 'UNSUPPORTED_SCHEME', message: 'Only http and https URLs are supported.' };
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
): ValidationResult<RequestEnvelope<'enqueue_download', EnqueueDownloadPayload>> {
  const validatedUrl = validateHttpUrl(url);
  if (!validatedUrl.ok) {
    return validatedUrl;
  }

  return {
    ok: true,
    value: {
      protocolVersion: PROTOCOL_VERSION,
      requestId,
      type: 'enqueue_download',
      payload: {
        url: validatedUrl.value,
        source: sanitizeSource(source),
      },
    },
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
      return 'Enter a valid http or https URL.';
    default:
      return fallback;
  }
}
