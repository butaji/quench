# Hot legacy candidate admission trial

This was a screening trial for measuring general-dispatch work in hot legacy JIT candidates while retaining the existing compile-price gate. It expanded the tracked frame set beyond the targeted sampler and is rejected.

The 10 JIT reports contain 11 interleaved rounds each, output-parity checks, and per-process throughput and RSS. The candidate binary was `5b0e3d97e0f561d9fb88a4455a7b8490cf4fa5eb0637d3bbbe765886e933a418`; the control binary was `0fd1ddc4dee59d52040f8abeaf4c14bb337c663bdffd72be265da28349348616`. The PGO profile predated this trial's one-line admission change, and rustc reported a function control-flow hash mismatch, so these reports are diagnostic screening evidence rather than a qualified final build.

| Workload | Throughput vs fastest peer | Throughput vs control | Paired wins vs control | RSS vs largest peer* |
|---|---:|---:|---:|---:|
| esbuild-transform | -77.4% | -0.0% | 5/11 | -46.2% |
| fastify-json-http | -99.3% | +0.3% | 5/11 | -49.3% |
| fastify-json-inject | -94.5% | -0.6% | 6/11 | -63.3% |
| next-route-match | -88.1% | -1.2% | 1/11 | +196.5% |
| pino-json-logger | -88.7% | +1.0% | 7/11 | -34.2% |
| react-ssr | -96.8% | -0.1% | 7/11 | -72.4% |
| sharp-resize-concurrent | -6.1% | +0.1% | 5/11 | -60.0% |
| undici-pool | -94.2% | -3.0% | 2/11 | -54.7% |
| vite-transform | -73.6% | -3.7% | 1/11 | -31.5% |
| ws-echo | -96.1% | -2.6% | 2/11 | -47.2% |

No JIT row reached the required +20% throughput margin. Nine rows appeared below 80% of the largest peer's sampled RSS; `next-route-match` used 2.97x the largest peer RSS. Linux process-tree RSS is diagnostic only, so none of these memory results qualify the RSS gate.

This admission expansion was reverted. The working tree retains the narrower targeted work sampler. The no-JIT path was not changed by this trial; use [`final-pgo-recheck/scorecard.md`](../final-pgo-recheck/scorecard.md) for the unchanged no-JIT baseline.

\* RSS is sampled process-tree RSS from `/proc/<pid>/status` every 50 ms; it may miss short-lived children and counts shared pages per process.
