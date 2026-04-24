# Threat Model

Inbound requests are untrusted.

Sources:

- arbitrary web pages through the extension
- user-pasted URLs
- browser extension messaging layer

MVP rules:

- accept only `http` and `https`
- reject local file paths
- reject browser-internal URLs
- do not accept save path from the extension
- do not accept cookies, headers, or session replay material
- cap metadata sizes
- sanitize filenames in the desktop app
- log request origin fields
- rate-limit native host requests

Security boundary:

- extension validates for UX
- native host validates for protocol integrity
- desktop app validates again before queueing
