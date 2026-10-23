# TOD-173 Zod baseline screen

This screen re-establishes the current production `quench-node` baseline for the pinned
`zod@4.6.7` workload on the current Linux x86_64 host (5 logical CPUs). It ran five
interleaved rounds of 2,500 rounds each, with 100 warmups. All engines produced checksum
`60000`. These are short screens, not qualification results; max RSS uses the npm runner's
sampled Linux process-tree VmRSS diagnostic.

Binary SHA-256: `e819b9d1e2e635c87a0ee8a9382f53efcb7e636ca2d2a2e5c5012f81c32d417e`.

## No-JIT

| Engine | Median operations/s | Median max RSS |
| --- | ---: | ---: |
| Quench interpreter | 28,571 | 25.3 MiB |
| Node `--jitless` | 68,027 | 42.2 MiB |
| Bun `BUN_JSC_useJIT=0` | 57,471 | 37.3 MiB |

Quench is smallest on RSS (32% below Bun, 40% below Node), but its throughput is 50% below
Bun and 58% below Node.

## JIT

| Engine | Median operations/s | Median max RSS |
| --- | ---: | ---: |
| Quench `--jit` | 28,736 | 19.7 MiB |
| Node | 294,118 | 50.9 MiB |
| Bun | 303,030 | 43.1 MiB |

Quench is smallest on RSS (54% below Bun, 61% below Node), but its throughput is about 90%
below both. This is consistent with the profile showing zero native entries in the Zod hot
helpers. Small cache and GC cadence changes cannot plausibly close this speed gap. The
selected runtime investment is a generic tagged-`Value` baseline tier with rooted frame
slots and exact interpreter continuation on unsupported operations, calls, throws,
interrupts, or guard failure. No such implementation or 20% win is claimed here.

Raw JSON: `zod-no-jit-baseline-5.json` and `zod-jit-baseline-5.json`.

## Fresh integrated-head control

The `v2-merged` head `053d759ab` was rebuilt as a clean production profile
(without PGO) and measured on this Linux x86_64 host with Node `v24.19.0` and
Bun `1.4.4`. Both modes ran 11 interleaved rounds of 5,000 workload rounds
and 250 warmups. The unchanged `zod@4.6.7` input checksum matched across all
engines. These measurements establish the current control; they are Linux
process-tree RSS diagnostics, not Apple M4 qualification.

| Mode | Quench ops/s / RSS MiB | Node ops/s / RSS MiB | Bun ops/s / RSS MiB |
| --- | ---: | ---: | ---: |
| No-JIT | 45,351 / 29.95 | 155,039 / 47.85 (`--jitless`) | 111,111 / 33.97 (`useJIT=false`) |
| JIT | 42,918 / 22.82 | 500,000 / 49.88 | 588,235 / 40.95 |

Quench remains behind on throughput in both modes. Its RSS is lower than both
engines in JIT mode, but the no-JIT RSS margin over Bun is only 11.8%, short
of the independent 20% target. The Quench binary SHA-256 is
`f0d6b20f90bf3ffc73d23d8a93024ae9f95ce4caf6efd5bb0c4cc979cff251ad`;
the workload and lockfile hashes are recorded in the raw reports.

The current JIT profile has zero native entries for the top repeated Zod
helpers. `unwrapMessage` fell back 3,000 times, `alloc` 2,700 times,
`isObject` 2,400 times, and `handleArrayResult` 1,500 times in the diagnostic
sample. `Cached.value` ran 316 interpreter entries and performs memoizing
field writes, so its first read must retain an exact interpreter exit before
effects. These counts guide the shared JIT owner; no package-specific runtime
path is justified by them.

Raw reports: `current-head-no-jit-11.json`, `current-head-jit-11.json`, and
`current-head-jit-profile.txt` (with `current-head-jit-profile-output.json`
holding the checksum output for that diagnostic run).

## Array callback root reuse experiment (rejected)

