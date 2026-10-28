# Splay mark-on-enqueue screen — Linux, 2026-10-11

## Decision

Rejected and unretained. Splay remains the active fixture and neither Task 61
bar is met.

## Candidate and correctness

The experiment starts from the Task 61 40 B cell candidate and applies the
existing Splay screen policy (1/4 large-heap headroom and 1/4 full-collection
growth), direct cell-kind dispatch, shared-array backing deduplication, and
mark-on-enqueue. The candidate filters each worklist suffix against the mark
bitmap, marks new heap cells when enqueuing, and compacts the suffix in place.

- Direct-mark control binary SHA-256: `630483c3c02d0b168151d1eb65359a9dcb37dcc708a5757982f5b9312cd554b2`
- Mark-on-enqueue candidate binary SHA-256: `8e0107220f923eb9def14442d40208bd5bffdcff0b9baced9fb06481f5a72dae`
- Rust: `1.99.0 (b940084d7 2026-09-28)`
- Host: Linux x86_64, kernel `6.18.44`, 4 logical CPUs, 4 CPU quota, 16 GiB memory limit
- Fixture SHA-256: `1f875cec579d03af40d256ceaeb6823f3ef16c883652650ca51d183472a8185d`
- Runner SHA-256: `924e62eee00ea5964d140f1cb04a6d1e125a2dfec08271cb0f28d4b933f79deb`

The focused heap suite passed 28/28, including the weak-map ephemeron, weak-reference,
minor-collection, and shared-array backing tests. The full runtime suite ran 552 tests;
551 passed. The sole failure was `regression_intl_german_date_format_ignores_string_split_override`
(`2.1.1970` vs `1.1.1970`), outside the collector path. The stock and fixed-work
harnesses produced equal output in every round, including against Node 26.10.0 `--jitless`.

## Measurements

The 11-pair fixed-work comparison is valid and output-equal 11/11, but 0/11 rounds are
timing-qualified because Linux `wait4` does not provide the required IPC counters.
The raw setup-adjusted median wall delta was −24.2%, with pair deltas from −46.5% to
+53.3%; this is directional only. Median work-process RSS was 67,248,128 B for the
candidate and 67,526,656 B for the direct-mark control (−278,528 B). Setup-only RSS
was 54,018,048 B and 54,067,200 B, respectively.

The fresh 11-round stock candidate was valid and output-equal:

| Metric | Candidate | Reference bar | Gap |
| --- | ---: | ---: | ---: |
| Splay Score | 937 | Node `--jitless`: 2,559 | 36.6% of bar |
| Median maximum RSS | 67,338,240 B | Bun no-JIT: 61,886,464 B | +5,451,776 B |

A following 11-round direct-mark stock control scored 1,103 with 67,559,424 B maximum
RSS; its same-session Node and Bun scores were 2,460 and 1,913. These stock sessions are
not a paired timing estimate, and the reference scores drifted substantially between
them. The candidate stock report is not qualification-ready. The candidate does not meet
either bar.

Profile counters showed 13,484,763 marked cells. Mark-on-enqueue processed 13,484,763
work items versus 31,398,344 work items for the direct-mark profile (57.1% fewer popped
items). Two production-profile runs measured 694,472,556 ns and 1,220,297,850 ns across
33 full collections; the earlier direct-mark profile measured 579,437,926 ns. These
single-process counters are noisy and do not support a speed claim. The stock score and
unqualified fixed-work evidence do not justify keeping the algorithm.

## Reproduction

Pinned fixed-work screen:

```sh
target/production/quench-bench \
  quench-bench/js-engine-benchmark/v8-v7/splay.js \
  --fixed-work \
  --quench work/task94-40b-gc-full-025-fast-mark-quench-node \
  --quench-peer work/task94-40b-gc-full-025-mark-enqueue-quench-node \
  --runs 11 --timeout-ms 300000 \
  --out work/task94-gc-40b-mark-enqueue-vs-direct-fixed-11.json
```

Pinned candidate stock check:

```sh
target/production/quench-bench \
  quench-bench/js-engine-benchmark/v8-v7/splay.js \
  --quench work/task94-40b-gc-full-025-mark-enqueue-quench-node \
  --qjs target/stageb-tools/qjs/quickjs-2026-06-04/qjs \
  --bun target/stageb-tools/bun/bun-linux-x64/bun \
  --node target/stageb-tools/node/node-v26.10.0-linux-x64/bin/node \
  --runs 11 --timeout-ms 300000 \
  --out work/task94-gc-40b-mark-enqueue-stock-11.json
```

The compressed raw runner reports are `fixed-work-11.json.gz`,
`stock-candidate-11.json.gz`, and `stock-direct-control-11.json.gz`.
