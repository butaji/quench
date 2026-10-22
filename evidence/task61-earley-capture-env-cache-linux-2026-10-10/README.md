# EarleyBoyer capture environment cache

This is a local production win on the single EarleyBoyer fixture. It does not
close the cross-engine Task 61 Stage B gate.

## Change

Cache a frame's depth-zero capture environment only after the existing walk
returns its starting environment unchanged. That equality proves the lookup
skipped no `with` or lexical wrapper layers. The cache stores an environment,
not a binding value, so reads and writes still observe live mutable slots. The
cache resets when a frame is reused or a new invocation begins; suspended
frames resume uncached.

## Paired result

The 22 alternating production pairs used the same EarleyBoyer input and matched
benchmark output in all 22 pairs. The baseline was `e46537823eb25a37cf5646af793e6873ceaeff1069fb878a96813cae3a86ab32`; the candidate was
`e0f45f6e8eb9dc540807a116a4f81e19615c30a273adecd74e4fcba0b8eb0f32`. The
materialized input SHA-256 was
`aa379c1d54f5d13de32ebf2b50729d0cbc64bb23de5e524256c7b2270213cc0b`.

- Median Score: 423 baseline, 428 candidate; paired median delta +4.5.
- Paired bootstrap 95% interval for Score delta: +2 to +7.5.
- Median maximum RSS: 41,695,232 baseline, 41,240,576 candidate bytes.
- Paired median RSS delta: −503,808 bytes; 95% interval −528,384 to −397,312.
- Candidate Score was lower in 3/22 pairs; candidate RSS was lower in 21/22.

The first three screen rows did not retain wall-time values; their Score and
RSS samples and outputs are present. They remain in the paired interval because
wall time is not a qualification metric. Full ordered samples and stdout/stderr
are in `pairs.jsonl` and `pairs.json`; `report.json` records hashes, runner
settings, host, toolchain and bootstrap method.

## Semantic oracle

Node v24.19.0, the saved baseline, and the candidate produced byte-identical
output for same-depth live reads/writes, nested depth-two capture, loop-block
closure clones, direct-eval mutation, temporal-dead-zone access, and script
lexical capture. Output is in `node-oracle.out`; the source is
`node-oracle.js`. A separate `with` probe was rejected by this Quench build's
parser (`SyntaxError: 'with' statements are not allowed`), so it is not counted
as a Quench/Node match. The runtime cache remains guarded against skipped
wrapper layers.

## Research and limits

V8's bytecode generator resolves ordinary context reads to a context-chain
depth and slot, while dynamic lookup uses a separate path in its
[bytecode generator](https://chromium.googlesource.com/v8/v8/%2B/7d6deeb99a7746f01bfc1a940fedf63f7a286306/src/interpreter/bytecode-generator.cc#L3119).
That supports treating repeated static capture lookup as an interpreter
hot-path candidate; it does not predict this patch's speedup. The Self PIC
paper reports an 11% median for polymorphic *message-send* caches on its
benchmarks, which is a different cache and workload, so it is not used as a
performance estimate ([paper summary](https://bibliography.selflanguage.org/pics.html)).
The measured pairs, not those cross-runtime results, justify keeping this
change.

The V2 branch's fetched profiling fix scopes aggregate counters by residual
program. The earlier unscoped capture count is therefore only a lead, not a
claim in this record; the next dispatch census will use the scoped counters.

## Reproduction

Build command:

```sh
cargo build --locked -p quench-node --profile production --bin quench-node
```

The run used Rust 1.99.0, Linux x86_64, a 4-CPU quota and `wait4` maximum RSS.
The exact environment is in
`tasks/evidence/task61-v2cloud-linux-all8-one-round-2026-10-09.json`, under
`engines.quench.environment`. `runner.py` documents input materialization,
execution and measurement. `candidate.patch` has SHA-256
`c5b8c88d3a79fb2e0421643a6556d3fb4d916921767ede573ccf4b3cb30c4d07` and
reconstructs the candidate from the recorded source revision.
