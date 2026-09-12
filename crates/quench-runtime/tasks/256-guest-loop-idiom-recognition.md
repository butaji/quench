# 256 — Guest-loop idiom recognition and whole-loop kernels

Status: planned

Recognize a small, explicit set of semantic loop denotations in the quoted SSA/CFG
region before stencil tiling. The first `LoopIdiom` data type should cover:

- dense fill with a loop-invariant immediate or proven-borrowed value;
- dense non-overlapping copy and overlap-safe move;
- exact integer/bitwise popcount and leading/trailing-zero recurrences;
- bounded sentinel search over sequential Latin-1/UTF-16 or packed dense storage; and
- polynomial hash accumulation when integer truncation and overflow semantics match.

This is Lisp-style macroexpansion over one canonical region value: match recurrence,
memory, alias, range, and effect facts; replace the entire trace with an equivalent
`KernelRef` or `StencilTemplate` node; then emit once. It is also categorical: the loop
trace and replacement kernel are morphisms with the same `Stencil<In, Out>` type, and a
rewrite is legal only after its proof witnesses semantic equality in that context.

Do not match source text, function/property names, benchmark identity, or an exact
bytecode spelling. Normalize induction variables and commutative integer expressions
first, then recognize from the region's recurrence and MemorySSA graph. A plain
bytecode-window matcher is insufficient because it cannot prove aliasing, sparse-array
behavior, accessors, holes, prototype effects, or integer overflow semantics.

## Lowering contract

- A shared immutable kernel owns the general machine loop. A patched instance carries
  only bases, length/stride, constant value, side-exit, and continuation obligations.
- Dense-array guards prove no proxy/accessor/prototype-hole effects and stable element
  representation for the whole operation.
- `Copy` requires non-alias proof; otherwise choose `Move`. Reference copies/fills use
  the one derived batch barrier from Task 162 rather than one barrier per element.
- Exact integer/bitwise idioms may lower through Rust operations such as `count_ones`
  so rustc/LLVM selects the target instruction. Floating-point reductions may not be
  reassociated.
- Failure to prove an idiom leaves the original traced stencil loop intact. There is no
  interpreter fallback.

The initial corpus probes are Crypto's dense zero-fill loops and its canonical
`x &= x - 1` popcount loop, plus dense numeric fills/copies in Navier–Stokes. These are
coverage tests for a general recognizer, never selector keys.

Acceptance: structural tests recognize equivalent loops with different register names,
commuted exact-integer forms, and rotated CFG layout; adversarial tests reject sparse,
holey, accessor-bearing, alias-uncertain, effectful, overflow-incompatible, and floating-
point-reassociated forms; disassembly shows a whole-loop kernel/intrinsic rather than
per-opcode continuation traffic; exact per-idiom counters show selection; Crypto and
Navier–Stokes targeted alternating A/B improve before a full-suite acceptance run.

Primary source: LLVM's loop-idiom recognizer and its `memset`, `memcpy`, `memmove`,
`strlen`, popcount/find-first-set, polynomial-hash, and CRC transformations:
<https://llvm.org/doxygen/LoopIdiomRecognize_8cpp.html>.
