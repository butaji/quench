# npm package performance workloads

Work one package at a time: finish its no-JIT comparison first, then continue
the no-JIT package sequence. Repeat the package sequence with JIT enabled
afterward. Quench defaults to the interpreter and enables its compiled mode
with `--jit`; its production executable is `target/production/quench-node`.

## Fastify

`fastify-json-http.cjs` measures a schema-validated JSON route over one real
HTTP/1.1 keep-alive connection. `fastify-json-inject.cjs` measures the same
route through Fastify's `app.inject()` path and checks status, body, content
headers, and repeated `Set-Cookie` values. Fastify 5.12.5 and its dependencies
are pinned in the lockfile.

```sh
cd tests/npm-perf
npm ci
node run-workload.mjs fastify-json-http no-jit 11 ../../target/fastify-no-jit.json
node run-workload.mjs fastify-json-http jit 11 ../../target/fastify-jit.json
node run-workload.mjs fastify-json-inject no-jit 11 ../../target/fastify-inject-no-jit.json
node run-workload.mjs fastify-json-inject jit 11 ../../target/fastify-inject-jit.json
```

The inject scenario exercises virtual response records because LightMyRequest
captures response bytes through its mock socket. The shared response header
record remains authoritative, and the response factory projects `getHeaders()`
from that record. Treat inject and real socket results as separate scenarios.

The runner reports end-to-end client request throughput, output parity, and
sampled peak RSS for Quench, Node, and Bun. This workload includes both the
client request machinery and Fastify's server path, so compare identical engine
and runtime versions on the same host.

Set `QUENCH_BENCH_CONCURRENCY` to compare bounded parallel HTTP load; the
default is one in-flight request. For example, a local 500-request, 7-round
screen can run as:

```sh
QUENCH_BENCH_CONCURRENCY=32 QUENCH_BENCH_ROUNDS=500 \
  QUENCH_BENCH_WARMUPS=25 \
  node run-workload.mjs fastify-json-http no-jit 7 ../../target/fastify-concurrency32.json
```

The no-JIT concurrency sweep peaked at 32: Quench measured 1,572 requests/s /
44.96 MB, Node `--jitless` 1,984 / 54.43 MB, and Bun no-JIT 1,866 / 49.39 MB.
At 64 concurrent requests Quench fell to 1,534 requests/s. In the 32-request
JIT screen, Quench measured 1,259 / 43.13 MB versus Node at 3,497 / 69.69 MB
and Bun at 4,630 / 78.86 MB. These 7-round, 500-request samples are
diagnostic only; Quench still loses on throughput in both phases.

An 11-round, 1,000-request Linux diagnostic measured no-JIT Quench at
304.5 requests/s / 45.56 MB sampled max RSS, Node `--jitless` at 1,658.4 /
52.94 MB, and Bun no-JIT at 1,416.4 / 49.29 MB. JIT measured Quench at
314.5 / 43.18 MB, Node at 3,891.1 / 71.93 MB, and Bun at 3,194.9 /
81.54 MB. Output parity matched. Quench's sampled RSS was lower, but throughput
was far behind both engines in both phases; Linux RSS is diagnostic only.

A candidate that removed repeated request and response header-boundary scans
was rejected after interleaved 11-round A/B screens at 1,000 requests. No-JIT
candidate/control measured 304.5 / 45.56 MB versus 327.5 / 45.11 MB; JIT
measured 314.5 / 43.18 MB versus 323.3 / 42.75 MB. Output parity matched, but
the candidate lost speed and added RSS in both phases, so the runtime change
was reverted. The paired reports are in `work/fastify-perf/`.

A response-composition candidate that wrote directly into the socket's pending
output vector also lost throughput. Paired 7-round, 500-request screens at
concurrency 32 measured no-JIT candidate/control at 1,316 / 44.28 MB versus
1,330 / 42.95 MB, and JIT at 1,121 / 42.17 MB versus 1,266 / 41.22 MB.
Output parity matched; the candidate was reverted.

A candidate that moved framed request bodies directly into byte-backed buffers
to avoid the Latin-1 string round trip was also rejected. In paired 7-round,
500-request screens at concurrency 32, no-JIT candidate/control measured
1,344 / 46.58 MB versus 1,355 / 42.54 MB; JIT measured 1,179 / 44.08 MB
versus 1,279 / 43.24 MB. Output parity matched, but this implementation was
slower and used more RSS. The body conversion change was reverted.

## React / react-dom

The first workload repeatedly server renders a fixed component tree with
`renderToString`. Its deterministic byte count and checksum must match in
every engine. Package versions and transitive dependencies are pinned in
`package-lock.json`.

Install the pinned packages and build Quench from the repository root:

The production build uses the repository's general PGO corpus; install the
toolchain's `llvm-tools` component if `tools/pgo-build.sh` cannot find
`llvm-profdata`.

