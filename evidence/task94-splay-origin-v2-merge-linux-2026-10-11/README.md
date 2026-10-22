# Task 94: Splay collector after origin/v2 integration

Date: 2026-10-11. Branch: `v2-cloud`. The explicitly fetched `origin/v2`
head was `8c24b46b9`; the merge base was `883e5c00f`. The merge commit is
`8eeb9f4da`, pushed to `origin/v2-cloud`. A post-commit fetch and merge found
no newer `origin/v2` changes.

## Integration decisions

- Adopted trunk's no-generation weak handles. Heap weak fields are cleared
  before sweep makes dead slots reusable, and the VM function cache is pruned
  immediately after collection before any post-collection cleanup can allocate.
- Kept Task 94's sticky marks, remembered owners, young-cell tracking, and
  slab-local 16-bit free offsets (2,048 slots per slab). Removed the generation
  array, overflow map, and retired-slot accounting.
- Full weak pruning visits marked owners only. This prevents an unreachable
  `FinalizationRegistry` from scheduling callbacks. Node v26.10.0 with
  `--expose-gc` collected a registry held only by a `WeakRef` and produced
  `{"registryCollected":true,"callbacks":[]}`. The Rust heap test
  `unreachable_finalization_registry_does_not_enqueue_jobs` covers this case.

## Correctness

- `RUSTFLAGS='-D warnings' cargo +1.99.0 check -p quench-runtime --all-targets`:
  passed.
- `RUSTFLAGS='-D warnings' cargo +1.99.0 test -p quench-runtime --lib`:
  547/548 passed. The one failure is the previously known unrelated German
  `Intl` date-format expectation (`2.1.1970` vs `1.1.1970`). All heap, weak
  reference, finalization, and VM tests passed.
- `RUSTFLAGS='-D warnings' cargo +1.99.0 test -p quench-runtime --lib
  unreachable_finalization_registry_does_not_enqueue_jobs`: passed.
- Test262 stages 47, 79, 80, and 81: 302/302 passed using the iteration
  profile, a 900,000 ms per-test timeout, and four workers.
- Full Wasm directive suite: 67,124/67,124 passed with a 60,000 ms file timeout.

## Splay stock diagnostic

The four-engine stock Splay runner completed 11/11 valid rounds with equal
outputs. Medians on this Linux x86_64 container (4 CPU quota, 16 GiB memory
limit):

| Engine | Score | Maximum RSS |
| --- | ---: | ---: |
| Quench | 1,741 | 116,944,896 B |
| Node v26.10.0 `--jitless` | 2,795 | 389,369,856 B |
| Bun 1.4.2 no-JIT | 1,731 | 62,144,512 B |
| QuickJS 2026-06-04 | 2,263 | 149,565,440 B |

Quench is at 62.3% of Node's Score and uses 1.88x Bun's RSS. Both Splay
bars remain open. This run is diagnostic: the merge was uncommitted and the
report records `source_dirty=true`; it contains only Splay, so it is not a
qualification report. The reference Node RSS is not the memory bar; the bar is
Bun no-JIT.

Reproduction from the repository root:

```sh
source /workspace/quench-build-env.sh
RUSTFLAGS='-D warnings' cargo +1.99.0 build --locked --offline --profile production \
  -p quench-node --bin quench-node -p quench-bench --bin quench-bench
target/production/quench-bench \
  quench-bench/js-engine-benchmark/v8-v7/splay.js \
  --quench target/production/quench-node \
  --qjs target/stageb-tools/qjs/quickjs-2026-06-04/qjs \
  --bun target/stageb-tools/bun/bun-linux-x64/bun \
  --node target/stageb-tools/node/node-v26.10.0-linux-x64/bin/node \
  --runs 11 --timeout-ms 300000 \
  --out target/task94-merged-trunk-splay-stock-11.json
```

The raw runner JSON is compressed at `stock-splay-11.json.gz` (SHA-256
`dde239cf074665ec7a8f871bd7a639367034ca0b0e554a18147ff7a89e6cf5ad`). The
Quench production binary SHA-256 is
`c3c93e9c90e3981941dc987a238aa98d5c67c6d5579a7a269716895c09a82a7a`.

Next: continue Splay-only collector work, first measuring how many completely
free slabs the run creates, then test whether releasing those slabs lowers
RSS enough without losing Score. Keep both gates open until matched Splay
measurements clear them.
