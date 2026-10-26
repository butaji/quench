# TOD-20 SourceTextModule graph candidate

## Source and build identity

- Base commit: `4eb213d8c83b9784c7bf01109c3e94d45db1a754`.
- VM implementation files: `crates/quench-node/src/modules/vm_shared_vm.rs`, `crates/quench-node/src/host/node_host.rs`, `crates/quench-node/src/host/shared_vm.rs`, `crates/quench-runtime/src/api_native.rs`, and `crates/quench-runtime/src/vm/embedding.rs`.
- The release runner was built from the combined candidate tree, whose complete binary diff at build start was SHA-256 `b2035b8c734dd95cf881a1accf3a3d844547ca6b932fb903544bdf8414072f23`. The same tree also contained separately tested cluster, worker, FS, and HTTP experiments. This VM record claims only the six VM fixture outcomes below.
- Command: `cargo build -p quench-node-test --bin run-parallel --release`, Rust 1.99, one build job, incremental disabled, target `/tmp/tod20-v164-session-target`.
- Build log SHA-256: `557632aad1352908b7d808c36188637f5f12cd614f3b96224c52b2545e56fb5c`.
- Runner SHA-256: `3a26bd8feda17f59d5d7dca26fecb1d526c4ba8514b6aac7ddb89e08cb284bd1`.

## Pinned focused result

The run used official Node `v26.12.1`, executable SHA-256 `08f12d42fb304468e8ee86247fe686aae5b225836ea65e9d6566c11ae6ba2e8e`, official fixtures from `/workspace/work/node-stable-release/node-v26.12.1/test/parallel`, five workers, and 60 seconds per fixture. The selected inventory had 38 fixtures and hash `fa8095fab00f4534`.

All six selected VM fixtures passed: `test-vm-module-evaluate-while-evaluating.js` (crash→pass), `test-vm-module-link.js`, `test-vm-module-link-shared-deps.js`, and the three `test-vm-module-linkmodulerequests` fixtures (all fail→pass). The overall selected run was 17 pass / 16 fail / 5 timeout / 0 crash / 0 skip / 0 unclassified; other failures and timeouts belong to separate candidate areas.

- Results SHA-256: `f2997d43c9e8d872c05cbc9e875606357af739cc0249731f3427a644ab412e53`.
- Checkpoint SHA-256: `3c2b684ac1eef181a93ff26c0a55b00d9340c40c60e3f67862f690d4ad9c0044`.
- Run log SHA-256: `91e1ba721bd8d8072c68700e3b88bc98e4af30f8937372af753f5c72066fb20e`.

This is focused evidence only and adds no passes to the frozen complete v362 result (3,564 pass / 650 skip / 638 fail / 157 timeout / 10 crash / 4 unclassified across 5,023 fixtures). The 4,000-pass target and complete latest-trunk merged-head census remain open.
