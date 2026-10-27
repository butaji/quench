# TOD-20 v392 FileHandle readFile buffer option candidate

- Source before evidence commit: `3067a41ed6457591605cd90757016f4199d402ea` plus the sole tracked edit `crates/quench-node/src/modules/fs_shared_vm.rs` (20 insertions, 2 deletions).
- Candidate behavior: `fs.promises.readFile` applies `options.buffer` or its size callback to returned file bytes; `FileHandle.readFile` fills the provided Buffer, validates insufficient capacity with `ERR_INVALID_ARG_VALUE`, supports encoding, and returns a view into the filled buffer. This follows Node v26.12.1 `readFileHandleWithUserBuffer` semantics.
- Official oracle/inventory: Node v26.12.1, 5,021 fixtures, inventory hash `aa0d2d758e3809c7`, fixture root `/tmp/tod20-node-v26.12.1-official-source/test/parallel`.
- Focused `test-fs-promises-*` cohort: 36 cases; candidate 31 PASS / 5 FAIL; matched v391 baseline 30 PASS / 6 FAIL. Transition matrix: 30 PASS→PASS, 5 FAIL→FAIL, 1 FAIL→PASS (`test-fs-promises-readfile-buffer-option.js`), zero regressions.
- Full `test-fs-*` cohort: 385 cases; candidate 351 PASS / 11 TAP SKIP / 20 FAIL / 3 TIMEOUT / 0 CRASH / 0 UNCLASSIFIED. Matched v391 baseline 350 / 11 / 21 / 3 / 0 / 0. Transition matrix: 350 PASS→PASS, 20 FAIL→FAIL, 11 SKIP→SKIP, 3 TIMEOUT→TIMEOUT, 1 FAIL→PASS (the buffer-option fixture), zero regressions.
- Runner SHA-256: `be10dce68c2f350dcb4acd0f8a07d767241b3065effdd0bd60625922962f43b3`.
- Quench CLI SHA-256: `1f7e20c2a9f695c26be9d06da77dfd30f4e51907eb9643b0431f2aec11f5cbf7`.
- Node v26.12.1 executable SHA-256: `08f12d42fb304468e8ee86247fe686aae5b225836ea65e9d6566c11ae6ba2e8e`.
- Build: Rust 1.99 release; `PKG_CONFIG_PATH=/tmp/tod20-icu/pkgconfig:/workspace/quench/work/node-stable-zlib/lib/pkgconfig:/workspace/quench/work/node-stable-brotli/lib/pkgconfig`; matched `quench-node` and `run-parallel` built together in `/tmp/tod20-v164-session-target`.
- Result JSON SHA-256: `a391a65a1cee8c1c65b8549d2fdf0578879c227120af14746bb64a33c1b6965a`.
- Checkpoint JSONL SHA-256: `c5f7518ef1a1eb580b3dff6d1347e2c702baa55998cd5a654863183a63643e49`.
- These cohort passes are not added to the official 5,021-case gate. v391 remains the last full census until the new committed exact-source full run completes.
