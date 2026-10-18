# Wasm JIT follow-up experiments

These reports are diagnostic, one-round screens from a dirty worktree at
`e64538e072a89c6b0fe7b85904565ff332fd5acf`. They are preserved to show how
the fast-path changes and the rejected coverage expansion behaved. Every
report validated all selected fixtures and recorded equal output, but none is
qualification evidence (`qualification_ready` is false).

## Fast-path screen

`one-round-pretrap/jetstream3-wasm-jit.json` measures the source after fusing
the Wasm indirect-call helper, adding same-pool signature identity checks, and
removing the common indirect-call signature heap clone. Quench remains below
both JIT peers on Score in all five fixtures. It uses less RSS than both peers
for gcc-loops and quicksort, and less than Node for HashSet; it does not win
both metrics on any fixture.

| Fixture | Quench JIT Score / RSS MiB | Bun JIT | Node JIT |
| --- | ---: | ---: | ---: |
| HashSet | 32.46 / 113.1 | 220.09 / 55.5 | 225.38 / 126.9 |
| gcc-loops | 76.21 / 48.0 | 187.81 / 55.5 | 187.33 / 91.8 |
| quicksort | 172.27 / 41.9 | 351.87 / 49.4 | 425.97 / 88.5 |
| tsf | 13.61 / 224.7 | 175.87 / 136.3 | 173.65 / 139.4 |
| zlib | 12.42 / 112.8 | 85.75 / 104.3 | 85.60 / 110.8 |

The separate TSF diagnostics in `one-round-pretrap/tsf-jit-diagnostics.json`
recorded 156/244 native functions, 177,800 emitted bytes, 180,224 mapped
bytes, about 116.7 ms compile time, and 2,816,456 compiled-to-VM callouts.
This points away from executable mapping size as TSF's main RSS cost.

## Reverted eager coverage experiment

`reverted-unreachable/` preserves the screen and diagnostics from temporarily
adding native lowering for Wasm `unreachable`. It compiled all 244 TSF
functions, but grew emitted code from 177,800 bytes to 9,226,568 bytes and
compile time from about 117 ms to about 3.04 s. The one-round TSF Score fell
from 13.61 to 7.61 and RSS rose from 224.7 MiB to 281.4 MiB; the attempt was
reverted. These screens do not establish a statistically significant causal
effect, but the diagnostic growth is direct evidence against eager admission
of all newly eligible functions. The compiler lowering and its test are not
part of the current source.

The raw reports can be reproduced with the commands below after building the
production binaries and setting the pinned Node and Bun paths. `--qjs
/bin/false` satisfies the runner's unused QuickJS version probe for this
JetStream lane.

```sh
fixtures=HashSet-wasm,gcc-loops-wasm,quicksort-wasm,tsf-wasm,zlib-wasm
target/production/quench-bench --fixtures "$fixtures" --suite jetstream3 --jit \
  --runs 1 --timeout-ms 600000 --quench target/production/quench-node \
  --qjs /bin/false \
  --node /workspace/.local/bench-runtimes/node-v26.10.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun-linux-x64/bun \
  --out evidence/jit-jetstream3-wasm-2026-10-18/one-round-pretrap/jetstream3-wasm-jit.json
```

The reverted experiment used the same command with the experimental build and
output path `reverted-unreachable/jetstream3-wasm-jit.json`; its diagnostics
were collected separately with `--jit-diagnostics`.

## Clean 11-round comparison

The qualification report and paired bootstrap analysis are in
`qualification/`. The run used clean source revision
`db977687a9bd91dc229432cf91491c17451b591b`, the pinned JetStream 3.0 checkout,
Node v26.10.0 with Maglev/TurboFan verified by preflight, and Bun 1.4.2 with
`useJIT=true` verified by preflight. All five fixtures completed 11 matched
rounds, passed JetStream validation, produced equal outputs, and carry source
hashes. The report is qualification-ready; the comparison itself is not a
win.

The paired ratios below are Quench divided by the comparator. Intervals are
95% paired bootstrap intervals for the median log-ratio. A score ratio below
1 means Quench is slower; an RSS ratio below 1 means Quench uses less memory.

| Fixture | Quench JIT median Score / RSS MiB | Score vs Bun | RSS vs Bun | Score vs Node | RSS vs Node |
| --- | ---: | ---: | ---: | ---: | ---: |
| HashSet | 29.62 / 113.1 | 0.147 [0.141, 0.154] | 2.035 [2.019, 2.041] | 0.128 [0.118, 0.141] | 0.892 [0.891, 0.893] |
| gcc-loops | 70.96 / 48.1 | 0.414 [0.384, 0.429] | 0.856 [0.847, 0.866] | 0.400 [0.361, 0.410] | 0.524 [0.524, 0.525] |
| quicksort | 209.84 / 42.0 | 0.605 [0.554, 0.715] | 0.863 [0.860, 0.867] | 0.497 [0.476, 0.538] | 0.476 [0.475, 0.476] |
| tsf | 13.38 / 224.8 | 0.068 [0.062, 0.072] | 1.677 [1.611, 1.692] | 0.071 [0.067, 0.073] | 1.614 [1.547, 1.631] |
| zlib | 13.62 / 112.9 | 0.156 [0.151, 0.162] | 1.126 [1.116, 1.134] | 0.154 [0.151, 0.158] | 1.018 [0.998, 1.021] |

