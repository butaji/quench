# TOD-20 HTTP Agent free-slot lifecycle candidate

## Source and build identity

- Published base commit: `fab0d4e6707c5dfebd56ccc7179912332bf3d3ee`.
- Changed implementation: `crates/quench-node/src/modules/http_shared_vm/client.rs`.
- File SHA-256: `7b11089a3aea9a6e3ebaeb7b72f186ae7fdba3ebd647d9243307014f615bc902`.
- The runner was built against a combined working-tree candidate based on `4eb213d8c83b9784c7bf01109c3e94d45db1a754`, complete diff SHA-256 `b2035b8c734dd95cf881a1accf3a3d844547ca6b932fb903544bdf8414072f23`. That exact binary also contained separate VM, cluster, worker, FS, HTTP/1 and HTTP/2 experiments; this record claims only the Agent transitions below.
- Release build command: `cargo build -p quench-node-test --bin run-parallel --release`; Rust 1.99; one build job, incremental disabled; target `/tmp/tod20-v164-session-target`.
- Build log SHA-256: `557632aad1352908b7d808c36188637f5f12cd614f3b96224c52b2545e56fb5c`.
- Runner SHA-256: `3a26bd8feda17f59d5d7dca26fecb1d526c4ba8514b6aac7ddb89e08cb284bd1`.

## Pinned focused result

The run used official Node `v26.12.1` (binary SHA-256 `08f12d42fb304468e8ee86247fe686aae5b225836ea65e9d6566c11ae6ba2e8e`), official fixture files from `/workspace/work/node-stable-release/node-v26.12.1/test/parallel`, five workers and the unchanged 60-second per-fixture deadline. Selected inventory hash: `fa8095fab00f4534` (38 fixtures).

Three prior timeouts moved to pass: `test-http-agent-error-on-idle.js`, `test-http-agent-timeout.js`, and `test-http-agent-timeout-debug.js`. `test-http-agent-free-socket-pending-read.js` and `test-http-agent-free-socket-data-guard.js` remained pass. The two uninitialized-Agent fixtures remained timeouts, and `test-http-agent-domain-reused-gc.js` remained fail, so those distinct roots remain open.

- Results SHA-256: `f2997d43c9e8d872c05cbc9e875606357af739cc0249731f3427a644ab412e53`.
- Checkpoint SHA-256: `3c2b684ac1eef181a93ff26c0a55b00d9340c40c60e3f67862f690d4ad9c0044`.
- Run log SHA-256: `91e1ba721bd8d8072c68700e3b88bc98e4af30f8937372af753f5c72066fb20e`.

These are focused results only. The complete v362 lane baseline remains 3,564 pass / 650 skip / 638 fail / 157 timeout / 10 crash / 4 unclassified across 5,023 fixtures; no full-census passes are claimed until the exact committed source completes that gate.
