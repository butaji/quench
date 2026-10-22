# EarleyBoyer initialized capture slot fast path

This is a second local production win on EarleyBoyer. It does not close the
cross-engine Task 61 Stage B gate.

## Change and profile lead

The residual-scoped physical dispatch census on the depth-zero-cache candidate
recorded 50,223,125 `LoadCapture` and 6,970,440 `StoreCapture` dispatches among
723,899,193 main-program dispatches. The instrumented profile is diagnostic;
its Score (141) is not performance evidence. `LoadCapture` ranked eighth by
count, behind local loads/stores, constants, branches, field reads, moves and
binary operations.

For an initialized slot whose environment belongs to a non-root function, the
candidate reads the live environment slot directly. Root-function environments
still use the generic global/module path, and deleted slots still use the
existing generic TDZ error path. No captured value is cached.

## Paired result

Eleven alternating Linux production pairs used the same EarleyBoyer input and
matched benchmark output in all 11 pairs. The baseline was
`e0f45f6e8eb9dc540807a116a4f81e19615c30a273adecd74e4fcba0b8eb0f32`; the
candidate was `c00031392bbe312d5d1261c869ee1a221de987a59e5df2d7eb8e38967365a4e7`.
The materialized fixture SHA-256 was
`aa379c1d54f5d13de32ebf2b50729d0cbc64bb23de5e524256c7b2270213cc0b`.

- Median Score: 415 baseline, 423 candidate; paired median delta +7.
- Paired bootstrap 95% interval for Score delta: +5 to +13.
- Median maximum RSS: 41,734,144 baseline, 41,340,928 candidate bytes.
- Paired median RSS delta: −393,216 bytes; 95% interval −548,864 to −303,104.
- Candidate Score was lower in 1/11 pairs; candidate RSS was lower in all 11.

The full ordered raw results and output text are in `pairs.jsonl` and
`pairs.json`; hashes, interval method, host, toolchain and Node results are in
`report.json`. The source change is in `candidate.patch`.

## Semantic oracle and research

Node v24.19.0, the saved baseline and the candidate produced byte-identical
output for same-depth live reads/writes, nested depth-two capture, loop-block
closure clones, direct-eval mutation, TDZ access, and script lexical capture.
The source is `node-oracle.js`; output is `node-oracle.out`. This Quench build
rejects `with` at parse time, so no cross-engine with-wrapper output is claimed.

V8's [bytecode generator](https://chromium.googlesource.com/v8/v8/%2B/7d6deeb99a7746f01bfc1a940fedf63f7a286306/src/interpreter/bytecode-generator.cc#L3119)
uses explicit context depth and slot for ordinary lexical reads and has a
separate dynamic lookup path. This informed where to profile; the measured
EarleyBoyer pairs, not a transfer of V8 performance, justify the fast path.
The V2 branch's fetched follow-up removes virtual fused-opcode accounting;
this census uses physical dispatch counts, already scoped per residual in
`v2-cloud`, so that unrelated change was not imported.

## Reproduction

Build command:

```sh
cargo build --locked -p quench-node --profile production --bin quench-node
```

The run used Rust 1.99.0, Linux x86_64, a 4-CPU quota and `wait4` maximum RSS.
The exact runner environment is in
`tasks/evidence/task61-v2cloud-linux-all8-one-round-2026-10-09.json`, under
`engines.quench.environment`. `runner.py` documents materialization and paired
execution. The candidate patch SHA-256 is
`98cf84b03fb3c5ffe6fe08bf8b39d77ada2c3d2fd98dbfad35dc868cf5939ce9`.
