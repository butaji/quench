# Web board 17

3 rounds x 10 s, PGO build of v2-web 7f4637f07 (board 16 plus 128-register liveness,
promoted argument slots of default and pattern parameter lists in functions and arrows,
frameless cross-program constructions, the class-field layout fix and the 04:07-08:07
v2-merged syncs), same 4-CPU host.

Quench / Node --jitless (ping query body video; RSS): express 1.51 1.36 1.09 1.52, 0.25;
fastify 1.96 1.87 1.34 1.55, 0.28; hono 1.61 1.14 1.40 1.15, 0.28. Quench / Bun no-JIT:
fastify 2.35 2.08 1.75 1.76, RSS 0.91; hono (Bun.serve) 0.32 0.31 0.36 0.14, RSS 1.30.

Every runtime ran slower on this board than on board 16 (Bun elysia ping 94542 -> 76534,
Node express ping 2760 -> 2393); the ratios above compare matched rounds. Against board 16
the ratios below 1.2x rose: hono query 1.02 -> 1.14, express body 1.03 -> 1.09, hono video
1.04 -> 1.15. Node's rows carry the same incomplete marks as boards 15 and 16.
Below 1.2x Node: express body, hono query, hono video.
