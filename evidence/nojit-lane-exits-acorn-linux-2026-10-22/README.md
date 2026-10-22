# `acorn-wtb` lane-exit diagnostic

This is an instrumented no-JIT diagnostic at Quench source revision
`22c352fa6f28dd688364ab33d0f8345120c07612`. It is not a performance
comparison: `profile-lane-exits` changes runtime cost, the printed JetStream
score is excluded, and Bun was not available for a matched run in this
environment.

## Host and run

- Linux x86_64, kernel `6.18.55`, Intel Xeon Platinum 8573C, 5 CPUs.
- Cgroup memory limit: 16 GiB.
- Built `quench-node` with
  `CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 cargo build --locked --profile production -p quench-node --features quench-runtime/profile-lane-exits`.
- Ran the stock JetStream 3 driver directly with the checked-in Node shell
  prelude:
  `target/production/quench-node /tmp/quench-jetstream-prelude.js quench-bench/jetstream --no-prefetch --test=acorn-wtb`.
- The prelude was extracted from `quench-bench/src/jetstream.rs`; stdout and
  stderr are preserved in this directory. The driver validated its benchmark
  assertions and exited successfully.

## Counters

There were 2,829,002 lane exits. The leading opcodes were:

| Opcode | Exits |
| --- | ---: |
| `SetFieldStrict` | 1,979,925 |
| `Call` | 587,925 |
| `Binary` | 101,045 |
| `CreateRegExpLiteral` | 69,470 |
| `LoadName` | 54,582 |

The lane function census counted 2,651,935,416 dispatched instructions, of
which 3,517,405 were dispatched by the general loop. The repeated
`SetFieldStrict` exits are concentrated in `finishNodeAt`; call exits cluster
in the parser bundle. This suggests profiling shared field-write and call
handling across more parser workloads, but these counts alone do not quantify
time saved by changing the lane.

The complete score/timing lines and all counters are retained in
`stdout.txt` and `stderr.txt`. Because only a single fixture was run, this is a
profile, not a JetStream qualification sample.
