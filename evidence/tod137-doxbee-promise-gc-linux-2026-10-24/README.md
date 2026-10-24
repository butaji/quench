# Why the collector is ~20% of doxbee-promise (jet lane's report)

Workload: JetStream `simple/doxbee-promise.js`, with a standalone runner (`doxbee-runner.js`,
20 iterations). Each iteration starts 10,000 promise chains and awaits `Promise.all`.

Build: static production, v2-gc `3ed4c736fe`, with `profile-gc-time` and `gc-tuning`.
Throughput policy (the default), no JIT. Host: Linux 6.18 x86_64 VM, 4 CPUs.

## What the collector does

`QUENCH_GC_TIME_PROFILE` over the 20 iterations (4.17 s in all):
- **Collections:** 78 in all, 59 of them full.
- **Full-collection marking:** 535 ms, about 9 ms each.
- **Sweep:** 200 ms, of which the record-family phase is 177 ms.
- **Live set at the end:** about 150 K slots.

Why so many full collections: a chain stays pending until its iteration's `Promise.all`
settles, so more than half of young allocation survives each young collection. Past
`survival_limit` (half), the Throughput table runs full collections only, one live set of
allocation apart. Each one marks the whole live set, serially.

Marking is serial for two reasons:
- `mark_work_parallel_or_serial` falls back to the serial trace whenever any record family
  holds live records (`has_direct_records`), which is now always.
- On a host of 4 CPUs or fewer, the default is one marker anyway.

## Pacing probes (gc-tuning overrides, one run each)

| Override | Time | Collections (full) | Max RSS |
| -- | --: | --: | --: |
| none | 4173 ms | 78 (59) | 32.8 MB |
| full-only headroom 2× (`QUENCH_GC_FULL_ONLY_NUM=2`) | 3671 (−12%) | 47 (36) | 48.3 MB |
| full-only headroom 4× | 3463 (−17%) | 29 (22) | 70.0 MB |
| allocation cap 2× or 4× | 4170–4273 | 78 (59) | 33.5–33.9 MB |
| no survival switch | 4340 | 129 (32) | 37.7 MB |

Reading:
- **Pacing:** more headroom in full-only mode trades memory for time almost linearly. The
  Throughput table belongs to the jet lane and is bounded by other lanes' measurements, so
  the trade is theirs to make with these numbers.
- **Structural levers, in the collector:**
  - parallel marking that understands record families, which today disables it;
  - a cheaper record-family sweep (177 ms of the 200).
