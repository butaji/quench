# Persistent monomorphic indirect-call cache

These reports measure the dirty `v2-jit` working tree with opt-in artifact-local native caches for stable Wasm `call_indirect` targets. The compiler keeps a slow resolver for misses, and the fast path checks the cached VM epoch, instance environment, dynamic table index, artifact function range, and non-null entry. The VM advances its epoch after table mutations and garbage collection. Cache entries are published only after a successful same-artifact call and a final table-slot recheck.

The 11-round report is a subset screen, not a JetStream qualification: it contains only the five Wasm fixtures that previously won both metrics in the 2026-10-16 no-JIT qualification, so the runner marks `qualification_ready: false`. All five fixtures have 11 valid samples, equal outputs, and validated JetStream results.

| Fixture | Quench JIT Score / RSS MiB | Bun JIT | Node JIT |
| --- | ---: | ---: | ---: |
| HashSet | 32.94 / 47.94 | 207.06 / 54.51 | 247.64 / 127.17 |
| gcc-loops | 54.80 / 48.01 | 185.36 / 54.05 | 187.07 / 92.08 |
| QuickSort | 234.88 / 42.84 | 362.31 / 47.18 | 464.30 / 88.51 |
| TSF | 11.74 / 225.62 | 213.42 / 126.09 | 204.22 / 145.27 |
| zlib | 14.10 / 112.63 | 90.05 / 114.06 | 88.62 / 113.51 |

Quench retains an RSS advantage over Bun on HashSet, gcc-loops, QuickSort, and zlib; it loses Score on every fixture. TSF remains the memory outlier. These medians do not establish a score-and-RSS win in JIT mode.

The separate one-round diagnostic report is attribution data, not timing evidence. It records one reference resolver callout per QuickSort artifact for the hot function-4 `pc 38` site targeting function 5; before the native cache this site generated thousands of resolver callouts. Thousands of imported calls remain and are the next QuickSort/GCC boundary cost. The five-fixture report's medians are 32.94 / 47.94 MiB (HashSet), 54.80 / 48.01 (gcc-loops), 234.88 / 42.84 (QuickSort), 11.74 / 225.62 (TSF), and 14.10 / 112.63 (zlib).

Cache publication also retains a strong heap root across the native target call. A re-entrant call can mutate a table and collect before returning, so publication rechecks that the rooted live cell is still the expected function and that the current table slot still points to it. This guards against heap-slot reuse while the target runs; the artifact's executable mapping remains strongly held by its `Rc`.

Run metadata and raw samples are in `five-fixture-11-round.json`; the one-round callout diagnostics are in `five-fixture-diagnostics.json`. The source revision is `3dda94daca038690b5e59498692023f945efa272` with a dirty worktree, as recorded in each report.
