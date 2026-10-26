# TOD-20 official Node 26.12.1 source census provenance

## Oracle and exact official fixtures

- Official release: Node v26.12.1 source tarball `node-v26.12.1.tar.xz`, SHA-256 `053e3b296859b6d8d93f99ef97fa9cec1c4ee171c72968100c649978c1dbd38c` (checked against the release `SHASUMS256.txt`).
- Oracle executable: `/workspace/work/node-v26.12.1-linux-x64/bin/node`, v26.12.1, SHA-256 `08f12d42fb304468e8ee86247fe686aae5b225836ea65e9d6566c11ae6ba2e8e`.
- The exact release `test/parallel` fixture names are at `/tmp/tod20-node-v26.12.1-official-source/test/parallel`: 5,021 names; name+content manifest SHA-256 `261b0f6242516c56864ebd9f3ff70f7dca089180295b1609a46bd24c31c681d2`; harness inventory hash `aa0d2d758e3809c7`.
- Official `lib/` files are physically copied to `/tmp/tod20-node-v26.12.1-official-source/lib`, with no symlinks. The tree has 428 files; sorted path+content manifest SHA-256 `62b780b0381a4ae1c1b28f6beeb5c614c90ae936f468a48624986ad525c6148c`. Key sources: `primordials.js` `c5bd695fc5a09d206acb917862411694a12c696f9063d982ccf1a25787ee3622`, `internal/console/global.js` `3822573834e98586cf342c37ff92e0b03eeefdb859e15f722e324b062ff04a0e`, `internal/main/test_runner.js` `351ce585c9d7b504691637223b77295fada44d42839a27766e6a327983c258c3`, and `fs.js` `21e9604151bcdfdacec9ec6e127e8ba906fb1b4e9415fd0de3221b7e92a08ef3`.
- Focused source-root checks with the pinned Quench executable and Node v26.12.1 environment: `test-abort-controller-any-timeout.js` passes 1/1, and `test-console-*` passes 21/21. The original `primordials` failure was caused by symlink canonicalization taking internal library files outside `QUENCH_NODE_LIBRARY_ROOT`; the physical copy preserves their internal-module classification.

## Frozen Quench executables and native inputs

- Runtime source commit: `ef64d9b235962b17ab88005d79bf8439b5039208`; embedded `tests/node` submodule: `26e72d85a611e222e3f06025de8a3c496923bacd`. The selective external release root has no Git metadata, so result JSON's `runtime_commit`/`tests_node_commit` fields do not identify these; these commits are recorded from the frozen build worktree.
- Quench child executable `/tmp/tod20-v164-session-target/release/quench-node`: SHA-256 `71b058699fda9146c61de93db81e668f281eb10288eb7eedb57fedf8f32ba4ad`.
- Parallel runner `/tmp/tod20-v164-session-target/release/run-parallel`: SHA-256 `b0e8e9db988ab934b3a2a491129bc4b35410a78864ba100af73724a9a2853b64`.
- Every full attempt uses five workers, 60-second fixture deadlines, the same v26.12.1 binary on `PATH`, and the same `QUENCH_NODE_TEST_EXECUTABLE`. Build/runtime environment includes `CARGO_TARGET_DIR=/workspace/quench/target`, `CARGO_HOME=/workspace/.cargo`, `RUSTUP_HOME=/workspace/.rustup`, incremental dev/test/iteration profiles, `CARGO_BUILD_JOBS=4`, `NODE_EXTRA_CA_CERTS=/usr/local/share/ca-certificates/environment-proxy-ca.crt`, and `NODE_PATH=/opt/codex/runtimes/codex-primary-runtime/dependencies/node/node_modules`.
- Static linked inputs were verified before the complete-root census: zlib archive `work/node-stable-zlib/lib/libquench_node_zlib.a` SHA-256 `752b99b500732600d77273e6841d2786d1cc208fe2a897b1016defc21b637217`, pkg-config SHA-256 `ffe8cfba16be369f59c8a9150e08cdccab8925461e2e9c26efda1b1f8537cd7c`; Brotli archive `work/node-stable-brotli/lib/libquench_node_brotli.a` SHA-256 `dd9005f7541ab4de214fb7bb133a15a67f277a3f6ccb7e9384d6526f0d348cb8`, pkg-config SHA-256 `07267b7a9dd67dd2eb584ac098623a9bd14b9a305f1df7cc8e76f3726dd99073`; ICU pkg-config `/tmp/tod20-icu/pkgconfig/icu-uc.pc` SHA-256 `e4606f126a25674bedc3cf928f38691f9aa31a0140740c33b06aec1b0fcef51e`.

