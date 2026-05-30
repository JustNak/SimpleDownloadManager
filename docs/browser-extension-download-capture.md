# Browser Extension Download Capture

The extension uses prompt-first capture for eligible browser downloads.

## Modes

- `off`: the extension ignores browser downloads.
- `ask`: the extension stops the browser download, then opens the Simple Download Manager prompt.
- `auto`: the extension stops the browser download, then sends it directly to Simple Download Manager.

In `ask` mode, cancelling, closing, dismissing, or timing out the prompt dismisses the captured download entirely. The browser must not resume the download later.

## Firefox

Firefox capture uses `webRequest.onSendHeaders` to snapshot the browser-sent request headers, `webRequest.onBeforeRedirect` to preserve source download URLs before CDN redirects, then `webRequest.onHeadersReceived` with blocking response handling to classify the final response. When response headers indicate an eligible download, the extension returns `{ cancel: true }` and sends the source URL, filename/size metadata, and the bounded request-header snapshot to the desktop app.

If a Canvas/Instructure download redirects to a signed CDN URL, the extension hands SDM the original `/courses/.../files/.../download?download_frd=1` URL when the redirect chain exposes it. The CDN URL is used only as evidence for filename, size, and download classification.

## Chromium

Chromium capture uses `webRequest.onSendHeaders` to snapshot request headers, `webRequest.onBeforeRedirect` to preserve source download URLs before CDN redirects, and the downloads API to stop browser-owned downloads. When an eligible `downloads.DownloadItem` is observed, the extension calls `downloads.cancel(id)`, then attempts best-effort cleanup with `downloads.removeFile(id)` and `downloads.erase({ id })` when those APIs are available.

Filename suggestions use only basenames, never absolute local paths.

## Browser Session Headers

For Canvas/Instructure-style downloads and signed CDN URLs, handing off the final CDN URL is often wrong because it can reject replay with `401 Unauthorized`. The extension prefers the original Instructure download endpoint from the redirect chain and forwards only the request headers already sent by the browser for the captured download request, limited to desktop-accepted headers such as `Cookie`, `Authorization`, `Referer`, `Origin`, `User-Agent`, `Accept`, `Accept-Language`, `Sec-Fetch-*`, and `Sec-CH-UA*`.

The desktop app validates those headers again, stores them only in memory for the queued job, and applies them only to the original URL or same-origin redirects. Cross-origin CDN redirects do not receive browser session headers; they must rely on the signed redirect URL itself.

## Eligibility

Downloads are eligible when they have strong download evidence, such as:

- `Content-Disposition: attachment`
- a configured captured file extension
- an explicit known download URL
- a known downloadable MIME type in a non-page-internal context

The extension avoids capturing excluded hosts, browser extension packages, structured API traffic, page-internal requests without strong download intent, and tiny generic app probes.

## Timeout

Desktop prompts time out after five minutes. Timeout resolves the prompt as cancel and advances any queued prompt.
