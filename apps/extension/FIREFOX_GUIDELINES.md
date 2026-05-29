# Firefox WebExtension Guidelines

This file documents the Firefox-specific review and packaging rules for the Simple Download Manager extension.

## Manifest Rules

- Firefox uses the generated Manifest V2 output in `apps/extension/dist/firefox`.
- Keep `manifest_version` at `2` for Firefox until the interception flow is intentionally migrated.
- Alpha package versions must use a numeric `version` such as `0.3.45` and a human-readable `version_name` such as `0.3.45-alpha`.
- The extension release version is sourced from `apps/extension/package.json` and may differ from the desktop/Tauri app version.
- Keep `browser_specific_settings.gecko.strict_min_version` aligned with the AMO lint target used by the release scripts.
- Declare `data_collection_permissions` when download URLs, referrer/page metadata, response headers, filename hints, content length, user download actions, or completed local download paths are used for local handoff decisions.

## Permission Rationale

- `nativeMessaging` is required to communicate with the installed native desktop host.
- `downloads` is required to observe browser downloads and adopt completed files into the desktop app queue.
- `webRequest` and `webRequestBlocking` are required so Firefox can distinguish real attachment/download responses from page-internal traffic before completed-file adoption.
- `<all_urls>` is required because download links can originate from arbitrary HTTP(S) hosts. Runtime filtering still applies by scheme, excluded host patterns, captured file extensions, and user settings.
- `storage` is required for extension settings.
- `contextMenus` is required for the link context menu action.

## AMO Review

- The AMO upload is intended for public hosting on addons.mozilla.org. In the AMO Developer Hub, choose "On this site", not "On your own".
- No remote code is used. The submitted ZIP must contain only bundled extension JavaScript, static assets, and `manifest.json`.
- The extension does not include analytics, advertising, tracking, or remote configuration.
- Data is transmitted only to the local native desktop application through Firefox native messaging.
- Reviewer notes should explain the native app dependency, completed-file adoption flow, wildcard excluded hosts, and why each permission is needed.
- The AMO upload ZIP must contain extension files at the archive root, not inside a nested folder.
- The source ZIP should include source files and build scripts, while excluding generated or heavy folders such as `node_modules`, `dist`, `release`, `.tmp`, and Rust `target`.
- `AMO_LISTING_METADATA.json` is generated for `web-ext sign --channel=listed --amo-metadata=...`.
- `PRIVACY_POLICY.md` is generated from `docs/privacy-policy.md` and should be pasted into the AMO privacy policy field.

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
- `release/firefox-amo/AMO_LISTING_METADATA.json`
- `release/firefox-amo/PRIVACY_POLICY.md`
- `release/firefox-amo/AMO_REVIEWER_NOTES.md`

Use `npm run package:firefox-test` only for temporary local testing. Temporary Firefox add-ons are not AMO upload artifacts.
