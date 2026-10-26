# TOD-20 worker structured-clone host-value preservation — v377

## Candidate and hypothesis

- Base lane commit: `30f95d9325` (v374 census and provenance recorded).
- Changed source: `crates/quench-node/src/polyfills/shared_vm.rs`, SHA-256 `c9becf542d4ec975469bb69c7593c1832b05ac1bfa9d794c2744c6a64fbed017`.
- In the MessagePort transfer graph walk, preserve zero-enumerable non-plain objects so the shared structured-clone codec can process their opaque internal slots. This covers CryptoKey, KeyObject and SharedArrayBuffer. The old walk replaced them with empty objects before the codec saw them.
- The first no-MessagePort fast-path candidate was rejected: it failed `test-worker-workerdata-messageport.js` because a MessagePort omitted from `transferList` must still throw `DataCloneError`. The accepted candidate keeps the walk and its transfer-list validation.
- `git diff --check` passes.

## Build and oracle

- Command: `cargo build -p quench-node-test --bin run-parallel --release`.
- Toolchain/environment: Rust 1.99, `CARGO_TARGET_DIR=/tmp/tod20-v164-session-target`, one build job, incremental disabled.
- Build log `work/tod20-v377-worker-clone-host-values-build.log`, SHA-256 `65cad46e5f1f9c0ba936443c41fa3ef64bd25be16d34ad405a877d2553803034`.
- Release runner `/tmp/tod20-v164-session-target/release/run-parallel`, SHA-256 `54a0230ec06b768f098618891cb6a25bb4594bbba4cc508042914d2a6af4a7df`.
- Companion runtime `/tmp/tod20-v164-session-target/release/quench-node`, SHA-256 `58dd76a0112119cbbd5482cd5e520480fc8b158db7cf322418b5c8984bb07302`.
- Official Node `/workspace/work/node-v26.12.1-linux-x64/bin/node`, v26.12.1, SHA-256 `08f12d42fb304468e8ee86247fe686aae5b225836ea65e9d6566c11ae6ba2e8e`.
- Fixture root `/workspace/work/node-stable-release/node-v26.12.1/test/parallel`.
- Native config uses ICU, zlib and Brotli inputs pinned with exact `.pc` and archive hashes in `work/tod20-v374-fs-writer-provenance.md`.

## Focused results

The exact transition set of 11 official fixtures all passes on v377. It includes the six CryptoKey/KeyObject worker cases, both SharedArrayBuffer worker cases, the three worker MessagePort transfer gains, and the invalid-transfer-list assertion in `test-worker-workerdata-messageport.js`.

The broader 214-case `worker` filename cohort completed as 149 pass / 11 skip / 51 fail / 2 timeout / 1 crash / 0 unclassified, inventory `2197f3d3249bd0f9`. Compared with those same fixtures on complete v374: 145 pass→pass, 51 fail→fail, 4 fail→pass, 11 skip→skip, 2 timeout→timeout, and 1 crash→crash. No previously passing fixture regressed. All 11 skips contain the explicit TAP skip marker.

- Results `work/tod20-v377-worker-family-results.json`, SHA-256 `52f1efd90f58fbaaaddc6de27dcb6b98cef01ce1257ff56fd5eb56d2fe7f7b8e`.
- Checkpoint `work/tod20-v377-worker-family-checkpoint.jsonl`, SHA-256 `33b1eac85c20fd92c8a0b2bab304d762b9b6be24f2c3843f13e1b0385a4c14af`.
- Log `work/tod20-v377-worker-family.log`, SHA-256 `c2798270da443d4bcc4387fe667690fa2e38fc3de226aa252d6f3d4a1ec7bcf5`.

The broader 258-case `crypto` (including webcrypto) cohort completed as 219 pass / 11 skip / 27 fail / 1 timeout / 0 crash / 0 unclassified, inventory `e3eb017826120573`. Compared with matched v374 files: 213 pass→pass, 27 fail→fail, 6 fail→pass, 11 skip→skip, 1 timeout→timeout. No previously passing fixture regressed. All 11 skips contain the explicit TAP skip marker.

- Results `work/tod20-v377-crypto-family-results.json`, SHA-256 `3f7fad7ea73e192158a2b1810d55c15a560244504b0bee12856f6769a8de529b`.
- Checkpoint `work/tod20-v377-crypto-family-checkpoint.jsonl`, SHA-256 `248c00bd6ab90424dd1e5a6f09d6c7321f3c822f4f54729dc06dbfa3083cd8d2`.
- Log `work/tod20-v377-crypto-family.log`, SHA-256 `71b307c23f0d03e95115ff3add11f197d4143e159fc14a6a5c17e2734eab8efb`.

A separate composition probe sends both a CryptoKey and a transferred MessagePort in the same payload and verifies the key algorithm/type and a message through the transferred endpoint. It passes (`CryptoKey secret AES-GCM`); log SHA-256 `25e31f21a743a77c4288bc6a567b30883d6efbcff3b5db9d4f0a03105c631fc7`. The selected official test command is preserved in `work/run-tod20-v377-worker-clone-host-values-selected.sh`, SHA-256 `7c5865e12b4ed745168d6fe0937b2529cc97da9ea889470b85301f8a48a7ac38`.

These are focused results only. Latest complete full census remains v374 at 3,574 pass / 650 skip / 629 fail / 159 timeout / 7 crash / 4 unclassified, 426 passes short of the lane gate. This candidate must be committed, rebuilt from its committed source and run through the unchanged complete census before any pass gains count. Latest-trunk integration and the merged-head full gate remain required.
