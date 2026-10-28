# V8-v7 native JIT coverage screen

Host: Linux x86_64, 4 vCPU quota, 16 GiB memory cgroup. Source revision:
`1d70d4b340590d8b3b36e2619cee06197133c4bc` (the later `8ffdf85bc` change only
added evidence). Quench executable SHA-256:
`aca61031784f3bd9848c9bfbfab86fc9513a1ead9a287a31449279a53625774a`.
V8-v7 corpus is the repository's pinned submodule checkout.

This is a one-run, diagnostic-only execution of the stock V8-v7 benchmark source
for the four requested workloads. Each input concatenates `base.js`, that
fixture, and the same stock `BenchmarkSuite.RunSuites` result/error/score runner
used by `quench-bench/v8-v7-rss.cjs`; Quench was invoked with `--jit` and
`QUENCH_JS_JIT_STATS=1`. All four processes exited successfully and printed a
stock Score. Diagnostic counters and stdout are retained in `raw/`. This run
measures coverage and output completion only; it does not compare peers, use the
RSS runner, or establish a performance win.

| Fixture | Stock Score | JS native entries | OSR region runs | Notes |
| -- | --: | --: | --: | -- |
| Richards | 258 | 0 | 0 | Hot constructors and functions stayed interpreted. |
| DeltaBlue | 219 | 7,331 | 0 | Native entries came from three tiny functions; the two busiest interpreted functions had 21,568 and 8,640 entries. |
| Crypto | 2,530 | 3,123 | 5,880,537 | `nbits` entered compiled code; the `am3` loop used OSR. |
| NavierStokes | 22,253 | 0 | 67,726 | Numeric loops used OSR, but compiled JS function entries remained zero. |

The code-coverage data puts the shared VM continuation for effectful calls on
the critical path. Loop OSR already covers the hot numeric loops in Crypto and
NavierStokes. Richards needs loop admission for its current control flow plus a
native call path; DeltaBlue's many constructor and callback entries need a
semantics-preserving call region rather than more numeric leaf recognizers.

Reproduction used these repository inputs:

```text
quench-bench/js-engine-benchmark/v8-v7/base.js
quench-bench/js-engine-benchmark/v8-v7/{richards,deltablue,crypto,navier-stokes}.js
```

For each fixture, concatenate `base.js`, the fixture source, and the runner
below into a temporary script, then run it with
`QUENCH_JS_JIT_STATS=1 target/production/quench-node --jit <script>`:

```js
let succeeded = true;
const print = typeof console !== "undefined" && typeof console.log === "function"
  ? console.log.bind(console) : globalThis.print;
BenchmarkSuite.RunSuites({
  NotifyResult(name, result) { print(name + ": " + result); },
  NotifyError(name, error) { succeeded = false; print(name + ": " + error); },
  NotifyScore(score) { if (succeeded) { print("----"); print("Score: " + score); } },
});
```
