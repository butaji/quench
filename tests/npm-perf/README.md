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
These Linux RSS measurements are diagnostic, not qualification.

Recent no-JIT probes used profile-guided candidates for the remaining React
dispatch work. Frame-capture cache priming, direct `hasOwnProperty`, a projected
global-capture shortcut, fused local-branch dispatch, and a single-probe string
concat path all regressed against the 312.9 renders/s / 41.67 MB production
baseline and were reverted. The best memory-first policy screen remains
217.0 renders/s / 23.76 MB; it clears the RSS margin, but is far below Node and
Bun throughput.

A lane-exit profile showed 6,600 of 6,742 `SetFieldStrict` exits over 500
renders came from direct-array receivers in React's `flushCompletedQueues`
function; the hot sites assign `length = 0`. A guarded fast path for this dense
array case preserved output but measured 288.4 renders/s / 41.39 MB. A version
that gated the path on the `length` atom before trying the ordinary field cache
measured 279.6 renders/s / 41.43 MB. Both were reverted after 11-round
production comparisons. The profile attribution is useful for future work, but
this array-length shortcut is not a performance win. The reports and profile
logs from these probes remain under the local `work/react-perf` scratch folder;
Linux RSS sampling is diagnostic, not qualification.

The full-size no-JIT baseline uses 5,000 renders and 250 warmups per sample.
Its 11-round Linux report is
[`reports/react-no-jit-full-linux-11.json`](reports/react-no-jit-full-linux-11.json):
Quench measured 283.8 renders/s / 39.06 MB, Node `--jitless` 1,483.2 / 36.20
MB, and Bun no-JIT 1,297.4 / 37.38 MB. Output matched at 24,980,000 bytes
with checksum 414318. Quench misses both the throughput and RSS targets.

The full-size JIT pilot is
[`reports/react-jit-full-linux-1.json`](reports/react-jit-full-linux-1.json).
Quench measured 229.9 renders/s / 36.85 MB, Node 10,352.0 / 48.92 MB, and Bun
13,888.9 / 81.52 MB, with matching output. This one-round pilot is not a
qualification; it shows Quench's sampled RSS below both engines while speed
remains much slower. JS-tier diagnostics show the hot object-heavy React
functions are unsupported by the current narrow compiler tier and have zero
compile attempts. Linux RSS sampling remains diagnostic, not qualification.

The full-size no-JIT memory-first policy report is
[`reports/react-no-jit-memory-first-full-linux-11.json`](reports/react-no-jit-memory-first-full-linux-11.json).
It measured 228.1 renders/s / 29.30 MB, versus Node `--jitless` at 1,580.3 /
36.20 MB and Bun no-JIT at 1,375.5 / 37.37 MB, with matching output. It clears
the 20% RSS margin against Bun and narrowly misses Node, while throughput is
still far behind both. This policy does not meet the combined performance
target.

A guarded fast path for non-global/non-sticky RegExp `.test` misses was screened
on the 500-render React workload. It preserved output (2,498,000 bytes,
checksum 41,826) but measured 179.2 renders/s and 35.76 MB RSS in the three
round iteration screen. Because it did not show a throughput case worth a
production comparison, the change was reverted. The local screen is
`work/react-perf/react-regex-test-nomatch-iteration-3.json`; Linux RSS sampling
is diagnostic only.

The aggregate React profile recorded 3.34 million `Move → Call` pairs over
500 renders. A lane handler that executed both records together preserved
output but its three-round iteration screen measured 181.6 renders/s no-JIT
and 204.5 renders/s JIT. Removing the forced inline hint reduced JIT further
to 165.4 renders/s. This fusion was reverted without a production comparison:
the small no-JIT signal did not compensate for the JIT regression. Screens
remain at `work/react-perf/react-move-call-fusion-iteration-3.json`,
`work/react-perf/react-move-call-fusion-jit-iteration-3.json`, and
`work/react-perf/react-move-call-fusion-noinline-jit-iteration-3.json`.

A lane-only `LoadConst → Return` fusion also preserved output. Its full 11-round
no-JIT memory-first run measured 210.5 renders/s / 29.39 MB, below the existing
228.1 / 29.30 MB baseline. The full 11-round JIT run measured 286.8 / 40.91
MB, but its comparator baseline is only a one-round full-size pilot and the
candidate increased Quench RSS. The candidate was reverted because it did not
improve both modes and memory together. Reports are in
`work/react-perf/react-const-return-full-nojit-memory-first-linux-11.json` and
`work/react-perf/react-const-return-full-jit-linux-11.json`; Linux RSS is
diagnostic only.

A guarded miss path for `RegExp.prototype.exec` on non-global/non-sticky
builtin regexps preserved the React SSR output. Its three-round iteration
screens measured 185.4 renders/s / 36.97 MB no-JIT and 179.9 / 36.77 MB JIT.
They did not provide a compelling signal for a production comparison, so the
change was reverted. Reports remain at
`work/react-perf/react-regexp-exec-miss-iteration-nojit-3.json` and
`work/react-perf/react-regexp-exec-miss-iteration-jit-3.json`; Linux RSS is
diagnostic only.

A matched full-size JIT baseline is now saved at
[`reports/react-jit-full-linux-11.json`](reports/react-jit-full-linux-11.json).
With 5,000 renders and 250 warmups per sample, Quench measured 288.6
renders/s / 42.72 MB, Node 15,873.0 / 52.04 MB, and Bun 13,404.8 / 92.19
MB. Output matched at 24,980,000 bytes with checksum 414318. Quench's RSS is
17.9% lower than Node, short of the required 20%, and throughput trails Node
by 55x; it trails Bun throughput by 46x. This matched baseline confirms the
previous `LoadConst → Return` candidate's 286.8 renders/s did not improve
throughput, despite its lower 40.91 MB sampled RSS. The candidate remains
reverted. The next React JIT work needs to target the unsupported object-heavy
render functions with a materially broader compiled path; small dispatch fusions
have not closed the gap. Linux RSS sampling is diagnostic, not qualification.

A guarded JIT entry path for direct parameter-zero data-field getters was
screened against React SSR. The full 11-round JIT run matched output and measured
298.1 renders/s / 42.63 MB sampled RSS, compared with the clean baseline's
288.6 / 42.72 MB. A separate 500-render diagnostic found zero candidate and
zero fast entries, so the small full-run difference is run-to-run noise rather
than a path used by React. The code was reverted. The next JIT work must cover
the central render function's repeated field, mutation, and guest-call sequence
instead of leaf getters. Reports remain in `work/react-perf`; Linux RSS is
diagnostic only.
