# Deferred Wasm indirect-call fallback lookups

Commit `ba75d897833c746c2ed173facbb4f6adb90c4c6d` moves the residual-program
and void-receiver lookups into the interpreter fallback branch of
`reference_callout`. Eligible same-program compiled callees no longer perform
those unused lookups or clone the program's `Rc` on each call. The change also
adds coverage for a compiled indirect call with a no-result callee. Focused
and full runtime tests pass (702 unit tests, 18 integration tests).

The clean-source comparison used 11 matched rounds for the five Wasm fixtures
that had beaten Bun no-JIT on both Score and RSS. It verified Node 26.10.0
Maglev/TurboFan, Bun 1.4.2 `useJIT=true`, JetStream 3.0 inputs and outputs,
and source revision. The raw report and paired-bootstrap analysis are stored
here. The run is complete and qualification-ready, but did not qualify: no
fixture reaches a score ratio of 1 against either peer.

| Fixture | Quench Score / RSS MiB | Score vs Bun | RSS vs Bun | Score vs Node | RSS vs Node |
| --- | ---: | ---: | ---: | ---: | ---: |
| HashSet | 32.51 / 113.2 | 0.153 [0.148, 0.172] | 2.033 [2.026, 2.045] | 0.133 [0.125, 0.134] | 0.892 [0.892, 0.893] |
| gcc-loops | 71.53 / 48.1 | 0.398 [0.384, 0.415] | 0.853 [0.847, 0.867] | 0.391 [0.367, 0.425] | 0.524 [0.522, 0.526] |
| quicksort | 189.91 / 41.9 | 0.549 [0.526, 0.601] | 0.860 [0.858, 0.862] | 0.481 [0.432, 0.509] | 0.475 [0.474, 0.475] |
| tsf | 14.41 / 224.7 | 0.070 [0.066, 0.080] | 1.689 [1.624, 1.695] | 0.072 [0.072, 0.074] | 1.552 [1.536, 1.626] |
| zlib | 13.38 / 112.8 | 0.155 [0.150, 0.160] | 1.125 [1.117, 1.129] | 0.157 [0.146, 0.161] | 0.996 [0.992, 1.015] |

Ratios are Quench divided by the JIT-enabled peer; intervals are paired 95%
bootstrap intervals. gcc-loops and quicksort beat both RSS bars but remain
well below peer scores. HashSet beats Node on RSS only; TSF loses on both
metrics; zlib's Node RSS interval contains parity. The change leaves the
requested score-and-RSS goal open.

The next high-value measurement is separating same-program compiled-callee
hits from interpreter fallbacks at indirect-call sites. The existing total
reference counter includes both paths and cannot establish whether table
resolution, argument marshaling, the VM boundary, or fallback execution is the
dominant cost. A table cache should wait for that attribution because it must
invalidate on Wasm and JavaScript table mutations and preserve cross-instance
environments and traps.

Reproduction:

```sh
source /workspace/quench-build-env.sh
PATH=/workspace/.cargo/bin:$PATH target/production/quench-bench \
  --fixtures HashSet-wasm,gcc-loops-wasm,quicksort-wasm,tsf-wasm,zlib-wasm \
  --suite jetstream3 --jit --runs 11 --timeout-ms 600000 \
  --quench target/production/quench-node \
  --node /workspace/.local/bench-runtimes/node-v26.10.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun-linux-x64/bun \
  --checkpoint target/jetstream3-wasm-jit-ba75d897.checkpoint.json \
  --out target/jetstream3-wasm-jit-ba75d897.json
PATH=/workspace/.cargo/bin:$PATH target/production/quench-bench \
  --analyze target/jetstream3-wasm-jit-ba75d897.json \
  --out target/jetstream3-wasm-jit-ba75d897-analysis.json
```
