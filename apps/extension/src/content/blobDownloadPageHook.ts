(() => {
  const EVENT_NAME = 'simple-download-manager:blob-download';
  const MESSAGE_SOURCE = 'simple-download-manager-blob-download';
  const BYPASS_ATTRIBUTE = 'data-simple-download-manager-blob-bypass';

  function isBlobHref(href: string): boolean {
    return typeof href === 'string' && href.trim().toLowerCase().startsWith('blob:');
  }

  function downloadFilename(anchor: HTMLAnchorElement): string | undefined {
    const download = anchor.getAttribute('download');
    if (download && download.trim()) {
      return download.trim();
    }

    return undefined;
  }

  function emitBlobDownload(anchor: HTMLAnchorElement | null, event?: Event): boolean {
    if (!anchor || anchor.getAttribute(BYPASS_ATTRIBUTE) === 'true') {
      return false;
    }

    const blobUrl = anchor.href;
    if (!isBlobHref(blobUrl)) {
      return false;
    }

    const detail = {
      source: MESSAGE_SOURCE,
      blobUrl,
      filename: downloadFilename(anchor),
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
      emitBlobDownload(anchor, event);
    },
    true,
  );

  const originalClick = HTMLAnchorElement.prototype.click;
  if (originalClick) {
    HTMLAnchorElement.prototype.click = function patchedClick(this: HTMLAnchorElement): void {
      if (emitBlobDownload(this)) {
        return;
      }

      originalClick.call(this);
    };
  }
})();
