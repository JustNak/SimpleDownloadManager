import type { HostToExtensionResponse } from '@myapp/protocol';
import browser from '../background/browser';
import {
  BLOB_DOWNLOAD_BYPASS_ATTRIBUTE,
  BLOB_DOWNLOAD_INTERCEPT_EVENT,
  BLOB_DOWNLOAD_PAGE_MESSAGE_SOURCE,
  BROWSER_BLOB_CHUNK_SIZE_BYTES,
  BROWSER_BLOB_DOWNLOAD_PORT,
  blobDownloadFilename,
  createBrowserBlobBeginRequest,
  createBrowserBlobCancelRequest,
  createBrowserBlobChunkRequest,
  createBrowserBlobFinishRequest,
  createBrowserBlobStreamId,
  pageManagedDownloadKind,
  shouldHandlePageManagedDownload,
  type PageManagedDownloadCandidate,
  type PageManagedDownloadKind,
} from '../background/blobDownloads';
import { getExtensionSettings } from '../background/state';

type PageManagedDownloadMessage = PageManagedDownloadCandidate & {
  blobUrl?: string;
  downloadUrl?: string;
  pageUrl?: string;
  referrer?: string;
};

type PendingPortResponse = {
  resolve(response: HostToExtensionResponse): void;
  reject(error: Error): void;
};

injectPageHook();

window.addEventListener('message', (event) => {
  const candidate = blobCandidateFromPageMessage(event.data);
  if (!candidate) {
    return;
  }

  void handleBlobDownload(candidate);
});

window.addEventListener(BLOB_DOWNLOAD_INTERCEPT_EVENT, (event) => {
  const candidate = blobCandidateFromPageMessage((event as CustomEvent).detail);
  if (!candidate) {
    return;
  }

  void handleBlobDownload(candidate);
});

async function handleBlobDownload(candidate: PageManagedDownloadMessage): Promise<void> {
  const settings = await getExtensionSettings();
  const enrichedCandidate = {
    ...candidate,
    pageUrl: candidate.pageUrl || location.href,
  };

  if (!shouldHandlePageManagedDownload(enrichedCandidate, settings)) {
    replayPageDownload(candidate);
    return;
  }

  if (candidate.kind === 'url') {
    const response = await browser.runtime.sendMessage({
      type: 'page_download_intent',
      url: candidate.url,
      filename: candidate.filename,
      pageUrl: enrichedCandidate.pageUrl,
      pageTitle: document.title || undefined,
      referrer: candidate.referrer || document.referrer || undefined,
      incognito: extensionIsIncognito(),
    });
    if (shouldReplayPageDownload(response)) {
      replayPageDownload(candidate);
    }
    return;
  }

  let blob: Blob;
  try {
    const response = await fetch(candidate.url);
    blob = await response.blob();
  } catch {
    return;
  }

  const streamId = createBrowserBlobStreamId();
  const filename = blobDownloadFilename(candidate.filename, blob.type || candidate.mimeType);
  const port = createBlobDownloadPort();
  let offset = 0;
  let beganStream = false;

  try {
    const beginResponse = await port.send(createBrowserBlobBeginRequest({
      streamId,
      source: {
        entryPoint: 'browser_download',
        browser: detectBrowser(),
        extensionVersion: browser.runtime.getManifest().version,
        pageUrl: enrichedCandidate.pageUrl,
        referrer: candidate.referrer || document.referrer || undefined,
        incognito: extensionIsIncognito(),
      },
      suggestedFilename: filename,
      totalBytes: blob.size > 0 ? blob.size : undefined,
      mimeType: blob.type || candidate.mimeType,
    }));
    if (!beginResponse.ok) {
      return;
    }
    beganStream = true;

    for await (const chunk of blobChunks(blob)) {
      const response = await port.send(createBrowserBlobChunkRequest(streamId, offset, chunk));
      if (!response.ok) {
        throw new Error(response.message);
      }
      offset += chunk.byteLength;
    }

    const finishResponse = await port.send(createBrowserBlobFinishRequest(streamId));
    if (!finishResponse.ok) {
      throw new Error(finishResponse.message);
    }
  } catch (error) {
    if (beganStream) {
      await port.send(createBrowserBlobCancelRequest(
        streamId,
        error instanceof Error ? error.message : 'Browser blob stream failed.',
      )).catch(() => undefined);
    }
  } finally {
    port.disconnect();
  }
}

