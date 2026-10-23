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

## sharp async and concurrency follow-up

Additional workloads are `sharp-metadata.cjs`, `sharp-resize-concurrent.cjs`
(four simultaneous `toBuffer()` tasks), and a WebP effort-zero preview variant.
`sharp-resize-effort0.cjs` and all resulting five-run measurements are retained
with the original package reports.

| Workload / phase | Quench ops/s / MiB | Quench control | Node ops/s / MiB | Bun ops/s / MiB |
| --- | ---: | ---: | ---: | ---: |
| metadata, eventfd no-JIT | 3,953 / 40.19 | 4,065 / 42.48 | 5,141 / 78.00 | 7,299 / 50.50 |
| metadata, eventfd JIT | 4,124 / 34.56 | 4,098 / 37.81 | 5,208 / 88.07 | 7,463 / 62.81 |
| resize x4, eventfd no-JIT | 650 / 45.92 | 642 / 48.29 | 675 / 96.96 | 680 / 65.59 |
| resize x4, eventfd JIT | 642 / 45.43 | 654 / 47.93 | 678 / 110.26 | 675 / 75.37 |

The eventfd implementation is compared to the immediately preceding Unix
socket-pair candidate; its speed is within 3% in every phase and its sampled
RSS is lower. The initial condition-variable prototype measured metadata at
4,338 / 36.47 MiB no-JIT and 4,167 / 34.77 MiB JIT versus the original
production control at 819 / 41.91 MiB and 814 / 38.35 MiB. It was replaced
because a condition-variable wait could defer socket readiness; eventfd shares
the existing `ppoll` with TCP descriptors. The new eventfd run is documented
separately and does not claim the prototype's exact speedup.

The effort-zero encoder did not close the gap: Quench measured 376 / 47.40 MiB
no-JIT and 379 / 46.54 MiB JIT, versus Node at 552 / 57.99 and 526 / 70.09,
and Bun at 546 / 57.86 and 552 / 67.93. None of these Linux samples
qualifies. The eventfd change improves async completion delivery, but sharp
still does not meet the cross-runtime throughput gate.

Additional follow-up reports:

- `sharp-metadata-baseline-no-jit-5.json`, `sharp-metadata-baseline-jit-5.json`
- `sharp-metadata-eventfd-no-jit-5.json`, `sharp-metadata-eventfd-jit-5.json`
- `sharp-concurrent-eventfd-no-jit-5.json`, `sharp-concurrent-eventfd-jit-5.json`
- `sharp-effort0-no-jit-5.json`, `sharp-effort0-jit-5.json`

## Next response and routing follow-up

The original `next-route-response.cjs` parses its generated body with
`response.json()`. `next-route-generate.cjs` measures route matching plus
`NextResponse.json()` and handing the body stream off, with no readback in the
handler. `next-route-match.cjs` isolates dynamic route regex matching and
parameter extraction.

| Workload / phase | Quench ops/s / MiB | Node ops/s / MiB | Bun ops/s / MiB |
| --- | ---: | ---: | ---: |
| Generate response, no-JIT | 5,112 / 67.81 | 26,596 / 70.00 | 113,636 / 37.26 |
| Generate response, JIT | 4,965 / 47.72 | 58,140 / 84.10 | 208,333 / 6.15 |
| Match route, no-JIT | 56,818 / 25.64 | 322,581 / 24.83 | 212,766 / 33.25 |
| Match route, JIT | 64,516 / 17.73 | 555,556 / 33.65 | 625,000 / 1.90 |

The profile's most frequent locations were Undici WebIDL dictionary conversion,
`Response` state accessors, and body-stream setup. Route matching alone is
roughly an order of magnitude faster than response generation in Quench; the
response construction/stream path is the larger target. The Bun samples run
under 50 ms, so sampled RSS is not meaningful for those rows. Linux reports
remain diagnostic and no Next case meets the throughput gate.

Additional reports: `next-generate-no-jit-5.json`, `next-generate-jit-5.json`,
`next-match-no-jit-5.json`, and `next-match-jit-5.json`.

`next-response-json.cjs` isolates global `Response.json()` without the Next
wrapper. Quench measured 11,601 / 63.42 MiB no-JIT and 12,821 / 42.58 MiB JIT;
Node measured 48,544 / 60.82 and 94,340 / 70.62. Bun completed these 5,000
operations in under 10 ms (625k and 833k ops/s) and its Linux RSS sample is
only a few MiB; both Bun numbers are below the reliable timing and RSS windows.
This confirms the response stream implementation is still a major cost apart
from Next's wrapper, but it does not produce a valid package win. Reports:
`next-response-json-no-jit-5.json` and `next-response-json-jit-5.json`.

