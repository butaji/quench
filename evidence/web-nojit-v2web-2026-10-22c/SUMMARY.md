# No-JIT web suite, v2-web at d64477ee

`web --seconds 10 --rounds 3`, same host and tools as evidence/web-nojit-v2web-2026-10-22b.
Medians of the three rounds' mean requests/s:

| framework | workload | Quench | Node --jitless | Bun no-JIT |
|---|---|---:|---:|---:|
| express | ping | 4895 | 4221 | n/a |
| express | query | 4008 | 3887 | n/a |
| express | body | 2621 | 2973 | n/a |
| express | video | 52 | 35 | n/a |
| fastify | ping | 10313 | 7601 | 6667 |
| fastify | query | 8602 | 7138 | 6399 |
| fastify | body | 5605 | 5037 | 5024 |
| fastify | video | 50 | 33 | 32 |
| hono | ping | 9856 | 7278 | 42991 |
| hono | query | 5030 | 5883 | 22713 |
| hono | body | 5150 | 5035 | 25254 |
| hono | video | 31 | 28 | 177 |

Every runtime measured slower than on the previous board (host variance), so rows compare
within this board. Ratios to Node: fastify ping 1.36, query 1.21, body 1.11 (0.92 on the
previous board); hono ping 1.35, body 1.02 (0.90 before), query 0.85; express ping 1.16,
query 1.03, body 0.88 (0.85 before). Fastify body now leads Bun no-JIT (1.12). Bun hono runs
Bun.serve. Node rows carry bombardier timeouts (5-6 per row).

Peak VmHWM per round (MB): Quench express 48/46/45, fastify 58/58/51, hono 48/48/47;
Node 170-172 / 175-176 / 176-180; Bun fastify 59/58/58, hono 44/45/45.

Instructions per 1000 requests (callgrind, same binaries' runtime path) moved from
911.9M to 834.4M on fastify body and 1994.3M to 1857.8M on express body across the commits
since 049159f9 (see each commit message for its own measurement).
