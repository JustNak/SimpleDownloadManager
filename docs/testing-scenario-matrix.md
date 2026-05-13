# Testing Scenario Matrix

Default gates stay offline and deterministic. Live torrent swarm validation is opt-in through `SDM_TORRENT_BENCH_MAGNET`.

| Workflow | Deterministic scenario tests | Default rust/soak | Opt-in live |
| --- | --- | --- | --- |
| Normal HTTP download | `scenario_normal_http_completion_verifies_bytes_integrity_and_temp_cleanup` | Yes | No |
| Segmented HTTP retry/resume | `scenario_segmented_http_resumes_after_transient_segment_interruption` | Yes | No |
| Protected browser handoff | `scenario_protected_browser_handoff_uses_auth_without_persisting_secret` | Yes | No |
| Bulk downloads and archive identity | `scenario_bulk_download_retry_finalize_and_cleanup_flow` | Yes | No |
| Torrent queueing and lifecycle | `scenario_torrent_lifecycle_routes_pauses_reseeds_and_deletes_without_live_swarm` | Yes | No |
| Failure and recovery paths | `scenario_failure_paths_cover_server_range_integrity_duplicate_missing_and_restart` | Yes | No |
| Live HTTP segmented throughput | `http_bench::tests::live_http_benchmark_from_env` | No | `npm run test:live:http` |
| Full live torrent swarm transfer | `torrent_bench::tests::live_torrent_benchmark_from_env` | No | `npm run test:live:torrent` |

## Gate Mapping

| Command | Scope |
| --- | --- |
| `npm run test:scenarios` | Fast targeted pass for the deterministic workflow scenario layer. |
| `npm run test:rust` | Full desktop Rust test suite, including scenario tests, plus native-host Rust tests. |
| `npm test` | TypeScript/MJS tests followed by full Rust tests. |
| `npm run test:soak -- --minutes 60` | Repeats the full deterministic gate for one hour; live torrent checks are excluded. |
| `npm run test:live:http` | Runs the ignored live HTTP segmented benchmark only when `SDM_HTTP_BENCH_URL` is explicitly set. |
| `npm run test:live:torrent` | Runs the ignored live torrent benchmark only when `SDM_TORRENT_BENCH_MAGNET` is explicitly set. |

## Test Runner Notes

- Node-based test and scenario runners share `scripts/lib/run.mjs` for repo-root resolution, recursive test discovery, child-process execution, and local temp directory setup.
- Scenario and live torrent runners keep generated runtime output under `.tmp` so build outputs, release artifacts, and ignored local logs stay outside normal cleanup work.

## Coverage Notes

- Scenario tests use local fixture servers, temporary workspace paths, and state transitions only.
- Torrent default coverage validates routing, progress snapshots, pause/resume, seeding, external-use pause/reseed, scheduler slot release, cancel/delete cleanup, and persisted state behavior without joining a swarm.
- Live swarm behavior remains intentionally outside default and soak gates because it depends on external peers, trackers, and legal/user-authorized magnet input.
