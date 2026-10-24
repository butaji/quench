# TOD-173 refreshed package matrix

Date: 2026-10-24

Branch HEAD: `8806091f3f635ecf338d8d909eea746dee62d578`

Production binary SHA-256: `53a0ff44f4f8f2759b3716c36aa1d2e240182671d4a3a40fdfa25a3a96aabbd3`

Node: `v24.19.0`; Bun: `1.4.4`

Host: `Linux 54f8de2cc531 6.18.55 #1 SMP Wed Oct 14 19:45:18 UTC 2026 x86_64 GNU/Linux`

Quench JIT runs used `QUENCH_JS_JIT_GENERAL_DISPATCH=1`. Each row is an independent 11-round end-to-end report with output parity checked. The report files include source/lock hashes, exact commands, engine versions and per-sample RSS. Linux process-tree RSS is diagnostic, so these are not platform-qualified RSS results.

The 20% gates require at least 1.20x throughput and at most 0.80x maximum RSS versus every peer. The table uses the fastest peer for throughput and the lowest-RSS peer for the more demanding memory comparison.

| Workload | Mode | Throughput margin | RSS reduction | Both 20% gates |
|---|---|---:|---:|---|
| react-ssr | no-jit | -57.6% vs node-jitless | -10.9% vs node-jitless | FAIL |
| react-ssr | jit | -96.8% vs node-jit | +48.1% vs node-jit | FAIL |
| next-route-match | no-jit | -67.2% vs node-jitless | -3944.7% vs node-jitless | FAIL |
| next-route-match | jit | -86.7% vs bun-jit | -2242.9% vs node-jit | FAIL |
| vite-transform | no-jit | -24.6% vs bun-no-jit | +13.9% vs bun-no-jit | FAIL |
| vite-transform | jit | -73.0% vs bun-jit | +28.2% vs bun-jit | FAIL |
| esbuild-transform | no-jit | -75.6% vs bun-no-jit | +10.8% vs bun-no-jit | FAIL |
| esbuild-transform | jit | -77.5% vs bun-jit | +22.3% vs bun-jit | FAIL |
| sharp-resize-concurrent | no-jit | -3.0% vs node-jitless | +23.1% vs bun-no-jit | FAIL |
| sharp-resize-concurrent | jit | -10.4% vs node-jit | +16.0% vs bun-jit | FAIL |
| fastify-json-http | no-jit | -95.5% vs node-jitless | -4.6% vs bun-no-jit | FAIL |
| fastify-json-http | jit | -99.4% vs bun-jit | +31.8% vs node-jit | FAIL |
| fastify-json-inject | no-jit | -26.6% vs bun-no-jit | -9.9% vs bun-no-jit | FAIL |
| fastify-json-inject | jit | -94.0% vs bun-jit | +39.9% vs bun-jit | FAIL |
| undici-pool | no-jit | -15.3% vs bun-no-jit | -1.2% vs bun-no-jit | FAIL |
| undici-pool | jit | -94.0% vs bun-jit | +46.8% vs node-jit | FAIL |
| ws-echo | no-jit | -90.7% vs bun-no-jit | -1915.4% vs bun-no-jit | FAIL |
| ws-echo | jit | -95.5% vs bun-jit | -1218.1% vs bun-jit | FAIL |
| pino-json-logger | no-jit | -54.8% vs node-jitless | -20.4% vs node-jitless | FAIL |
| pino-json-logger | jit | -88.0% vs bun-jit | -974.1% vs bun-jit | FAIL |
| zod-parse | no-jit | -56.9% vs node-jitless | +22.4% vs bun-no-jit | FAIL |
| zod-parse | jit | -93.0% vs bun-jit | -847.1% vs bun-jit | FAIL |

No-JIT Node is unsupported for Vite and Undici in Node 24.19.0: `--jitless` removes the `WebAssembly` global required by the pinned package paths. Those two reports compare Quench with Bun and record the Node omission.

Overall package/mode gate: **NOT MET**.
