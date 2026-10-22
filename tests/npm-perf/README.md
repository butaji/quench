# npm package performance workloads

Work one package at a time: finish its no-JIT comparison first, then continue
the no-JIT package sequence. Repeat the package sequence with JIT enabled
afterward. Quench defaults to the interpreter and enables its compiled mode
with `--jit`; its production executable is `target/production/quench-node`.

## React / react-dom

The first workload repeatedly server renders a fixed component tree with
`renderToString`. Its deterministic byte count and checksum must match in
every engine. Package versions and transitive dependencies are pinned in
`package-lock.json`.

Install the pinned packages and build Quench from the repository root:

```sh
cd tests/npm-perf
npm ci
cd ../..
cargo build --profile production -p quench-node --bin quench-node
```

Run the no-JIT diagnostic or qualification from `tests/npm-perf`:

```sh
node run-workload.mjs react-ssr no-jit 1 ../../target/iteration/npm-perf-pilots/react-no-jit.json
node run-workload.mjs react-ssr no-jit 11 ../../target/react-no-jit-linux.json
```

The rows run `quench-node`, Node with `--jitless`, and Bun with
`BUN_JSC_useJIT=0`. Bun's option dump must report `useJIT=false`; Node's
`process.execArgv` confirms `--jitless`. The runner compares deterministic
outputs and records guest throughput, elapsed time, maximum RSS, versions,
commands and input hashes. It sets `NODE_ENV=production` for every engine.
Linux runs sample the kernel-maintained `/proc/<pid>/status` `VmHWM` every 2 ms
and are diagnostic only;
macOS qualification reads process maximum RSS from `/usr/bin/time -l`. Set
`QUENCH_NODE_BIN`, `NODE_BIN` and `BUN_BIN` when the binaries are outside their
defaults.

After each package's no-JIT win is established, run the JIT-enabled phase:

```sh
node run-workload.mjs react-ssr jit 11 ../../target/react-jit-linux.json
```

JIT phase rows use `quench-node --jit` and the default JIT-enabled Node and Bun
engines. Linux runs are diagnostic; retain the host, version, output and RSS
details with each comparison.

Winning means at least 20% higher throughput and at least 20% lower maximum
RSS than both Node and Bun in the active phase. Compare the throughput and RSS
margins independently against each engine, and keep each package's evidence
separate.

The pre-merge runtime's 11-round Linux no-JIT pilot (50 timed renders and 50
warmups per sample) is saved at
[`reports/react-no-jit-linux-11.json`](reports/react-no-jit-linux-11.json).
It verifies matching output but shows Quench behind both comparators on speed
and maximum RSS; it is a diagnostic baseline, not a win or qualification.

After enabling program-scoped inline caches for dynamically compiled CommonJS
modules, a second 11-round Linux pilot is saved at
[`reports/react-no-jit-program-caches-linux-11.json`](reports/react-no-jit-program-caches-linux-11.json).
It also verifies matching output, but Quench remains behind both comparators
on throughput and maximum RSS. The earlier report remains as the pre-cache
baseline.

The compiler then fused `LoadLocal` followed by `GetField`, a frequent React
rendering pair. The resulting 11-round pilot is saved at
[`reports/react-no-jit-local-field-linux-11.json`](reports/react-no-jit-local-field-linux-11.json).
It verifies matching output and improves Quench throughput in this Linux
diagnostic, but the performance gap remains large and max RSS still loses.

The React SSR interpreter then increased its young-generation allocation budget
from 1/64 to 1/2 of the live heap, retaining the existing young-generation cap.
The 11-round, 500-render production diagnostic is saved at
[`reports/react-no-jit-gc-young-1-2-linux-11.json`](reports/react-no-jit-gc-young-1-2-linux-11.json).
It produced 217.0 renders/s and 27.59 MB sampled max RSS, with identical output
across engines. This is a partial no-JIT improvement: it remains about 7.5x
slower than Node `--jitless`, and its RSS is only about 13% below Node, so it
does not meet the 20% win threshold. Linux RSS sampling remains diagnostic,
not qualification.

For function-level React profiling, build the iteration runner with
`--features quench-runtime/profile-aggregate`, then run:

```sh
NODE_ENV=production QUENCH_PROFILE_ALL_PROGRAMS=1 \
QUENCH_PROFILE_RETAIN_BYTECODE=1 ../../target/iteration/quench-node \
  workloads/react-ssr.cjs
```

The retain flag is diagnostic only: it keeps replayable function records so the
report can include hot function names and instruction counts. Each
`quench-profile-program` record includes its source name for attribution.

