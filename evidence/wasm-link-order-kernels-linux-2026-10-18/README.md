# Wasm kernels in the static link-order corpus (Linux x86_64, 2026-10-18)

The static `quench-node` order (`crates/quench-node/link/x86_64-linux-static.order`) was
trained on JavaScript only, so the interpreter's Wasm handlers and the Cranelift pipeline
were left wherever the linker put them, and a Wasm run faulted in code scattered across the
binary. `quench-bench/wasm-micros/kernels.cjs` is a set of generic Wasm kernels (integer and
floating-point loops, every memory width, direct and indirect calls, a `br_table` state
machine, globals, `memory.grow`), built with V8's `wasm-module-builder.js`. It is never a
scored module (decision 13): no CoreMark profile is in the corpus.

The recipe (`evidence/tod123-rss-linux-2026-10-14/README.md`, "Run-phase layout") now runs
the kernels without and with `--jit` as the last start-up profiles. Profiles are laid out in
first-appearance order, so the corpus's existing layout is unchanged and only functions it
never executed are added: 3,947 symbols, 442 of them C code (OpenSSL among it), placed ahead with the rest by `--foreign-first`. Commands:

```sh
for mode in no-jit jit; do
  valgrind --tool=cachegrind --cache-sim=no --demangle=no \
    --cachegrind-out-file=train/wasm-kernels-$mode.out \
    $BINARY $([ $mode = jit ] && echo --jit) quench-bench/wasm-micros/kernels.cjs 3
done
```

The kernels print `wasm kernels 3 rounds: 819680534` on Node 26.12.0 and on Quench in both
modes.

## Max RSS

Host: Intel Xeon @ 2.10 GHz, 4 vCPUs, 15 GiB, Linux 6.18.44, no cgroup limits. Both
binaries are the static production build of `v2-merged` `948f03f99`, without BOLT,
differing only in the order; each run is preceded by evicting both binaries from the page
cache (`evict.py`) and runs under `maxrss.c`. Five interleaved runs, KB, medians
(`maxrss-raw.txt`):

| Probe | Previous order | With kernels |
| --- | ---: | ---: |
| Empty start | 3,972 | 3,716 |
| CoreMark instantiation, no JIT | 10,132 | 9,540 |
| CoreMark instantiation, JIT | 10,132 | 9,540 |
| CoreMark full run, no JIT | 10,140 | 9,708 |
| CoreMark full run, JIT | 12,504 | 11,628 |
| Richards | 4,068 | 3,832 |
| DeltaBlue | 4,128 | 3,892 |
| Crypto | 4,644 | 4,412 |
| RayTrace | 4,136 | 3,900 |
| EarleyBoyer | 8,204 | 8,240 |
| RegExp | 8,036 | 7,916 |
| Splay | 39,684 | 39,716 |
| NavierStokes | 4,496 | 4,344 |

CoreMark is `evidence/wasm-coremark-quench-node-linux-2026-10-17/run-js.mjs`; the V8-v7
fixtures are stock `base.js` + the bench + `run.js`'s runner, one bench per process.
EarleyBoyer and Splay are within run-to-run spread. The JavaScript gains are most likely
start-up functions added since the JS order was trained, which the kernels also execute
(not verified per symbol); a full retrain by the RSS lane remains the authority for the
JS layout. The dynamic order (`x86_64-linux.order`) is not retrained here.