```sh
cd tests/npm-perf
npm ci
cd ../..
tools/pgo-build.sh production
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
Linux runs sample the live process tree's summed `/proc/<pid>/status` `VmRSS`
every 50 ms and are diagnostic only. The sample can miss short-lived children
and counts shared pages once per process; use the platform's maximum-RSS tool
for qualification;
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

For a same-run Quench build comparison, set `QUENCH_CONTROL_BIN` to a second
`quench-node` binary. The runner interleaves both Quench binaries with Node
and Bun and records the control binary hash separately. This helps distinguish
small PGO or code-change effects from host load drift across separate reports.
For no-JIT GC pacing comparisons, set `QUENCH_CONTROL_GC_MAX_YOUNG` to override
the control row's young-generation cap while the candidate uses
`QUENCH_GC_MAX_YOUNG`.

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

Two follow-up string probes used the full 5,000-render workload, 250 warmups,
11 rounds, the memory-first policy, and a 16,384 young-generation cap. The
matched clean 2,048-unit rope-prefix build measured 243.7 renders/s / 29.91 MB;
the 512-unit candidate measured 276.2 / 30.34 MB. Output matched at
24,980,000 bytes with checksum 414318. The candidate raised throughput 13.3%
but also raised RSS 1.4% over its clean control, and its RSS was only 12.1%
below Node. The earlier three-round screen's larger RSS margin did not hold,
so the threshold change was rejected. Reports are in
`work/react-perf/react-rope-prefix-2048-memory-first-max-young-16k-nojit-11.json`
and `work/react-perf/react-rope-prefix-512-memory-first-max-young-16k-nojit-11.json`.

A second candidate read one UTF-16 code unit from a rope without flattening it,
on the hypothesis that React SSR's final `charCodeAt` sample flattened an
otherwise transient HTML string. It measured 238.5 renders/s / 29.87 MB versus
the same 243.7 / 29.91 MB clean control, with matching output. The small RSS
difference is noise and throughput was 2.1% lower, so this path was reverted.
Its report is `work/react-perf/react-rope-charcodeat-memory-first-max-young-16k-nojit-11.json`.
These results shift the next React work to the object-heavy call and interpreter
path; string assembly experiments have not produced a combined speed and memory
win. Linux RSS is diagnostic only.

A follow-up allocator experiment kept empty-page list links in separate mmap
metadata and used `MADV_DONTNEED` before reuse. The 3-round, 1,000-render
memory-first screen preserved output (4,996,000 bytes, checksum 83793), but
measured 203.5 renders/s / 30.88 MB sampled max RSS. The earlier pageout screen
at the same 4K young cap measured 280.0 / 30.70 MB, so this design was rejected
and its implementation reverted. The report is
`work/react-perf/react-quench-alloc-dontneed-memory-first-max-young-4k-nojit-screen-3.json`.

The subsequent call-path audit found that ordinary fast-lane calls already copy
the common 0–4 arguments directly into lane frames; 92% of React's user calls
fall in that range. Avoiding `CallArguments::from_slice` would mostly target the
cold general fallback. The next no-JIT experiment will select a structural
interpreter or allocation target from measured React lane profiles. Runtime
research suggests profile-guided quickening for no-JIT and a broad one-pass
baseline tier for the object-heavy React functions currently outside Quench's
narrow compiled tier. Neither direction has yet demonstrated the required
20% win.

The workload build instructions now use the repository's general PGO production
build (`tools/pgo-build.sh production`); its corpus excludes the scored engine
benchmarks. An interleaved 11-round comparison of that binary and a clean
system-allocator build used the same memory-first, 32,768 young-cap settings.
PGO measured 276.8 renders/s / 26.28 MB versus the clean build at 229.7 /
28.75 MB, with matching 24,980,000-byte output and checksum 414318. PGO improves
Quench's throughput 20.5% and RSS 8.6% against the clean binary, and its RSS is
27.4% below Node and 29.7% below Bun. It remains far slower than both engines:
1,639.3 renders/s for Node and 1,509.2 for Bun. The paired report is
`work/react-perf/react-pgo-vs-clean-paired-memory-first-max-young-32k-nojit-11.json`.
Linux RSS sampling is diagnostic, not qualification.

Combining general PGO with a 512-unit flat-prefix threshold improved React
no-JIT throughput against the paired PGO control. In 11 rounds at 5,000 renders
per sample, the 512-unit build measured 305.1 renders/s / 28.11 MB versus
261.3 / 26.26 MB for the 2,048-unit build. Output matched (24,980,000 bytes,
checksum 414318). The speed gain was 16.8%; the RSS increase was 7.1%. RSS
remains 22.4% below Node and 24.9% below Bun, but throughput is still about 5x
slower than either runtime. The paired report is
`work/react-perf/react-pgo-prefix-512-vs-2048-memory-first-max-young-32k-nojit-11.json`.
Linux RSS sampling is diagnostic, not qualification.

A concat-cache probe removed the pre-number-dispatch lookup because the React
profile showed 625,214 string-string concats and zero hits. The initial
3-round screen appeared 23.2% faster, but the full 11-round paired run measured
299.6 renders/s / 28.35 MB versus 311.5 / 26.84 MB for the unchanged PGO+512
control. Output matched, but speed fell 3.8% and RSS grew 5.6%, so the probe was
reverted. Reports are
`work/react-perf/react-pgo-prefix-512-single-concat-lookup-vs-control-memory-first-32k-nojit-screen-3.json`
and `work/react-perf/react-pgo-prefix-512-single-concat-lookup-vs-control-memory-first-32k-nojit-11.json`.
Linux RSS sampling is diagnostic, not qualification.

A smaller 256-unit flat-prefix threshold did not improve enough over the retained
512-unit PGO build to justify its memory cost. In the paired 11-round full-size
run, 256 measured 319.3 renders/s / 28.07 MB versus 310.1 / 26.81 MB for 512,
a 3.0% speed gain for 4.7% more RSS. Output matched at 24,980,000 bytes and
checksum 414318. RSS was 22.3% below Node and 24.9% below Bun, but the 512-unit
build remains the better memory/speed balance. Report:
`work/react-perf/react-pgo-prefix-256-vs-512-memory-first-max-young-32k-nojit-11.json`.

A 256-entry capture-walk memo table was screened against the default 64-entry
table, targeting the repeated outer-capture walks in React's lane profile. The
3-round screen appeared 8.3% faster, but the paired 11-round full-size run was
only 4.0% faster (314.5 renders/s vs 302.5) and used 4.6% more RSS (28.06 MB
vs 26.83 MB). Output matched at 24,980,000 bytes/checksum 414318, but the small
speed increase did not justify the memory cost, so the table was restored to 64.
Report: `work/react-perf/react-pgo-capture-walks-256-vs-64-memory-first-max-young-32k-nojit-11.json`.

A 64K young-generation cap did not improve the paired 32K PGO+512 control. The
11-round full-size run measured 303.0 renders/s / 26.82 MB at 64K versus 325.9 /
26.83 MB at 32K (-7.0% speed, effectively identical RSS), with matching output.
The 32K cap remains selected. Report:
`work/react-perf/react-pgo-prefix-512-young64k-vs-32k-11.json`.

An early string-pair branch tried to bypass numeric operator checks for the
625k React string-string additions. The paired three-round screen measured
307.0 renders/s / 28.02 MB versus 305.4 / 26.89 MB for the unchanged PGO+512
control: only 0.5% more throughput and 4.2% more RSS, with matching output. The
fast path was reverted without a full run because it showed no useful signal.
Report: `work/react-perf/react-pgo-string-pair-fastpath-vs-control-memory-first-32k-screen-3.json`.

After the diagnostic GC builds, the ordinary PGO+512 production `quench-node`
was restored and rerun with the full 5,000-render workload, 250 warmups,
memory-first policy, and 32K young cap for 11 interleaved rounds. It measured
325.9 renders/s / 26.86 MB versus Node `--jitless` at 1,662.2 / 36.22 MB and
Bun no-JIT at 1,496.6 / 37.44 MB. Output matched at 24,980,000 bytes and
checksum 414318. RSS remains over 20% lower than both engines, but throughput
is about five times slower, so React no-JIT is not won. Report:
`work/react-perf/react-pgo512-restored-full-memoryfirst-32k-nojit-11.json`.

A PGO build with `-Ctarget-cpu=native` looked 5.7% faster in a three-round
screen, but the full 11-round interleaved comparison reversed that result:
298.5 renders/s / 27.81 MB versus the generic PGO+512 control at 319.5 /
28.63 MB. Output matched at 24,980,000 bytes / checksum 414318. Native codegen
reduced sampled RSS by 2.9% while lowering speed by 6.6%, so it was rejected
and the ordinary PGO binary restored. Report:
`work/react-perf/react-pgo512-nativecpu-vs-generic-memoryfirst-32k-nojit-11.json`.

A feature-gated call-target census now samples both lane calls and same-program
calls entered through the general dispatch loop. In a 500-render React run it
found four dominant monomorphic sites (two with roughly 90k sampled calls each,
and two with roughly 13k each); all other sites were sparse. The new general
dispatch hook did not reveal a second class of high-volume call sites, so a
per-site target memo has too little likely coverage to justify its complexity
as the next optimization. A followup also sampled normal frame materialization
and left the same four sites on top. These profile builds are intentionally
instrumented; the extended run's 52.9 renders/s is not a performance result.
Output matched at 2,498,000 bytes/checksum 41826. Full diagnostic output:
`work/react-perf/react-call-target-profile-all.log`.

The lane profiler now reports dynamic record counts by opcode and the next
opcode after each Move. Over the same 500-render React input, Move led with
20.9M records, followed by GetField (14.2M), JumpBinaryFalse (10.7M),
LoadLocalPlain (8.6M), JumpFalse (8.2M), and Call (7.9M). Move's successors
were Move (5.8M), Call (4.5M), JumpFalse (3.6M), and GetField (1.1M); the
existing Move-pair fusion already handles adjacent moves. This supports testing
a Move-to-JumpFalse fusion, which skips a dispatch on about 3.6M records in this
sample. Diagnostic output:
`work/react-perf/react-lane-opcode-profile.log` and
`work/react-perf/react-lane-move-successors.log`.

The Move-to-JumpFalse fusion screen matched output, but the full 11-round paired
run was effectively flat: 319.26 renders/s / 27.20 MB versus 318.69 / 27.70 MB
for the generic PGO+512 control (+0.18% speed, -1.8% RSS). The fused handler
was reverted. Reports:
`work/react-perf/react-move-jumpfalse-vs-generic-screen-3.json` and
`work/react-perf/react-move-jumpfalse-vs-generic-11.json`.

Reordering the monomorphic own-field cache lookup to fetch the site cache before
loading the receiver object record was also rejected. The 11-round paired
React no-JIT run measured 316.7 renders/s / 27.78 MB versus 327.1 / 27.69 MB
for the generic PGO+512 control (-3.2% speed; effectively unchanged RSS).
Output matched at 24,980,000 bytes/checksum 414318. The small screen had also
shown no reliable throughput gain. Report:
`work/react-perf/react-getfield-load-order-11.json`.

An activation-window experiment filled only the registers after call arguments,
avoiding initialization stores that were immediately overwritten by the
argument copy. In the full 11-round React no-JIT comparison it measured 307.0
renders/s / 26.78 MB versus 312.6 / 27.66 MB for the generic PGO+512 control
(-1.8% speed, -3.2% RSS). Output matched. Because it reduced RSS without a
throughput win, the change was reverted. The existing PGO profile also dropped
counts for inlined call handlers after the change, so a fresh profile would be
needed to evaluate the candidate fairly. Report:
`work/react-perf/react-activation-fill-after-args-11.json`.

The feature-only lane profiler now counts dynamic opcode transitions. Across
500 React SSR renders, the most frequent pairs were `LoadLocalPlain` →
`JumpBinaryFalse` (5.75M), `Move` → `Call` (5.57M), `JumpBinaryFalse` →
`LoadLocalPlain` (5.22M), and `Move` → `JumpFalse` (4.22M). These dynamic counts
include taken control flow and are more useful for choosing fusion candidates
than static instruction adjacency. The instrumented run produced the expected
2,498,000 bytes/checksum 41826; its throughput is diagnostic only. Report:
`work/react-perf/react-dynamic-transition-profile.log`.

A direct `LoadConst` → `Return` fusion was screened from the dynamic pair
census. The short 3-round screen measured 311.1 renders/s / 26.91 MB versus
316.1 / 27.72 MB for the control, but the full 11-round paired run reversed
that result: 303.0 / 27.37 MB versus 299.6 / 27.67 MB (+1.1% speed, -1.1%
RSS). Output matched at 24,980,000 bytes/checksum 414318, so the fusion is
retained as a small speed-and-memory improvement. It returns the already
resolved constant through the existing activation/frame return path, avoiding
the temporary register write/read and one dispatch. Reports:
`work/react-perf/react-loadconst-return-screen-3.json` and
`work/react-perf/react-loadconst-return-11.json`.

The next diagnostic classified a plain `Move` immediately before `Call` by
whether its destination supplies the callee, receiver, or a call-window argument.
Among the classified instances in a 500-render React SSR run, no destinations
supplied the callee or receiver: 1.77M wrote argument 0, 154K argument 1, 112K
argument 2, 198K argument 3, and 112K a later argument. Calls with one argument
accounted for 1.77M instances. This narrows a possible fusion to argument-window
transfer, but does not establish a profitable or safe generalized call fast path;
the instrumented run's timing is diagnostic only. Output matched at 2,498,000
bytes/checksum 41826. Report:
`work/react-perf/react-movecall-relations-500.json` and
`work/react-perf/react-movecall-relations-500.log`.

Clearing script activation windows of at most eight registers exactly, instead
of always writing the 16-register fill block, reduced work but lost throughput.
The short screen measured 308.3 renders/s / 25.95 MB versus 335.7 / 27.75 MB
for the same-run control. The full 11-round paired run confirmed a smaller
regression: 313.2 / 27.22 MB versus 327.0 / 27.68 MB (-4.2% speed, -1.7% RSS).
Output matched at 24,980,000 bytes/checksum 414318; the implementation was
reverted. A guarded adjacent own-field pair was then screened from the 2.91M
dynamic `GetField → GetField` transitions in the 500-render profile. The full
11-round run measured 320.1 renders/s / 26.9 MB versus 317.7 / 27.6 MB for the
paired control (+0.8% speed, -2.5% RSS). Output matched at 24,980,000
bytes/checksum 414318. The small speed gain did not clear the experiment's
noise margin, so the implementation was reverted. Reports:
`work/react-perf/react-exact-small-fill-screen-3.json` and
`work/react-perf/react-exact-small-fill-11.json`,
`work/react-perf/react-adjacent-field-pair-screen-3.json`, and
`work/react-perf/react-adjacent-field-pair-11.json`. The next `Move → Call`
argument-window attempt used compiler forward-substitution for the one-argument
form. It preserved output but failed the three-round screen: 285.1 renders/s /
24.5 MB versus 308.3 / 24.7 MB for the paired control. It was reverted without
a full run. Report: `work/react-perf/react-call-window-forward-screen-3.json`.
A wide-aware `LoadCapture → Move` compiler rewrite was screened from the 2.73M
dynamic transitions in the React profile. It preserved output but measured
261.7 renders/s / 27.0 MB versus 325.8 / 27.6 MB for the paired control, so the
compiler change was reverted without a full run. The build reused PGO data from
before the rewrite-function change and reported a function hash mismatch; this
limits attribution but does not provide a positive signal. Report:
`work/react-perf/react-wide-capture-move-screen-3.json`. It was reverted.

The next profile-ranked no-JIT candidate coalesces an exact plain local load
with the following register/register integer-comparison branch. It keeps the
branch record independently callable and sends non-integer operands to the
existing generic relation handler. The full 11-round React SSR run measured
331.1 renders/s / 26.90 MB versus the paired generic PGO control at 323.4 /
27.14 (+2.4% throughput, -0.9% RSS), with matching 24,980,000 output bytes and
checksum 414318. Node `--jitless` measured 1,696.1 / 36.18 MB and Bun no-JIT
1,512.9 / 37.51 MB. Quench's RSS is over 20% below both, but throughput is
still 5.1x behind Node and 4.6x behind Bun; this is a retained local
improvement, not a package win. Report:
`work/react-perf/react-loadlocal-jumpbinary-nojit-11.json`.

The reverse-order `JumpBinaryFalse → LoadLocalPlain` pair had a misleadingly
positive three-round screen when built with a stale profile: 333.6 renders/s /
25.87 MB versus 326.9 / 27.48 for the paired control. Rebuilding with a fresh
PGO training profile reversed the result: 307.8 / 26.26 MB versus 313.9 /
26.58 (-2.0% throughput, -1.2% RSS), with matching output. The reverse-pair
change was reverted. Reports:
`work/react-perf/react-jumpbinary-loadlocal-screen-3.json` and
`work/react-perf/react-jumpbinary-loadlocal-freshpgo-screen-3.json`.

The PGO build can optionally include React SSR samples by setting
`QUENCH_PGO_REACT_ROUNDS` after installing `tests/npm-perf` dependencies. With
1,000 training rounds, the three-round screen measured 394.6 renders/s /
25.31 MB versus the generic-PGO control at 362.8 / 26.33. The 11-round paired
run measured 346.3 / 25.62 versus 309.4 / 26.39 (+11.9% throughput, -2.9%
RSS), with matching 24,980,000 output bytes and checksum 414318. Node `--jitless`
measured 1,604.6 / 36.22 MB and Bun no-JIT 1,580.3 / 37.52 MB. Quench remains
4.6x slower than both, so this profile-directed build is a substantial local
improvement, not a package win. Linux RSS is diagnostic. Reports:
`work/react-perf/react-profile-guided-build-screen-3.json` and
`work/react-perf/react-profile-guided-build-nojit-11.json`.

Increasing the optional React training sample to 5,000 rounds passed an
11-round paired no-JIT screen at 5,000 measured renders per round: 345.6
renders/s / 24.50 MB sampled max RSS versus the generic-profile control at
330.6 / 26.43 (+4.5% throughput, -7.3% RSS). Output matched at 24,980,000
bytes and checksum 414318. Node `--jitless` measured 1,649.6 renders/s /
36.19 MB, and Bun no-JIT 1,479.7 / 37.39 MB. The larger training weight
improves the paired Quench result but remains 4.8x slower than Node and 4.3x
slower than Bun. The candidate clears the RSS margin but is not a package win.
Linux RSS remains diagnostic. The 5,000-round instrumented run took longer
than the default per-fixture timeout, so `tools/pgo-build.sh` now gives this
optional training workload its own 120-second limit. Report:
`work/react-perf/react-profile-guided-build-5000-screen-3.json` and
`work/react-perf/react-profile-guided-build-5000-nojit-11.json`.

After merging `origin/v2-merged`, the latest 11-round no-JIT baseline uses the
new PGO corpus (38 curriculum programs, 100 diagnostic micros, Node-compat
fixtures, and 5,000 React rounds). It measures 365.1 renders/s / 21.07 MB
sampled max RSS. Node `--jitless` is 1,638.3 / 36.18 MB and Bun no-JIT is
1,484.6 / 37.40 MB. Outputs still match at 24,980,000 bytes/checksum 414318.
Quench remains 4.5x slower than Node and 4.1x slower than Bun, despite 42–44%
lower sampled RSS; this is the fresh merged-source baseline, not a package win.
Report: `work/react-perf/react-ssr-postmerge-pgo-5000-nojit-11.json`.

The post-merge `profile-memory` diagnostic run (500 React SSR rounds, 50 warmups;
unoptimized diagnostic binary, so its elapsed time is not comparable with the
production score) found 16,525 live out-of-line object properties, 16,482
narrowable (99.7%), plus 5,321/5,321 narrowable dense-array elements. These
counts suggest compact storage is feasible, but their total backing capacity is
small compared with process RSS. The same snapshot attributed 1,017,728 bytes
to field-cache side tables, 917,504 bytes to shape records, and 198,198 bytes to
shape lookup indexes. Profile output also included 39.6 MB of diagnostic
instrumentation, which is excluded from runtime memory totals. Next, measure
field-cache occupancy and target high-capacity tables before prototyping a
representation change. Diagnostic log: `work/react-perf/react-memory-counters.stderr`.

The follow-up occupancy probe found 1,288 active mono field-cache entries out
of 22,831 sites (5.64%). The 1,017,728-byte field-cache total includes the
monomorphic table, per-site indexes, and field-add cache sets; it is not all
recoverable by making the mono table sparse. A sparse mono table could save
roughly 259 KB at this occupancy, but adds an indexed indirection to a hot read.
Before changing that path, split the aggregate and inspect the unusually large
fixed-depth prototype storage in `FieldAddCacheSet`. Diagnostic log:
`work/react-perf/react-memory-sites.stderr`.

The split profile measured 369,792 B for mono entries, 123,264 B each for
megamorphic and field-add indexes, and 401,408 B for field-add sets. Of 343
sets, 340 use one way, two use two ways, and one uses three; none uses the
fourth. Across 347 entries, 266 have prototype depth 0, 57 depth 1–2, six
depth 4, and 18 depth 7. This points to two compact-layout experiments: a
three-way set (no evictions in this sample) and a short inline prototype prefix
with an overflow path, preserving deep-chain behavior. Both still require
paired production throughput/RSS checks across package workloads before they
can be accepted. Diagnostic log:
`work/react-perf/react-memory-field-add-occupancy.stderr`.

The fresh post-merge JIT baseline (three rounds, 1,000 renders each) matched
output and measured 537.6 renders/s / 28.90 MB for Quench, 15,625 / 46.87 MB
for Node, and 8,621 / 98.58 MB for Bun. This is a diagnostic sample; Quench is
still far behind on speed.

I tested removing a duplicate string-concat cache probe on the string+string
miss path, then retrained PGO against that change. In 11-round paired screens
at 1,000 renders, no-JIT measured 516.8 / 37.39 MB versus control at 513.6 /
36.40 MB (+0.6% throughput, +2.7% RSS); JIT measured 514.1 / 29.40 MB versus
509.7 / 29.04 MB (+0.9% throughput, +1.2% RSS). All outputs matched. These
results show no useful performance win, so I reverted the cache-probe change.
The aggregate profiler did miss early cache hits; its counter now records those
hits in `binary()` so future string-cache decisions use complete counts. This
report change is profile-only. Reports:
`work/react-perf/react-ssr-concat-dedup-nojit-1000-3.json` and
`work/react-perf/react-ssr-concat-dedup-jit-1000-3.json`, plus the 11-round
paired reports with the same names and `-11` suffix.

Two further React SSR candidates were rejected after paired 11-round screens
at 1,000 renders per round. A liveness-checked `Move`→`Move` rewrite measured
566.9 renders/s / 37.43 MB no-JIT versus control at 544.7 / 36.31 MB; JIT
measured 533.6 / 29.35 MB versus 536.5 / 28.94 MB. The speed increase did not
come with an RSS reduction. An occupied-only full sweep driven by the existing
per-slab free bitmap measured 527.7 / 37.62 MB no-JIT versus 533.6 / 36.23 MB;
JIT measured 539.1 / 29.48 MB versus 523.8 / 28.84 MB. It was slightly slower
no-JIT and increased RSS in both modes. Both runtime changes were reverted.
Linux RSS samples are diagnostic; output parity matched in all runs. Reports
are in the local `work/react-perf` scratch directory as
`react-movemove-{nojit,jit}-1000-11.json` and
`react-occupied-sweep-{nojit,jit}-1000-11.json`.

A lazy canonical empty-string record was tested with GC rooting and
snapshot/restore reconstruction. It passed its focused heap test, but lost the
interleaved 11-round no-JIT React screen: 518.4 renders/s / 37.59 MB versus the
control at 548.2 / 36.27 MB. Output matched. The allocation branch and
long-lived root cost more than the saved empty records in this workload, so
the runtime change was reverted. Report:
`work/react-perf/react-empty-string-nojit-1000-11.json`.

The aggregate string profiler was corrected after review found that its old
`JsString::capacity()` probe flattened ropes while measuring them. The updated
profile reads existing flat payload bytes and records non-flattening concat
shapes. Across 1,000 React renders it counted 1,192,215 newly allocated
string+string concats: 601,065 inline-flat plus 591,150 rope left operands,
1,192,215 inline-flat right operands, and 600,015 flat plus 592,200 rope
results. Early cache hits are counted separately and are not included in these
shape totals. Output matched at 4,996,000 bytes/checksum 83793. This is
instrumented diagnostic data, not a performance score. Summary:
`target/perf-reports/react-aggregate-string-shapes.json`.

A `Move`→`GetField` bytecode rewrite was screened with a production PGO build
trained on 500 React renders. A correctness review added a guard for parameter
registers, since `Function.arguments` can observe writes that ordinary
liveness considers dead; the focused parameter/temp rewrite test passed. In
three 500-render paired samples, no-JIT measured 427.4 renders/s / 36.34 MB
for the candidate versus 469.9 / 32.15 MB for the PGO control (-9.1% speed,
+13.0% RSS). JIT measured 519.8 / 29.62 MB versus 508.6 / 30.47 MB (+2.2%
speed, -2.8% RSS). Output matched in both modes. The no-JIT regression rules
out this candidate for the dual-mode target, so the runtime rewrite and its
test were reverted. Reports:
`target/perf-reports/react-move-getfield-pgo-screen-no-jit.json` and
`target/perf-reports/react-move-getfield-pgo-screen-jit.json`.

A guarded `completeShell` entry prefix read the cached `trackedPostpones`
property, tested it against null, staged the capture/literal/argument registers,
and resumed immediately before the dynamic call. The call and all misses stayed
on the interpreter path. Its 11-round, 1,000-render JIT screen measured 481.7
renders/s / 29.41 MB sampled maximum RSS versus the control at 502.3 / 29.76
MB (-4.1% throughput, -1.2% RSS); output matched at 4,996,000 bytes/checksum
83793. Linux RSS is diagnostic, and the candidate/control PGO profiles were
not identical, so this was a screen rather than qualification. The short prefix
does not amortize its entry guard cost and was reverted. The next JIT direction
is a multi-operation, generic-Value tracelet that can cover repeated
field/load/branch regions in the hot React functions, with interpreter side
exits for dynamic operations.

To select a region from dynamic rather than static bytecode shape, the
`profile-aggregate` diagnostic now records straight-line triples by program,
function, and start PC. A 500-render React profile found the most repeated site
in `react-dom-server-legacy.node.production.js`: function 192, PCs 0–2,
`Binary → JumpFalse → LoadCapture` executed 537,350 times. Another site in
function 175, PCs 68–70, `GetField → Binary → JumpBinaryFalse`, had the same
count. The workload produced 2,498,000 bytes/checksum 41826. This is an
instrumented profile, not a timing result. The next candidate must lower a
bounded region with generic values and exact side exits; `Binary` can invoke
JavaScript coercion, so it cannot be treated as a primitive operation without
a guard and a pre-effect exit. This profile narrows the search, but it does not
yet establish a performance win.

The Undici workload uses pinned `undici@7.30.1` `Pool.request()` calls against
a loopback HTTP server with four connections, fixed JSON responses, and body
checks. It resolves the local npm package explicitly because Bun's built-in
`undici` shim omits `Pool.close()`. Quench initially failed package startup
because `node:http.maxHeaderSize` was missing; the API now exposes Node's
default 16,384-byte value through a getter with Node's property descriptor,
and the full workload completes.

The 11-round JIT screen used 1,000 requests and 50 warmups per process. Quench
measured 1,116 requests/s and 44.6 MB sampled RSS, Node 5,464 / 81.3 MB, and
Bun 6,250 / 93.5 MB. Quench is about 80–82% slower, though its sampled RSS is
45–52% lower. For JIT-off, Node `--jitless` removes the `WebAssembly` global
that Undici's llhttp parser requires, so that comparison is unavailable.
The 11-round Quench/Bun screen measured 1,171 / 45.8 MB for Quench and
2,119 / 49.1 MB for Bun: Quench is 44.7% slower and uses only 6.7% less
sampled RSS. Linux RSS is diagnostic, so neither mode qualifies for the 20%
target. Reports are local at `target/perf-reports/undici-pool-{jit,nojit}-11.json`.

The first Zod v4 workload uses flat and nested object schemas, a discriminated
union, successful `parse`, and invalid `safeParse` with issue-path checks. The
11-round clean-production screen used 1,000 rounds and 50 warmups per process;
output checks matched across engines. No-JIT throughput was 22,599 operations/s
for Quench, 78,431 for Node `--jitless`, and 61,538 for Bun with JIT disabled.
JIT throughput was 22,857 for Quench, 137,931 for Node, and 173,913 for Bun.
Quench already used less sampled RSS in every comparison: 28.5 MB versus
39.5/35.0 MB no-JIT, and 24.5 MB versus 52.1/47.9 MB with JIT. It does not
meet the 20% threshold on no-JIT RSS against Bun (18.5% lower), and it misses
the throughput threshold by a wide margin in both modes. Linux RSS is
diagnostic; these results are a screen, not platform qualification. The local
reports are `target/perf-reports/zod-parse-nojit-11.json` and
`target/perf-reports/zod-parse-jit-11.json`. A separate JIT diagnostic run
recorded 210 function records and zero native compiles. The wrapper's
`operations` function alone fell back 1,050 times with
`unsupported-lowering-shape-or-codegen-failure`; its bytecode includes 34
`GetField`, 13 `Binary`, four dynamic `Call`, and nine `JumpTrue` operations.
This explains why enabling JIT barely changes Zod throughput: the hot call path
does not enter native code. Extending native execution across these dynamic
operations is the primary JIT direction, with exact semantic side exits.

A same-build 11-round no-JIT comparison tested the existing `memory-first`
policy at 5,000 rounds and 250 warmups. It lowered Quench's sampled RSS from
27.9 MB to 16.3 MB (-41.6%), but also lowered throughput from 37,594 to
31,201 operations/s (-17.0%). The shared checksum was 120,000. Node measured
91,743 operations/s / 50.2 MB and Bun 79,051 / 38.2 MB in this run, so the
policy still loses throughput to both competitors and fails the joint target.
This is an allocator/GC tradeoff screen, not a win; reports are local at
`target/perf-reports/zod-memory-policy-{throughput,memory-first}-nojit-11.json`.

A feature-only call-cache census found that Zod's general-dispatch in-loop
cache (64 entries) hit 56.9% of lookups; the lane's per-call-site links missed
12.6%. Increasing the general cache to 256 entries was screened in five paired
no-JIT rounds. Output matched, but paired throughput moved only +3.1% at the
median with round deltas from -13.7% to +13.4%; sampled RSS changed by less
than 1%. The capacity change and temporary counters were reverted because the
gain was not reliable. Diagnostic report:
`target/perf-reports/zod-callcache-256-screen-5-retest.json`.

The Pino workload uses pinned `pino@10.4.0` with structured JSON, a request
serializer, and redaction into a synchronous in-memory sink. Timestamp and base
fields are disabled so output checksums are stable between processes. The
11-round, 5,000-record screen matched output across all engines. No-JIT
throughput was 15,823 records/s for Quench, 58,140 for Node, and 44,248 for
Bun; sampled RSS was 38.1, 31.0, and 42.3 MB. JIT throughput was 14,451 for
Quench, 151,515 for Node, and 172,414 for Bun; sampled RSS was 34.8, 43.8,
and 49.3 MB. Quench misses speed in both modes; JIT RSS is over 20% lower than
both engines, while no-JIT RSS is 22.8% higher than Node and only 10% lower
than Bun. Linux RSS remains diagnostic. Reports are local at
`target/perf-reports/pino-json-{nojit,jit}-11.json`.

JIT diagnostics show the `logRequest` wrapper has 5,250 fallback entries,
zero native entries, and is rejected as
`unsupported-lowering-shape-or-codegen-failure`. Its body includes object
creation, a dynamic logger call, and redaction data access. One small getter
compiled, but that does not cover the hot logging operation; broad generic
call-capable execution remains necessary.

## ws

`ws-echo.cjs` uses pinned `ws@8.18.3` for a local WebSocket client/server
upgrade and sequential binary echo workload. It validates every echoed frame
and reports bytes and checksum. Quench now transfers upgraded HTTP transports
to `net.Socket` on both client and server, emits the server `upgrade` event,
and delivers status 101 through `ClientRequest.upgrade`. The client also
accepts the string port returned by `URL.port`.

A three-round Linux diagnostic used 300 echo messages and 20 warmups per
process. All engines produced the same 12,490 bytes and checksum 25,143. Quench
still loses throughput and RSS against both runtimes:

| Mode | Quench | Node | Bun |
| --- | ---: | ---: | ---: |
| No-JIT | 1,277 ops/s, 38.7 MB | 7,692 ops/s, 33.1 MB | 42,857 ops/s, 7.1 MB |
| JIT | 1,068 ops/s, 33.8 MB | 12,500 ops/s, 36.2 MB | 42,857 ops/s, 3.1 MB |

These short screens establish compatibility only, not a performance win.
Their Linux process-tree RSS samples are diagnostic; reports are in
`target/perf-reports/ws-echo-upgrade-{nojit,jit}-3.json`.

## Fresh `v2-merged` production screen

After merging the latest `v2-merged` tip, the PGO production binary was rebuilt
and these Linux 11-round screens were rerun. Quench still loses throughput on
all three workloads; these measurements do not meet the 20% speed target.
Linux `VmHWM` is sampled every 2 ms and remains diagnostic, not platform
qualification.

| Workload and mode | Quench median | Node median | Bun median |
| --- | ---: | ---: | ---: |
| React SSR, no-JIT, 1,000 renders | 533/s, 38.9 MB | 1,629/s, 36.2 MB | 1,466/s, 37.3 MB |
| React SSR, JIT, 1,000 renders | 500/s, 30.0 MB | 14,493/s, 47.2 MB | 7,692/s, 92.0 MB |
| Zod parse, no-JIT, 1,000 ops | 36,364/s, 26.0 MB | 68,966/s, 39.5 MB | 65,574/s, 35.1 MB |
| Zod parse, JIT, 1,000 ops | 35,088/s, 23.1 MB | 222,222/s, 52.0 MB | 235,294/s, 47.7 MB |
| Pino JSON, no-JIT, 5,000 records | 21,459/s, 34.1 MB | 58,824/s, 31.0 MB | 43,860/s, 42.2 MB |
| Pino JSON, JIT, 5,000 records | 18,868/s, 31.4 MB | 147,059/s, 43.3 MB | 185,185/s, 49.7 MB |

React's JIT screen retains a large RSS reduction but is 96.6% slower than Node.
Zod's RSS is over 20% lower than both engines in both modes, while throughput
is 47–85% lower. Pino JIT RSS is 27–37% lower, while throughput is 87–90%
lower; in no-JIT its RSS is above Node's. Reports are in
`target/perf-reports/{react,zod-parse,pino-json}-{nojit,jit}-merged-11.json`.
The React JIT diagnostic recorded zero native entries across 58 functions;
the hottest render functions fell back 42,680–215,160 times each in a
200-render sample. The next runtime change should target broad native coverage
for closure/capture-heavy functions and validate both throughput and RSS.

## Added package workloads (October 23, 2026)

The pinned dependency set now includes all ten requested npm packages:
React/react-dom, Next, Vite, esbuild, sharp, Fastify, Undici, ws, Pino, and
Zod. The added workloads cover Next dynamic route matching and `NextResponse`
JSON, Vite's TSX transform, esbuild's TSX transform, and sharp raw-image resize
plus WebP encoding. Inputs, lockfile and binary hashes, versions, parity
checksums, medians, and sampled RSS are retained in
[`evidence/tod173-npm-10-2026-10-23/`](../../evidence/tod173-npm-10-2026-10-23/).

Five-run Linux diagnostics show no package win. Quench must exceed both
comparators' throughput by 20% and reduce both maximum RSS values by 20%; Linux
RSS here is sampled every 50 ms and cannot qualify a result.

| Workload / phase | Quench ops/s / MiB | Node ops/s / MiB | Bun ops/s / MiB | Result |
| --- | ---: | ---: | ---: | --- |
| esbuild no-JIT | 305 / 59.29 | 899 / 64.28 | 965 / 56.88 | Loses speed and Bun RSS |
| esbuild JIT | 334 / 59.15 | 1,042 / 80.15 | 1,104 / 67.06 | Loses speed |
| sharp no-JIT | 367 / 50.54 | 453 / 56.18 | 452 / 52.20 | Closest no-JIT; misses both gates |
| sharp JIT | 360 / 46.53 | 441 / 68.57 | 450 / 61.06 | RSS improves, speed loses |
| Next no-JIT, 20k rounds | 4,196 / 53.22 | 19,342 / 42.18 | 112,360 / 46.14 | Loses both |
| Next JIT, 20k rounds | 4,314 / 39.56 | 81,301 / 60.04 | 210,526 / 52.52 | RSS improves, speed loses |
| Vite no-JIT, 500 rounds | 8,333 / 68.88 | unavailable | 13,889 / 59.13 | Node `--jitless` cannot load Oxc WASM |
| Vite JIT, 500 rounds | 8,333 / 56.69 | 27,778 / 70.86 | 31,250 / 66.42 | Loses speed |
| Undici no-JIT | 1,984 / 60.07 | unavailable | 3,257 / 46.64 | Node `--jitless` cannot load llhttp WASM |

Next's extended 20,000-operation samples remove the clock-resolution artifact
in the earlier short runs. The output matched across all participating
engines. The extended JIT sample shows lower Quench RSS, but throughput is
about 19x below Node and 49x below Bun. Sharp's native work and asynchronous
Node-API completion path make it the nearest candidate: with JIT, Quench RSS is
24% below Bun, but throughput remains about 20% below Bun and 18% below Node.
The small package sweep does not justify a runtime change yet; further work
needs function-level profiling and paired candidate/control screens.

Vite and Undici node no-JIT omissions are explicit in their reports and do not
mean a runtime result was invented. Bun and Quench run those phases. All package
screens have `qualificationReady: false`; compare them as host-local
performance diagnostics only. The Zod study and its source-rejected candidates
remain documented separately in `evidence/tod173-zod-2026-10-23/`.
