# Web board 10

3 rounds x 10 s, PGO build of v2-web 0b9e91f53 (trunk merge 39e6e96f8 plus the LocalField promotion and string-site fixes), same model-85 host as board 9.

Quench / Node --jitless (ping query body video; RSS): express 1.35 1.28 0.95 1.42, 0.28; fastify 1.74 1.78 1.30 1.43, 0.29; hono 1.50 1.01 1.12 0.82, 0.30. Quench / Bun no-JIT: fastify 2.28 1.98 1.43 1.70, RSS 0.97; hono (Bun.serve) 0.30 0.25 0.30 0.11, RSS 1.29.
