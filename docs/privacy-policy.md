# Simple Download Manager Firefox Extension Privacy Policy

Simple Download Manager is a companion browser extension for the local Simple Download Manager desktop app.

## Data Sent To The Local Desktop App

When the extension is enabled and a download is eligible for handoff, it may send the following data from Firefox to the local native desktop app through Firefox native messaging:

- Download URL.
- Suggested filename and content length when Firefox exposes them.
- Page URL, page title, referrer, entry point, extension version, and incognito flag when available.
- User actions such as context-menu handoff, popup handoff, browser-download prompt, accepted handoff, canceled prompt, or fallback.
- Extension settings such as capture mode, excluded sites, ignored file extensions, Protected Downloads settings, badge preference, and progress-window preference.
- If Protected Downloads is enabled, bounded browser request headers for the exact download being handed off, such as Cookie or Authorization headers after extension-side filtering.

## Local-Only Use

The extension sends this data only to the local native desktop app installed on the same device. The extension does not transmit data to a remote server, does not use analytics, does not use advertising, does not track users, and does not use remote configuration.

## Storage

The extension stores its settings in Firefox extension storage. Protected-download request headers are held only in extension memory for a short time, are capped, and are cleared when Protected Downloads is disabled.

## User Controls

Users can disable browser download interception, choose prompt or automatic handoff, exclude sites, ignore file extensions, and disable Protected Downloads from the extension options page.
