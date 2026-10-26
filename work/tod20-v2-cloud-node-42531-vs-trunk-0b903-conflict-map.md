# TOD-20 current conflict map against live merged trunk (2026-10-26)

Exact task-lane source: `42531a0f8a5576a69b47e661b7dcf19b631898ac` (`origin/tod20/v2-cloud-node-v316`). Exact live trunk: `0b903cbb9101803f4dde9564a19215c8e0c4a2c4` (`origin/v2-merged`, fetched to task-local `refs/tod20/latest-v2-merged-20261026`). Merge base: `2a8b2a22e5c11f0897eb6aebd377e97c61710ea9`. The read-only `git merge-tree --write-tree --name-only` probe produced 499 conflict paths: 496 add/add, 3 content. Merge-tree result tree: `401cf5ecb278a1ef904b6cfdc716b2f1db26653b` (not a published merge). The current conflict path set and conflict kinds exactly match the earlier 499-path map, despite the updated task head and live trunk tip.

Raw merge-tree output: `work/tod20-v2-cloud-node-42531-vs-trunk-0b903-merge-tree.txt`, SHA-256 `de9366e1e2f2e9a5eb548c50b1a1fee2ba64ed5e918a94fe5ecf4dd29f52ea24`. Normalized sorted TSV: `work/tod20-v2-cloud-node-42531-vs-trunk-0b903-conflict-paths.tsv`, SHA-256 `a1c3f5b4e7bc2868af22e4a1ee918b053a92d5334eb33b4facd2de09b117b8f4`. Path-set SHA-256 (sorted UTF-8 path names plus trailing newlines): `49bbb81b115e554b988b7fb3a8cb592b834af968ae05fb8a778bacb6c8628fa6`. These are tracked-source conflicts only; the uncommitted cluster round-robin candidate and user dirty `tests/node` are excluded.

## Owner routing

Routing follows current Linear ownership. TOD-20 owns Node host APIs and Node-owned modules including process/cluster, net/TLS/HTTP, FS, events/abort, timers, CLI and the fixture harness. TOD-127 owns loader/N-API, Buffer/clone, workers/zlib/DNS and related module-owned areas. TOD-136 owns shared runtime and conformance-core behavior. Host registration or shared state needs joint Node API/runtime review. Preserve unique behavior and validate at the exact merged source rather than choosing a side wholesale.

| Subsystem | Conflict paths | Review route |
|---|---:|---|
| `quench-runtime/src/**` | 197 | TOD-136 core review, split by VM, compile, heap, bytecode, wasm and profile |
| `quench-node/src/modules/**` | 176 | TOD-20; TOD-127 for loader/N-API/Buffer/clone/workers/zlib/DNS ownership |
| `quench-node/src/host/**` | 7 | Joint TOD-20 API and TOD-136 runtime review |
| `quench-node-test/**` | 12 | TOD-20 harness and fixture-owner review |
| Remaining crates, docs, tools, vendor and root build files | 107 | Owning crate; retain original build/test evidence |

Largest module path clusters: `tls_shared_vm` (11), `fs_shared_vm` (9), `http_shared_vm` (6), `napi` (4). Largest runtime clusters: `vm` (115), `compile` (27), `heap` (14), `bytecode` (7), `wasm` (7), `profile` (5).

## Integration sequence

1. Keep the 499-path inventory frozen while the complete official-source Node census and lane fixes proceed. Do not merge the candidate cluster patch or make direct trunk pushes.
2. After the lane reaches 4,000 passes, re-fetch the live `v2-merged` tip and refresh this map. Begin small owner-reviewed batches with the shared runtime prerequisites and Node-owned HTTP/TLS/FS clusters, then review host/harness seams jointly. Route TOD-127-owned paths through their module owner.
3. For each batch, compare both implementations and behavior-specific Node v26.12.1 fixtures, preserve unique lane behavior, run focused tests and required crate checks, then recalculate the conflict inventory. Resolve modify/delete and add/add paths explicitly; never choose ours/theirs wholesale.
4. Once the exact integrated head is ready, rebuild with the pinned native zlib/Brotli/ICU inputs and run the full unchanged official 5,021-fixture gate. Acceptance remains at least 4,000 passes on that exact merged SHA.
