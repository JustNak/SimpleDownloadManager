# Threat Model

Inbound requests are untrusted.

Sources:

- arbitrary web pages through the extension
- user-pasted URLs
- browser extension messaging layer

MVP rules:

- accept only `http`, `https`, and `magnet` where the request surface supports torrents
- keep multi-download and bulk archive input HTTP(S)-only
- reject local file paths
- reject browser-internal URLs
- do not accept save path from the extension
- do not accept cookies, headers, or session replay material from manual URLs
- for exact browser download handoff only, accept bounded protected-download request headers when enabled; keep them memory-only, redact values from diagnostics, and never persist them
- cap metadata sizes
- sanitize filenames in the desktop app
- log request origin fields
- rate-limit native host requests
- treat torrenting as P2P: peers can observe participation, seeding may continue after download completion, and cancel stops tracking without deleting downloaded files

Security boundary:

- extension validates for UX
- native host validates for protocol integrity
- desktop app validates again before queueing
