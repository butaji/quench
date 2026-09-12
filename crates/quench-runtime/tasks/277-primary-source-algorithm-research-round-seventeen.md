# 277 — Primary-source algorithm research, round seventeen

Status: complete

This pass searched current LLVM, V8, CPython, JavaScriptCore, and 2025 compiler
literature for additional algorithms that fit the standing contract: every eligible
function is linked to stencils before first execution, runtime generation is bounded
copy/patch/share, rustc/LLVM cooks a finite general catalog AOT, and neither hotness nor
benchmark/source identity selects code.

## New mechanisms after deduplication

### 1. Runtime no-alias loop versioning (Task 278)

The quoted numeric-region model knows which array backings a loop touches, but it does
not yet refine `MayAlias` into a guarded disjoint context. LLVM's LoopVersioningLICM and
LoopAccessAnalysis build bounded runtime interval checks, branch to a no-alias clone,
and retain the conservative original loop. LLVM's vectorizer uses the same mechanism to
legalize vectorization when static alias analysis is insufficient.

This maps cleanly to the stencil category: one pure `AliasPartition` fact derives a
preheader guard and two ordinary loop morphisms with the same external context. The
disjoint arm selects rustc-cooked slice/noalias templates; the may-alias arm selects the
existing ordered stencil loop. This is a new prerequisite for C-like multi-array loops,
especially before Task 39 attempts SIMD.

### 2. Dominance-safe guard products (Task 279)

LLVM GuardWidening combines multiple guards into one and eliminates dominated checks,
but only where the condition can be made available safely and where moving the check is
profitable. V8's branch elimination similarly propagates known path conditions to erase
redundant deopt branches.

This project needs the corresponding quoted rewrite over its own semantic guards,
because independently cooked leaves otherwise repeat shape, tag, range, and protector
branches. The safe unit is not an arbitrary boolean expression: it is a normalized
product of pure guard atoms that share a canonical materialization state and failure
continuation. Effect-token or exit-state disagreement forbids merging.

### 3. Direct numeric text conversion kernels (Task 280)

The local `parseInt` path clones, trims, removes/drains, collects a second digit string,
then calls `from_str_radix`; integer radix formatting grows a string forward and then
allocates again to reverse it. Crypto and Earley-Boyer contain real radix parsing and
formatting calls, while Splay constructs string payloads from numeric keys.

V8 scans flat one-byte/two-byte strings directly, has separate power-of-two radix
handling, writes integer output into caller-owned bounded buffers, caches trivial
results, and recently moved shortest decimal conversion to Dragonbox. The smallest
project-local experiment is not a new formatting framework: borrow the canonical string
view, scan or fill one bounded stack buffer, then allocate exactly the final JS string.

## Existing owners strengthened by current sources

- **Task 79:** patched constants are too late for LLVM to optimize. Generate constant-
  *class* variants so power-of-two division/remainder becomes shifts/masks and other I32
  constant divisors use LLVM's multiply/add/shift magic-number lowering.
- **Task 183:** V8's current Maglev reducer lowers guarded `sqrt`, `abs`, `floor`,
  `ceil`, `round`, `min`, and `max` calls to typed nodes; its 2026 reducer also covers
  IEEE-754 `pow`, `log`, and trigonometric builtins. The canonical builtin recipe should
  derive equivalent typed stencil/kernel selection rather than preserving native-call
  argument materialization.
- **Tasks 158 and 172:** CPython 3.15 independently reports that basic register
  allocation removes stack traffic and that explicit reference/uniqueness tracking
  eliminates refcount operations and enables in-place operations. These remain ahead of
  adding more isolated opcode leaves.
- **Task 164:** JavaScriptCore's persistent/delta stackmap representation already matches
  the task's parent/delta frame-state design; do not add a second metadata format.
- **Tasks 190 and 227:** V8's 2025 stringifier validates one-byte/two-byte specialized
  kernels and segmented builders. Those are derived consumers of the canonical string
  sum, not separate string representations.
- **Tasks 202 and 222:** the 2025 QuickJS work on prior hidden-class construction and
  fixed-offset method specialization corroborates prebuilding constructor shapes; it is
  not a new hidden-class subsystem.
- **Task 204:** V8's mutable I32/F64 binding-cell state machine already exactly matches
  this task; its reported 2.5x local benchmark and 1.6% JetStream2 aggregate improvement
  strengthen priority but do not justify a duplicate task.

## Ranked experiments

1. Finish Task 214's range/relative-bound facts and Task 158's arbitrary register-
   resident connectors.
2. Implement Task 278; use its disjoint context to unlock Tasks 173, 175, 39, and 265.
3. Complete Task 146/181 direct guest calls and Task 172 ownership flow—the current
   native profiles still put broad cost in generic execution, frames, and destruction.
4. Complete Task 152 shape propagation and Task 183 typed builtin reduction.
5. Add Task 79 constant-class variants, starting with Word32 power-of-two and constant-
   divisor operations proved by Tasks 214/264.
6. Implement Task 280 after the canonical borrowed string view exists; it can begin with
   the current flat string representation without committing to ropes.
7. Implement Task 279 only after Task 164 and Task 173 make failure state and effect
   legality explicit.

The ranking rejects breadth for its own sake. The new algorithms either move a proof
across multiple operations or remove an allocation/helper boundary. Copying more tiny
stencils without those facts remains low leverage.

## Primary sources

- LLVM LoopVersioningLICM:
  <https://llvm.org/doxygen/LoopVersioningLICM_8cpp_source.html>
- LLVM LoopAccessAnalysis and runtime pointer groups:
  <https://llvm.org/docs/doxygen/LoopAccessAnalysis_8h_source.html>
- LLVM vectorizer runtime pointer checks:
  <https://llvm.org/docs/Vectorizers.html>
- LLVM GuardWidening:
  <https://llvm.org/docs/doxygen/GuardWidening_8h_source.html>
- V8 branch elimination:
  <https://chromium.googlesource.com/v8/v8/+/refs/heads/main/src/compiler/branch-elimination.cc>
- LLVM division-by-constant lowering:
  <https://github.com/llvm/llvm-project/blob/main/llvm/include/llvm/Support/DivisionByConstantInfo.h>
- V8 Maglev builtin reducer:
  <https://chromium.googlesource.com/v8/v8/+/refs/heads/main/src/maglev/maglev-reducer-inl.h>
- V8 numeric parsing/conversion source:
  <https://chromium.googlesource.com/v8/v8/+/refs/heads/main/src/numbers/conversions.cc>
- V8 numeric formatting and segmented string-building report:
  <https://v8.dev/blog/json-stringify>
- CPython 3.15 JIT mechanisms:
  <https://github.com/python/cpython/blob/main/Doc/whatsnew/3.15.rst>
- JavaScriptCore speculation and persistent exit state:
  <https://webkit.org/blog/10308/speculation-in-javascriptcore/>
- Static hidden-class construction in QuickJS:
  <https://doi.org/10.1145/3742876.3742877>

