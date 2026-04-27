# Protocol

## Extension -> Host

All browser requests use this envelope:

```json
{
  "protocolVersion": 1,
  "requestId": "uuid",
  "type": "enqueue_download",
  "payload": {}
}
```

Supported request types:

- `ping`
- `enqueue_download`
- `prompt_download`
- `open_app`
- `get_status`
- `save_extension_settings`

`ping` and `get_status` return the current desktop connection and queue summary:

```json
{
  "ok": true,
  "requestId": "uuid",
  "type": "pong",
  "payload": {
    "appState": "running",
    "connectionState": "connected",
    "queueSummary": {
      "total": 8,
      "active": 3,
      "attention": 1,
      "queued": 1,
      "downloading": 2,
      "completed": 3,
      "failed": 1
    },
    "extensionSettings": {
      "enabled": true,
      "downloadHandoffMode": "ask",
      "listenPort": 1420,
      "contextMenuEnabled": true,
      "showProgressAfterHandoff": true,
      "showBadgeStatus": true,
      "excludedHosts": [],
      "ignoredFileExtensions": []
    }
  }
}
```

`enqueue_download` payload:

```json
{
  "url": "https://example.com/file.zip",
  "source": {
    "entryPoint": "context_menu",
    "browser": "firefox",
    "extensionVersion": "0.1.0",
    "pageUrl": "https://example.com/page",
    "pageTitle": "Example Page",
    "referrer": "https://example.com/page",
    "incognito": false
  }
}
```

`save_extension_settings` payload:

```json
{
  "enabled": true,
  "downloadHandoffMode": "auto",
  "listenPort": 1420,
  "contextMenuEnabled": true,
  "showProgressAfterHandoff": true,
  "showBadgeStatus": true,
  "excludedHosts": ["example.com"],
  "ignoredFileExtensions": ["exe", "zip", "txt", "pdf"],
  "authenticatedHandoffEnabled": true,
  "authenticatedHandoffHosts": ["chatgpt.com"]
}
```

URL handling is intentionally narrow:

- Manual/context-menu/popup requests accept `http:`, `https:`, and `magnet:` URLs.
- Browser download interception uses `http:`/`https:` download items and hands `.torrent` URLs or filenames to the desktop app as torrent jobs.
- The envelope version stays `1`; torrent handoff uses the existing `url` field.

`ignoredFileExtensions` applies only to automatic browser download capture.
Manual sends, popup sends, and context-menu sends are still allowed.
`authenticatedHandoffHosts` is retained for backward compatibility with older settings.
When Protected Downloads is enabled, exact browser download handoffs may include bounded, memory-only request headers in `handoffAuth`; auth header values are never persisted or written to diagnostics.
`listenPort` defaults to `1420` and is normalized to a valid TCP port from `1` to `65535`.

## Host -> Extension

Success:

```json
{
  "ok": true,
  "requestId": "uuid",
  "type": "accepted",
  "payload": {
    "status": "queued",
    "jobId": "job_123",
    "appState": "running"
  }
}
```

If the URL is already in the desktop queue, the host still returns `accepted` with
`status: "duplicate_existing_job"` and the existing `jobId`.

For automatic browser download capture, `status: "canceled"` means the desktop
prompt was canceled and the extension should return control to the browser's
original download flow. `status: "queued"` and `status: "duplicate_existing_job"`
mean the extension should cancel and erase the original browser download item.
Torrent cancel/remove requests stop app tracking but do not delete downloaded torrent data.

Error:

```json
{
  "ok": false,
  "requestId": "uuid",
  "type": "rejected",
  "code": "HOST_NOT_AVAILABLE",
  "message": "Native host not installed"
}
```

## Host -> App

Transport:

- Windows named pipe
- path: `\\.\pipe\myapp.downloads.v1`
- one JSON line request, one JSON line response

Supported request types:

- `ping`
- `get_status`
- `enqueue_download`
- `prompt_download`
- `show_window`
- `save_extension_settings`

## App -> Host

Success types:

- `ready`
- `queued`
- `duplicate_existing_job`
- `prompt_canceled`

Error codes:

- `INVALID_URL`
- `UNSUPPORTED_SCHEME`
- `DESTINATION_NOT_CONFIGURED`
- `DESTINATION_INVALID`
- `DUPLICATE_JOB`
- `PERMISSION_DENIED`
- `DOWNLOAD_FAILED`
- `INTERNAL_ERROR`
