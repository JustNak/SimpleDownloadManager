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
- `open_app`
- `get_status`

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
      "queued": 1,
      "downloading": 2,
      "completed": 3,
      "failed": 1
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
- `show_window`

## App -> Host

Success types:

- `ready`
- `queued`

Error codes:

- `INVALID_URL`
- `UNSUPPORTED_SCHEME`
- `DESTINATION_NOT_CONFIGURED`
- `DESTINATION_INVALID`
- `DUPLICATE_JOB`
- `PERMISSION_DENIED`
- `DOWNLOAD_FAILED`
- `INTERNAL_ERROR`
