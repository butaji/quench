# Parser lane-exit diagnostics

These are instrumented no-JIT diagnostics at Quench source revision
`22c352fa6f28dd688364ab33d0f8345120c07612`. They are not performance
comparisons: `profile-lane-exits` changes runtime cost, the printed JetStream
scores are excluded, and Bun was not available for matched runs in this
environment.

## Host and run

- Linux x86_64, kernel `6.18.55`, Intel Xeon Platinum 8573C, 5 CPUs.
- Cgroup memory limit: 16 GiB.
- Built `quench-node` with
  `CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 cargo build --locked --profile production -p quench-node --features quench-runtime/profile-lane-exits`.
- Ran the stock JetStream 3 driver directly with the checked-in Node shell
  prelude, once per test:
  `target/production/quench-node /tmp/quench-jetstream-prelude.js quench-bench/jetstream --no-prefetch --test=acorn-wtb`
  and `target/production/quench-node /tmp/quench-jetstream-prelude.js quench-bench/jetstream --no-prefetch --test=babylon-wtb`.
- The prelude was extracted from `quench-bench/src/jetstream.rs`; stdout and
  stderr are preserved in this directory. The driver validated its benchmark
  assertions for both tests and exited successfully.

## Counters

For `acorn-wtb`, there were 2,829,002 lane exits. The leading opcodes were:

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

| Fixture | Total lane exits | `SetFieldStrict` | `DefineField` | `Call` | `Return` |
| --- | ---: | ---: | ---: | ---: | ---: |
| `acorn-wtb` | 2,829,002 | 1,979,925 | 4,743 | 587,925 | 1,579 |
| `babylon-wtb` | 4,700,888 | 1,925,070 | 1,096,767 | 979,599 | 548,817 |

The function census counted 2,651,935,416 instructions for Acorn
(3,517,405 general) and 1,455,297,689 for Babylon (4,851,618 general).
`SetFieldStrict` exits in both tests concentrate in `finishNodeAt`. The
similarity across these parser fixtures makes field-write lowering a
cross-workload hypothesis; Babylon's higher `DefineField` and `Return` counts
may be parser-specific. Exit counts still do not measure exclusive time or
predict speedup.

Complete score/timing lines and all counters are retained in
`stdout.txt`, `stderr.txt`, `babylon-stdout.txt`, and `babylon-stderr.txt`.
These two diagnostic fixtures do not qualify JetStream performance.
