# TOD-20 MessagePort worker transfer bridge

## Source and build identity

- Base commit: `c42764e12818fc461b100649d9744142213c717a`.
- Implementation files: `crates/quench-node/src/modules/worker_threads_shared_vm.rs` and `crates/quench-node/src/polyfills/shared_vm.rs`.
- File SHA-256 values: worker module `e8c20aecea21db539e914107f35107c1a034ba3c89290089e314c55c693629ae`; MessagePort polyfill `d5292fe489ddaa8f62b92204629630cea1c445af10b52d83fcf91b5fb8a6f9ec`.
- Host registration:
  `method "workerRemotePortPost" (0) => crate::modules::worker_threads_shared_vm::remote_port_post,`
  adjacent to `workerPost` in `crates/quench-node/src/host/shared_vm.rs`.
- Runner was built from a combined candidate tree based on the same commit; full binary diff SHA-256 `b13afd5d54d3b55c28da98be27eda34845f7b8470f1414ee011d82cdf1b80c82`. It also contained independent cluster, FS and HTTP experiments; this record claims only the two worker fixture transitions below.
- Command: `cargo build -p quench-node-test --bin run-parallel --release`, Rust 1.99, one build job, incremental disabled, target `/tmp/tod20-v164-session-target`.
- Build log SHA-256: `cbc1cfc9c99db1e361d2eb3c400ccdaefcae9427fe82fff05528eeef8f333048`.
- Runner SHA-256: `1ab10486a43b0b658371e132e0a6e74c71db9f882b3dcaf78bae175c765cbf31`.

## Pinned focused result

The run used official Node `v26.12.1` (executable SHA-256 `08f12d42fb304468e8ee86247fe686aae5b225836ea65e9d6566c11ae6ba2e8e`), official Node fixtures, five workers and the unchanged 60-second per-fixture limit. The selected 38-fixture inventory hash was `fa8095fab00f4534`.

Both transfer fixtures moved fail→pass: `test-worker-message-channel.js` verifies the received endpoint is an instance of `MessagePort`; `test-worker-workerdata-messageport.js` verifies a workerData port can post from its receiving VM context.

- Results SHA-256: `9506b0bc6cd97d2f30f4c6ae2ff1856b576208dbcd75e92aff91fc16ef50ba9e`.
- Checkpoint SHA-256: `d8897b83c0275933742cec7cfada19b04ec08f2b66a10bae07ad930f1157c9fe`.
- Run log SHA-256: `853b4f6f53f6de10da17a9a99820afc118a241b46b1208031c491e5da4e24e70`.

The whole selected run finished 26 pass / 7 fail / 5 timeout / 0 crash / 0 skip / 0 unclassified, improving from the matched v362 selected baseline of 6 pass / 25 fail / 6 timeout / 1 crash. This focused result does not alter the frozen full v362 census (3,564 pass / 650 skip / 638 fail / 157 timeout / 10 crash / 4 unclassified across 5,023 fixtures).
