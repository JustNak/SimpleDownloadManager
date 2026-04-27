import {
  sanitizeHandoffAuth,
  type ExtensionIntegrationSettings,
  type HandoffAuth,
  type HandoffAuthHeader,
} from '@myapp/protocol';

export type HandoffAuthRequestHeader = {
  name: string;
  value?: string;
};

export type HandoffAuthRequestDetails = {
  requestId?: string;
  url: string;
  method?: string;
  incognito?: boolean;
  requestHeaders?: HandoffAuthRequestHeader[];
};

type CapturedAuth = {
  requestId?: string;
  url: string;
  incognito: boolean;
  capturedAt: number;
  auth: HandoffAuth;
};

const CAPTURE_TTL_MS = 30_000;
const capturedByRequestId = new Map<string, CapturedAuth>();
const capturedByUrl = new Map<string, CapturedAuth[]>();

export function filterHandoffAuthHeaders(headers: HandoffAuthRequestHeader[] | undefined): HandoffAuthHeader[] {
  return sanitizeHandoffAuth({
    headers: (headers ?? []).map((header) => ({
      name: header.name,
      value: header.value ?? '',
    })),
  })?.headers ?? [];
}

export function buildHandoffAuthForUrl(
  url: string,
  headers: HandoffAuthRequestHeader[] | undefined,
  settings: ExtensionIntegrationSettings,
): HandoffAuth | undefined {
  if (!settings.authenticatedHandoffEnabled || !isHttpUrl(url)) {
    return undefined;
  }

  const filteredHeaders = filterHandoffAuthHeaders(headers);
  return filteredHeaders.length > 0 ? { headers: filteredHeaders } : undefined;
}

export function captureHandoffAuthHeaders(
  details: HandoffAuthRequestDetails,
  now = Date.now(),
): void {
  if (details.method && details.method.toUpperCase() !== 'GET') {
    return;
  }

  if (!isHttpUrl(details.url)) {
    return;
  }

  const filteredHeaders = filterHandoffAuthHeaders(details.requestHeaders);
  const auth = filteredHeaders.length > 0 ? { headers: filteredHeaders } : undefined;
  if (!auth) {
    return;
  }

  pruneCapturedAuth(now);
  const captured: CapturedAuth = {
    requestId: details.requestId,
    url: normalizeUrlKey(details.url),
    incognito: details.incognito ?? false,
    capturedAt: now,
    auth,
  };

  if (captured.requestId) {
    capturedByRequestId.set(captured.requestId, captured);
  }

  const byUrl = capturedByUrl.get(captured.url) ?? [];
  byUrl.push(captured);
  capturedByUrl.set(captured.url, byUrl.slice(-8));
}

export function takeCapturedHandoffAuth(
  details: Pick<HandoffAuthRequestDetails, 'requestId' | 'url' | 'incognito'>,
  settings: ExtensionIntegrationSettings,
  now = Date.now(),
): HandoffAuth | undefined {
  if (!settings.authenticatedHandoffEnabled) {
    return undefined;
  }

  const captured = findCapturedHandoffAuth(details, now);
  if (!captured) {
    return undefined;
  }

  deleteCaptured(captured);
  return captured.auth;
}

export function hasCapturedHandoffAuth(
  details: Pick<HandoffAuthRequestDetails, 'requestId' | 'url' | 'incognito'>,
  now = Date.now(),
): boolean {
  return findCapturedHandoffAuth(details, now) !== undefined;
}

function findCapturedHandoffAuth(
  details: Pick<HandoffAuthRequestDetails, 'requestId' | 'url' | 'incognito'>,
  now: number,
): CapturedAuth | undefined {
  pruneCapturedAuth(now);
  const incognito = details.incognito ?? false;
  if (details.requestId) {
    const captured = capturedByRequestId.get(details.requestId);
    if (captured && captured.incognito === incognito && isFresh(captured, now)) {
      return captured;
    }
  }

  const urlKey = normalizeUrlKey(details.url);
  const candidates = capturedByUrl.get(urlKey) ?? [];
  for (let index = candidates.length - 1; index >= 0; index -= 1) {
    const captured = candidates[index];
    if (captured.incognito === incognito && isFresh(captured, now)) {
      return captured;
    }
  }

  return undefined;
}

function pruneCapturedAuth(now: number): void {
  for (const captured of capturedByRequestId.values()) {
    if (!isFresh(captured, now)) {
      deleteCaptured(captured);
    }
  }

  for (const [url, entries] of capturedByUrl.entries()) {
    const fresh = entries.filter((entry) => isFresh(entry, now));
    if (fresh.length > 0) {
      capturedByUrl.set(url, fresh);
    } else {
      capturedByUrl.delete(url);
    }
  }
}

function deleteCaptured(captured: CapturedAuth): void {
  if (captured.requestId) {
    capturedByRequestId.delete(captured.requestId);
  }

  const entries = capturedByUrl.get(captured.url);
  if (!entries) return;
  const remaining = entries.filter((entry) => entry !== captured);
  if (remaining.length > 0) {
    capturedByUrl.set(captured.url, remaining);
  } else {
    capturedByUrl.delete(captured.url);
  }
}

function isFresh(captured: CapturedAuth, now: number): boolean {
  return now - captured.capturedAt <= CAPTURE_TTL_MS;
}

function normalizeUrlKey(url: string): string {
  try {
    return new URL(url).toString();
  } catch {
    return url;
  }
}

function isHttpUrl(url: string): boolean {
  try {
    const protocol = new URL(url).protocol;
    return protocol === 'http:' || protocol === 'https:';
  } catch {
    return false;
  }
}
