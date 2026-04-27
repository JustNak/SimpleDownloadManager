# Firefox WebExtension Guidelines

This file documents the Firefox-specific review and packaging rules for the Simple Download Manager extension.

## Manifest Rules

- Firefox uses the generated Manifest V2 output in `apps/extension/dist/firefox`.
- Keep `manifest_version` at `2` for Firefox until the interception flow is intentionally migrated.
- Alpha package versions must use a numeric `version` such as `0.3.42` and a human-readable `version_name` such as `0.3.42-alpha`.
- Keep `browser_specific_settings.gecko.strict_min_version` aligned with the AMO lint target used by the release scripts.
- Declare `data_collection_permissions` when download URLs, referrer/page metadata, response headers, filename hints, content length, user download actions, or opt-in authenticated handoff headers are sent to the local native app.

## Permission Rationale

- `nativeMessaging` is required to communicate with the installed native desktop host.
- `downloads` is required to observe, cancel, remove, erase, and restart browser downloads during handoff and fallback.
- `webRequest` and `webRequestBlocking` are required so Firefox can intercept attachment/download responses before the default Save As dialog opens and capture request headers for explicitly allowlisted authenticated handoff hosts.
- `<all_urls>` is required because download links can originate from arbitrary HTTP(S) hosts. Runtime filtering still applies by scheme, excluded host patterns, ignored file extensions, and user settings.
- `storage` is required for extension settings.
- `contextMenus` is required for the link context menu action.

## AMO Review

- No remote code is used. The submitted ZIP must contain only bundled extension JavaScript, static assets, and `manifest.json`.
- The extension does not include analytics, advertising, tracking, or remote configuration.
- Data is transmitted only to the local native desktop application through Firefox native messaging.
- Reviewer notes should explain the native app dependency, download handoff flow, fallback behavior, wildcard excluded hosts, opt-in authenticated handoff hosts, and why each permission is needed.
- The AMO upload ZIP must contain extension files at the archive root, not inside a nested folder.
- The source ZIP should include source files and build scripts, while excluding generated or heavy folders such as `node_modules`, `dist`, `release`, `.tmp`, and Rust `target`.

## Build And Package

Run these commands from the repository root:

```powershell
npm run build:extension
npm run lint:firefox
npm run package:firefox-amo
```

Expected AMO artifacts:

- `release/firefox-amo/simple-download-manager-firefox-upload.zip`
- `release/firefox-amo/simple-download-manager-firefox-source.zip`
- `release/firefox-amo/AMO_REVIEWER_NOTES.md`

Use `npm run package:firefox-test` only for temporary local testing. Temporary Firefox add-ons are not AMO upload artifacts.
