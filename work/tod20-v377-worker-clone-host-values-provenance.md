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

These are focused results only. v378 below is the complete committed-source census; focused gains do not count toward the gate. Latest-trunk integration and the merged-head full gate remain required.


## v378 committed-source full census (2026-10-26)

The committed v377 source was rebuilt (build log `work/tod20-v378-committed-worker-clone-build.log`, SHA-256 `09f2b93c4bce5bfb43dca18ca8d9c1dca2831c7d2ffb73b00b96f8f73a607f86`) and tested without changing the acceptance configuration. Runtime commit is `4e70c06b7398bc5c4a1345f42eae91a03a2e2d70`; `crates/quench-node/src/polyfills/shared_vm.rs` SHA-256 is `c9becf542d4ec975469bb69c7593c1832b05ac1bfa9d794c2744c6a64fbed017`. Runner `/tmp/tod20-v164-session-target/release/run-parallel` SHA-256 is `54a0230ec06b768f098618891cb6a25bb4594bbba4cc508042914d2a6af4a7df`.

Pinned inventory: Node v26.12.1, `/workspace/work/node-stable-release/node-v26.12.1/test/parallel`, inventory hash `47b65dd4945a6e57`, 5,023 fixtures, 5 jobs, 60-second fixture timeout, Linux x86_64. Terminal accounting is **3,583 pass / 650 skip / 623 fail / 157 timeout / 6 crash / 4 unclassified** (total 5,023). The runner ended with exit 1 because fixtures failed; its `all:` line agrees with the result JSON. All 650 skip outputs contain an explicit TAP skip marker. The four unclassified records remain the four `test-process-execve*` cases (`test-process-execve-on-exit.js`, `test-process-execve-permission-granted.js`, `test-process-execve-socket.js`, `test-process-execve.js`): exit 0, no worker outcome and empty stdout/stderr. None is counted as a pass. JSON has 5,023 unique results; the 5,023-line checkpoint has the same fixture set and observations.

Artifacts: results `work/full-node26-v378-worker-clone-results.json`, SHA-256 `5948a218406abe07099141695e515c80514d8ca03a45ba0deb15db5ef3930f8d`; checkpoint `work/full-node26-v378-worker-clone-checkpoint.jsonl`, SHA-256 `1eb6ae4c4e82347a10aad9e9de89362abf3649a9a783644eea1c7ca3c20e9295`; log `work/full-node26-v378-worker-clone-run.log`, SHA-256 `76bb1614429ced914c41aa87d98c3bb0f37a43502fdf4a9443cace967dff2513`.

Compared with v374, transitions are 3,572 pass→pass; 650 skip→skip; 621 fail→fail; 156 timeout→timeout; 5 crash→crash; 4 unclassified→unclassified; 8 fail→pass; 2 timeout→pass; 1 crash→pass; 1 timeout→fail; 1 crash→timeout; 1 pass→fail; and 1 pass→crash. Net gain is 9 passes (3,574 to 3,583), leaving 417 passes to the lane gate. The clone-family gains are preserved, but the full run also contains transitions outside that family and does not meet acceptance.

The two apparent pass regressions were `test-http2-options-server-response.js` (pass→fail) and `test-worker-cjs-workerdata.js` (pass→crash). A single-job exact-fixture recheck reproduced the HTTP/2 `ECONNRESET`; it passed the worker CJS fixture. The only timeout→fail was `test-tls-get-ca-certificates-system-without-flag.js`; a single-job recheck reproduced its system CA certificate list mismatch. These cases do not exercise the shared clone traversal changed by v377, so treat them as unexplained test/environment outcomes until an exact-source comparison establishes causation. `test-vfs-fs-promises.js` also changed crash→timeout and `test-vfs-watch-recursive.js` crash→pass. Preserve these transitions; do not infer a clean regression-free census. Recheck artifacts are `work/v378-recheck-test-http2-options-server-response.{json,jsonl,log}`, `work/v378-recheck-test-tls-get-ca-certificates-system-without-flag.{json,jsonl,log}`, and `work/v378-recheck-test-worker-cjs-workerdata.{json,jsonl,log}`.

Next: use the complete v378 transitions to investigate a large, shared remaining failure cluster. Do not count focused rechecks toward the gate. Keep the lane gate at 4,000 complete pinned-census passes, then do owner-reviewed integration with latest trunk and rerun the same complete census on exact merged head. Latest trunk conflict inventory and subsystem map remain in `work/tod20-v362-vs-trunk-20261026-conflict-map.md`.
