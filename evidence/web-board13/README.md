# Web board 13

3 rounds x 10 s, PGO build of v2-web a3d603dc9 (board 12 plus the register-base string method read, the string-only length read and the present-only global recording), same model-85 host as board 12.

Quench / Node --jitless (ping query body video; RSS): express 1.37 1.34 1.10 1.65, 0.27; fastify 2.09 1.83 1.34 1.54, 0.29; hono 1.54 1.15 1.26 1.10, 0.30. Quench / Bun no-JIT: fastify 2.36 2.20 1.59 1.82, RSS 0.95; hono (Bun.serve) 0.32 0.29 0.31 0.14, RSS 1.33.

Below 1.2x Node: express body, hono query, hono video (video rates are 16-18 per second, within the board's round-to-round noise).
