# v362 HTTP Agent slot lifecycle experiment

## Scope

The lane baseline is v358 at runtime `87a6c362626d1a62351f48734a41c019ad78aa6b`, using official Node v26.12.1. This experiment changes the shared ClientRequest/Agent pool lifecycle in `crates/quench-node/src/modules/http_shared_vm/client.rs`:

- Keep a pre-response active Agent slot visible through the request error callback and release it on request `close`.
- For a request aborted from its response callback, keep the slot visible synchronously, mark close ready, and release on request `close` only after the socket teardown lifecycle.
- Prevent normal response settlement from releasing an aborted or already-released slot a second time.

This avoids both the prior premature pool removal and a queued request using a replacement socket before its aborted predecessor closes.

## Build and oracle

- Release build: `cargo build -p quench-node-test --bin run-parallel --release`, target `/tmp/tod20-v164-session-target`, Rust 1.99, `CARGO_INCREMENTAL=0`, one build job.
- Build log SHA-256: `cbc1cfc9c99db1e361d2eb3c400ccdaefcae9427fe82fff05528eeef8f333048`.
- Runner binary SHA-256: `ec0be0dbef8e722a84a39de28ba47662a84eb5673a3deec1658b8de695fcf9a4`.
- Official Node executable: `/workspace/work/node-v26.12.1-linux-x64/bin/node`, SHA-256 `08f12d42fb304468e8ee86247fe686aae5b225836ea65e9d6566c11ae6ba2e8e`.
- Official fixture root: `/workspace/work/node-stable-release/node-v26.12.1/test/parallel`.
- Focused deadline 60 seconds per fixture, one worker. The complete family inventory hash is `7ecafe548eccc85f` (28 fixtures).
- Matched native configuration: ICU at `/tmp/tod20-icu`, zlib at `/workspace/quench/work/node-stable-zlib`, Brotli at `/workspace/quench/work/node-stable-brotli`, via their pkg-config directories.

## Focused results

The exact official Agent family completed as 19 pass / 0 skip / 2 fail / 7 timeout / 0 crash / 0 unclassified. Compared with the same 28 filenames in v358, transitions were pass→pass 17, fail→pass 2, fail→fail 2, timeout→timeout 7. Only `test-http-agent-keepalive.js` and `test-http-agent-maxsockets-respected.js` changed from fail to pass; no regressions were observed.

- Family results SHA-256: `b547e1041de121844309e0256fb6d5e272dd84e5538b3a58be897afa4cc9798e`.
- Family checkpoint SHA-256: `734baf8d42e6f0a4dd6ee571e499f37f6d8397e41f9e4f148cbe4896a77019a1`.
- Separate official `test-http-agent-maxsockets-respected.js` rerun: pass 1/1, results SHA-256 `fd5c6ed3b341b3eacae8ef706e5f8a375a788afabe146f62ca400f915ba2cf10`.
- The latest full pass total is still the v358 census until a complete matched 5,023-fixture run finishes. Focused counts do not add to qualification.

## Follow-up and acceptance

Run the unchanged complete 5,023-fixture Node v26.12.1 census against the committed runtime source, preserve all six outcome classes and exact binary/native input identities, reconcile checkpoint/result classifications, and compare fixture transitions with v358. Keep the four execve replacement outcomes unclassified unless replacement-side worker evidence exists. Then continue the highest-coverage reproduced roots toward 4,000 passes, integrate latest trunk only through the recorded owner-reviewed conflict plan, and finish with the complete merged-head census.
