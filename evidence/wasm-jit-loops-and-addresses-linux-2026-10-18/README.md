# Compiled Wasm: guard-only addresses and zero-extended i32 views (Linux x86_64, 2026-10-18)

Intel Xeon @ 2.10 GHz, 4 vCPUs, 15 GiB, Linux 6.18.44 (no PMU: `perf` samples `cpu-clock`
only), no cgroup limits. Static production `quench-node --jit`; base `v2-merged` `d91e131e6`.

## What compiled code paid

Callgrind's instruction-level profile of CoreMark's fixed work
(`evidence/wasm-jit-interrupt-checks-linux-2026-10-18/run-fixed.mjs`, the dumped code
buffer disassembled and matched to per-address counts) and a `perf` `cpu-clock` profile of a
real run:

- Every access with a nonzero offset compared its index with the end of the reservation
  (`cmp $0xffff0000` and a branch): the memory was mapped a second time above 2^32 so that an
  i32 address sum could be formed in 64 bits, and an index past it would have met that
  repetition instead of the guard. Since `ee75becfe` forms the sum in i32 and zero-extends
  it, nothing reads the repetition.
- Every indexed access zero-extended its i32 index first (`mov %edx,%eax`): Cranelift elides
  the extension of a 32-bit operation's result, but not across the block parameters a loop
  carries. Register moves were 16.7% of the compiled code's instructions.
- Tight loops paid their interrupt check (two instructions), back edge and loop-carried moves
  on every iteration (a list traversal: three moves and a jump of ten instructions).

## Change

- A memory32 reservation is one private mapping of 8 GiB plus a page; any i32 index plus
  offset plus access width lands in it, so the guard alone bounds every access (the wrapped
  alias, `wraps` and the wrapped-index check are gone; the accessible prefix may now be the
  whole 4 GiB).
- A register's i32 view is held zero-extended in 64 bits (`view_ir`); reading it is free, and
  an access uses it as its offset directly (`zero_extend`).

Unrolling innermost loops was tried and withdrawn (below).

## Results

CoreMark fixed work, Callgrind instructions:

| Build | Instructions |
| -- | --: |
| base `d91e131e6` | 4,332M |
| guard-only addresses | 4,083M |
| + zero-extended i32 views | 3,821M |

`run-js.mjs` Score, five interleaved rounds per campaign (`run-coremark.sh`, `ONLY` selecting
the engines):

| Build | Quench `--jit` median | Node v26.12.0 median | Wasmtime median |
| -- | --: | --: | --: |
| base | 18,545 (3 rounds) | 19,594 | 18,353 |
| + guard-only addresses and zero-extended views | 18,860 | 19,827 | 18,240 |

Max RSS is unchanged (8.7–8.8 MB).

JetStream 3 Wasm fixtures, Callgrind instructions of the fixtures' own iterations
(`jetstream-fixed.cjs`, below), base `d91e131e6` against this change:

| Fixture | 1 iteration (with compile) | 3 iterations |
| -- | --: | --: |
| HashSet | -3.35% | -3.92% |
| gcc-loops | -1.75% | -1.74% |
| quicksort | +0.04% | +0.10% |
| richards | -1.49% | -1.58% |
| TSF | -2.83% | -2.23% |
| zlib | -1.56% | -1.96% |

## Withdrawn: unrolling innermost loops

Innermost straight-line loops unrolled up to four times (budget 48 instructions, the
interrupt check kept on the last copy's back edge) cut CoreMark's fixed work to 3,559M and
raised its Score to 19,326 (Node 19,699), but failed the JetStream gate. Wall-clock rounds
on this host vary by up to 30% per run, so the decision used Callgrind counts of the
fixtures' own iterations (`jetstream-fixed.cjs <glue.js> <module.wasm> <benchmark.js> <N>`
runs the fixture's emscripten glue and `benchmark.js` and calls `runIteration` N times; one
and three or four iterations separate compilation from each further iteration), with and
without unrolling:

| Fixture | 1 iteration (with compile) | each further iteration |
| -- | --: | --: |
| gcc-loops | +2.5% | -6.6% |
| HashSet | +1.3% | +1.2% |
| quicksort | -0.9% | - |
| richards | +0.1% | 0.0% |
| TSF | +6.0% | 0.0% |
| zlib | +1.0% | 0.0% |

Wasm modules compile eagerly at first call, so every innermost loop pays Cranelift for its
copies whether it runs or not; JetStream scores the first iteration as a third of each
fixture's Score, and TSF's first iteration grows 6%. In HashSet's rehash, unrolling a
probe loop of about 1.2 iterations raised register pressure across the function, and the
outer loop reloads the memory base and table bounds from the stack. Unrolling needs a
hotness signal (a loop's trip counts before a recompilation) before it can pay; the
version measured is kept outside the tree.

An experiment rejected along the way: inlining leaves up to the interpreter's 512-operator
budget in compiled mode too cut CoreMark's fixed work 4.1% and scored about 3% higher, but
slowed JetStream's gcc-loops-wasm to 0.25x and HashSet, quicksort and richards by 8-10%;
compiled lowering keeps every call.

## Checks

Wasm testsuite (the whole `testsuite` directory): no JIT 67,124/67,124; JIT 66,952/66,952
with `call_indirect.wast`'s worker failing as before (TOD-121). Runtime lib 732/732.
