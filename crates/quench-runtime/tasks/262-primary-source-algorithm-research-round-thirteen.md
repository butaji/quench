# 262 — Primary-source algorithm research, round thirteen

Status: complete

Research additional algorithms and VM patterns under the standing contract: every
function executes a stencil image from first entry; rustc/LLVM cooks a finite general
catalog AOT; runtime work is bounded analysis, selection, copy, patch, composition, and
semantic IC repair; no interpreter fallback, hotness threshold, benchmark identity, or
exact source sequence selects execution.

## Evidence that changes the priority

Task 253 now contains the first complete same-host comparison. On six non-RegExp suites,
`dyn_block_step_impl` alone accounts for 18.48–29.23% of Deegen's native samples. Frame
creation/completion and `Value` destruction are also major named costs. V8's profiler,
meanwhile, attributes nearly all meaningful JavaScript ticks in Richards, DeltaBlue,
Crypto, RayTrace, Earley-Boyer, Splay, and Navier-Stokes to optimized (`*`) functions.

Therefore another isolated opcode stencil cannot close the gap. The missing abstraction
is a whole-region value machine that keeps representation, location, ownership, and
effects across operations, then tiles the result into rustc-cooked stencils.

## Findings

### 1. Keep SSA through register allocation and lower phis as edge morphisms

Wimmer and Franz construct intervals directly from SSA, exploit lifetime holes, and
integrate SSA destruction after allocation. Boissinot et al. separate correctness from
coalescing quality and identify the lost-copy/swap hazards of naive phi lowering. This
supplies the missing concrete join/loop algorithm for Task 158.

In this VM, every predecessor edge derives one parallel-copy obligation from its block
arguments. Allocation first coalesces compatible values. Remaining copies lower to an
ordinary `Stencil<GammaPred, GammaSucc>`; equal locations are identity, dependency-free
moves are ordered, and cycles use one named scratch location. There is no special phi
executor and no runtime assembler.

### 2. Propagate uses backward to form Word32 islands (new Task 264)

V8's lowering does not decide representation from a producer alone. A consumer that
observes only the low 32 bits sends a truncating-Word32 demand backward, allowing adds,
subtracts, shifts, and bitwise operations to remain wrapping machine integers without
intermediate Float64/tagged conversions or overflow checks. This is distinct from
forward type/range inference and from known tag bits.

Crypto's V8 profile is concentrated in `montReduce`, `bnpSquareTo`, and
`bnpMultiplyTo`, which are long integer/bitwise chains. Task 264 applies the algorithm
generally to any RegionPlan use graph; those functions are evidence, never selectors.

### 3. Make array element kind a real storage coproduct (new Task 265)

The current `ArrayStorage` is always `Vec<Value>` plus `non_number_count`; “packed
number” is a guard fact, not a different backing representation. V8 and JavaScriptCore
both use actual Int32, double, tagged, packed, and holey storage variants with monotone
widening. An `I32 -> F64 -> Tagged` backing lattice lets bitwise arrays stay machine
integers and gives the stencil context an exact load/store representation.

This composes with Task 264: a proven I32 load enters a Word32 island without conversion,
and an incompatible store widens through one canonical transition kernel. It also makes
Task 32's element-kind connector describe real storage rather than a count over tagged
words.

### 4. Reuse RegExp capture offset storage (new Task 263)

The post-Task-142 RegExp sample is no longer compilation-bound. Matcher code plus memory
allocation/free/move/zero routines dominate. Rust regex explicitly exposes
`capture_locations` plus `captures_read` so callers can amortize the capture-offset
allocation. The immutable automaton remains a shared Kernel; each mutable JS RegExp
instance owns reusable scratch beside `last_index`.

This is a small, independently measurable experiment and precedes a custom native regex
stencil. It does not duplicate the matcher engine.

### 5. Use copying reducers for the quoted optimizer

V8's current Turboshaft design moved back to a CFG and applies composable reducers while
copying an input graph into an output graph. This matches the Lisp discipline already
chosen here: immutable quoted data, pattern rewrites, one normalized output, one final
emission. Task 171 now records this as an implementation invariant rather than allowing
in-place mutation plus drifting side tables.

### 6. Refine ownership with uniqueness, but keep one analysis

CPython 3.15's copy-and-patch JIT reports avoiding reference counts through unique-
reference tracking and adding basic register allocation. This corroborates Tasks 172
and 158. Uniqueness is a field of the one Ownership fact, not a new pass; CPython's
runtime trace/hotness policy is not adopted.

## Ranked experiments

1. Task 158 with SSA intervals and parallel-copy edge stencils, on top of Task 171.
2. Task 264 Word32 demand propagation and derivative selection.
3. Task 265 real I32/F64/tagged array backing variants.
4. Task 263 reusable capture locations, because it is small and the RegExp profile is
   allocation-heavy.
5. Tasks 146/172/193/162 to remove frame and ownership costs shown by Task 253.

The first three are one coherent hierarchy: RegionPlan facts choose physical contexts;
edge morphisms connect those contexts; finite templates implement them. They must not
be decomposed into thousands of independently guarded leaves.

## Primary sources

- SSA linear scan and integrated SSA destruction:
  <https://c9x.me/compile/bib/Wimmer10a.pdf>
- Correct out-of-SSA translation and coalescing:
  <https://doi.org/10.1109/CGO.2009.19>
- V8 use-directed Word32 lowering:
  <https://chromium.googlesource.com/v8/v8/+/f45c842fe1b011f7fde237112067dcc999b71dd3/src/compiler/simplified-lowering.cc>
- V8 element-kind lattice and raw double storage:
  <https://v8.dev/blog/elements-kinds> and <https://v8.dev/blog/fast-properties>
- JavaScriptCore Int32/double/contiguous storage transitions:
  <https://github.com/WebKit/WebKit/blob/main/Source/JavaScriptCore/runtime/JSObject.h>
- Rust regex reusable capture locations:
  <https://docs.rs/regex/latest/regex/struct.Regex.html#method.captures_read>
- V8 CFG/copying-reducer rationale:
  <https://v8.dev/blog/leaving-the-sea-of-nodes>
- CPython 3.15 copy-and-patch register allocation and uniqueness work:
  <https://github.com/python/cpython/blob/main/Doc/whatsnew/3.15.rst>

