# Web board 9

3 rounds x 10 s per route, `quench-bench web` runner, at commit 70eafb4b.

- Quench binary: `tools/pgo-build.sh production` (PGO; boards 7 and 8 used plain
  `cargo build --profile production` binaries).
- Host: 4 vCPU Intel Xeon @ 2.80GHz (family 6 model 85), L2 1 MiB per core, L3 33 MiB,
  16 GiB, Linux 6.18.44-fc-v153 (Firecracker VM). This host differs from board 8's: Node
  hono query CPU per request rose 187 -> 230 us, Quench (plain build) 153 -> 260 us.

Quench / Node --jitless (ping query body video; peak RSS):

| framework | ping | query | body | video | RSS |
|---|---|---|---|---|---|
| express | 1.28 | 1.22 | 0.92 | 1.42 | 0.25 |
| fastify | 1.75 | 1.78 | 1.40 | 1.46 | 0.31 |
| hono | 1.67 | 1.14 | 1.24 | 1.00 | 0.27 |

Quench / Bun (BUN_JSC_useJIT=0): fastify 2.16 2.13 1.65 1.81, RSS 1.01; hono (Bun.serve)
0.34 0.31 0.34 0.14, RSS 1.30.
