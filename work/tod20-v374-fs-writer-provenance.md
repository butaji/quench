# TOD-20 FileHandle writer compatibility — v374 lane census

## Frozen implementation

- Base commit: `45b61875069239100cc3630cf94de3f53cfe3dca` on branch `v2-cloud-node`.
- Changed file: `crates/quench-node/src/modules/fs_shared_vm.rs`.
- File SHA-256: `7b9732d6252c3502fdcc50bd9027b99c70133b2f38ac726da90e79317bce6b53`.
- Binary git diff SHA-256 before commit: `5a1179a6057d978ca6c0428b081217c265622bdef753da08066fedcc916bfb71`.
- The change implements FileHandle writer limits/coercion, close and error states, queued write ordering, failure reasons, idempotent `end`, and async/sync disposal behavior.

## Build and oracle

- Release runner: `/tmp/tod20-v164-session-target/release/run-parallel`, SHA-256 `b3da9cb8aca2f15745bcf120a855b3bb3524c2c0e69e6f63d52d4594f9b4528c`.
- Companion runtime: `/tmp/tod20-v164-session-target/release/quench-node`, SHA-256 `58dd76a0112119cbbd5482cd5e520480fc8b158db7cf322418b5c8984bb07302`.
- Release build log: `work/tod20-fs-family-clean-cluster-v373-build.log`, SHA-256 `f3b3b35e9befad27b1c3c987a0c5d38849598d91d54edb8ddb4ab793110252eb`.
- Pinned Node executable `/workspace/work/node-v26.12.1-linux-x64/bin/node`, version `v26.12.1`, SHA-256 `08f12d42fb304468e8ee86247fe686aae5b225836ea65e9d6566c11ae6ba2e8e`.
- Official fixture root `/workspace/work/node-stable-release/node-v26.12.1/test/parallel`.
- Native `PKG_CONFIG_PATH=/tmp/tod20-icu/pkgconfig:/workspace/quench/work/node-stable-zlib/lib/pkgconfig:/workspace/quench/work/node-stable-brotli/lib/pkgconfig`.
- ICU descriptor `icu-uc.pc` SHA-256 `e4606f126a25674bedc3cf928f38691f9aa31a0140740c33b06aec1b0fcef51e`; libraries `libicuuc.so` SHA-256 `a8e433e81075732faf255b17d4a25ce28632e41fef1a75e727ee7f4ed73ab151` and `libicudata.so` SHA-256 `a04b2b906193fa1e40f968a3d16d7d6c844a1fafbdd5bce6e9f67b01c124ff24`.
- zlib descriptor `quench-node-zlib.pc` SHA-256 `ffe8cfba16be369f59c8a9150e08cdccab8925461e2e9c26efda1b1f8537cd7c`; archive `libquench_node_zlib.a` SHA-256 `752b99b500732600d77273e6841d2786d1cc208fe2a897b1016defc21b637217`.
- Brotli descriptor `quench-node-brotli.pc` SHA-256 `07267b7a9dd67dd2eb584ac098623a9bd14b9a305f1df7cc8e76f3726dd99073`; archive `libquench_node_brotli.a` SHA-256 `dd9005f7541ab4de214fb7bb133a15a67f277a3f6ccb7e9384d6526f0d348cb8`.

## Focused qualification

The official writer fixture passed 1/1 on the release runner, one worker, 60-second limit, inventory `149af17fe86a90c4`. Results SHA-256 `16ffd13dce957fdb0fcea3aaaa89d84b1f73bbd5cace9108a511013b3eb281a9`; checkpoint `28d54207bf943c29cad433c265638334f86db2e4e2548f2883affc321e8dfcea`; log `b51bbbb785b581ad4b76fa4338a27095a76a37091e23740148d32e9e931f4520`.

The full official `test-fs*` cohort passed 350 / 385, with 11 TAP-verified skips, 21 fails and 3 timeouts. Its inventory is `cc7939ad66a2ed2e`; results SHA-256 `d975a0e3da5da8b1fcca601a9587d16fa025245fa63803a8737abc9518343083`; checkpoint `19ba966343ceb31efa4713cfbc914b4e7b90118a6610ce72cbef581a1263df23`; log `62447ef9706b56be62c84ddf496b52f1190fd1552f4b1d28677ef3d92c29a87c`. The cohort matches the same 385 fixtures with the cluster experiment enabled; all 385 statuses are unchanged.

## Full census in progress

The frozen full run is `work/full-node26-v374-fs-writer-{results.json,checkpoint.jsonl,run.log}`. Command parameters are the unchanged official 5,023-file inventory (`47b65dd4945a6e57`), Node v26.12.1, five workers, 60-second per-fixture deadline, PATH and `QUENCH_NODE_TEST_EXECUTABLE` pinned to the official Node executable. The `PKG_CONFIG_PATH` is pinned to the ICU/zlib/Brotli locations above. At start the runner SHA was verified as `b3da9cb8aca2f15745bcf120a855b3bb3524c2c0e69e6f63d52d4594f9b4528c`.

The latest accepted complete census before v374 is v362: 3,564 pass / 650 skip / 638 fail / 157 timeout / 10 crash / 4 unclassified. Focused totals do not alter that result. v374 terminal classifications and hashes will be appended after the runner completes and are reconciled against checkpoint transitions; exit-only execve results remain unclassified absent worker payloads.