## v384: complete fixture set, incomplete Node library root (diagnostic)

The frozen v384 run used the exact official 5,021 fixture names and inventory hash, but its selective extraction omitted `lib/`. `parallel_checkout_root()` therefore selected an incomplete library root and Quench fell back to embedded `tests/node`. Preserve its output for fixture-root diagnostics only; it is not a matched official-library baseline.

- Source `ef64d9b235962b17ab88005d79bf8439b5039208`, Quench child and runner hashes as above; Node v26.12.1 executable as above.
- Terminal counts: 3,391 pass / 650 TAP skip / 793 fail / 177 timeout / 10 crash / 0 unclassified; runner exit 1 reflected fixture failures.
- Results `work/full-node26-v384-official-results.json`: SHA-256 `dbaf03579a3f131516ea81e57405588a12e01231feb5763aa65c7a50940b1c86`.
- Checkpoint `work/full-node26-v384-official-checkpoint.jsonl`: SHA-256 `871475830071d3358ebeb19c34f1ffd4aba56bfcf4e81e56d46f434e65c7cbf8`.
- Log `work/full-node26-v384-official-run.log`: SHA-256 `b2e05e3639c93bd869636f42f8836704002de76d86f08454caf0808e84e0e504`.
- All 5,021 fixture rows are unique, result JSON and checkpoint outcomes match exactly, all 650 TAP skips have the explicit `1..0 # Skipped:` marker, and unclassified is zero. Exact mapping is audited by `work/tod20-census-audit.py`.

## v385: stopped after a shared symlink-root failure (not qualifying)

This attempt used the complete fixture tree but linked `lib/` through a symlink. Quench canonicalizes library paths; the resolved paths then sat outside the configured root, bypassing its Node-internal CommonJS wrapper and its `primordials` argument. All 2,609 completed rows failed at `ReferenceError: primordials is not defined` in `lib/internal/console/global.js`. The repetitive sweep was stopped after preserving the evidence; it has no terminal results JSON and contributes no pass total.

- Checkpoint `work/full-node26-v385-official-complete-checkpoint.jsonl`: 2,609 failures, inventory hash `8e1486ae6198eeb5`, SHA-256 `98504b41bc3a30848edb49e49be77a0dbcda481d9e8947bbab28718f59d06f6e`.
- Partial log `work/full-node26-v385-official-complete-run.log`: SHA-256 `6ab2a812c8c92bf785175c45d0f0e41c5f39c5d947270232930e67f2fa1f888a`.
- Resolution: copy the exact Node v26.12.1 `lib/` files physically under the original tarball-extracted root. This preserves official fixture paths and inventory hash while keeping resolved internal source paths under the configured Node root. No runtime source or build input was changed for this setup repair.

## v386: complete official source, running

v386 runs the original 5,021 official fixtures at `/tmp/tod20-node-v26.12.1-official-source/test/parallel` with the physical official release `lib/` at `/tmp/tod20-node-v26.12.1-official-source/lib`; inventory hash is restored to `aa0d2d758e3809c7`. It uses the exact frozen v384 runner and Quench child, official Node v26.12.1 on `PATH`, five workers and 60-second deadlines. This is the first complete-source full census and is the candidate clean baseline; preserve its complete result and transitions before attributing fixes.

- Runner process PID `1679035`; started 2026-10-26 18:52 UTC. Results/checkpoint/log: `work/full-node26-v386-official-complete-results.json`, `work/full-node26-v386-official-complete-checkpoint.jsonl`, `work/full-node26-v386-official-complete-run.log`.
- At 2026-10-26 18:53 UTC, 78/5,021 observations had arrived: 74 pass / 4 fail / 0 skip / 0 timeout / 0 crash / 0 unclassified; no terminal results JSON yet. The initial `primordials` failure is absent. These are interim counts, not the gate result.
- Disk headroom at start was 115 MB on `/workspace` and 268 MB on `/tmp`; no build or second census is running concurrently.

The full original acceptance remains open: improve shared Node compatibility roots, qualify at 4,000+ official parallel passes on `v2-cloud-node`, then integrate the current owner-routed trunk batches and rerun the complete 5,021-fixture gate on the exact merged head.
