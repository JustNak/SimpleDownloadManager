# Threat Model

Inbound requests are untrusted.

Sources:

- arbitrary web pages through the extension
- user-pasted URLs
- browser extension messaging layer

MVP rules:

- accept only `http`, `https`, and `magnet` where the request surface supports torrents
- keep bulk link and bulk archive input HTTP(S)-only
- reject local file paths
- reject browser-internal URLs
- do not accept save path from the extension
- do not accept cookies, headers, or session replay material from manual URLs
- automatic browser capture should preserve original redirect source URLs and send only bounded browser request headers for captured downloads
- accept completed browser download paths only from legacy local native messaging extension flows and validate them before recording completed jobs
- cap metadata sizes
- bound native messaging frames, app response lines, named pipe request lines, pipe read/write time, and side-effect request rate
- sanitize filenames in the desktop app
- log request origin fields
- rate-limit native host requests
- reject remote named pipe clients; same-user local clients are still untrusted and must pass desktop validation
- treat torrenting as P2P: peers can observe participation, seeding may continue after download completion, and cancel stops tracking without deleting downloaded files

Security boundary:

- extension validates for UX
- native host validates for protocol integrity
- desktop app validates again before queueing
