# 185 — Prototype-membership and `instanceof` condition stencils

Status: complete

Represent an `instanceof` observation as immutable recipe data: constructor identity,
constructor-prototype identity, receiver/prototype shape chain, expected result, and the
canonical miss continuation. A rustc-cooked stencil guards the recipe and returns the
membership result without walking `Rc<RefCell>` links. A coarse condition form consumes
`InstanceOf; Unary(Not); JumpIfFalse` directly without materializing intermediate values.

Prototype mutation invalidates the recipe through the same monotone fuse/side-exit model
as Tasks 155 and 164. Bounded polymorphism is expressed as composition of recipe arms,
not benchmark-specific constructor cases.

The evidence is unusually direct: Task 132 recorded 745,752 entries of
`LoadLocal,LoadName,InstanceOf,Unary:Not,JumpIfFalse`, while the current helper walks the
prototype chain on every call. Acceptance requires positive and negative membership,
constructor/prototype replacement, deep chains, primitive operands, and custom
`Symbol.hasInstance` behavior to follow the canonical slow semantics; selection and
disassembly evidence; complete V8v7 A/B improvement.

Primary background: JSC's polymorphic `instanceof` IC landed through its ordinary IC
generator (<https://trac.webkit.org/timeline?from=2018-05-19T09%3A30%3A49-07%3A00&precision=second>).

## 2026-09-10 granularity experiment: direct prototype helper rejected

A general helper fast path compared an object's immediate prototype with the
constructor prototype before retaining or walking the chain. Direct instances therefore
performed no prototype `Rc` increment/decrement; inherited and negative cases retained
the canonical chain walk. A focused test proved the direct path kept the prototype's
strong count unchanged while preserving inherited positive and negative results. All 90
release tests passed.

The exact candidate (`40e42176...`) measured EarleyBoyer 2285→2287 across nine
alternating 500 ms pairs in
`reports/task185-direct-prototype-targeted-ab-9/comparison.txt`: **+0.09%**, effectively
neutral. It was removed. This falsifies helper-local refcount removal as a sufficient
optimization and sharpens the acceptance boundary: lower the full
`LoadLocal + LoadName + InstanceOf + Unary(Not) + JumpIfFalse` observation into one
guard/branch stencil with a prototype-dependency fuse or closed tag. Do not spend another
experiment on the loop body of `instance_of` alone.

## 2026-09-10 immediate-prototype whole-condition stencil rejected

The first coarse candidate needs no warm-up cache or mutable dependency recipe. The
current canonical object constructor stores clones of the same `Rc<ObjectCell>` in the
new object's immediate `prototype` field and the constructor's `prototype` field. The
rustc-cooked AArch64 stencil compares those two AOT-visible identity words directly.
Non-object receivers, non-function constructors, and null immediate prototypes produce
the canonical false-membership result; an equal immediate prototype takes the direct
branch; a distinct non-null prototype rejoins the complete canonical block before any
effect so inherited membership remains correct.

The candidate gave `FunctionValue` a `repr(C)` prototype-first layout, and named
word-offset constants plus compile-time and runtime identity-layout tests defined the
ABI. Its selector matched only the five-op semantic/liveness form; it did not inspect
source, constructor names, literals, benchmark identity, or execution counts.

Initial validation passes: all 92 release tests succeed, including structural/liveness
selection, prototype-word ABI identity, direct positive membership, prototype mutation,
inherited slow re-entry, constructor replacement, and primitive receiver cases. The
generated release catalog contains `deegen_dyn_dead_direct_instanceof_not`, and the
eight-suite semantic smoke is recorded in
`reports/task185-instanceof-whole-condition-smoke.jsonl`. The candidate binary before
the final test-only formatting cleanup has SHA-256
`0e9f28ac7b7749c1d8c861379efc7528f9cc068d522ab1fda047f2235e0f3bc2`.
Residual instrumentation proved that it removed all 745,752 entries of the exact target
block in a 100 ms Earley-Boyer run: direct selection rose from 429 blocks/883 opcodes to
435/913. That mechanical coverage did not translate to useful execution. The exact nine
alternating 500 ms pairs in
`reports/task185-instanceof-whole-condition-earley-ab-9/comparison.txt` measured
2280→2094, **-8.16%**. It was removed.

The result explains why coverage alone was misleading. Earley-Boyer frequently checks
membership through an inherited prototype chain. The immediate-prototype comparison
therefore performs tag/layout loads and then rejoins the full canonical chain walk on a
miss, adding work to the dominant case. The next viable Task 185 design must cache or
prove the complete prototype-chain result with mutation invalidation; do not retry an
immediate-prototype prefix or another helper-local shortcut.

## 2026-09-10 layout-only isolation

To separate stencil cost from the candidate's global ABI change, the prototype-first
`FunctionValue` layout was measured by itself with no new stencil. Seven alternating
500 ms Earley-Boyer pairs in
`reports/task185-function-layout-only-earley-ab-7/comparison.txt` measured 2306→2302,
**-0.17%**, which is neutral at this sample size. The layout experiment was removed.
This isolates the whole-condition candidate's -8.16% regression to its added
immediate-prototype precheck on the dominant inherited-chain path, rather than to field
reordering. A future attempt must guard a cached complete membership observation (plus
prototype-mutation validity), not merely compare the first prototype link.

## 2026-09-10 complete membership cache accepted

Task 285 implements the required complete observation as a per-bytecode cache behind one
shared immutable kernel, then consumes its result in 108/112-byte branch stencils. This
avoids both failed designs above: it does not repeat a partial chain prefix before a miss,
and it does not copy a large cache checker or allocate a name-snapshot slot per instance.
All 101 tests pass. The focused Earley-Boyer A/B is +1.53%; the full five-pair aggregate is
+0.10%, with Earley-Boyer +2.60% and no component below the standing floor. Task 185 is
complete; broader coproduct-tag and list-loop erasure remain separately owned by Tasks
220 and 221.