## Next response fast-path experiment

Added `next-response-components.cjs` to split JSON serialization from string,
byte-backed, and JSON `Response` construction. A narrow Undici source override
in Quench's build step specializes the private byte array created by
`Response.json()` and the common status-only initialization case. It preserves
generic construction for custom headers/status text and checks the final
response body byte-for-byte. The direct-state allocation variant was rejected:
its no-JIT change did not repeat in JIT.

The final five-run reports measured the response constructor at 13,405 / 107.7
MiB no-JIT and 13,012 / 87.4 MiB JIT for Quench; Node measured 53,050 / 145.7
and 96,154 / 161.7. Bun's measured times were below 50 ms, so its throughput
and RSS are timer/sampling limited. Next route generation measured Quench at
6,098 / 69.1 MiB no-JIT and 5,974 / 48.6 MiB JIT, versus Node at 27,027 / 78.9
and 66,667 / 94.4. Bun's no-JIT generation sample also ran under 50 ms; JIT
measured 263,158 / 5.1 MiB but RSS was undersampled. These are diagnostics, not
a package win. The latest paired same-source control was within run noise, so
the small response fast-path gains are not treated as established.

Reports: `response-components-no-jit.json`, `response-components-jit.json`,
`next-generate-no-jit.json`, and `next-generate-jit.json` in
`evidence/tod173-next-response-2026-10-23/`.

After merging the current `origin/v2-merged` checkpoint, a fresh five-run screen
measured `Response.json()` at 13,812 / 108.0 MiB no-JIT and 12,240 / 87.4 MiB
JIT. Node measured 54,054 / 145.6 and 109,890 / 163.2. Next route generation
measured 6,061 / 69.2 and 5,848 / 48.7 for Quench, versus Node at 28,902 / 79.6
and 57,471 / 88.5. Same-source Quench control rows were within measurement
noise; Bun's samples remain under 50 ms. The merge did not change the conclusion.
Post-merge reports use the `*-post-merge-*` names in the same evidence folder.

The private JSON-byte stream then dropped its explicit no-op `pull` and
`cancel` callbacks, using the stream's built-in no-op algorithms. Seven-run
paired `iteration` binaries (same merged code and compiler settings) measured
`Response.json()` at 7,342 versus 6,564 ops/s no-JIT and 7,215 versus 6,560
JIT. Next route generation measured 3,181 versus 2,982 ops/s no-JIT and 3,083
versus 3,075 JIT. The response-constructor gain repeated; the end-to-end JIT
route difference is within noise. This is a component improvement, not a
package win. Production screens with the callback-free implementation measured
15,588 / 89.7 MiB and 13,387 / 69.2 MiB for `Response.json()`, and 6,281 /
63.2 MiB and 6,427 / 42.5 MiB for Next route generation. Node remained 3.2–
10.7x faster on the reliable rows; Bun route and constructor samples were
under 50 ms. Paired `iteration` RSS is diagnostic, and production RSS is
sampled Linux data. Reports with `*-noop-*` names retain the runs.

A following production candidate moves plain JSON serialization directly into
UTF-8 bytes for `Response.json()`, bypassing the intermediate JavaScript string
and TextEncoder. Undici retains its normal path when serialization needs guest
code or special-value handling. Five-run paired production results against the
previous direct-state binary improved constructor throughput by 25.8% no-JIT
and 23.8% JIT, while sampled RSS fell 2.4% and 4.2%. Next route generation
improved 5.6% and 1.0%, with RSS down 3.1% and 4.3%. Quench remains slower than
Node and Bun, and its RSS remains higher than Bun's; these are component gains,
not package wins. The `*utf8-production-paired-*` reports in
`evidence/tod173-next-response-2026-10-23/` preserve all four paired runs.

A guarded `Object.entries()` path next targeted Next's repeated route-group
enumeration. It reads shape slots directly for ordinary objects with
enumerable data fields, while accessors, side descriptors, proxies, and other
unusual objects keep the generic path. Paired production route-generation
runs measured 7,924 versus 7,112 ops/s no-JIT and 7,813 versus 7,645 JIT;
sampled RSS fell from 63.38 to 60.73 MiB and 41.67 to 40.23 MiB. Eleven-run
route-matcher comparisons measured 82,645 versus 76,336 ops/s no-JIT and
75,758 versus 70,423 JIT; RSS fell from 27.39 to 25.66 MiB and 19.41 to
17.86 MiB. Node and Bun remain faster in every case, so this is a component
gain, not a Next win. Reports use `*entries-production-paired-*` in
`evidence/tod173-next-response-2026-10-23/`; Linux RSS is sampled diagnostics.
