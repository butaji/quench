# Web JIT board 1

`--mode jit`, 1 round x 10 s, PGO build of v2-web 51574547a, model-85 host: Node and Bun as shipped (Bun BUN_JSC_useJIT=1), Quench `--jit`.

Quench / Node (ping query body video; RSS): express 0.48 0.47 0.37 0.74, 0.21; fastify 0.41 0.34 0.36 0.80, 0.23; hono 0.43 0.26 0.42 0.61, 0.24. Quench / Bun: fastify 0.30 0.30 0.29 0.80, RSS 0.31; hono (Bun.serve) 0.20 0.13 0.18 0.13, RSS 0.78.

Quench `--jit` rates equal its no-JIT rates (board 12): no web workload enters native code yet, so JIT-mode wins need the baseline tier (TOD-110, TOD-176). A single round: a screen, not a claim.
