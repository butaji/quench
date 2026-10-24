# Web board 16

3 rounds x 10 s, PGO build of v2-web 167e4902a (board 15 plus array and empty-object
allocation without the generic cell dispatch, the one-allocation arguments object, frameless
base class constructions and the 00:12/02:07 v2-merged syncs), same 4-CPU host.

Quench / Node --jitless (ping query body video; RSS): express 1.35 1.37 1.03 1.42, 0.24;
fastify 2.01 1.77 1.39 1.48, 0.27; hono 1.52 1.02 1.25 1.04, 0.27. Quench / Bun no-JIT:
fastify 2.33 2.11 1.57 1.76, RSS 0.89; hono (Bun.serve) 0.33 0.29 0.34 0.16, RSS 1.26.

Against board 15, Quench's medians moved within the board's few-percent spread (express
query 3214 -> 3392, fastify ping 9384 -> 10078, hono query 4115 -> 3956), though callgrind
counts 3% fewer instructions per hono query request. Hono's peak RSS outlier (61 MB) did not
recur (47 MB). Node's rows carry the same incomplete marks as board 15.
Below 1.2x Node: express body, hono query, hono video.