No fixture beats both the score and RSS bars against either peer. Score
intervals are wholly below 1 for all fixtures. gcc-loops and quicksort beat
both RSS bars; HashSet beats Node on RSS but not Bun. TSF loses on both metrics,
and zlib uses more RSS than Bun while its Node RSS interval spans 1. The
no-JIT cases that previously beat Bun on both metrics therefore do not carry
that advantage into JIT mode.

The separate diagnostic report in `diagnostics/` is not timing evidence. It
records HashSet compiling 25/35 functions with 31,986 code bytes and about
72.5 ms of compilation; its two hot `HashTable::add` functions entered native
code 9.49M and 39.53M times. TSF compiled 156/244 functions with 177,800 code
bytes, about 104.7 ms of compilation, and 2,816,456 compiled-to-VM callouts.
Those mappings are small beside process RSS, so optimizing executable mapping
size alone will not close the memory gap. The measured score gap is much
larger than the compile-time cost and requires better generated-code/runtime
execution, with TSF also needing fewer expensive VM callouts.

Reproduction and analysis:

```sh
target/production/quench-bench --fixtures HashSet-wasm,gcc-loops-wasm,quicksort-wasm,tsf-wasm,zlib-wasm \
  --suite jetstream3 --jit --runs 11 --timeout-ms 600000 \
  --quench target/production/quench-node \
  --node /workspace/.local/bench-runtimes/node-v26.10.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun-linux-x64/bun \
  --checkpoint target/jetstream3-wasm-jit.checkpoint.json \
  --out target/jetstream3-wasm-jit.json
target/production/quench-bench --analyze target/jetstream3-wasm-jit.json \
  --out target/jetstream3-wasm-jit-analysis.json
```

## Indirect-callee fast path follow-up

Commit `744f2f9c9` invokes eligible same-program Wasm table targets with
`run_compiled` after the existing table and signature checks, using the
callee's instance environment. If the target has no native entry, execution
falls back through the ordinary VM call path. The regression test verifies a
compiled indirect caller reaches a nine-parameter compiled callee. Focused
Wasm tests and the full runtime suite pass (701 unit tests and 18 integration
tests).

The full five-fixture report and paired bootstrap analysis are in
`indirect-call-followup/`. They use the clean source revision
`744f2f9c9090d2620061bc9719f840ba6d3f1446`; all fixtures have 11 valid matched
rounds, valid JetStream outputs, and input hashes. The report is complete and
qualification-ready, but performance qualification failed. Ratios are
Quench divided by the JIT-enabled peer; intervals are paired 95% bootstrap
intervals.

| Fixture | Quench median Score / RSS MiB | Score vs Bun | RSS vs Bun | Score vs Node | RSS vs Node |
| --- | ---: | ---: | ---: | ---: | ---: |
| HashSet | 30.69 / 113.1 | 0.157 [0.153, 0.174] | 2.028 [2.014, 2.034] | 0.144 [0.137, 0.157] | 0.891 [0.891, 0.892] |
| gcc-loops | 71.94 / 47.9 | 0.417 [0.398, 0.455] | 0.859 [0.847, 0.870] | 0.409 [0.379, 0.443] | 0.522 [0.521, 0.523] |
| quicksort | 184.60 / 41.7 | 0.590 [0.539, 0.607] | 0.853 [0.851, 0.859] | 0.455 [0.391, 0.466] | 0.473 [0.472, 0.473] |
| tsf | 14.28 / 224.6 | 0.074 [0.073, 0.079] | 1.670 [1.650, 1.703] | 0.074 [0.072, 0.079] | 1.542 [1.532, 1.546] |
| zlib | 13.47 / 112.6 | 0.157 [0.148, 0.160] | 1.124 [1.121, 1.133] | 0.155 [0.149, 0.160] | 1.011 [0.992, 1.018] |

No fixture beats both Score and RSS against either peer. gcc-loops and
quicksort remain below both score bars while beating both RSS bars; TSF loses
on both metrics. The code change removes an interpreter activation for some
indirect calls, but each call still crosses into the VM. The split TSF
diagnostic records 2.869 million table/reference calls and 27 thousand imports;
it still does not separate compiled-callee hits from interpreter fallbacks.

Reproduction:

```sh
target/production/quench-bench --fixtures HashSet-wasm,gcc-loops-wasm,quicksort-wasm,tsf-wasm,zlib-wasm \
  --suite jetstream3 --jit --runs 11 --timeout-ms 600000 \
  --quench target/production/quench-node \
  --node /workspace/.local/bench-runtimes/node-v26.10.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun-linux-x64/bun \
  --checkpoint target/jetstream3-wasm-jit-744f2f9.checkpoint.json \
  --out target/jetstream3-wasm-jit-744f2f9.json
target/production/quench-bench --analyze target/jetstream3-wasm-jit-744f2f9.json \
  --out target/jetstream3-wasm-jit-744f2f9-analysis.json
```

The next clean-source 11-round report is under
`indirect-call-fallback-cleanup/` for commit `ba75d8978`. It moves two unused
fallback lookups out of the compiled-target path and adds a no-result compiled
indirect-call test. All five fixtures remain far below both peer score bars;
no JIT score-and-RSS win was established. The new diagnostic split and its
callout counts are under `callout-attribution/`.
