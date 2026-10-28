# Same-instance Wasm direct-entry experiment

This report measures commit `3399bb54774cd67774a22e3497ee3b6f811d7c75` on
the pinned JetStream 3.0 corpus (`06785cf861ac44855f168cbbe829278c2802e6de`).
It ran 11 matched rounds for HashSet, gcc-loops, quicksort, TSF, and zlib with
Quench `--jit`, Bun 1.4.3 `BUN_JSC_useJIT=1`, and Node v26.12.0. All selected
fixtures passed validation and output equality. The harness JIT probes passed.

The analyzer did not qualify any case. A case must beat both JIT comparators
on Score and max RSS, with the paired 95% interval beyond parity. The table
shows the median ratio and paired 95% interval; lower Score ratios are slower,
and lower RSS ratios use less memory.

| Fixture | Quench Score / RSS MiB | Score vs Bun | RSS vs Bun | Score vs Node | RSS vs Node |
| --- | ---: | ---: | ---: | ---: | ---: |
| HashSet | 32.21 / 112.87 | 0.157 [0.147, 0.167] | 2.289 [2.278, 2.300] | 0.136 [0.134, 0.141] | 1.065 [1.064, 1.065] |
| gcc-loops | 72.94 / 47.68 | 0.422 [0.404, 0.446] | 0.981 [0.957, 0.984] | 0.398 [0.384, 0.420] | 0.669 [0.669, 0.672] |
| quicksort | 206.68 / 41.46 | 0.604 [0.501, 0.686] | 0.985 [0.978, 0.987] | 0.473 [0.455, 0.533] | 0.608 [0.605, 0.608] |
| TSF | 14.73 / 224.47 | 0.076 [0.069, 0.079] | 1.881 [1.838, 1.890] | 0.077 [0.074, 0.080] | 1.883 [1.783, 1.903] |
| zlib | 13.65 / 112.55 | 0.154 [0.150, 0.155] | 1.031 [1.029, 1.032] | 0.159 [0.152, 0.165] | 1.224 [1.194, 1.232] |

The direct entry path preserves gcc-loops and quicksort's existing RSS wins,
but neither reaches a Score win. HashSet, TSF, and zlib lose both metrics
against at least one comparator. This change is not sufficient to meet the
JIT goal.

## Provenance

The production binary was built with `/workspace/quench-build-env.sh`, which
selects rustc 1.99.1. The benchmark process did not have `/workspace/.cargo/bin`
on its `PATH`, so its raw report left `host.rustc` empty and the analyzer
rejected that report before comparing fixtures. `report-raw.json` preserves
the original bytes. `report.json` differs only by adding the exact `rustc -Vv`
output from the same build environment to `host.rustc`; the raw samples,
command, executable hashes, source revision, and engine environments are
unchanged. The harness analyzer accepted the provenance-enriched report and
reported the per-fixture failures shown above. `rustc-Vv.txt` records the
added host metadata, and `analysis.json` is the analyzer output.

## Next target

HashSet's 49 million native entries are reached through `CallKnown` from an
interpreted `main` function rejected at `WasmUnreachable@66107`. The indirect
entry shortcut does not remove that interpreter-to-native transition. The
next experiment should target the known-callee boundary or test a narrowly
selected `main` plus its existing native `CallKnown` closure, with a strict
emitted-code budget. The previous global `WasmUnreachable` experiment grew
TSF's emitted code about 52 times and worsened both Score and RSS, so global
admission is not supported by these results.
