# Fused conditional backedge poll: rejected

The prototype combined a Wasm loop predicate with its atomic interrupt poll so
an uninterrupted taken backedge could branch directly to the loop header. The
poll remained on every taken edge, and the feature was opt-in.

The three-round randomized QuickSort screen rejected it. All rounds validated
and produced equal output, but the fused arm regressed Score: median 89.53
versus 116.10 with the feature off. The paired median ratio was 0.766 (95%
bootstrap interval 0.742–0.914). Median peak RSS was 47,472,640 bytes on and
47,566,848 bytes off (ratio 0.995; 95% interval 0.9909–1.0003), so the
candidate did not establish an RSS benefit either.

The same run scored 354.70 for Bun 1.4.3 and 480.20 for Node v26.12.0. This
was an exploratory `iteration`-profile build (opt-level 1, no LTO), with one
Quench executable shared by both A/B arms; it is not a production-profile or
qualification result. The captured report records the dirty experimental
source state, full engine provenance, output validation, round order, and
paired analysis. No 11-round confirmation was run because the screen showed a
clear regression.

The temporary compiler gate, benchmark option, and cancellation-test changes
were removed after the screen. `cargo check --tests` passed while the prototype
was present. The focused runtime test binary was not executed: rustc was
SIGKILLed at the container memory limit while compiling the large unit-test
crate.

Reproduce the archived screen with:

```sh
target/iteration/quench-bench quicksort-wasm --suite jetstream3 --jit \
  --jit-wasm-fused-backedge-poll-ab --runs 3 --timeout-ms 600000 \
  --quench target/iteration/quench-node --qjs /bin/false \
  --node /workspace/.local/bench-runtimes/node-v26.12.0-linux-x64/bin/node \
  --bun /workspace/.local/bench-runtimes/bun-v1.4.3/bun-linux-x64/bun \
  --out evidence/jit-jetstream3-wasm-2026-10-18/fused-backedge-poll/screen-iteration-3.json

target/iteration/quench-bench --analyze \
  evidence/jit-jetstream3-wasm-2026-10-18/fused-backedge-poll/screen-iteration-3.json \
  --out evidence/jit-jetstream3-wasm-2026-10-18/fused-backedge-poll/screen-iteration-3-analysis.json
```
