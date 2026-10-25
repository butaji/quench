# Shared JIT merge screen

Date: 2026-10-25 UTC  
Branch: `v2-zod`, merge commit `7f3372f29`  
Candidate production binary SHA-256: `6f7c7601e6bcecc68b74266b8a6160869279051a114ced514f33a5fb3644879a`  
Pre-merge control binary SHA-256: `44f127af7abf4682b56600b97c5640389fc40559658d5e7467bf80a4f58a5de0`

The candidate was built with `tools/pgo-build.sh production`. The training profile used the repository's general runtime curriculum and Node compatibility fixtures; the npm performance workloads were excluded. Each JIT row below is an interleaved 3-round diagnostic against the saved pre-merge binary, Node, and Bun. Every engine's observable output matched in each report. The reports set `qualificationReady=false`; three rounds are a regression screen, and sampled Linux process-tree RSS is diagnostic only.

| Scenario | JIT throughput vs control | vs fastest peer | RSS vs lowest-RSS peer |
| --- | ---: | ---: | ---: |
| react-ssr | +14.5% | -96.6% | -50.8% |
| next-route-match | +12.3% | -82.5% | +575.6% |
| vite-transform | +1.2% | -72.2% | -24.5% |
| esbuild-transform | +0.3% | -77.4% | -22.5% |
| sharp-resize-concurrent | -4.4% | -5.2% | -34.3% |
| fastify-json-http | -0.6% | -99.3% | -39.5% |
| fastify-json-inject | +6.7% | -92.2% | -43.6% |
| undici-pool | +10.7% | -90.5% | -53.8% |
| ws-echo | +2.6% | -93.9% | +207.9% |
| pino-json-logger | +9.6% | -84.3% | +159.0% |
| zod-parse | +12.0% | -91.6% | +183.9% |

“RSS vs lowest-RSS peer” compares candidate Quench RSS with the peer using less RSS; positive values mean Quench used more. The JIT implementation improves several candidate/control pairs, but no scenario reaches the 20% throughput target over peers. Four scenarios also use more RSS than the lower-RSS peer by a wide margin. The complete reports contain exact versions, commands, input hashes, samples, and outputs.

## Coverage findings

Separate `QUENCH_JS_JIT_STATS=1` runs show no native entries or OSR runs for React or Zod's measured functions. In the Zod workload, `operations` executes 36,754 dynamically counted general-dispatch opcodes; its specialized compiler attempt fails with `unsupported-lowering-shape-or-codegen-failure`, and no general-dispatch compile attempt occurs. Pino records 26,219 native entries in the small `get hasSubscribers` getter, while `_asJson` contributes 10,477 dynamically counted opcodes and remains lane-only. These diagnostics point to whole-function/lane coverage for calls, schema/render branches, and effectful helpers as the next shared JIT gap.

The JIT merge itself brings shared OSR field and call lowering, exact recovery oracles, and receiver/field-cache guards. It is in the common backend; this lane does not add package-specific lowering.

## No-JIT screen

Three-round candidate/control checks for React, Zod, and Pino are saved as `react-nojit-3.json`, `zod-nojit-3.json`, and `pino-nojit-3.json`. Throughput changes versus control are +1.6%, +0.5%, and +4.9%, respectively. The 11-scenario current-build no-JIT screen below is one round per runtime and is also diagnostic. Node `--jitless` is omitted for Vite and Undici because Node 24.19.0 removes the WebAssembly global those packages require.

| Scenario | No-JIT throughput vs control | vs fastest available peer | RSS vs lowest-RSS peer |
| --- | ---: | ---: | ---: |
| react-ssr | +0.9% | -56.1% | +9.3% |
| next-route-match | 0.0% | -66.7% | +464.9% |
| vite-transform | -2.0% | -27.0% | -4.3% |
| esbuild-transform | -1.8% | -76.2% | -11.6% |
| sharp-resize-concurrent | -3.8% | -4.2% | -23.9% |
| fastify-json-http | -2.6% | -95.5% | +1.5% |
| fastify-json-inject | +2.7% | -28.3% | +6.8% |
| undici-pool | +8.3% | -9.7% | +5.1% |
| ws-echo | +3.7% | -90.7% | +225.0% |
| pino-json-logger | +4.9% | -51.2% | +20.4% |
| zod-parse | +8.9% | -57.4% | -23.6% |

All reports enforce output parity. Every scenario misses the 20% throughput margin over at least one available peer. RSS numbers are sampled Linux process-tree values and do not qualify the memory gate.
