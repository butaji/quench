# TOD-20 v383 cluster listener lifecycle candidate

The v382 full census is still the authoritative acceptance result until v383's full 5,023-fixture census finishes. v382 remains exactly 3,410 pass / 649 TAP skip / 764 fail / 190 timeout / 9 crash / 1 unclassified.

## Reproduced cause and change

On v382, a direct cluster probe showed `cluster.Worker` receiving `online` but never `listening` after a worker's shared `http.Server.listen()` succeeded. Source inspection isolated the split: the `net.Server` facade installs a `NODE_CLUSTER/listening` notification and disconnect cleanup in `net_shared_vm.rs`, while Quench's separate shared HTTP server in `http_shared_vm/operations.rs` did not. The HTTP server's own `listening` event fired; its worker-to-primary cluster notification was missing. This caused `test-cluster-basic.js`, `test-cluster-rr-domain-listen.js`, and cluster worker kill/disconnect fixtures to stall.

`crates/quench-node/src/modules/http_shared_vm/operations.rs` now wraps the shared HTTP `listen` operation. For a cluster worker and a non-exclusive listener, it reports the bound address as the internal `NODE_CLUSTER/listening` message, updates worker state, and closes the listener across IPC disconnect, including the race where disconnect arrives before listening. An exclusive listener does not publish the cluster event.

## Focused verification

The complete official Node v26.12.1 `test-cluster-*` filename cohort contains 83 fixtures, inventory hash `ad7b2f56eeac1092`. At the candidate binary it finished **72 pass / 2 TAP skip / 5 fail / 4 timeout / 0 crash / 0 unclassified**. Compared fixture-by-fixture with v382, transitions were fail→fail 5, pass→pass 67, skip→skip 2, timeout→pass 5, timeout→timeout 4. There were **no prior-pass regressions**. Both skips (`test-cluster-bind-privileged-port.js` and `test-cluster-shared-handle-bind-privileged-port.js`) have explicit TAP skip markers. The five gains include `test-cluster-basic.js`, `test-cluster-rr-domain-listen.js`, `test-cluster-worker-disconnect.js`, `test-cluster-worker-kill.js`, and `test-cluster-worker-kill-signal.js`.

`test-cluster-disconnect.js` remains a fail with `ECONNREFUSED`; its worker creates `net.Server` listeners and the failure is the separate cluster TCP connection distribution path. The HTTP bridge does not resolve that. The unchanged timeouts include cluster IPC throw, net server drop-connection, primary-error and primary-kill. Preserve these outcome classes as observed.

Artifacts:

- Results `work/full-node26-v383-cluster-family-results.json`, SHA-256 `4e745d85a1bafb315b85091167acb0053103e5028027b6237ec990c5fa8e61f3`.
- Per-fixture checkpoint `work/full-node26-v383-cluster-family-checkpoint.jsonl`, SHA-256 `b759027586b847c6b352fcd76154d6d74bfd4d0bc9dc3ee680154d8150f10d85`.
- Terminal log `work/full-node26-v383-cluster-family-run.log`, SHA-256 `44bf332e3e667b4c0098f569e59cc7a9dd3ec2fe95cfb3299b3a2ce2e74b57bb`.
- Release build log `work/tod20-v383-cluster-build.log`, SHA-256 `6abfb1869687ae0dd2fd9f1304908d8082cde1792542dc321b303f3bb4601361`.

## Pinned run inputs

- Official oracle: Node v26.12.1 at `/workspace/work/node-v26.12.1-linux-x64/bin/node`, SHA-256 `08f12d42fb304468e8ee86247fe686aae5b225836ea65e9d6566c11ae6ba2e8e`.
- Candidate Quench CLI: `/tmp/tod20-v164-session-target/release/quench-node`, SHA-256 `71b058699fda9146c61de93db81e668f281eb10288eb7eedb57fedf8f32ba4ad`.
- Runner: `/tmp/tod20-v164-session-target/release/run-parallel`, SHA-256 `b0e8e9db988ab934b3a2a491129bc4b35410a78864ba100af73724a9a2853b64`.
- Candidate source file SHA-256 `17e9778b24c6a2af099a6c25e62525c25f2a0b04e087e6cf275b9ebc483a5cc9`.
- Fixture root: `/workspace/work/node-stable-release/node-v26.12.1/test/parallel`. The checkout's git revision reports `unknown`; do not claim a pinned submodule commit.
- The focused family used five? No: this family run used **one job** and 60 seconds per fixture. The acceptance run remains five jobs and 60 seconds per fixture.

Next: commit this isolated source/evidence batch, then rebuild/pin candidate v383 and rerun the complete 5,023-fixture census with the existing five-job, 60-second policy. Only the complete census can update the gate count.
