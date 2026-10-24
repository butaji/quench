# Fresh ten-package benchmark baseline

Built the current production PGO binary at `4e7c5bf53` after fast-forwarding `v2-zod` to `origin/v2-merged`. Host: Linux x86_64, 5 logical CPUs. Node `v24.19.0`; Bun `1.4.4`. Each report has 11 interleaved rounds, input and binary hashes, output checksum, and sampled process-tree RSS. The shared binary SHA-256 is `5ee06f9c402dec175c602b4b8a5cccce2d8ba05b64e6804f26c3529db2b75825`.

**Gate:** Quench must reach at least `1.20 ×` the faster peer throughput and at most `0.80 ×` the lower peer max RSS, separately for each package and phase. These Linux `VmRSS` samples are diagnostic (`qualificationReady: false`), not platform qualification.

| Workload | Phase | Quench ops/s / MiB | Node ops/s / MiB | Bun ops/s / MiB | Throughput / fastest peer | RSS / lowest peer |
|---|---|---:|---:|---:|---:|---:|
| React / react-dom | no-jit | 876 / 37.46 | 2,107 / 33.95 | 1,990 / 38.79 | 0.42× | 1.10× |
| React / react-dom | jit | 870 / 24.86 | 25,641 / 48.38 | 25,253 / 96.01 | 0.03× | 0.51× |
| Next | no-jit | 9,124 / 57.32 | 32,258 / 66.77 | 138,889 / 38.95 | 0.07× | 1.47× |
| Next | jit | 9,259 / 41.14 | 71,429 / 83.32 | 312,500 / 5.55 | 0.03× | 7.42× |
| Vite | no-jit | 10,040 / 52.96 | unavailable | 14,368 / 65.10 | 0.70× | 0.81× |
| Vite | jit | 10,000 / 53.87 | 34,965 / 74.96 | 38,462 / 75.34 | 0.26× | 0.72× |
| esbuild | no-jit | 370 / 58.20 | 1,157 / 93.82 | 1,213 / 67.72 | 0.31× | 0.86× |
| esbuild | jit | 380 / 58.19 | 1,303 / 107.39 | 1,415 / 79.17 | 0.27× | 0.74× |
| sharp | no-jit | 687 / 50.48 | 712 / 140.70 | 704 / 66.65 | 0.96× | 0.76× |
| sharp | jit | 696 / 50.97 | 713 / 152.64 | 729 / 75.16 | 0.95× | 0.68× |
| Fastify HTTP | no-jit | 579 / 46.45 | 2,710 / 50.53 | 2,660 / 49.01 | 0.21× | 0.95× |
| Fastify HTTP | jit | 589 / 43.80 | 5,000 / 70.26 | 5,208 / 78.28 | 0.11× | 0.62× |
| Fastify inject | no-jit | 2,653 / 51.56 | 4,878 / 54.43 | 5,263 / 47.64 | 0.50× | 1.08× |
| Fastify inject | jit | 2,445 / 44.10 | 7,752 / 77.08 | 9,346 / 73.59 | 0.26× | 0.60× |
| Undici | no-jit | 2,299 / 41.71 | unavailable | 3,397 / 49.20 | 0.68× | 0.85× |
| Undici | jit | 2,270 / 41.11 | 17,921 / 88.66 | 21,008 / 95.62 | 0.11× | 0.46× |
| ws | no-jit | 4,808 / 31.43 | 15,873 / 32.74 | 62,500 / 7.08 | 0.08× | 4.44× |
| ws | jit | 4,739 / 25.91 | 29,412 / 36.15 | 100,000 / 6.19 | 0.05× | 4.19× |
| Pino | no-jit | 36,765 / 33.28 | 89,286 / 29.52 | 70,423 / 34.43 | 0.41× | 1.13× |
| Pino | jit | 36,765 / 19.94 | 238,095 / 37.72 | 277,778 / 6.12 | 0.13× | 3.26× |
| Zod | no-jit | 69,444 / 26.80 | 175,439 / 48.64 | 127,389 / 37.01 | 0.40× | 0.72× |
| Zod | jit | 65,574 / 21.34 | 571,429 / 50.45 | 689,655 / 47.59 | 0.10× | 0.45× |

No workload clears both gates in both phases. Sharp resize is closest on throughput, within about 4% of Node/Bun, and its sampled RSS is already below the 20% target versus Bun; it still misses the required 20% throughput lead. React, Pino, and Zod JIT remain far behind on throughput. Node `--jitless` could not load Vite/Oxc or Undici/llhttp WebAssembly and is explicitly marked unavailable in those no-JIT reports.

## Extended scenario coverage

Fresh 11-round reports also cover Next route matching and `Response.json()`,
sharp metadata, plus longer Pino, WebSocket, and Zod runs. The extended route
matcher measured 120,482 ops/s no-JIT and 114,548 JIT for Quench; Node measured
349,650 / 1,020,408 and Bun 267,380 / 1,204,819. Extended `Response.json()`
was 20,517 no-JIT and 20,636 JIT for Quench, versus Node 59,524 / 132,802 and
Bun 746,269 / 1,351,351. Sharp metadata measured 5,411 no-JIT and 5,714 JIT
for Quench, versus Node 6,127 / 6,831 and Bun 8,503 / 9,785. Extended Pino
JIT measured 37,764 records/s for Quench, versus Node 314,465 and Bun 409,836.
These extra scenarios also miss the speed gate. Reports live beside the main
rows as `*-50k-11.json`, `*-100k-11.json`, `*-50000-11.json`,
`*-20000-11.json`, `*-10000-11.json`, and `sharp-metadata-*-11.json`.

The longer JIT reruns put Bun's `Response.json()`, route matcher, and Pino
samples at 74 ms, 83 ms, and 122 ms, respectively; Zod's 20,000-round Bun run
was 71 ms. Those remove the earlier timer-floor artifact, but RSS remains
diagnostic because Linux samples `VmRSS` every 50 ms. All Linux RSS
measurements remain non-qualifying sampled diagnostics.

## Current JIT coverage evidence

`QUENCH_JS_JIT_STATS=1` on the same production binary found 0 native entries across 58 React functions and 214 Zod functions. Pino had 26,219 native entries, all from one `diagnostics_channel` getter, and one successful compile; its hot `logRequest` function remained unsupported. Detailed per-function counters are in `jit-coverage-react-zod-pino.json`. This points to broader effectful field/call compilation as the common throughput blocker.

## Rejected activation-copy candidate

I screened a production PGO change that avoids copying promoted parameters into
local slots that activation setup immediately resets. The runtime suite passed
908/908 tests. Eleven paired rounds showed small, inconsistent changes: React
improved 2.5% no-JIT and 4.3% JIT versus the saved `v2-merged` control (8/11
rounds faster in each mode), while Zod was flat (1.000× no-JIT, 0.998× JIT)
and Pino lost 4.1% no-JIT and was flat in JIT. Candidate/control RSS ratios
ranged from 0.994× to 1.011×. The broad change was reverted because it missed
the ≥2% practical cutoff on Zod and Pino and did not close any package gap.
Candidate binary SHA-256 was
`ba13e9947e8de1864b8b07dfea5f031dd2e8c32a34c2f11b949f26691d7095f0`; control
SHA-256 was `5ee06f9c402dec175c602b4b8a5cccce2d8ba05b64e6804f26c3529db2b75825`.
The six paired reports beginning with `activation-` are in this directory.

## Raw reports

All package and phase reports are the `*-no-jit-11.json` and `*-jit-11.json` files in this directory.
