# Web board 11

3 rounds x 10 s, PGO build of v2-web 27ce7d6d0 (board 10 plus the rest-parameter environment activations, the plain `arguments` element read and `length` read, and the TOD-137 response weak-root release), same model-85 host as board 10. The runner reports the board incomplete: it marks Node rows with `*` when rounds saw errors, as in board 10.

Quench / Node --jitless (ping query body video; RSS): express 1.22 1.37 1.06 1.66, 0.27; fastify 1.82 1.76 1.40 1.54, 0.31; hono 1.59 1.09 1.28 1.00, 0.28. Quench / Bun no-JIT: fastify 2.02 1.95 1.58 1.64, RSS 1.00; hono (Bun.serve) 0.32 0.28 0.32 0.14, RSS 1.29.

Below 1.2x Node: express body, hono query, hono video. Peak RSS is the board-cumulative maximum (45-52 MB for Quench, 38 MB for Bun hono).
