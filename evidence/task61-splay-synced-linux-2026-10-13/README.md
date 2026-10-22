# Splay stock diagnostic on synced `v2-cloud` (Linux)

## Result

The source-clean `v2-cloud` head `6a275ed5d` completed an 11-round Splay stock
run with valid, output-equal results in all rounds. This is a single-fixture
diagnostic (`qualification_ready=false`), not a Linux qualification session;
the Linux gate still needs two agreeing sessions at least 30 minutes apart.

| Engine | Median Score | Median max RSS |
| --- | ---: | ---: |
| Quench | 4,103 | 95,920,128 B |
| QuickJS 2026-06-04 | 3,574 | 149,524,480 B |
| Bun 1.4.2, JIT disabled | 5,444 | 60,858,368 B |
| Node 26.10.0 `--jitless` | 7,701 | 406,405,120 B |

Quench beats QuickJS on Score and beats all three runtimes on max RSS except
Bun. The active bars remain Bun/Node: Quench is 1.327x behind Bun on Score,
1.877x behind Node on Score, and 1.576x over Bun on max RSS. Splay remains
unwon; no later fixture was started.

The harness reported a maximum one-minute load of 1.92 and five-minute load of
1.32 on a four-CPU quota. Its raw report marks all 11 samples valid and output
equal. It records Bun's `BUN_JSC_useJIT=0` option dump as `useJIT=false` and
Node's `--jitless` proof as true.

## Provenance

- Source: `6a275ed5de0c2f0ebbe960eb3eada5f3b8376820`, clean tree.
- Host: Linux x86_64, kernel `6.18.44`, four logical CPUs, cgroup CPU quota 4,
  16 GiB memory limit; metrics collected with Linux `wait4` rusage.
- Quench binary SHA-256:
  `1e74268b4507b987485167b43d0bfaa21ec7bb47daac6e3afbdbbde568a8e33f`.
- Runner SHA-256:
  `5dd4281520a75224a73e1b79d936dd7b0353b541aa7138c09697a32de0631e97`.
- QuickJS SHA-256:
  `d918d3a101bc5f390b675b07a458bfbec8f15ba72453aefeb5b67b3439bae12a`.
- Bun SHA-256:
  `a83d263767d839e4d2649ca8e35d07159c7afc99afdc96d731ced29e056dda0c`.
- Node SHA-256:
  `ab9c8eecf9f82d6693cdc3accced17034065c8d96213b0aa76a7e803d20ae1da`.
- Raw report SHA-256:
  `43e647ee247b18dea419b3ff408622584a38dc284fe4de982ab6a69e64b2c3af`.

## Reproduction

```sh
PATH="/workspace/quench/target/production:/workspace/quench-v2-cloud/target/stageb-tools/bun/bun-linux-x64-baseline:/workspace/quench-v2-cloud/target/stageb-tools/node/node-v26.10.0-linux-x64/bin:$PATH" \
  /workspace/quench/target/production/quench-bench \
  quench-bench/js-engine-benchmark/v8-v7/splay.js \
  --quench /workspace/quench/target/production/quench-node \
  --qjs /workspace/.local/usr/bin/qjs \
  --bun /workspace/quench-v2-cloud/target/stageb-tools/bun/bun-linux-x64-baseline/bun \
  --node /workspace/quench-v2-cloud/target/stageb-tools/node/node-v26.10.0-linux-x64/bin/node \
  --runs 11 --timeout-ms 300000 \
  --out work/task61-splay-synced-linux-2026-10-13/splay-stock-11.json
```

The production binaries were built from this source with:

```sh
source /workspace/quench-build-env.sh
CARGO_INCREMENTAL=0 cargo build --locked --offline --profile production \
  -p quench-node --bin quench-node -p quench-bench --bin quench-bench
```
