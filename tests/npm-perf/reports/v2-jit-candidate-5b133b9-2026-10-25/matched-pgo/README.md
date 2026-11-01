# Tagged-nullish OSR candidate screen

This is a focused package-path diagnostic, not a qualification run.

## Exact inputs

- Quench candidate source: `origin/v2-jit` commit `e1022da56e7cee1ae059a17143a3286a0dec0a86`.
- Quench control source: `origin/v2-merged` commit `56e56d131c7456c32bcb4c178ef6252bfa914e85`.
- Both binaries used the same existing general PGO profile, SHA-256 `cc489574ff15c556a38fa0961ff9b58e307838d3f82fee500548b8d3f05c9f77`, with `-Cllvm-args=-disable-icp`. The profile predates these source revisions; LLVM reported function hash mismatches, so treat these comparisons as diagnostic.
- Candidate binary SHA-256: `1c7349b75278fe84b28d14991ea9438aae14e0befd6d0b8d9f2c6e30704b4457`.
- Control binary SHA-256: `ce33ceede2c584b3ed8492c627028fc32fdf55eb28f4ad7d5f094b0255498eb8`.
- Reports contain 11-round matched candidate/control runs for Zod and Next. The stats-only reports are separate one-round probes.

## Results and attribution

- Zod candidate/control throughput: `0.990x`; Next: `0.983x`.
- One-round stats show 0 native OSR region runs and 0 successful OSR compiles in both candidate and control for both package workloads. Function status counts are identical between candidate and control.

The tagged-nullish code therefore does not execute on these package hot paths. Do not spend a full matrix run on this candidate. Next shared-JIT experiment should target the measured Zod `LoadEnvLocal` loop rejection and Pino dynamic call sites, using the exact-PC call/continuation and GC/throw fixtures documented in TOD-138.

The full Linux package gate is unchanged; these focused reports establish no speed or RSS pass.
