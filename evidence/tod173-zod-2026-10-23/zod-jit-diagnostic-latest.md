# Zod JIT hot-function diagnostic

This is an instrumented diagnostic, not a performance qualification. It used
the latest `v2-zod` source (`7ab54b41c`, including fetched `v2-merged`
`d9dc8f883f19be92235cff3b39c4f574bd8f3d2f`) and pinned `zod@4.6.7`.

Command:

```sh
QUENCH_JS_JIT_STATS=1 QUENCH_BENCH_ROUNDS=1000 QUENCH_BENCH_WARMUPS=50 \
  node run-workload.mjs zod-parse jit 1 \
  ../../target/iteration/zod-jit-diagnostics-1.json
```

All engines returned checksum `24000`. Do not compare this one-round run's
rates: detailed Quench JIT statistics substantially perturb Quench timing.

## Relevant profile records

The workload's `operations()` wrapper has 83 instructions, is marked
`unsupported` with `unsupported-lowering-shape-or-codegen-failure`, and has
1,050 interpreter fallbacks with zero native entries. Its general-dispatch
histogram records 6,304 `GetField` and 1,050 `Binary` instructions. The
interpreter inclusive time is 187,659,059 ns, which includes descendant calls
and must not be treated as exclusive time.

Zod's `Cached.value` getter in `zod/v4/core/util.cjs` has 12 instructions and
1,066 interpreter fallbacks with zero native entries; its inclusive time is
1,286,091 ns. Its instruction shape includes two `GetField`, a `Call`, a
`JumpBinaryFalse`, and two `SetThisFieldStrict` operations. The source shows
that the getter lazily calls `_getter`, memoizes the value into `_value`, then
clears `_getter`. This is effectful and needs a precise fallback for the first
read; the profile alone does not establish that optimizing it will yield a
material workload gain.

The most promising broad direction remains a frame-preserving compiled
region with rooted tagged values and exact interpreter exits. The wrapper's
field/call/branch shape is beyond today's pure numeric function ABI. Before
implementation, measure hot functions with exclusive costs or opcode-level
sampling and select a region that removes meaningful dispatch while preserving
call, exception, allocation, GC, and continuation semantics. This profile does
not support a 20% performance claim.
