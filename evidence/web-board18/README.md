# Web board 18

3 rounds x 10 s, PGO build of v2-web 4daf6a7a7 (board 17 plus frameless sloppy functions
that pass `arguments` on, the shared keyed-add transition cache, value steps, `in` and
`instanceof`, GetField and SetField in place beside frameless activations, environment
activations through local field reads, `.call` and other programs, private homes and
elements without per-instance environment keys, and the 12:07-18:07 v2-merged syncs),
same 4-CPU host.

Quench / Node --jitless (ping query body video; RSS): express 1.47 1.34 1.15 1.45, 0.24;
fastify 1.94 1.86 1.42 1.40, 0.28; hono 1.63 1.17 1.23 1.01, 0.28. Quench / Bun no-JIT:
fastify 2.48 2.14 1.63 1.57, RSS 0.92; hono (Bun.serve) 0.34 0.32 0.32 0.14, RSS 1.35.

Against board 17 the rows below 1.2x moved: express body 1.09 -> 1.15, hono query 1.14 ->
1.17. Hono video reads 1.01: Quench's rounds (18.3, 18.7, 16.5 req/s) match board 17's
(15.4, 17.6, 17.5), while Node's rose from 15.1-16.3 to 17.7-18.5, so the row is within
this host's variance at a sixth of a second per request and needs its own work. Node's
rows carry the same incomplete marks as boards 15-17.
Below 1.2x Node: express body, hono query, hono video. Fastify peak RSS stays above 0.8x
Bun.
