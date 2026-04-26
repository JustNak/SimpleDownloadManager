# Desktop App

This package contains the React/Tauri desktop shell for Simple Download Manager.

## Layout

- `src/` contains the React UI, queue interactions, settings, prompt windows, and typed backend command wrappers.
- `src-tauri/` contains the Rust backend: queue state, download workers, native-host IPC, storage, commands, windows, and diagnostics.
- `tests/` contains Node-based TypeScript assertion tests for UI-independent logic.

## Common Commands

Run commands from the repository root unless a package-specific workflow says otherwise.

- `npm run typecheck` checks protocol, extension, and desktop TypeScript.
- `npm run test:ts` runs desktop and extension TypeScript assertion tests.
- `npm run test:rust` runs desktop and native-host Rust tests.
- `npm run clippy` runs Rust clippy with warnings denied.
- `npm run build:desktop` builds the desktop frontend bundle.

## Integration Notes

The desktop app shares protocol types with the browser extension and native host. Keep queue states, extension settings, and handoff behavior aligned across `packages/protocol`, `apps/extension`, and `src-tauri` when changing the browser-to-desktop flow.