A production build tested reusing shared callback/receiver roots across the
`Array.map` loop and rooting only each callback's input and output transiently.
The targeted collection/exception test passed for both interpreter and
specialized execution. The paired screen had the same workload and lockfile
hashes and 11 rounds per mode. Relative to the integrated-head control, the
candidate was +6.3% no-JIT and +5.4% JIT throughput, but in the same paired run
it was only +2.2% and +1.6% respectively over the saved control binary. RSS
variation was larger than the throughput delta, and no stable 20% win emerged.
The code change was removed; these numbers are retained as a rejected,
low-confidence experiment rather than a package win.

Raw reports: `root-reuse-no-jit-11.json` and `root-reuse-jit-11.json`.

## Latest lane tip control

After fetching `v2-merged` and merging the latest `v2-zod` remote tip
(`21dce0c1f`, field-cache probe diagnostics), a clean production build was
screened again with the same workload on this Linux x86_64 host. The diagnostic
probe feature is compile-time gated and was not enabled in this build. These
11-round results are a fresh control for the lane tip, not a qualification run.

| Mode | Quench ops/s / RSS MiB | Node ops/s / RSS MiB | Bun ops/s / RSS MiB |
| --- | ---: | ---: | ---: |
| No-JIT | 46,512 / 30.02 | 155,039 / 47.85 (`--jitless`) | 115,607 / 34.01 (`useJIT=false`) |
| JIT | 42,735 / 22.78 | 512,821 / 49.44 | 540,541 / 38.19 |

Quench RSS is below both comparison runtimes in both modes, but no-JIT is only
11.7% below Bun and therefore misses the 20% RSS target. Throughput remains far
behind in both modes. The binary SHA-256 is
`e7189a58461f17530ed203bee92bd3be66968d4b39a82a0c18273dba454113f6`.

Raw reports: `latest-v2-zod-no-jit-11.json` and `latest-v2-zod-jit-11.json`.

## Rejected user-call metadata hoist

The user-call path reads the same function metadata once to count numeric call
targets and again to reject calls to class constructors. I tested retaining the
first lookup for both checks. The optimized production symbol had 1,028 static
instructions versus 1,042 for the saved control, but Linux `perf` is not
installed here, so static instruction count is not a cycles measurement.

The first pair of 11-round, 5,000-iteration screens was noisy: paired median
throughput moved +7.2% no-JIT and -1.2% JIT. A second pair put the medians at
+3.4% and -3.6%. A higher-work confirmation (15,000 iterations per round, 11
rounds) reversed the apparent no-JIT gain and showed regressions in both modes:
paired median throughput -4.7% no-JIT and -5.6% JIT, with fewer than half of
paired rounds faster. RSS fell by about 4.5% and 5.7%, but that did not make up
for the speed loss. The source change was removed; these reports remain as a
rejected experiment. Fewer static instructions did not predict faster execution.

Raw reports: `zod-callmeta-nojit-11.json`, `zod-callmeta-jit-11.json`,
`zod-callmeta-nojit-confirm-11.json`, `zod-callmeta-jit-confirm-11.json`,
`zod-callmeta-long-nojit-11.json`, and `zod-callmeta-long-jit-11.json`.

## Zod-only LLVM PGO ceiling screen

To test broader code layout, I reused the existing instrumentation build,
trained it for 15,000 Zod workload rounds plus 750 warmups, merged one 8.1 MiB
profile, then built a temporary production binary with profile use and indirect
call promotion disabled. In paired 11-round screens this profile-guided binary
was about 33% faster than the clean control in both modes; paired ratios were
above 1.0 in all 11 rounds. Its RSS was about 19% lower no-JIT and 20% lower
JIT than the control.

| Mode | Quench PGO ops/s / RSS MiB | Node ops/s / RSS MiB | Bun ops/s / RSS MiB |
| --- | ---: | ---: | ---: |
| No-JIT | 60,060 / 25.66 | 152,672 / 47.82 | 110,497 / 34.27 |
| JIT | 64,309 / 19.66 | 512,821 / 49.47 | 645,161 / 40.98 |

