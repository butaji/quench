# TOD-181: payload bytes per allocation under MemoryFirst

Host: Linux x86_64 VM, 4 vCPUs, kernel 6.18. Binary: static + BOLT `gc-tuning` build of v2-gc
`7ad78bf43f`. `QUENCH_GC_PAYLOAD_UNIT` sets how many payload bytes (string units, array
elements) count as one allocation toward the next collection; production uses 64. Runs: 5
without JIT and 3 with `--jit`. Ratios are against Throughput. Fixture order in
`summary.txt`: richards, deltablue, crypto, raytrace, earley-boyer, regexp, splay,
navier-stokes.

| Payload unit | Score, no JIT | Max RSS, no JIT | Score, `--jit` | Max RSS, `--jit` | RegExp max RSS | Crypto max RSS | Splay max RSS |
| -- | --: | --: | --: | --: | --: | --: | --: |
| 64 (production) | 0.944 | 0.648 | 0.927 | 0.651 | 7344 KiB | 4360 KiB | 31740 KiB |
| 32 | 0.903 | 0.637 | 0.907 | 0.640 | 7280 | 4380 | 29084 |
| 16 | 0.887 | 0.627 | 0.884 | 0.627 | 7168 | 4316 | 25136 |
| 8 | 0.840 | 0.621 | 0.828 | 0.604 | 7020 | 4352 | 22592 |

Smaller units don't reach the targets: RegExp (5767 KiB) and Crypto (3043 KiB) barely move,
and Score falls by up to 10%. Their peaks are live data, not garbage that waits for a late
collection. Splay falls by up to 29%, but it already meets its target. Production keeps 64.
