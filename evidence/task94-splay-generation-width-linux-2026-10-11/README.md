# Splay heap-slot generation width screen

## Decision

Retained as an incremental Task 94 memory improvement; Splay remains active and
neither Task 61 bar is met. The heap stores ordinary slot generations in 16
bits and keeps a sparse 32-bit overflow table for slots reused more than
65,535 times. A slot still retires only when its 32-bit generation is exhausted,
so the optimization preserves the prior weak-handle reuse lifetime.

The paired fixed-work screen was output-equal in 11/11 rounds. It reduced work
RSS by 3,166,208 B and setup RSS by 1,712,128 B. Raw setup-adjusted wall time
was 21.85% slower at the median, with only 3/11 wins and a range of -24.94% to
+367.94%; Linux counters were unavailable (0/11 clean rounds). Do not claim a
speed result from this screen.

Separate stock sessions were valid and output-equal in 11/11 rounds. The
baseline scored 1,967 vs Node --jitless at 4,458 (44.1% of the reference);
the candidate scored 1,391 vs Node at 3,154 (44.1%). Node and Bun moved
substantially between sequential sessions, so the scores do not establish a
candidate speed change. Candidate max RSS was 121,032,704 B vs Bun no-JIT at
61,886,464 B; baseline was 124,284,928 B vs Bun at 62,144,512 B. The candidate
saved 3,252,224 B in the stock check but still used 1.96x the local Bun RSS.
This is not an M4 qualification.

## Correctness

- Heap unit tests: 28/28 passed, including weak-handle overflow, WeakRef,
  FinalizationRegistry and u32 exhaustion.
- Test262 stages 47 (FinalizationRegistry), 79 (WeakMap), 80 (WeakRef) and 81
  (WeakSet): 302/302 passed. Reports are included below.
- Full runtime library suite: 548/549 passed. The sole failure is the existing
  unrelated German date-format regression
  (2.1.1970 vs 1.1.1970), also seen before this change.
- The change does not alter write barriers or shared JS/Wasm store paths.

## Build and measurements

- Candidate source starts at v2-cloud commit 5865ea951.
- Rust: 1.99.0 (b940084d7 2026-09-28).
- Host: Linux x86_64, kernel 6.18.44, 4 logical CPUs, 4 CPU quota, 16 GiB
  memory limit. Process measurements use Linux wait4 rusage.
- Splay fixture SHA-256:
  1f875cec579d03af40d256ceaeb6823f3ef16c883652650ca51d183472a8185d
- Runner SHA-256:
  924e62eee00ea5964d140f1cb04a6d1e125a2dfec08271cb0f28d4b933f79deb
- Baseline Quench binary SHA-256:
  1faabc11019a79dfe595440859ea6d059e1e9b18949a0ccc3e3b9ed8d851cc57
- Candidate Quench binary SHA-256:
  0e48bf7c26545e16100853840df0f1989882b02ca077d3491fb19eac02a54ba0
- Node 26.10.0 SHA-256:
  ab9c8eecf9f82d6693cdc3accced17034065c8d96213b0aa76a7e803d20ae1da
- Bun 1.4.2 SHA-256:
  a83d263767d839e4d2649ca8e35d07159c7afc99afdc96d731ced29e056dda0c
- QuickJS 2026-06-04 SHA-256:
  c798e32aac14495856a4709be67a11b0562a6e2d07fa89ad4f28efe46c7c7955
- Bun JIT was disabled and probed; Node used --jitless. Quench used its shared
  VM interpreter.

Task 61's Splay M4 bars remain Node --jitless Score leadership and maximum
RSS below 57,950,208 B (Bun no-JIT). The Linux candidate remains far above the
RSS bar and below the Score bar.

## Reproduction

Build the baseline at commit 5865ea951 and the candidate at this evidence
commit with Rust 1.99:

    source /workspace/quench-build-env.sh
    CARGO_TARGET_DIR=/workspace/quench/target cargo build --locked --offline --profile production -p quench-node --bin quench-node

Paired fixed-work screen:

    target/production/quench-bench quench-bench/js-engine-benchmark/v8-v7/splay.js --fixed-work --quench work/task94-v2-cloud-slotgen-baseline --quench-peer work/task94-v2-cloud-slotgen-candidate --runs 11 --timeout-ms 300000 --out work/task94-v2-cloud-slotgen-fixed-11.json

Candidate stock check (use the baseline path for the baseline report):

    target/production/quench-bench quench-bench/js-engine-benchmark/v8-v7/splay.js --quench work/task94-v2-cloud-slotgen-candidate --qjs target/stageb-tools/qjs/quickjs-2026-06-04/qjs --bun target/stageb-tools/bun/bun-linux-x64/bun --node target/stageb-tools/node/node-v26.10.0-linux-x64/bin/node --runs 11 --timeout-ms 300000 --out work/task94-v2-cloud-slotgen-stock-11.json

Focused correctness commands:

    CARGO_TARGET_DIR=target/task94-current CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --locked --offline -p quench-runtime --lib heap::tests::
    TEST262_TEST_TIMEOUT_MS=30000 TEST262_JOBS=4 target/iteration/run-stages 47 47
    TEST262_TEST_TIMEOUT_MS=30000 TEST262_JOBS=4 target/iteration/run-stages 79 81

Compressed reports:

- fixed-work-11.json.gz
- stock-baseline-11.json.gz
- stock-candidate-11.json.gz
- test262-stage-47.json
- test262-stages-79-81.json
