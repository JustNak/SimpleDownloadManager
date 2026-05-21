import {
  isProtectedDownloadAuthAllowedForUrl,
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

type CapturedBrowserSessionMarker = Omit<CapturedAuth, 'auth'>;

export type BrowserHandoffAuthResolution =
  | { status: 'ready'; handoffAuth?: HandoffAuth }
  | { status: 'protected_auth_required' };

const CAPTURE_TTL_MS = 30_000;
const MAX_CAPTURED_AUTH_ENTRIES = 64;
const capturedByRequestId = new Map<string, CapturedAuth>();
const capturedByUrl = new Map<string, CapturedAuth[]>();
const capturedEntries: CapturedAuth[] = [];
const browserSessionMarkersByRequestId = new Map<string, CapturedBrowserSessionMarker>();
const browserSessionMarkersByUrl = new Map<string, CapturedBrowserSessionMarker[]>();
const browserSessionMarkers: CapturedBrowserSessionMarker[] = [];

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
  if (!isHttpUrl(url) || !isProtectedDownloadAuthAllowedForUrl(url, settings)) {
    return undefined;
  }

  const filteredHeaders = filterHandoffAuthHeaders(headers);
  return filteredHeaders.length > 0 ? { headers: filteredHeaders } : undefined;
}

export function captureHandoffAuthHeaders(
  details: HandoffAuthRequestDetails,
  settings: ExtensionIntegrationSettings,
  now = Date.now(),
): void {
  if (details.method && details.method.toUpperCase() !== 'GET') {
    return;
  }

  if (!isHttpUrl(details.url)) {
    return;
  }

  if (hasBrowserSessionHeader(details.requestHeaders)) {
    captureBrowserSessionMarker(details, now);
  }

  if (!isProtectedDownloadAuthAllowedForUrl(details.url, settings)) {
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
    const existing = capturedByRequestId.get(captured.requestId);
    if (existing) {
      deleteCaptured(existing);
    }

    capturedByRequestId.set(captured.requestId, captured);
  }

  const byUrl = capturedByUrl.get(captured.url) ?? [];
  byUrl.push(captured);
  capturedByUrl.set(captured.url, byUrl.slice(-8));
  capturedEntries.push(captured);
  evictOldestCapturedAuth();
}

