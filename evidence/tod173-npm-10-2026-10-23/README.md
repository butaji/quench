# Ten-package performance sweep

Date: 2026-10-23. Branch: `v2-zod`. Host: Linux x64, 5 CPUs. Node v24.19.0,
Bun v1.4.4. All comparisons use five interleaved runs and output-parity checks.
Node no-JIT means `node --jitless`; Bun no-JIT sets and verifies
`BUN_JSC_useJIT=0`; Quench no-JIT uses its interpreter. JIT mode uses each
engine's normal JIT-enabled configuration.

These are diagnostics, not qualification: RSS is a 50 ms sampled process-tree
sum of `/proc/<pid>/status` `VmRSS`, and the reports say
`qualificationReady: false`. A win requires at least 20% higher throughput and
20% lower maximum RSS than both Node and Bun in the active phase.

The requested dependencies are pinned in `tests/npm-perf/package.json` and its
lockfile. Existing package workloads cover React/react-dom, Fastify, Undici,
ws, Pino, and Zod. This checkpoint adds Next, Vite, esbuild, and sharp.

| Workload | Phase | Quench ops/s / MiB | Node ops/s / MiB | Bun ops/s / MiB |
| --- | --- | ---: | ---: | ---: |
| esbuild TSX transform | no-JIT | 305 / 59.29 | 899 / 64.28 | 965 / 56.88 |
| esbuild TSX transform | JIT | 334 / 59.15 | 1,042 / 80.15 | 1,104 / 67.06 |
| sharp resize + WebP | no-JIT | 367 / 50.54 | 453 / 56.18 | 452 / 52.20 |
| sharp resize + WebP | JIT | 360 / 46.53 | 441 / 68.57 | 450 / 61.06 |
| Next route + JSON | no-JIT | 4,196 / 53.22 | 19,342 / 42.18 | 112,360 / 46.14 |
| Next route + JSON | JIT | 4,314 / 39.56 | 81,301 / 60.04 | 210,526 / 52.52 |
| Vite TSX transform | no-JIT | 8,333 / 68.88 | unavailable | 13,889 / 59.13 |
| Vite TSX transform | JIT | 8,333 / 56.69 | 27,778 / 70.86 | 31,250 / 66.42 |
| Undici pool | no-JIT | 1,984 / 60.07 | unavailable | 3,257 / 46.64 |

The Next measurements use 20,000 operations per run so Bun's measured elapsed
time is no longer near the timer's resolution. Short samples are retained only
in the ignored `work/` directory and are not evidence. Next output checksums
match. Its throughput is far below both engines in either phase, despite lower
Quench JIT RSS.

The sharp workload uses `sharp@0.35.5` to resize a 256x256 raw RGB image to
96x96 and encode WebP (quality 80, effort 4). It runs through the async
`toBuffer()` API and verifies the encoded result. JIT RSS is lower than both
runtimes by more than 20%, but throughput is not: this is the closest added
package screen, not a win. A brief `profile-aggregate` run showed the repeated
harness loop and `@img/colour` JavaScript helpers among hot code; it does not
identify a runtime candidate and is not timing evidence.

Vite uses its current `transformWithOxc` TSX API; esbuild uses
`transformSync`. Node `--jitless` cannot load Vite's Oxc WebAssembly, and
Undici's llhttp parser also requires WebAssembly after Node removes the global
under `--jitless`. Their no-JIT report omits Node with the reason recorded.

Reports:

- `esbuild-no-jit-5.json`, `esbuild-jit-5.json`
- `sharp-no-jit-5.json`, `sharp-jit-5.json`
- `next-no-jit-20000-5.json`, `next-jit-20000-5.json`
- `vite-no-jit-500-5.json`, `vite-jit-500-5.json`
- `undici-no-jit-5.json`
