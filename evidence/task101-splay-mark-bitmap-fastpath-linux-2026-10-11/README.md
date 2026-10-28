# Task 101 Splay mark bitmap fast path (Linux)

## Change

This collector step keeps the atomic side mark bitmap, lazy persistent worker
pool, parallel full-heap marker, and bounded parallel slab sweep. Marking now
checks the bit with a relaxed load before `fetch_or`, so an already-marked slot
does not force a write. On hosts exposing at most four CPUs the pool defaults
to one total marker; larger hosts default to four. `QUENCH_GC_MARK_THREADS`
still overrides that choice, clamped to one through four total markers.

The control and candidate use the same Splay allocation headroom. The headroom
policy remains open to the trunk. This is a Linux collector-lane result; M4
qualification has not been run.

## Host and provenance

- Host: Linux x86_64, kernel `6.18.44`, 4 logical CPUs, cgroup CPU quota
  `400000/100000`, memory limit 16 GiB.
- Rust: `rustc 1.99.0 (b940084d7 2026-09-28)`.
- Source base: `f8d5d79496c47e1fcfac04fe954e8406fa13eb84`; the measured candidate
  was built from a dirty tree before this evidence commit.
- Candidate production binary SHA-256:
  `6aaf73b79e0e71419a1ed9ebfe0e705e8c755c72e6ac0f1a4ea9b68e61e27bbd`.
- Fixed-work control binary SHA-256:
  `bdc3b1fc8b1a45c7bbbd42b3cb58589b9bdeee7a17abcc9fdd67be22a423be19`.
  This is the locally pinned pre-fast-path executable; the executable is not
  retained in git.
- Measurement runner SHA-256:
  `0c553c518dcdd9d57ed0900ad4eb494804eb844675ba0942907cca4cf15c7d20`.
- Stock comparator binaries: QuickJS
  `c798e32aac14495856a4709be67a11b0562a6e2d07fa89ad4f28efe46c7c7955`, Bun
  `a83d263767d839e4d2649ca8e35d07159c7afc99afdc96d731ced29e056dda0c`, and
  Node `ab9c8eecf9f82d6693cdc3accced17034065c8d96213b0aa76a7e803d20ae1da`.

The harness uses Linux `wait4` rusage and has no usable hardware-counter data.
All 11 paired timing rounds were valid and output-equal, but 0/11 were clean;
`qualification_ready=false`. Raw paired times below are screening data, not a
qualified speed claim. Regenerable raw JSON and direct-profile logs remain in
ignored `work/task101/`.

## Splay results

The fixed-work harness ran 300 iterations per benchmark and alternated the
two pinned binaries for 11 pairs. Marginal time is `(work wall - setup-only
wall) / 300`.

| Metric | Control | Candidate | Change |
| --- | ---: | ---: | ---: |
| Median marginal wall | 2,857,773 ns/run | 2,771,062 ns/run | paired median −109,933 ns (−3.85%) |
| Candidate wins | — | 7/11 pairs | bootstrap 95% CI for paired delta: −173,170 to +644,714 ns |
| Work max RSS | 117,239,808 B | 116,989,952 B | −249,856 B |
| Setup max RSS | 63,471,616 B | 63,250,432 B | −221,184 B |

The fresh 11-round stock Splay run was valid and output-equal (11/11 for all engines):

| Engine | Median Score | Median max RSS |
| --- | ---: | ---: |
| Quench candidate | 2,773 | 117,227,520 B |
| Node v26.10.0 `--jitless` | 5,142 | 406,147,072 B |
| Bun no-JIT | 3,704 | 62,410,752 B |

Quench reached 54.0% of Node's Score and used 1.88x Bun's RSS. The stock
screen is not qualification-ready. Both Splay bars remain open.

A direct instrumented K0/K300 profile of the load-before-`fetch_or` source
measured 0.389 ms/run marking and 0.251 ms/run sweeping (0.639 ms/run total,
one unpaired sample). It is about 10.7x above Task 101's 0.06 ms/run target;
the measurement is diagnostic and does not qualify.

## Correctness gates

- Rust 1.99 production build with `RUSTFLAGS='-D warnings'`: passed.
- Rust 1.99 `cargo check -p quench-runtime` with warnings denied: passed.
- Focused parallel mark/sweep tests: 2/2 passed.
- Full `quench-runtime` library suite: 551/552 passed. The sole failure is
  the previously observed unrelated
  `api::tests::regression_intl_german_date_format_ignores_string_split_override`
  (`2.1.1970` vs `1.1.1970`).
- Test262 FinalizationRegistry stage 47 and WeakMap/WeakRef/WeakSet stages
  79–81: 302/302 passed.
- Full Wasm directive suite: 67,124/67,124 passed.

## Reproduction

Build the production candidate from the repository root:

```sh
source /workspace/quench-build-env.sh
RUSTFLAGS='-D warnings' cargo +1.99.0 build --locked --offline \
  --profile production -p quench-node --bin quench-node
```

Run the stock Splay screen with the pinned comparator tools under
`target/stageb-tools/`:

```sh
target/production/quench-bench \
  quench-bench/js-engine-benchmark/v8-v7/splay.js \
  --quench target/production/quench-node \
  --qjs target/stageb-tools/qjs/quickjs-2026-06-04/qjs \
  --bun target/stageb-tools/bun/bun-linux-x64/bun \
  --node target/stageb-tools/node/node-v26.10.0-linux-x64/bin/node \
  --runs 11 --timeout-ms 300000 \
  --out work/task101/splay-stock-final-fast-mark-formatted-11.json
```

For the fixed-work pair, use `--fixed-work --runs 11`, the control executable
with SHA-256 `bdc3b1fc…`, the candidate executable above, and an output path
under `work/task101/`. The old control executable is intentionally not stored
in this report; its hash identifies the locally pinned measurement artifact.

Correctness commands, after sourcing the build environment:

```sh
RUSTFLAGS='-D warnings' cargo +1.99.0 check --locked --offline \
  -p quench-runtime
RUSTFLAGS='-D warnings' cargo +1.99.0 test --locked --offline \
  -p quench-runtime --lib
TEST262_TEST_TIMEOUT_MS=900000 TEST262_JOBS=4 RUSTFLAGS='-D warnings' \
  cargo +1.99.0 run --locked --offline --profile iteration \
  -p quench-test262 --bin run-stages -- 47 47
TEST262_TEST_TIMEOUT_MS=900000 TEST262_JOBS=4 RUSTFLAGS='-D warnings' \
  cargo +1.99.0 run --locked --offline --profile iteration \
  -p quench-test262 --bin run-stages -- 79 81
WASM_FILE_TIMEOUT_MS=60000 RUSTFLAGS='-D warnings' \
  cargo +1.99.0 run --locked --offline --profile iteration \
  -p quench-wasm-test --bin run -- \
  --report target/iteration/wasm-collector-final.json \
  crates/quench-wasm-test/testsuite
```
