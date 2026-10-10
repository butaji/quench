# EarleyBoyer scoped profiler check (2026-10-10)

The V2 per-residual accounting change was adapted to `v2-cloud` and compiled
with and without aggregate profiling. A `QUENCH_OPCODE_CENSUS=1` run recorded
734,166,270 physical dispatches and exactly the same number of site counts for
the MAIN residual. A separate normal profile run supplied the scoped adjacent
pair ranking below. The extra profiler data is diagnostic; the instrumented
Score is not compared with production runs.

The highest remaining counted pairs were `StoreLocalPlain → LoadLocalPlain`
(53.84M), `LoadLocalPlain → GetField` (52.59M),
`LoadLocalPlain → StoreLocalPlain` (45.60M), `Binary → JumpFalse` (33.28M),
`GetField → StoreLocalPlain` (22.96M), `Move → Move` (22.83M), and
`LoadLocalPlain → Binary` (22.11M). Several top pairs already have measured
optimizations in the candidate; `Binary → JumpFalse` is the leading untested
branch-fusion hypothesis. It must still win paired production measurements
and preserve Node-observed semantics.

The JSON records the binary and fixture hashes, source revision, command,
scoped counters, and top 32 pairs.
