# No-JIT web suite, v2-web at 2cc9500c

`web --seconds 10 --rounds 3` (bun-http-framework-benchmark upstream), Quench production
build vs Node v26.12.0 `--jitless` vs Bun 1.4.2 `BUN_JSC_useJIT=0`. Host: Linux
6.18.44-fc-v139 x86_64 VM, 4 vCPUs, 15 GiB RAM, no cgroup CPU or memory limit; bombardier
(fasthttp) on the same host. Medians of the three rounds' mean requests/s (`report.json`):

| framework | workload | Quench | Node --jitless | Bun no-JIT |
|---|---|---:|---:|---:|
| express | ping | 4952 | 4440 | n/a |
| express | query | 4386 | 4227 | n/a |
| express | body | 2668 | 3168 | n/a |
| express | video | 55 | 37 | n/a |
| fastify | ping | 9894 | 7386 | 7577 |
| fastify | query | 8388 | 7412 | 7006 |
| fastify | body | 5153 | 5392 | 4818 |
| fastify | video | 51 | 35 | 34 |
| hono | ping | 9394 | 7552 | 44198 |
| hono | query | 5240 | 6289 | 25551 |
| hono | body | 4572 | 5130 | 26310 |
| hono | video | 30 | 31 | 183 |

Peak VmHWM after all workloads, round 0 (MB): Quench express 55, fastify 58, hono 52;
Node 168 / 173 / 173; Bun 46 (hono), 58 (fastify).

Caveats: every Node row reports bombardier timeouts (5-6 per row) at 500 connections, so the
runner marks the matrix `complete=false`; Node throughput is listed for comparison only.
Bun hono runs Bun.serve (the suite's Bun entry), not node:http.
