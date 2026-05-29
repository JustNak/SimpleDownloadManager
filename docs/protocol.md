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

`requestId` is bounded to a short ASCII token and must be unique enough for the
caller to match one response to one request.

Supported request types:

- `ping`
- `enqueue_download`
- `prompt_download`
- `adopt_browser_download`
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
      "ignoredFileExtensions": [],
      "capturedFileExtensions": ["7z", "exe", "pdf", "torrent", "zip"],
      "downloadCaptureDebugLogging": false
    },
    "appearanceSettings": {
      "theme": "system",
      "accentColor": "#3b82f6"
    }
  }
}
```

`enqueue_download` payload:

```json
{
  "url": "https://example.com/file.zip",
  "suggestedFilename": "file.zip",
  "totalBytes": 1048576,
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

`prompt_download` uses the same metadata fields as `enqueue_download`, but the
desktop app asks the user to confirm, rename, replace, or cancel before enqueue.

`adopt_browser_download` is used for automatic browser capture. The extension
lets the original browser download complete, then sends the completed local path
so the app can add a completed queue entry without replaying the browser request:

```json
{
  "url": "https://example.com/protected/file.zip",
  "localPath": "C:\\Users\\Me\\Downloads\\file.zip",
  "suggestedFilename": "file.zip",
  "totalBytes": 1048576,
  "mimeType": "application/zip",
  "source": {
    "entryPoint": "browser_download",
    "browser": "chrome",
    "extensionVersion": "1.1.1"
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
  "ignoredFileExtensions": [],
  "capturedFileExtensions": ["7z", "exe", "pdf", "torrent", "zip"],
  "downloadCaptureDebugLogging": false
}
```

URL handling is intentionally narrow:

- Manual/context-menu/popup requests accept `http:`, `https:`, and `magnet:` URLs.
- Automatic browser download interception uses `http:`/`https:` download items, configurable captured file extensions, and completed-file adoption.
- Manual/context-menu/popup torrent handoffs include optional `transferKind: "torrent"` metadata so magnet links and explicit torrent URLs use the torrent path.
- Legacy browser stream messages, `browserFallback`, and `handoffAuth` are no longer part of the extension-facing protocol.
- Protected-download auth remains a desktop/internal HTTP hoster capability. The extension does not name, configure, or send that auth path for browser-download adoption.

Torrent lifecycle behavior:

- Resume keeps the torrent session identity and asks the torrent engine to continue or verify existing pieces.
- Restart forgets app and torrent-engine session metadata but keeps downloaded torrent files so they can be rechecked.
- Cancel/remove stops app tracking and forgets the torrent session without deleting downloaded torrent data.
- Completed torrents may keep seeding until the configured ratio/time policy stops them.

`capturedFileExtensions` is the user-editable automatic capture gate. Removing a
default extension stops automatic capture for that filename extension. Manual
sends, popup sends, and context-menu sends are still allowed. `ignoredFileExtensions`
is retained only for backward compatibility with older settings.
Automatic capture always lets the browser own the transfer, then adopts the
completed local file into the app queue. Replaying only the URL and selected
headers is not equivalent to preserving the original browser request.
`listenPort` defaults to `1420` and is normalized to a valid TCP port from `1` to `65535`.
`appearanceSettings` is returned with status responses so the popup and options UI can mirror the desktop theme and accent color. It is display-only for the extension and is not part of `save_extension_settings`.

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

`status: "dismissed"` means the user canceled the prompt. Automatic browser
capture no longer cancels the browser's original download; it only adopts a
completed browser-owned file.
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
- local clients only; remote pipe clients are rejected
- pipe instances, request line size, and read/write time are bounded

Supported request types:

- `ping`
- `get_status`
- `enqueue_download`
- `prompt_download`
- `adopt_browser_download`
- `show_window`
- `save_extension_settings`

The desktop app validates protocol version, request id, request type, source
metadata, open-app reason, URL shape, and completed browser adoption paths
before any prompt, settings, or queue side effects. Excess side-effecting
requests return `RATE_LIMITED`.

## App -> Host

Success types:

- `ready`
- `queued`
- `duplicate_existing_job`
- `prompt_dismissed`

Error codes:

- `INVALID_URL`
- `UNSUPPORTED_SCHEME`
- `DESTINATION_NOT_CONFIGURED`
- `DESTINATION_INVALID`
- `DUPLICATE_JOB`
- `PERMISSION_DENIED`
- `RATE_LIMITED`
- `DOWNLOAD_FAILED`
- `INTERNAL_ERROR`
