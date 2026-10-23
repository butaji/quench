# TOD-19 tail-call microbenchmark

This fixture isolates the shared Wasm dispatch cost for a million Wasm tail
calls and compares it with a one-million-iteration loop. It is a diagnostic
microbenchmark, not a substitute for the upstream `return_call` tests or a
matched runtime score.

## Workload and command

The inputs are [tail-return.wast](../../quench-bench/tail-return.wast) and
[tail-loop.wast](../../quench-bench/tail-loop.wast). Both invoke one exported
function with `1_000_000` steps and assert the same result (`42`). Each input is
run in its own worker process by `quench-wasm-test`.

Build the runner in the iteration profile, then measure each file separately:

```sh
CARGO_TARGET_DIR=/workspace/quench/target \
CARGO_INCREMENTAL=0 \
CARGO_PROFILE_ITERATION_OPT_LEVEL=0 \
CARGO_PROFILE_ITERATION_CODEGEN_UNITS=16 \
CARGO_PROFILE_ITERATION_DEBUG=0 \
CARGO_BUILD_JOBS=2 \
cargo build --locked --profile iteration -p quench-wasm-test --bin run

export WASM_FILE_TIMEOUT_MS=120000
time -p /workspace/quench/target/iteration/run --report /tmp/tail-nojit.json \
  quench-bench/tail-return.wast -no-jit
time -p /workspace/quench/target/iteration/run --report /tmp/loop-nojit.json \
  quench-bench/tail-loop.wast -no-jit
time -p /workspace/quench/target/iteration/run --report /tmp/tail-jit.json \
  quench-bench/tail-return.wast -jit
time -p /workspace/quench/target/iteration/run --report /tmp/loop-jit.json \
  quench-bench/tail-loop.wast -jit
```

The build command is unoptimized (`iteration` profile with opt-level 0). The
runner was built once and reused for the timings below. These numbers include
runner startup, worker process startup, validation, translation, and execution.
Two sequential samples were taken, with this result:

| Mode | Tail call samples (s) | Loop samples (s) |
| --- | ---: | ---: |
| Interpreter (`-no-jit`) | 4.41, 4.17 | 2.61, 2.50 |
| Compiled (`-jit`) | 3.59, 3.91 | 2.39, 2.28 |

All four workload/mode combinations passed both WAST directives in both
samples. The repeated timing gap is material for this workload, but it must not
be projected to optimized builds or JetStream scores.

## Compiler coverage evidence

With `QUENCH_WASM_JIT_STATS=1`, the return-call module reports:

```text
QUENCH_WASM_JIT_REJECTION 0 unsupported-instruction:CallKnown@4
QUENCH_WASM_JIT_STATS {"program":1,"compile_attempts":1,"compile_failed":true,"total_functions":1,"compile_ns":179722}
```

The loop compiles as one function (178 code bytes, 4096 mapped bytes), with
`native_entry_requests:[[0,1]]`, no interpreter fallback entries, and no
compiled callouts. Thus `--jit` does not compile this tail-call case: its timed
path remains interpreted. This points first to missing terminal `CallKnown`
compiler coverage; it does not show that the per-edge `general_cursor` rebuild
or `maybe_collect` threshold check dominates interpreter time.

## Follow-up

The active tail-call arm rebuilds `GeneralCodeCursor` and checks the collection
threshold on every `StepResult::TailCall`. Keep collection checks in place
unless a safe safepoint proof supports changing them. A same-program,
same-function cursor reuse experiment can isolate the cursor setup cost; retain
it only if repeated matched timings show a gain and the runtime library tests
remain green. The higher-impact compiler work is terminal `CallKnown` support
with exact guard/deopt behavior, which needs a separate correctness design.
