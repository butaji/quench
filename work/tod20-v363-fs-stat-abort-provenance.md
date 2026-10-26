# TOD-20 FS stat AbortSignal candidate

## Source identity

- Base commit: `f7a6d33dd643fcb5840f244b68d4f6e6b9b62744`.
- Changed implementation: `crates/quench-node/src/modules/fs_shared_vm.rs`.
- File SHA-256: `c28ee5176e2c12af2626b4718f131f15f4ad55a10357687d424e75b51517f10d`.
- The release runner was built from the frozen combined working-tree candidate (full binary diff SHA-256 `dac2250e8eb2a84b40f93531c5e0c73a57240c95cdd86e1637a4d27dfcb03fed`), which also contained separate cluster, VM/runtime, and HTTP experiments. Their focused outcomes are recorded independently; this document makes no claim that those experiments passed.

## Build and oracle

- Command: `cargo build -p quench-node-test --bin run-parallel --release`.
- Rust toolchain: 1.99; `CARGO_BUILD_JOBS=1`, `CARGO_INCREMENTAL=0`; target `/tmp/tod20-v164-session-target`.
- Matched native setup: ICU pkg-config `/tmp/tod20-icu/pkgconfig`, zlib `/workspace/quench/work/node-stable-zlib/lib/pkgconfig`, Brotli `/workspace/quench/work/node-stable-brotli/lib/pkgconfig`. The pinned package/archive identities are listed in `work/tod20-ipc-v362-provenance.md`.
- Build log SHA-256: `a57cb5c01f94c96719e54350c26aef7b4082b0b341388325936022fba9b297aa`.
- Runner SHA-256: `685429d2c031fd7bf2318745310944d13c649a3954c86e18ecdaead8916909a9`.
- Oracle: official Node `v26.12.1`, executable `/workspace/work/node-v26.12.1-linux-x64/bin/node`, SHA-256 `08f12d42fb304468e8ee86247fe686aae5b225836ea65e9d6566c11ae6ba2e8e`; fixtures from `/workspace/work/node-stable-release/node-v26.12.1/test/parallel`.

## Focused result

The pinned 30-fixture selected run used five workers and the unchanged 60-second per-fixture limit, inventory hash `58d2d8a8b1f4dfbe`. `test-fs-stat-abort-test.js` passed, covering callback and Promise stat/lstat AbortSignal assertions. `test-fs-promises-file-handle-stat.js` remained pass. The unrelated existing `test-fs-readfile-one-roundtrip.js` async-hooks failure remained fail.

- Results SHA-256: `1e482be2dac8040293dcf89ec17827d31812a400e2a2308026ec61f38d40ff43`.
- Checkpoint SHA-256: `dd9e1b386a160635aafe2b5825a69d75fb9383afeee313a739aad4c36fcf7f7d`.
- Run log SHA-256: `62b70ed1b741b5bdb53dc196da943d9fa18c4d74ed562b4be86004705d8175c1`.

This is focused-family evidence only. It does not change the frozen v362 full-census result of 3,564 pass / 650 skip / 638 fail / 157 timeout / 10 crash / 4 unclassified across 5,023 fixtures, and it does not count toward the 4,000-pass gate until the accepted committed source is run through the complete pinned census.