This is a useful partial gain, but still far from the 20% throughput advantage
over both runtimes. No-JIT RSS clears the 20% margin over Bun; JIT clears it by
a wide margin. The profile was trained only on Zod, so the binary is a ceiling
screen and could regress other workloads. I did not retain it or change the
general production PGO recipe. Raw reports: `zod-pgo-nojit-11.json` and
`zod-pgo-jit-11.json`.

## Balanced npm workload LLVM PGO screen

To check whether the Zod-only PGO gain generalized, I trained one temporary
LLVM profile with seven installed workloads (Fastify HTTP, Fastify inject,
Pino JSON logging, React SSR, Undici pool, WebSocket echo, and Zod parse) in
both modes. Each training run used 1,000 workload operations plus 100 warmups.
The profile had 14 workload/mode inputs. I built a temporary production
candidate with `-Cprofile-use` and indirect-call promotion disabled, then ran
it interleaved with the unchanged production control.

The longer no-JIT comparisons used 11 repetitions for Fastify HTTP, Fastify
inject, Pino, React, and Zod; WebSocket used five repetitions because its
request/response workload is substantially slower in Quench. The JIT screen
used five repetitions at 500 operations plus 50 warmups for Fastify HTTP,
Fastify inject, Pino, React, and WebSocket; Zod used 11 repetitions at the
runner's 5,000-operation default. All measurements are Linux diagnostics, not
qualification measurements.

| Workload | Mode | PGO Quench op/s | Clean Quench op/s | Node op/s | Bun op/s |
| --- | --- | ---: | ---: | ---: | ---: |
| Fastify HTTP | No-JIT | 517 | 485 | 2,732 | 2,370 |
| Fastify inject | No-JIT | 2,809 | 2,110 | 4,831 | 4,854 |
| Pino JSON | No-JIT | 26,316 | 23,810 | 76,923 | 66,667 |
| React SSR | No-JIT | 781 | 653 | 1,961 | 1,883 |
| WebSocket echo | No-JIT | 4,032 | 3,086 | 13,158 | 50,000 |
| Zod parse | No-JIT | 57,143 | 46,948 | 151,515 | 106,952 |
| Fastify HTTP | JIT | 742 | 639 | 3,677 | 4,000 |
| Fastify inject | JIT | 2,222 | 1,812 | 4,808 | 8,197 |
| Pino JSON | JIT | 21,739 | 17,241 | 100,000 | 100,000 |
| React SSR | JIT | 688 | 559 | 14,286 | 8,772 |
| WebSocket echo | JIT | 3,846 | 2,591 | 14,286 | 62,500 |
| Zod parse | JIT | 54,645 | 45,455 | 512,821 | 666,667 |

The balanced profile improved Quench throughput over its clean control on all
these sampled workloads, with paired median gains of about 6–33% in no-JIT and
16–49% in the short JIT screens. It still did not beat Node and Bun on any
workload. Zod-only PGO was faster than the balanced profile on Zod, so profile
specialization is material; neither profile closes the throughput gap. This
remains a rejected production change and does not modify `tools/pgo-build.sh`.

Node's `--jitless` removes the `WebAssembly` global in this environment.
Undici 7.30.1 uses its bundled llhttp WebAssembly parser with no JavaScript
fallback, so its Node no-JIT row fails before the workload starts. The
workload can still be run on Quench, Bun, and Node JIT, but that is not a
comparable no-JIT four-engine result. Bun's sampled RSS for Pino and WebSocket
was implausibly low (under 2 MiB); treat those Bun RSS samples as invalid. The
remaining Linux RSS values are diagnostics only.

Raw paired reports are the matching `balanced-pgo-*-no-jit.json` and
`balanced-pgo-*-jit.json` files, plus
`balanced-pgo-zod-parse-no-jit-11.json` and
`balanced-pgo-zod-parse-jit-11.json`.
