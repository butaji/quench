# Wasm indirect-call cache

Commit `f40e5ddeda1765ace5506af81a37fddb1b6de2b0` adds a monomorphic cache per compiled Wasm `call_indirect` site. It guards the caller site, table slot and binding, index, signature, environment, and current table element. Cache entries weakly reference the target artifact; each hit rebuilds the activation's memory and global pointers. Cache misses use the existing resolver, retaining the bounds, null, signature, and fallback checks. GC clears cached identities before heap slots can be reused.

The clean-source JetStream 3 screen ran 11 paired rounds on Linux x86_64 with Quench `--jit`, Bun 1.4.3 with JIT enabled, and Node v26.12.0. Both fixtures validated and produced equal output. The harness marked the report complete, but the analyzer correctly did not qualify it: Quench loses Score on both fixtures, despite lower maximum RSS. This is a two-fixture screen, not full-suite qualification.

| Fixture | Runtime | Median Score | Median max RSS |
| --- | --- | ---: | ---: |
| QuickSort Wasm | Quench | 190.49 | 43.07 MiB |
|  | Bun | 345.96 | 43.89 MiB |
|  | Node | 405.88 | 74.11 MiB |
| gcc-loops Wasm | Quench | 64.47 | 48.19 MiB |
|  | Bun | 186.70 | 50.99 MiB |
|  | Node | 192.71 | 77.32 MiB |

Paired 95% bootstrap intervals for Quench/comparator ratios:

| Fixture | Comparator | Score ratio | Max RSS ratio |
| --- | --- | ---: | ---: |
| QuickSort | Bun | 0.569 [0.520, 0.606] | 0.982 [0.976, 0.987] |
| QuickSort | Node | 0.465 [0.416, 0.500] | 0.581 [0.579, 0.583] |
| gcc-loops | Bun | 0.340 [0.324, 0.373] | 0.945 [0.929, 0.957] |
| gcc-loops | Node | 0.342 [0.329, 0.350] | 0.624 [0.623, 0.626] |

The full report, executable and corpus provenance, rounds, raw samples, and analyzer output are in `report.json` and `analysis.json`. Validation used `cargo +1.99.1 test --locked -p quench-runtime --lib` (711 passed) and `cargo +1.99.1 build --locked --profile production -p quench-node -p quench-bench`.
