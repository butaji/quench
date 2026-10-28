# No-JIT web suite, v2-web at 32d29547 (after the v2-merged 19b3b0dc sync: Throughput memory policy default)

`web --seconds 10 --rounds 3`, same host and tools as evidence/web-nojit-v2web-2026-10-22.
Medians of the three rounds' mean requests/s:

| framework | workload | Quench | Node --jitless | Bun no-JIT |
|---|---|---:|---:|---:|
| express | ping | 5167 | 4950 | n/a |
| express | query | 4749 | 4719 | n/a |
| express | body | 3047 | 3598 | n/a |
| express | video | 62 | 42 | n/a |
| fastify | ping | 11359 | 8610 | 7818 |
| fastify | query | 9639 | 8130 | 7544 |
| fastify | body | 5574 | 6031 | 5612 |
| fastify | video | 57 | 39 | 37 |
| hono | ping | 11090 | 8428 | 48212 |
| hono | query | 5870 | 6715 | 28119 |
| hono | body | 5375 | 5976 | 27169 |
| hono | video | 32 | 34 | 190 |

Peak VmHWM, round 0 (MB): Quench express 43, fastify 52, hono 50; Node 163 / 171 / 171;
Bun hono 41, fastify 51. Every runtime measured faster than the previous board (host
variance); ratios to Node: fastify ping 1.32, hono ping 1.32, express ping 1.04, express body
0.85, fastify body 0.92, hono body 0.90, hono query 0.87. Node rows carry bombardier
timeouts (5-6 per row); Bun hono runs Bun.serve.
