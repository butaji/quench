# Web board 15

3 rounds x 10 s, PGO build of v2-web 46c8610de (board 13 plus the plain activation return, the frame-run environment local access and the RegExp prefilter rejection), same model-85 host.

Quench / Node --jitless (ping query body video; RSS): express 1.37 1.31 1.06 1.48, 0.28; fastify 1.83 1.75 1.38 1.54, 0.29; hono 1.64 1.05 1.34 0.98, 0.36. Quench / Bun no-JIT: fastify 2.26 2.08 1.62 1.79, RSS 0.96; hono (Bun.serve) 0.34 0.30 0.35 0.14, RSS 1.69.

Node's own rates rose on this board (hono query 3440 -> 3902 against board 13); Quench's rates rose too (hono query 3939 -> 4115, body 3769 -> 4218). Below 1.2x Node: express body, hono query, hono video.
