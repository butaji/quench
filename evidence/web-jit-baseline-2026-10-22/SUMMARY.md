# JIT-mode baseline, web suite (single runs)

Each runtime with its default execution mode (Node v26.12.0 default JIT, Bun 1.4.2 default
JIT; Bun's hono entry is src/bun/hono.ts on Bun.serve), Quench production build with `--jit`
and without. bombardier -c 500 (fasthttp) on the same host after a 2 s warm-up, then 6 s
measured; one run each (`run.sh`). Linux 6.18.44 x86_64 VM, 4 vCPUs. Requests/s:

| framework | route | Node | Bun | Quench --jit | Quench no-JIT |
|---|---|---:|---:|---:|---:|
| fastify | ping | 61606 | 77425 | 11388 | 12373 |
| fastify | body | 35512 | 42833 | 5953 | 6433 |
| hono | ping | 54957 | 108404 | 11717 | 12301 |
| hono | body | 33073 | 72455 | 6146 | 6137 |
| express | ping | 22665 | n/a | 5831 | 6260 |
| express | body | 15346 | n/a | 3417 | 3550 |

Quench's `--jit` tier compiles essentially none of these request paths (docs/v2/jit-design.md);
the gap to Node's and Bun's JITs is 4-12x. Single short runs: indicative, not a board.
