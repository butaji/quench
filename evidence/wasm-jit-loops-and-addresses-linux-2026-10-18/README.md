# Compiled Wasm: guard-only addresses, zero-extended i32 views, unrolled innermost loops (Linux x86_64, 2026-10-18)

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
- An innermost loop whose body runs straight through but for branches out of it, with no
  other entry, no call and no VM callout, and that fits the budget at least twice, is
  unrolled up to four times (`unrollable_loops`, `UNROLL_BUDGET`, `MAX_UNROLL_FACTOR`): the
  latch's back edge enters the next copy without an interrupt check, and the last copy's back
  edge returns to the header with it, so a loop that never stops still checks every four
  iterations. Branches out of the loop leave every copy through the original blocks.

## Results

CoreMark fixed work, Callgrind instructions:

| Build | Instructions |
| -- | --: |
| base `d91e131e6` | 4,332M |
| guard-only addresses | 4,083M |
| + zero-extended i32 views | 3,821M |
| + unrolled innermost loops | 3,559M (−17.8%) |

`run-js.mjs` Score, five interleaved rounds per campaign (`run-coremark.sh`, `ONLY` selecting
the engines):

| Build | Quench `--jit` median | Node v26.12.0 median | Wasmtime median |
| -- | --: | --: | --: |
| base | 18,545 (3 rounds) | 19,594 | 18,353 |
| + guard-only addresses and zero-extended views | 18,860 | 19,827 | 18,240 |
| + unrolled innermost loops | 19,326 | 19,699 | 18,216 |

Max RSS is unchanged (8.7–8.8 MB).

JetStream 3 Wasm fixtures (`quench-bench --suite jetstream3 --jit --runs 3`), see
`jetstream3-*.json`.

An experiment rejected along the way: inlining leaves up to the interpreter's 512-operator
budget in compiled mode too cut CoreMark's fixed work 4.1% and scored about 3% higher, but
slowed JetStream's gcc-loops-wasm to 0.25x and HashSet, quicksort and richards by 8-10%;
compiled lowering keeps every call.

## Checks

Wasm testsuite: no JIT 65,188/65,188; JIT 65,016/65,016 with `call_indirect.wast`'s worker
failing as before (TOD-121). Runtime lib 733/733, including
`compiled_wasm_unrolled_loops_leave_from_every_copy` and the termination test (its spin loops
are unrolled and still stop).