function createBlobDownloadPort() {
  const port = browser.runtime.connect({ name: BROWSER_BLOB_DOWNLOAD_PORT });
  const pending = new Map<string, PendingPortResponse>();

  port.onMessage.addListener((message: object) => {
    const response = message as HostToExtensionResponse;
    const requestId = response?.requestId;
    if (!requestId) {
      return;
    }

    const entry = pending.get(requestId);
    if (!entry) {
      return;
    }

    pending.delete(requestId);
    entry.resolve(response);
  });

  port.onDisconnect.addListener(() => {
    const error = new Error('Native host stream closed before the blob download finished.');
    for (const entry of pending.values()) {
      entry.reject(error);
    }
    pending.clear();
  });

  return {
    send(request: { requestId: string }): Promise<HostToExtensionResponse> {
      return new Promise((resolve, reject) => {
        pending.set(request.requestId, { resolve, reject });
        try {
          port.postMessage(request);
        } catch (error) {
          pending.delete(request.requestId);
          reject(error instanceof Error ? error : new Error('Could not send blob stream chunk.'));
        }
      });
    },
    disconnect(): void {
      port.disconnect();
    },
  };
}

async function* blobChunks(blob: Blob): AsyncGenerator<Uint8Array> {
  if (blob.stream) {
    const reader = blob.stream().getReader();
    try {
      while (true) {
        const { done, value } = await reader.read();
        if (done) {
          break;
        }
        if (!value) {
          continue;
        }
        yield* splitChunk(value);
      }
    } finally {
      reader.releaseLock();
    }
    return;
  }

  const bytes = new Uint8Array(await blob.arrayBuffer());
  yield* splitChunk(bytes);
}

function* splitChunk(bytes: Uint8Array): Generator<Uint8Array> {
  for (let offset = 0; offset < bytes.byteLength; offset += BROWSER_BLOB_CHUNK_SIZE_BYTES) {
    yield bytes.slice(offset, offset + BROWSER_BLOB_CHUNK_SIZE_BYTES);
  }
}

function replayPageDownload(candidate: PageManagedDownloadMessage): void {
  const anchor = document.createElement('a');
  anchor.href = candidate.url;
  const filename = candidate.kind === 'stream'
    ? blobDownloadFilename(candidate.filename, candidate.mimeType)
    : candidate.filename;
  if (filename) {
    anchor.download = filename;
  }
  anchor.setAttribute(BLOB_DOWNLOAD_BYPASS_ATTRIBUTE, 'true');
  anchor.style.display = 'none';
  document.documentElement.append(anchor);
  anchor.click();
  anchor.remove();
}

function injectPageHook(): void {
  const script = document.createElement('script');
  script.src = browser.runtime.getURL('blobDownloadPageHook.js');
  script.async = false;
  script.onload = () => script.remove();
  (document.documentElement || document.head).append(script);
}

function shouldReplayPageDownload(response: unknown): boolean {
  return isAcceptedResponse(response) && response.payload.status === 'canceled';
}

function isAcceptedResponse(
  response: unknown,
): response is Extract<HostToExtensionResponse, { type: 'accepted' }> {
  return typeof response === 'object'
    && response !== null
    && (response as Partial<HostToExtensionResponse>).ok === true
    && (response as Partial<HostToExtensionResponse>).type === 'accepted';
}

function blobCandidateFromPageMessage(value: unknown): PageManagedDownloadMessage | null {
  if (!value || typeof value !== 'object') {
    return null;
  }

  const message = value as Partial<PageManagedDownloadMessage> & { source?: unknown };
  if (message.source !== BLOB_DOWNLOAD_PAGE_MESSAGE_SOURCE) {
    return null;
  }

  const url = typeof message.url === 'string'
    ? message.url
    : typeof message.blobUrl === 'string'
      ? message.blobUrl
      : typeof message.downloadUrl === 'string'
        ? message.downloadUrl
        : undefined;
  if (!url) {
    return null;
  }

  const kind = message.kind === 'stream' || message.kind === 'url'
    ? message.kind
    : pageManagedDownloadKind(url, Boolean(message.filename));
  if (!kind) {
    return null;
  }

  return {
    kind: kind as PageManagedDownloadKind,
    url,
    blobUrl: kind === 'stream' ? url : undefined,
    downloadUrl: kind === 'url' ? url : undefined,
    pageUrl: typeof message.pageUrl === 'string' ? message.pageUrl : location.href,
    filename: typeof message.filename === 'string' ? message.filename : undefined,
    mimeType: typeof message.mimeType === 'string' ? message.mimeType : undefined,
    referrer: typeof message.referrer === 'string' ? message.referrer : document.referrer || undefined,
  };
}

function detectBrowser() {
  const userAgent = navigator.userAgent.toLowerCase();
  if (userAgent.includes('firefox')) return 'firefox' as const;
  if (userAgent.includes('edg/')) return 'edge' as const;
  return 'chrome' as const;
}

function extensionIsIncognito(): boolean | undefined {
  const extension = (browser as typeof browser & {
    extension?: { inIncognitoContext?: boolean };
  }).extension;

  return extension?.inIncognitoContext;
}
