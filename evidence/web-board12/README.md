# Web board 12

3 rounds x 10 s, PGO build of v2-web 51574547a (board 11 plus the exact-shape String.prototype site hit and the global-name site recording), same model-85 host as board 11.

Quench / Node --jitless (ping query body video; RSS): express 1.32 1.33 1.10 1.47, 0.26; fastify 1.98 1.66 1.39 1.58, 0.32; hono 1.64 1.06 1.30 0.96, 0.26. Quench / Bun no-JIT: fastify 2.39 1.98 1.56 1.82, RSS 1.04; hono (Bun.serve) 0.36 0.28 0.34 0.14, RSS 1.19.

Below 1.2x Node: express body, hono query, hono video. Absolute rates rose on every runtime against board 11 (Node hono ping 4199 -> 4845), so compare ratios, not rates, across boards.
