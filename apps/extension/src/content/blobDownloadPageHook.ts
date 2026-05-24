(() => {
  const EVENT_NAME = 'simple-download-manager:blob-download';
  const MESSAGE_SOURCE = 'simple-download-manager-blob-download';
  const BYPASS_ATTRIBUTE = 'data-simple-download-manager-blob-bypass';

  type PageManagedDownloadKind = 'stream' | 'url';

  function isStreamHref(href: string): boolean {
    const normalized = typeof href === 'string' ? href.trim().toLowerCase() : '';
    return normalized.startsWith('blob:') || normalized.startsWith('data:');
  }

  function downloadFilename(anchor: HTMLAnchorElement): string | undefined {
    const download = anchor.getAttribute('download');
    if (download && download.trim()) {
      return download.trim();
    }

    return undefined;
  }

  function downloadKind(anchor: HTMLAnchorElement): PageManagedDownloadKind | null {
    const href = anchor.href;
    if (isStreamHref(href)) {
      return 'stream';
    }

    if (!anchor.hasAttribute('download')) {
      return null;
    }

    try {
      const protocol = new URL(href).protocol;
      return protocol === 'http:' || protocol === 'https:' ? 'url' : null;
    } catch {
      return null;
    }
  }

  function dataMimeType(href: string): string | undefined {
    if (!href.trim().toLowerCase().startsWith('data:')) {
      return undefined;
    }

    const metadata = href.slice(5).split(',', 1)[0]?.split(';', 1)[0]?.trim();
    return metadata || undefined;
  }

  function emitPageDownload(anchor: HTMLAnchorElement | null, event?: Event): boolean {
    if (!anchor || anchor.getAttribute(BYPASS_ATTRIBUTE) === 'true') {
      return false;
    }

    const url = anchor.href;
    const kind = downloadKind(anchor);
    if (!kind) {
      return false;
    }

    const detail = {
      source: MESSAGE_SOURCE,
      kind,
      url,
      blobUrl: kind === 'stream' ? url : undefined,
      downloadUrl: kind === 'url' ? url : undefined,
      filename: downloadFilename(anchor),
      mimeType: dataMimeType(url),
      pageUrl: location.href,
      referrer: document.referrer || undefined,
    };

    window.dispatchEvent(new CustomEvent(EVENT_NAME, { detail }));
    window.postMessage(detail, '*');
    event?.preventDefault();
    event?.stopImmediatePropagation();
    return true;
  }

  document.addEventListener(
    'click',
    (event) => {
      const target = event.target;
      const anchor = target instanceof Element
        ? target.closest<HTMLAnchorElement>('a[href]')
        : null;
      emitPageDownload(anchor, event);
    },
    true,
  );

  const originalClick = HTMLAnchorElement.prototype.click;
  if (originalClick) {
    HTMLAnchorElement.prototype.click = function patchedClick(this: HTMLAnchorElement): void {
      if (emitPageDownload(this)) {
        return;
      }

      originalClick.call(this);
    };
  }
})();
