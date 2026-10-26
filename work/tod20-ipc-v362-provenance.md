# Advanced IPC serialization compatibility candidate

## Source

- Base runtime commit: `caebc7359f40761d8466ea4b09771477a5984922` on `v2-cloud-node`.
- Modified tracked source: `crates/quench-node/src/host/shared_vm/commonjs.rs`.
- Candidate source file SHA-256: `95a9084d533e0b705841983ce9e1b4dc94e73124a08571738f0d0054868dfbaa`.
- Binary git-diff SHA-256: `76e03b4ef3691c5877fa5aafc3ac25693d89297b91892e9473a627ac74fbbdca`.
- `git diff --check` passes.

The V8 IPC decoder now maps invalid host-object tags to `ERR_INVALID_STATE`. Error `message` and `cause` properties are defined with their V8-compatible non-enumerable descriptors, fixing advanced Error round trips. The surrounding V8 wire encoding/decoding is the current experimental candidate; none of its focused outcomes are included in a full-census pass count.

## Build identity

- Command: `cargo build -p quench-node-test --bin run-parallel --release`.
- Rust toolchain: `1.99`; `CARGO_TARGET_DIR=/tmp/tod20-v164-session-target`; `CARGO_BUILD_JOBS=1`; `CARGO_INCREMENTAL=0`.
- Build log: `work/tod20-ipc-focused-build.log`, SHA-256 `3a4823740df7a2d30b1de85fae4be5bf8586f5108433d411de9a5015efa11052`.
- Runner: `/tmp/tod20-v164-session-target/release/run-parallel`, SHA-256 `68cba74394f8daf3a59a0ab03292aedca5cc928b48faac5619114c91e72d173f`.
- `PKG_CONFIG_PATH=/tmp/tod20-icu/pkgconfig:/workspace/quench/work/node-stable-zlib/lib/pkgconfig:/workspace/quench/work/node-stable-brotli/lib/pkgconfig`.
- ICU pkg-config descriptor `/tmp/tod20-icu/pkgconfig/icu-uc.pc`, SHA-256 `e4606f126a25674bedc3cf928f38691f9aa31a0140740c33b06aec1b0fcef51e`; ICU libraries `libicuuc.so` SHA-256 `a8e433e81075732faf255b17d4a25ce28632e41fef1a75e727ee7f4ed73ab151` and `libicudata.so` SHA-256 `a04b2b906193fa1e40f968a3d16d7d6c844a1fafbdd5bce6e9f67b01c124ff24`.
- zlib descriptor `work/node-stable-zlib/lib/pkgconfig/quench-node-zlib.pc`, SHA-256 `ffe8cfba16be369f59c8a9150e08cdccab8925461e2e9c26efda1b1f8537cd7c`; static archive `work/node-stable-zlib/lib/libquench_node_zlib.a`, SHA-256 `752b99b500732600d77273e6841d2786d1cc208fe2a897b1016defc21b637217`.
- Brotli descriptor `work/node-stable-brotli/lib/pkgconfig/quench-node-brotli.pc`, SHA-256 `07267b7a9dd67dd2eb584ac098623a9bd14b9a305f1df7cc8e76f3726dd99073`; static archive `work/node-stable-brotli/lib/libquench_node_brotli.a`, SHA-256 `dd9005f7541ab4de214fb7bb133a15a67f277a3f6ccb7e9384d6526f0d348cb8`.

## Oracle and focused results

- Node executable `/workspace/work/node-v26.12.1-linux-x64/bin/node`, version `v26.12.1`, SHA-256 `08f12d42fb304468e8ee86247fe686aae5b225836ea65e9d6566c11ae6ba2e8e`.
- Official fixture root `/workspace/work/node-stable-release/node-v26.12.1/test/parallel`.
- The five-fixture `advanced-serialization` filter used one worker and the unchanged 60-second per-fixture deadline; filter inventory hash `d05f86a4f08d8733`.
- Exact result: 5 pass / 0 skip / 0 fail / 0 timeout / 0 crash / 0 unclassified. All five fixtures: host-objects, largebuffer, splitted-length-field, advanced serialization, and cluster advanced serialization.
- Results: `work/tod20-ipc-v362-advanced-serialization-results.json`, SHA-256 `e4ca7783cd148c877d2477476d7cc1ca2c5ed404ed7a0b48c635651f16d3efa1`.
- Checkpoint: `work/tod20-ipc-v362-advanced-serialization-checkpoint.jsonl`, SHA-256 `f0f1715ef3f962598b705c0f302356c0f995c131e3f7bd67e718bf5286d9941d`.
- Run log: `work/tod20-ipc-v362-advanced-serialization-run.log`, SHA-256 `9b645423fc052d8f040cc12f8c7760dc305b56dd2030c29c1470c2d525bf70cd`.
- Standalone host-object fixture also passes 1/1; result SHA-256 `ffffb6fe5859bc266e3fe56cb9cccf82ba006cc0cc580a2fd2b2ac3a4309d9be`, checkpoint SHA-256 `134377866d6b08bc93610d48dfacc0fe84e991a80b48df657d78c513db83776d`, log SHA-256 `0f292f4d20c8b7d7bac840de8763b464ec8ec216e661643edd7cfbf3c9341645`.

## Acceptance status

These are family-only diagnostics. The frozen full Node v26.12.1 v362 census remains 3,564 pass / 650 skip / 638 fail / 157 timeout / 10 crash / 4 unclassified across 5,023 fixtures, source `cc9b25cb3ea74b7c076e3d511ff514fb7dda1181`. No new full-census passes are claimed. The lane gate remains 4,000 full-census passes, followed by latest-trunk integration and a complete merged-head census.