export function takeCapturedHandoffAuth(
  details: Pick<HandoffAuthRequestDetails, 'requestId' | 'url' | 'incognito'>,
  settings: ExtensionIntegrationSettings,
  now = Date.now(),
): HandoffAuth | undefined {
  if (!settings.authenticatedHandoffEnabled) {
    clearCapturedHandoffAuth();
    return undefined;
  }

  if (!isProtectedDownloadAuthAllowedForUrl(details.url, settings)) {
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

export function hasCapturedBrowserSessionHeaders(
  details: Pick<HandoffAuthRequestDetails, 'requestId' | 'url' | 'incognito'>,
  now = Date.now(),
): boolean {
  return findCapturedBrowserSessionMarker(details, now) !== undefined;
}

export function resolveBrowserHandoffAuth(
  details: Pick<HandoffAuthRequestDetails, 'requestId' | 'url' | 'incognito'>,
  settings: ExtensionIntegrationSettings,
  now = Date.now(),
): BrowserHandoffAuthResolution {
  const hadBrowserSessionHeaders = hasCapturedBrowserSessionHeaders(details, now);
  const handoffAuth = takeCapturedHandoffAuth(details, settings, now);
  if (hadBrowserSessionHeaders && !handoffAuth) {
    return { status: 'protected_auth_required' };
  }

  return handoffAuth ? { status: 'ready', handoffAuth } : { status: 'ready' };
}

export function clearCapturedHandoffAuth(): void {
  capturedByRequestId.clear();
  capturedByUrl.clear();
  capturedEntries.splice(0, capturedEntries.length);
  browserSessionMarkersByRequestId.clear();
  browserSessionMarkersByUrl.clear();
  browserSessionMarkers.splice(0, browserSessionMarkers.length);
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
  const candidates = (capturedByUrl.get(urlKey) ?? []).filter(
    (captured) => captured.incognito === incognito && isFresh(captured, now),
  );
  if (candidates.length === 1) {
    return candidates[0];
  }

  return undefined;
}

function captureBrowserSessionMarker(
  details: HandoffAuthRequestDetails,
  now: number,
): void {
  pruneBrowserSessionMarkers(now);
  const marker: CapturedBrowserSessionMarker = {
    requestId: details.requestId,
    url: normalizeUrlKey(details.url),
    incognito: details.incognito ?? false,
    capturedAt: now,
  };

  if (marker.requestId) {
    const existing = browserSessionMarkersByRequestId.get(marker.requestId);
    if (existing) {
      deleteBrowserSessionMarker(existing);
    }

    browserSessionMarkersByRequestId.set(marker.requestId, marker);
  }

  const byUrl = browserSessionMarkersByUrl.get(marker.url) ?? [];
  byUrl.push(marker);
  browserSessionMarkersByUrl.set(marker.url, byUrl.slice(-8));
  browserSessionMarkers.push(marker);
  evictOldestBrowserSessionMarkers();
}

function findCapturedBrowserSessionMarker(
  details: Pick<HandoffAuthRequestDetails, 'requestId' | 'url' | 'incognito'>,
  now: number,
): CapturedBrowserSessionMarker | undefined {
  pruneBrowserSessionMarkers(now);
  const incognito = details.incognito ?? false;
  if (details.requestId) {
    const captured = browserSessionMarkersByRequestId.get(details.requestId);
    if (captured && captured.incognito === incognito && isFresh(captured, now)) {
      return captured;
    }
  }

  const urlKey = normalizeUrlKey(details.url);
  return (browserSessionMarkersByUrl.get(urlKey) ?? []).find(
    (captured) => captured.incognito === incognito && isFresh(captured, now),
  );
}

function pruneCapturedAuth(now: number): void {
  for (const captured of [...capturedEntries]) {
    if (!isFresh(captured, now)) {
      deleteCaptured(captured);
    }
  }
}

function pruneBrowserSessionMarkers(now: number): void {
  for (const marker of [...browserSessionMarkers]) {
    if (!isFresh(marker, now)) {
      deleteBrowserSessionMarker(marker);
    }
  }
}

function evictOldestCapturedAuth(): void {
  while (capturedEntries.length > MAX_CAPTURED_AUTH_ENTRIES) {
    const captured = capturedEntries[0];
    if (!captured) {
      return;
    }

    deleteCaptured(captured);
  }
}

function evictOldestBrowserSessionMarkers(): void {
  while (browserSessionMarkers.length > MAX_CAPTURED_AUTH_ENTRIES) {
    const marker = browserSessionMarkers[0];
    if (!marker) {
      return;
    }

    deleteBrowserSessionMarker(marker);
  }
}

function deleteCaptured(captured: CapturedAuth): void {
  if (captured.requestId) {
    capturedByRequestId.delete(captured.requestId);
  }

  const entryIndex = capturedEntries.indexOf(captured);
  if (entryIndex >= 0) {
    capturedEntries.splice(entryIndex, 1);
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

function deleteBrowserSessionMarker(marker: CapturedBrowserSessionMarker): void {
  if (marker.requestId) {
    browserSessionMarkersByRequestId.delete(marker.requestId);
  }

  const entryIndex = browserSessionMarkers.indexOf(marker);
  if (entryIndex >= 0) {
    browserSessionMarkers.splice(entryIndex, 1);
  }

  const entries = browserSessionMarkersByUrl.get(marker.url);
  if (!entries) return;
  const remaining = entries.filter((entry) => entry !== marker);
  if (remaining.length > 0) {
    browserSessionMarkersByUrl.set(marker.url, remaining);
  } else {
    browserSessionMarkersByUrl.delete(marker.url);
  }
}

function isFresh(captured: { capturedAt: number }, now: number): boolean {
  return now - captured.capturedAt <= CAPTURE_TTL_MS;
}

function hasBrowserSessionHeader(headers: HandoffAuthRequestHeader[] | undefined): boolean {
  return (headers ?? []).some((header) => {
    const normalized = header.name.trim().toLowerCase();
    return normalized === 'cookie' || normalized === 'authorization';
  });
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
