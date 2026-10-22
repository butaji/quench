# Splay 5/4 full-growth screen on Linux

This clean-source diagnostic screens commit `29cf92fdd` on Linux x86_64
(kernel 6.18.44, 4 logical CPUs, 16 GiB cgroup memory limit) with Rust 1.99.0.
The production Quench binary SHA-256 is
`6be9c3baf077b95b622a0fae914c457231a0158636bfd39dae3ebb3a9372ed04`.
The complete stock-harness report is `stock-splay-11.json`.

## Policy and result

The prior policy made full collections after allocations equal to the live
set. This screen raises the cap to 5/4: a young collection can run after the
first live-set of allocations, followed by a full mark after one additional
quarter-set.

All 11 Splay rounds were valid and output-equal. Quench scored 2,022 against
Node v26.10.0 `--jitless` at 5,433, or 37.2% of the score bar. Quench max RSS
was 137,904,128 bytes against Bun 1.4.2 no-JIT at 60,006,400 bytes (2.30x the
RSS bar). QuickJS scored 2,958 at 149,643,264 bytes. This does not win Splay;
the report is one fixture, so `qualification_ready` is false.

An immediate 11-round 1/1 control used the previously built production binary
from `aa88c4a99` (SHA-256
`1faabc11019a79dfe595440859ea6d059e1e9b18949a0ccc3e3b9ed8d851cc57`). It
scored 2,094 against Node at 5,289, with Quench max RSS 123,875,328 B against
Bun at 60,522,496 B. Both reports were valid and output-equal 11/11. Relative
to that sequential control, the 5/4 run scored 72 points lower and used
14,028,800 B more max RSS. The control report has `source_dirty: true` because
this evidence directory was untracked during that run; it is diagnostic only,
and its binary and engine hashes are preserved in the report. Along with the
large remaining distance from both bars, this rejects the extra growth. The
policy is reverted in the follow-up commit.

The mechanism follows the usual generational tradeoff: a more selective
remembered set can reduce collector scanning but raises barrier and metadata
cost. See Detlefs et al., [Concurrent Remembered Set Refinement in
Generational Garbage Collection](https://www.usenix.org/publications/library/proceedings/jvm02/full_papers/detlefs/detlefs_html/index.html).
Here the measured stock score and RSS, not the general result, decide the
screen.

## Reproduction

```sh
source /workspace/quench-build-env.sh
CARGO_TARGET_DIR=target/splay-125 cargo test --locked --offline -p quench-runtime --lib heap::tests::
CARGO_TARGET_DIR=target/splay-125 cargo test --locked --offline -p quench-runtime --lib vm::tests::minor_collection_roots_external_edges_of_old_owners
CARGO_TARGET_DIR=target/splay-125 cargo build --locked --offline --profile production -p quench-node --bin quench-node
target/release/quench-bench quench-bench/js-engine-benchmark/v8-v7/splay.js \
  --quench target/splay-125/production/quench-node \
  --qjs target/stageb-tools/qjs/quickjs-2026-06-04/qjs \
  --node target/stageb-tools/node-v26.10.0/bin/node \
  --bun target/stageb-tools/bun/bun-linux-x64/bun \
  --runs 11 --timeout-ms 300000 --out work/splay-125-stock-11.json
```

Focused verification passed: 26 heap tests and the old-owner external-edge VM
test. The one-fixture stock report is diagnostic, not full-suite qualification.
