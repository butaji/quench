# TOD-20 v392 exact-source official Node parallel census

- Completed: 2026-10-27 03:42 UTC
- Source: branch `v2-cloud-node`, commit `93fc0ed70a9ea24e0767da61f577c714f1db9798` (`fix(fs): honor readFile buffer options`).
- Node oracle: v26.12.1 executable `/workspace/work/node-v26.12.1-linux-x64/bin/node`, SHA-256 `08f12d42fb304468e8ee86247fe686aae5b225836ea65e9d6566c11ae6ba2e8e`.
- Inventory: official Node v26.12.1 `test/parallel`, 5,021 fixtures, inventory hash `aa0d2d758e3809c7`, root `/tmp/tod20-node-v26.12.1-official-source/test/parallel`.
- Runner SHA-256 `be10dce68c2f350dcb4acd0f8a07d767241b3065effdd0bd60625922962f43b3`; child Quench CLI SHA-256 `1f7e20c2a9f695c26be9d06da77dfd30f4e51907eb9643b0431f2aec11f5cbf7`.
- Matched release build: Rust 1.99, packages `quench-node` and `quench-node-test`, bins `quench-node` and `run-parallel`, target `/tmp/tod20-v164-session-target`; pinned ICU/zlib/Brotli `PKG_CONFIG_PATH=/tmp/tod20-icu/pkgconfig:/workspace/quench/work/node-stable-zlib/lib/pkgconfig:/workspace/quench/work/node-stable-brotli/lib/pkgconfig`.
- Runner configuration: 5 jobs, 60-second per-fixture deadline, launched with cwd `/workspace/quench` so the result records runtime commit `93fc0ed70a9ea24e0767da61f577c714f1db9798`.
- Terminal counts: **3,410 PASS / 651 TAP SKIP / 777 FAIL / 175 TIMEOUT / 8 CRASH / 0 UNCLASSIFIED** = 5,021. The 4,000-PASS goal remains 590 PASS away.
- Audit: 5,021 result paths, all unique; 5,021 unique checkpoint paths; path sets match; all checkpoint rows have inventory hash `aa0d2d758e3809c7`; all 651 skip rows begin with TAP `1..0 # Skipped:`.
- Compared with v391 full baseline (3,407 PASS / 651 SKIP / 776 FAIL / 179 TIMEOUT / 9 CRASH / 0 UNCLASSIFIED): transition matrix is 3,401 PASS→PASS, 773 FAIL→FAIL, 651 SKIP→SKIP, 173 TIMEOUT→TIMEOUT, 7 CRASH→CRASH, 3 FAIL→PASS, 5 TIMEOUT→PASS, 1 CRASH→PASS, 4 PASS→FAIL, 2 PASS→TIMEOUT, 1 TIMEOUT→CRASH. Net +3 PASS. The v391 run used a different runner binary; the non-FS transitions below are retained as source-matched outcomes, not attributed to the FS patch.
- Changed fixtures: `test-child-process-exec-kill-throws.js` fail→pass; `test-cluster-primary-kill.js` pass→timeout; `test-domain-stack.js` timeout→pass; `test-domain-vm-promise-isolation.js` timeout→pass; `test-fs-promises-readfile-buffer-option.js` fail→pass; `test-http2-autoselect-protocol.js` timeout→crash; `test-http2-client-socket-destroy.js` pass→fail; `test-http2-https-fallback.js` timeout→pass; `test-http2-max-settings.js` timeout→pass; `test-http2-options-server-request.js` fail→pass; `test-http2-options-server-response.js` pass→fail; `test-http2-session-timeout.js` pass→fail; `test-pipe-abstract-socket-http.js` pass→fail; `test-vfs-watch.js` crash→pass; `test-worker-http2-stream-terminate.js` timeout→pass; `test-zlib-zip-fuzz.js` pass→timeout.

Compressed terminal evidence in this directory:
- `tod20-v392-official-results.json.gz` (raw SHA-256 `966b6fc0d09d6cc955a06a0a099dad7de5d157f2601f3ee50b29cd41872bf013`)
- `tod20-v392-official-checkpoint.jsonl.gz` (raw SHA-256 `9de606908c340f171266bdad21ec4c63f3d5d18b0bc2381b8f9ead7ecca987e1`)
- `tod20-v392-official-run.log.gz` (raw SHA-256 `0d31a4080012b4eb5c1967b25eaf3cba7406d0e8662162279857d6f24639ee1d`)

The previous official full run remains preserved under `tod20-v391-session-finish-official-*`. This v392 census is the current acceptance result; focused-family passes are excluded.
