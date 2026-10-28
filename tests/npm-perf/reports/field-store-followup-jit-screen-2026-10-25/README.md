# Tagged OSR field-store follow-up screen

Five-round, interleaved JIT package diagnostics on Linux x86_64 after adding
unchanged tagged-value stores and owner write barriers. Candidate binary:
`bdb8322cef27c4013791605d8c34454b09fe89074d661b23c741c2b72ad39dd6`.
Node was v24.19.0 and Bun was v1.4.4. Checksums matched for all rows.

| Workload | Quench ops/s | Node ops/s | Bun ops/s | Quench sampled RSS MiB |
| --- | ---: | ---: | ---: | ---: |
| React SSR | 1,315 | 42,017 | 35,211 | 24.54 |
| Pino JSON logger | 56,818 | 357,143 | 384,615 | 19.50 |
| Zod parse | 105,263 | 800,000 | 1,052,632 | 20.18 |

The paired Quench control is the saved binary with hash
`6f7c7601e6bcecc68b74266b8a6160869279051a114ced514f33a5fb3644879a`, which
predates other changes in the current merged source. Candidate/control deltas
therefore do not isolate the tagged-store change and are diagnostic only.
The RSS sampler sums process-tree `VmRSS` every 50 ms; it can miss short-lived
children and counts shared pages per process. Very short Bun samples especially
understate RSS. None of these rows qualifies for the throughput or max-RSS gate.

Fresh-binary `QUENCH_JS_JIT_STATS=1` runs (500 workload rounds, 50 warmups;
250 React rounds) found zero field-store cache-hit functions and zero native
OSR regions containing `SetField`/`SetFieldStrict` in all three packages. The
new tagged-store path is verified by the runtime regression, but these packages
do not exercise it. No package gain is claimed.
