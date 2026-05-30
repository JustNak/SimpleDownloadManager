# Browser Extension Manual QA

Use this checklist after building the extension with `npm run build:extension`.

## Setup

1. Register or verify the native host for the browser under test.
2. Start Simple Download Manager.
3. Load the unpacked extension:
   - Firefox: `apps/extension/dist/firefox`
   - Chromium/Edge: `apps/extension/dist/chromium`
4. Confirm extension settings:
   - Enabled: on
   - Silent Download off for `ask` checks
   - Silent Download on for `auto` checks
5. If testing private/incognito windows, enable the extension for private/incognito browsing in the browser extension settings.

## Firefox Ask Mode

- [ ] Direct downloadable URL, such as a `.zip`: prompt opens, browser download does not continue.
- [ ] Click Download in the SDM prompt: SDM queues the download.
- [ ] Click Cancel in the SDM prompt: no browser download, no SDM job.
- [ ] Close the SDM prompt window: same result as Cancel.
- [ ] Leave the prompt open for five minutes: prompt is dismissed as cancel, no browser fallback.
- [ ] CDN redirect URL: prompt opens with the resolved filename when headers expose one.
- [ ] Browser-session download from a signed/app URL, such as a course file download: browser transfer is canceled, SDM receives the original Instructure download URL, and SDM downloads the file.
- [ ] Opaque URL with strong headers, such as `Content-Disposition` or known download MIME: prompt opens.
- [ ] Page-internal API/XHR traffic: no prompt and no browser interruption.
- [ ] Canvas/Instructure CDN redirect: SDM queue row URL/source is based on the original `/courses/.../files/.../download?download_frd=1` endpoint, not the `cdn.inst-fs...` URL.
- [ ] Cross-origin redirect from a protected page to a CDN URL does not send page cookies to the CDN; the CDN URL must work from its own signed query/token.

## Firefox Auto Mode

- [ ] Direct downloadable URL: browser request is cancelled and SDM queues directly.
- [ ] Duplicate URL/file: existing desktop duplicate flow is used.
- [ ] Canvas/Instructure CDN redirect: browser item is canceled and SDM downloads through the original Instructure endpoint.

## Chromium Or Edge Ask Mode

- [ ] Direct downloadable URL: browser download item is cancelled, SDM prompt opens.
- [ ] Click Download in the SDM prompt: SDM queues the download.
- [ ] Click Cancel in the SDM prompt: no browser download, no SDM job.
- [ ] Close the prompt window: same result as Cancel.
- [ ] Check the browser downloads shelf/list: cancelled item should not keep a successful completed browser file.
- [ ] Check the filesystem for partial files; partial cleanup is best-effort via `downloads.removeFile` and `downloads.erase`.
- [ ] CDN redirect URL: browser item is cancelled and SDM prompt/auto handoff receives filename metadata when the browser exposes it.
- [ ] Browser-session download from a signed/app URL, such as a course file download: browser item is canceled and SDM receives the original Instructure download URL.

## Chromium Or Edge Auto Mode

- [ ] Direct downloadable URL: browser item is cancelled and SDM queues directly.
- [ ] Duplicate URL/file: existing desktop duplicate flow is used.
- [ ] SDM/native failure: popup shows Retry and Cancel; browser does not resume automatically.

## Off Mode

- [ ] Set handoff mode to off.
- [ ] Direct downloadable URL downloads normally in the browser.
- [ ] No SDM prompt appears.
- [ ] No SDM job is created.

## Regression Checks

- [ ] Extension package downloads such as Firefox `.xpi` stay in the browser.
- [ ] Excluded hosts are ignored.
- [ ] YouTube/Telegram/session verification requests do not trigger prompts.
- [ ] Incognito/private downloads preserve incognito metadata when extension permission is enabled.
- [ ] Retry after native host/app failure retries only the SDM handoff; it does not recreate a browser-owned fallback download.
