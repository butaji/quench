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
