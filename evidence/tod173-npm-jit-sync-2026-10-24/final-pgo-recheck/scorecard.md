# TOD-173 final PGO recheck

Date: 2026-10-24

Production binary SHA-256: `0fd1ddc4dee59d52040f8abeaf4c14bb337c663bdffd72be265da28349348616`

Node: `v24.19.0`; Bun: `1.4.4`

The 10 workloads were run for 11 matched rounds in no-JIT and JIT modes. All 22 reports use the same clean PGO production binary, trained on the general runtime curriculum, and verify deterministic output parity. The no-JIT reports were rerun with Node included in all compatible workloads; Node 24.19.0 cannot run the pinned Vite and Undici package paths under `--jitless` because that mode removes the WebAssembly global, so those two rows compare Quench and Bun.

The 20% gates require at least 1.20x the throughput and at most 0.80x maximum RSS of each peer. Margins below compare throughput against the fastest peer and RSS reduction against the lowest-RSS peer. Linux process-tree RSS is diagnostic only, so it does not qualify the memory gate.

| Workload | Mode | Throughput margin | RSS reduction | Both 20% gates | Report |
|---|---|---:|---:|---|---|
| react-ssr | no-jit | -56.5% vs node-jitless | -9.4% vs node-jitless | FAIL | [`react-ssr-no-jit-11.json`](react-ssr-no-jit-11.json) |
| react-ssr | jit | -96.8% vs node-jit | +47.9% vs node-jit | FAIL | [`react-ssr-jit-11.json`](react-ssr-jit-11.json) |
| next-route-match | no-jit | -67.8% vs node-jitless | -799.5% vs node-jitless | FAIL | [`next-route-match-no-jit-11.json`](next-route-match-no-jit-11.json) |
| next-route-match | jit | -88.2% vs bun-jit | -1088.5% vs node-jit | FAIL | [`next-route-match-jit-11.json`](next-route-match-jit-11.json) |
| vite-transform | no-jit | -21.0% vs bun-no-jit | +12.3% vs bun-no-jit | FAIL | [`vite-transform-no-jit-11.json`](vite-transform-no-jit-11.json) |
| vite-transform | jit | -72.1% vs bun-jit | +28.2% vs bun-jit | FAIL | [`vite-transform-jit-11.json`](vite-transform-jit-11.json) |
| esbuild-transform | no-jit | -75.4% vs bun-no-jit | +11.2% vs bun-no-jit | FAIL | [`esbuild-transform-no-jit-11.json`](esbuild-transform-no-jit-11.json) |
| esbuild-transform | jit | -77.0% vs bun-jit | +22.3% vs bun-jit | FAIL | [`esbuild-transform-jit-11.json`](esbuild-transform-jit-11.json) |
| sharp-resize-concurrent | no-jit | -0.8% vs node-jitless | +21.7% vs bun-no-jit | FAIL | [`sharp-resize-concurrent-no-jit-11.json`](sharp-resize-concurrent-no-jit-11.json) |
| sharp-resize-concurrent | jit | -6.9% vs node-jit | +20.0% vs bun-jit | FAIL | [`sharp-resize-concurrent-jit-11.json`](sharp-resize-concurrent-jit-11.json) |
| fastify-json-http | no-jit | -95.4% vs node-jitless | -4.7% vs bun-no-jit | FAIL | [`fastify-json-http-no-jit-11.json`](fastify-json-http-no-jit-11.json) |
| fastify-json-http | jit | -99.3% vs bun-jit | +31.8% vs node-jit | FAIL | [`fastify-json-http-jit-11.json`](fastify-json-http-jit-11.json) |
| fastify-json-inject | no-jit | -26.1% vs bun-no-jit | -9.8% vs bun-no-jit | FAIL | [`fastify-json-inject-no-jit-11.json`](fastify-json-inject-no-jit-11.json) |
| fastify-json-inject | jit | -94.2% vs bun-jit | +39.6% vs bun-jit | FAIL | [`fastify-json-inject-jit-11.json`](fastify-json-inject-jit-11.json) |
| undici-pool | no-jit | -14.6% vs bun-no-jit | -5.1% vs bun-no-jit | FAIL | [`undici-pool-no-jit-11.json`](undici-pool-no-jit-11.json) |
| undici-pool | jit | -93.6% vs bun-jit | +47.7% vs node-jit | FAIL | [`undici-pool-jit-11.json`](undici-pool-jit-11.json) |
| ws-echo | no-jit | -90.7% vs bun-no-jit | -422.1% vs bun-no-jit | FAIL | [`ws-echo-no-jit-11.json`](ws-echo-no-jit-11.json) |
| ws-echo | jit | -95.4% vs bun-jit | -525.1% vs bun-jit | FAIL | [`ws-echo-jit-11.json`](ws-echo-jit-11.json) |
| pino-json-logger | no-jit | -54.1% vs node-jitless | -20.1% vs node-jitless | FAIL | [`pino-json-logger-no-jit-11.json`](pino-json-logger-no-jit-11.json) |
| pino-json-logger | jit | -87.8% vs bun-jit | -471.3% vs bun-jit | FAIL | [`pino-json-logger-jit-11.json`](pino-json-logger-jit-11.json) |
| zod-parse | no-jit | -57.8% vs node-jitless | +21.3% vs bun-no-jit | FAIL | [`zod-parse-no-jit-11.json`](zod-parse-no-jit-11.json) |
| zod-parse | jit | -92.9% vs bun-jit | -384.1% vs bun-jit | FAIL | [`zod-parse-jit-11.json`](zod-parse-jit-11.json) |

Overall package/mode gate: **NOT MET**. Zero of 22 rows pass both numerical gates; RSS also remains unqualified on this Linux sampler.

## Reproduction

Run from `tests/npm-perf` with the recorded binary paths and environment in each JSON report:

```sh
node run-workload.mjs <workload> no-jit 11 <report.json>
node run-workload.mjs <workload> jit 11 <report.json>
```

Each JSON report contains versions, commands, exact inputs and lockfile hashes, per-round throughput and RSS, output parity, and the production binary hash.
