# Splay aggregate profile: program-scoped accounting

The aggregate profile previously keyed physical opcode sites, adjacent pairs,
numeric regions, function entries, and GC root-map observations by function/PC
without the residual program ID. Dynamic `eval` residuals can reuse those
coordinates, while the report decodes the counters against the main residual.
The profile now includes the program ID in these execution-site records and
reports the main residual's counters against its own bytecode.

## Verification

- `cargo test -p quench-runtime --features profile-aggregate,profile-memory profile::`
  passed 4 profile tests, including colliding function/PC coordinates in two
  residual programs.
- `cargo check -p quench-runtime --features profile-memory` passed.
- `cargo check -p quench-runtime` passed.
- One fixed-work Splay run under `quench-node` completed with an additional
  direct `eval` residual containing 50 functions. The eval returned 49. The
  profile report completed through 13 GC collections; the main residual had
  15,300,575 physical dispatches, 0 virtual steps, and 15,300,575 semantic
  steps. No dispatch accounting panic occurred.
- The same one-run profile without the eval residual completed with 15,300,560
  main physical and semantic steps and 12 collections.
- With `QUENCH_OPCODE_CENSUS=1`, the main-program dispatched and site totals
  also matched exactly: 15,300,575 each, while the eval still returned 49.

This is profiler correctness evidence only; it makes no timing, Score, RSS, or
Splay-distance claim. The Splay workstream remains active.

## Reproduction

Build the diagnostic binary with `profile-aggregate` and `profile-memory`, then
run `splay-profile-one-plus-eval.js`. The captured output is in
`node-splay-profile-one-plus-eval.stdout` and
`node-splay-profile-one-plus-eval.stderr`.

Pinned diagnostic binary SHA-256:
`ad582162a826e4be9c6e45f740c960514e65ec2178d7d6f4cf768ca957dc6efa`

The pinned binary is at
`target/pinned/ad582162a826e4be9c6e45f740c960514e65ec2178d7d6f4cf768ca957dc6efa/quench-node`.
The diagnostic host was an Apple M4 arm64 running macOS 26.5 (Darwin 25.5.0),
with rustc 1.99.0 (`b940084d7`, 2026-09-28).
