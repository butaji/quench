# Linux no-JIT matrix with short-process RSS sampling

Diagnostic 11-round refresh of the complete package no-JIT matrix after the
runner's process-tree traversal and 5 ms RSS sampling change. Every raw report
has `qualificationReady: false`; these results are not gate passes.

## Exact inputs

- Package lane source revision: `de47be58bd867d2edcf56dbfbdc15f7bd90eb411`
- Quench production binary SHA-256:
  `3010c9f68840ad8c451a8c487754d099de3ba2ff9d82823258027772f9c997ef`
- Node: `v24.19.0`; `--jitless` for no-JIT rows
- Bun: `1.4.4`; no-JIT option confirmed by the runner
- Host: Linux x64, 5 visible CPUs
- Package lock SHA-256:
  `92dbfcb2312b42ddf55a8f0b06c40368544ca4b5e861782ead6770268da3c1b0`
- Sampler: 5 ms. Node `--jitless` is omitted for Vite and Undici because their
  pinned WebAssembly dependencies cannot load in that mode.
- All workload outputs/checksums matched across engines and all 11 reports
  completed. The report schema retains per-round commands, hashes, environment,
  throughput and RSS details.

## Results

The unchanged gate requires Quench to reach at least 1.20× the fastest valid
peer's throughput and at most 0.80× the lowest valid peer's maximum RSS in each
row. No row meets the speed gate. The RSS ratios below use the 5 ms engine
process HWM; descendant/tree aggregation is still diagnostic, so no memory pass
is claimed.

| Workload | Quench ops/s | Fastest peer ops/s | Speed ratio | Quench root HWM | Lowest peer root HWM | RSS ratio |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| esbuild transform | 419 | 1,671 (Bun) | 0.251 | 39.7 MiB | 47.1 MiB | 0.843 |
| Fastify JSON HTTP | 200 | 4,219 (Node) | 0.047 | 47.3 MiB | 46.7 MiB | 1.013 |
| Fastify JSON inject | 5,501 | 7,364 (Bun) | 0.747 | 49.3 MiB | 44.8 MiB | 1.101 |
| Next route match | 172,414 | 500,000 (Node) | 0.345 | 22.5 MiB | 25.7 MiB | 0.875 |
| Pino JSON logger | 59,524 | 128,205 (Node) | 0.464 | 33.0 MiB | 29.5 MiB | 1.119 |
| React SSR | 1,383 | 3,222 (Node) | 0.429 | 36.9 MiB | 34.4 MiB | 1.073 |
| sharp resize concurrent | 867 | 899 (Node) | 0.965 | 50.3 MiB | 65.9 MiB | 0.763 |
| Undici pool | 4,448 | 4,960 (Bun) | 0.897 | 51.0 MiB | 47.0 MiB | 1.085 |
| Vite transform | 20,080 | 26,316 (Bun) | 0.763 | 60.1 MiB | 63.1 MiB | 0.952 |
| WebSocket echo | 9,615 | 100,000 (Bun) | 0.096 | 32.3 MiB | 30.4 MiB | 1.062 |
| Zod parse | 109,290 | 253,165 (Node) | 0.432 | 26.7 MiB | 34.1 MiB | 0.783 |

Sharp and Zod are below the diagnostic 0.80 RSS ratio; this does not qualify
them because descendant aggregation remains diagnostic. Throughput fails all
11 workload rows by substantial margins. All packages and both JIT modes remain
in scope.

The same-day Fastify-free loopback echo experiment is in the parent report
directory. It confirms that Quench's severe Fastify HTTP gap is present in the
generic transport path; transport implementation belongs to the web-runtime
lane. Zod/Pino OSR fixtures remain prerequisites for the shared JIT owner and
do not claim performance wins.
