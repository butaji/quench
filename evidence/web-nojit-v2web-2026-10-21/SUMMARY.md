# No-JIT web suite, v2-web at 6826ad57

`web --seconds 10 --rounds 3` (bun-http-framework-benchmark upstream), Quench
production build vs Node v26.12.0 `--jitless` vs Bun 1.4.2 `BUN_JSC_useJIT=0`.
Host: Linux 6.18.44-fc-v139 x86_64 VM, 4 vCPUs, 15 GiB RAM, no cgroup CPU or
memory limit; the load generator (bombardier, fasthttp client) runs on the same
host. Medians of the three rounds' mean requests/s (`report.json`, per-sample
`*.json`):

| framework | workload | Quench | Node --jitless | Bun no-JIT |
|---|---|---:|---:|---:|
| express | ping | 4006 | 4294 | n/a |
| express | query | 3820 | 3847 | n/a |
| express | body | 2352 | 3058 | n/a |
| express | video | 37 | 37 | n/a |
| fastify | ping | 9176 | 7693 | 7251 |
| fastify | query | 7827 | 7046 | 6812 |
| fastify | body | 4567 | 5333 | 4822 |
| fastify | video | 35 | 35 | 34 |
| hono | ping | 9536 | 7461 | 44802 |
| hono | query | 4938 | 5682 | 25517 |
| hono | body | 4456 | 5235 | 25200 |
| hono | video | 29 | 29 | 180 |

Peak VmHWM after all workloads (kB): Quench express 54688, fastify 59464,
hono 54748; Node 172752 / 175824 / 178464; Bun hono 46100, fastify 58620.

Caveats: every Node row reports bombardier dial or request timeouts (5-6 per
row) under 500 connections, so the runner marks Node rows invalid and the
matrix `complete=false`; their throughput is listed for comparison only.
Bun serves hono through its native `Bun.serve`, not the Node HTTP stack.
Quench and Bun reported no errors.