The merged runtime has separate throughput and memory-first GC policies. Select
the latter for a screen with `QUENCH_MEMORY_POLICY=memory-first`; the default is
`throughput`. On the post-merge React SSR Linux pilot, raising memory-first's
young budget from 1/64 to 1/1 increased Quench from 114.7 to 208.3 renders/s,
with sampled max RSS moving from 30.37 to 31.20 MB in an initial five-round
screen. An 11-round follow-up at the 2^17 cap measured 194.9 renders/s and
30.74 MB. Reducing the memory-first cap to 2^16 measured 196.7 renders/s and
29.74 MB; Node `--jitless` measured 1432.7 renders/s / 31.94 MB and Bun no-JIT
1292.0 / 37.50 MB. Quench remains about 7.3x slower than Node and its RSS is
only 6.9% lower than Node, so this is a partial improvement, not a win. A 2^18
cap measured 191.7 renders/s / 30.66 MB versus 208.3 / 31.20 MB at 2^17 in a
separate five-round screen. The 2^16 cap was selected on the 11-round follow-up.
These Linux RSS samples are diagnostic, not qualification.

The optional `quench-alloc` feature raised the 11-round React screen to 224.3
renders/s but also raised sampled max RSS to 32.49 MB (from 196.7 renders/s /
29.74 MB with the system allocator). It remains disabled for this workload
because the speed gain comes with a larger memory footprint.

The React profile then identified compiler-private `for...in` helper loads as
the largest fast-lane exit. The lane now resolves the two internal names to the
same `ForInKeys` and `ForInKeyIsEnumerable` native functions used by the normal
name resolver. The 11-round Linux diagnostic is saved at
[`reports/react-no-jit-internal-name-fast-lane-linux-11.json`](reports/react-no-jit-internal-name-fast-lane-linux-11.json).
It produced 220.6 renders/s and 29.33 MB sampled max RSS, versus Node
`--jitless` at 1483.7 / 31.90 MB and Bun no-JIT at 1199.0 / 37.41 MB. Output
matched across engines. This improves on the prior 196.7 renders/s / 29.74 MB
Quench screen, but still misses the required 20% throughput advantage and 20%
RSS reduction against both engines. Linux RSS sampling remains diagnostic,
not qualification.

The next profile found frequent `$$typeof` reads on arrays whose absent-property
proof stopped at `Array.prototype`, because inherited field caches only walked
ordinary-object prototypes. The cache now carries the field atom while walking
shape-supported prototypes, including arrays, and admits absent non-index,
non-virtual names on direct arrays. Prototype links remain guarded by shape and
prototype-layout proofs. The 11-round Linux diagnostic is saved at
[`reports/react-no-jit-array-prototype-cache-linux-11.json`](reports/react-no-jit-array-prototype-cache-linux-11.json).
It produced 245.0 renders/s and 29.41 MB sampled max RSS, versus Node
`--jitless` at 1683.5 / 31.92 MB and Bun no-JIT at 1184.8 / 36.98 MB. Output
matched across engines. The lane-exit census dropped these `GetField` exits
from 5,390 over 55 renders to 2 over 55 renders. This improves on the prior
220.6 renders/s screen, but still misses the 20% throughput and RSS gates
against Node. Linux RSS sampling remains diagnostic, not qualification.

The next lane census found about 24 `JumpBinaryFalse` exits per render for
nullish/Number equality. The lane now resolves that pair in its shared cold
relation fallback: JavaScript loose and strict equality both make the values
unequal without coercion. The initial inline placement removed 6,600 exits over
275 renders and measured 290.0 renders/s, but sampled RSS rose to 41.75 MB; its
11-round report is
[`reports/react-no-jit-nullish-number-equality-linux-11.json`](reports/react-no-jit-nullish-number-equality-linux-11.json).
Moving the check into the cold fallback measured 312.9 renders/s / 41.67 MB
with matching output in
[`reports/react-no-jit-nullish-equality-cold-linux-11.json`](reports/react-no-jit-nullish-equality-cold-linux-11.json).
Under `QUENCH_MEMORY_POLICY=memory-first`, the same cold-fallback build measured
217.0 renders/s / 23.76 MB in
[`reports/react-no-jit-nullish-equality-memory-first-linux-11.json`](reports/react-no-jit-nullish-equality-memory-first-linux-11.json).
That screen clears the RSS margin against both runtimes but is still far behind
their throughput. Experimental GC tuning with a 2x young budget and a 1/2
budget both regressed; those iteration-profile diagnostics are recorded at
[`reports/react-no-jit-memory-first-young-2-linux-11.json`](reports/react-no-jit-memory-first-young-2-linux-11.json)
and
[`reports/react-no-jit-memory-first-young-half-linux-11.json`](reports/react-no-jit-memory-first-young-half-linux-11.json).
The benchmark runner records memory and GC policy settings in each report.
The lane census also attributes about 268,000 capture reads to the React server
root over 275 renders; the next optimization target is that repeated global
capture path. These Linux RSS measurements are diagnostic, not qualification.
