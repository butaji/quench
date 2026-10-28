# Young-collection budget cap: web servers and V8-v7

Host: Linux x86_64 cloud guest, 4 vCPU, 15 GB. Binary: quench-node production build with
`--features quench-runtime/gc-tuning`, at `cc66d593` plus the `maximum_young` knob. Every
knob is read from the environment (`QUENCH_GC_*`).

## Web (bun-http-framework-benchmark bundles, ping route, bombardier -c 500 -d 5s)
`gcsweep.sh fw 500 5 <configs>` reports requests per second and VmHWM. Single rounds:
throughput varies by about ±8% between rounds; peak RSS by about ±2 MB.

| config | hono rps / peak | fastify rps / peak | express rps / peak |
| -- | -- | -- | -- |
| production (young 8×live, no cap) | 5994 / 84 MB | 7854 / 96 MB | 2845 / 85 MB |
| young cap 2^16 | 6174 / 54 MB | 7124 / 70 MB | 2920 / 50 MB |
| young cap 2^17 (chosen) | 5507 / 53 MB | 7343 / 72 MB | 2949 / 53 MB |
| young cap 2^18 | 5442 / 64 MB | 6951 / 79 MB | 2808 / 62 MB |
| young ½×live (trunk table) | 6335 / 60 MB | 6824 / 68 MB | 3245 / 44 MB |
| old growth and allocation cap 1× only | 6140 / 87 MB | — | 2984 / 86 MB |

The young budget drives peak RSS; the full-collection interval does not. Fastify's throughput
was lower under every tighter young budget in these rounds. Every collection sweeps the
object and array records to their high-water mark (`heap.rs`, `collect_with_object_roots`),
so a young collection's cost grows with the old heap. That is reported to the memory lane.

## V8-v7 (`v8run.py`, three rounds, median; same build)
`v8-v7-pacing.txt` has two runs of the production config. They differ by up to 20% on
regexp and 10% on deltablue and raytrace, which is the band at three rounds on this host.
With the 2^17 cap every Score stays inside that band; peak RSS falls for earley-boyer
(46 → 28 MB), splay (161 → 92 MB) and regexp (20 → 18 MB).
