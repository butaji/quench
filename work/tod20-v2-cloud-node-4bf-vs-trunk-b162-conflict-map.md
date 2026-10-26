# TOD-20 conflict map against current merged trunk (2026-10-26)

Exact task source: `4bf61501a191e01f39810a8fd2d886ee0bf60ab5` (`origin/tod20/v2-cloud-node-v316`). Exact live trunk: `b1622474e5fc6c305a2d45299329e1a2f453e446` (`origin/v2-merged`, fetched to `refs/tod20/latest-v2-merged-20261026b162`). Merge base: `2a8b2a22e5c11f0897eb6aebd377e97c61710ea9`. The read-only merge-tree probe produced 499 conflict paths: 496 add/add and 3 content. Merge-tree result tree: `137736941e9586e8cb5f5551bfcf56a0e346a563` (not a published merge). The conflict path set is identical to the prior 499-path map against trunk `0b903cbb`, though the trunk now contains substantial additional conflict-free changes.

Raw merge-tree output is `work/tod20-v2-cloud-node-4bf-vs-trunk-b162-merge-tree.txt`, SHA-256 `81465f71c170e0edf161c88c983219a2cf1475fedf4400e799270ec2489b6469`. Sorted path list is `work/tod20-v2-cloud-node-4bf-vs-trunk-b162-conflict-paths.txt`, SHA-256 `49bbb81b115e554b988b7fb3a8cb592b834af968ae05fb8a778bacb6c8628fa6`; it contains 499 unique sorted paths. The merge-tree object database was isolated in `/tmp/tod20-b162-objects`; the primary repository status was not mutated by the merge probe.

## Owner routing

Routing follows current Linear ownership. TOD-20 owns Node host APIs and Node-owned modules including process/cluster, net/TLS/HTTP, FS, events/abort, timers, CLI and the fixture harness. TOD-127 owns loader/N-API, Buffer/clone, workers/zlib/DNS and related module-owned areas. TOD-136 owns shared runtime and conformance-core behavior. Host registration or shared state needs joint Node API/runtime review. Preserve unique behavior and validate at the exact merged source rather than choosing a side wholesale.

| Subsystem | Conflict paths | Review route |
|---|---:|---|
| `quench-runtime/src/**` | 197 | TOD-136 core review, split by VM, compile, heap, bytecode, wasm and profile |
| `quench-node/src/modules/**` | 176 | TOD-20; TOD-127 for loader/N-API/Buffer/clone/workers/zlib/DNS ownership |
| `quench-node/src/host/**` | 7 | Joint TOD-20 API and TOD-136 runtime review |
| `quench-node-test/**` | 12 | TOD-20 harness and fixture-owner review |
| Other crate, vendor, tool, doc and root-build paths | 107 | Owning crate; retain original build/test evidence |

The 499 conflict paths total 441 under `crates/` and 58 elsewhere. The three content conflicts are `.gitignore`, `Cargo.lock`, and `Cargo.toml`; all other 496 are add/add. Largest prior reviewed module clusters are `tls_shared_vm` (11), `fs_shared_vm` (9), and `http_shared_vm` (6); largest runtime clusters are `vm` (115), `compile` (27), `heap` (14), `bytecode` (7), `wasm` (7), and `profile` (5). The identical path-set hash confirms these counts remain current.

## Integration sequence

1. Keep the 499-path inventory frozen while lane fixes proceed. Do not merge the rejected cluster handoff candidate or make direct trunk pushes.
2. After the lane reaches 4,000 passes, re-fetch live `v2-merged` and refresh this map. Begin small owner-reviewed batches with shared runtime prerequisites and Node-owned HTTP/TLS/FS clusters, then review host/harness seams jointly. Route TOD-127-owned paths through their module owner.
3. For each batch, compare both implementations and behavior-specific Node v26.12.1 fixtures, preserve unique lane behavior, run focused tests and required crate checks, then recalculate the conflict inventory. Resolve modify/delete and add/add paths explicitly; never choose ours/theirs wholesale.
4. Once the exact integrated head is ready, rebuild with pinned native zlib/Brotli/ICU inputs and run the full unchanged official 5,021-fixture gate. Acceptance remains at least 4,000 passes on that exact merged SHA.
