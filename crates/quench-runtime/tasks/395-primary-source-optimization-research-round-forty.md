# 395 — Primary-source optimization research, round forty

Status: complete

Research general-purpose algorithms that can close the measured native-to-kernel gap while
preserving this VM's constraints: OXC plus Rust, no third-party VM, no interpreter escape,
rustc/LLVM cooking only at build time, copy-and-patch execution from the first call, no
execution-count hotness threshold, and no selector shaped around V8v7 source.

## Local evidence changes the question

Task 393's non-perturbing native census proves that most transfers already enter direct
machine code. In 20 ms runs, the semantic-kernel counter is still material: 694,529 in
Richards, 512,235 in DeltaBlue, 349,494 in Crypto, 322,259 in RayTrace, and 1,465,914 in
Earley-Boyer. Crypto also enters a numeric region 388,524 times, while the earlier numeric
statistics recorded 158,923 guard failures.

The current `GuardPlan::validate` evaluates one conjunction over every numeric, dense-array,
index, and property requirement. Any `GuardFailure` sends the complete `[start, end)` range
to `execute_region_fallback`. Thus one false assumption discards every still-valid native
fact and operation in the region. The immediate problem is no longer "make stencils run";
it is "preserve the strongest valid native context when one assumption fails."

## Ranked algorithms and patterns

### 1. Failure-directed native cover as a bounded decision DAG

Combine three established algorithms:

- static/lazy basic-block versioning specializes blocks by incoming type context and reports
  eliminating 71% of executed type tests in its evaluated JavaScript VM;
- Deoptless replaces a generic deoptimization destination with assumption-polymorphic native
  continuations selected by the current context;
- CacheIR represents an IC as guards, idempotent operations, and one terminal result, making
  each failed guard a precise semantic boundary.

Apply them without runtime compilation or heat. At link time, enumerate the bounded contexts
already demanded by the bytecode CFG. Lower the region guard product into a shared-prefix
decision DAG. A failed atom selects the most-specific compatible prelinked native
continuation; only the bottom `Unknown` context reaches the general semantic kernel. This is
Task 396, a concrete first slice of Tasks 144 and 371.

Categorically, each guard refines `Gamma` and every edge remains a typed morphism. The DAG is
a factored coproduct of contexts, not an ordered `if` pile. In Lisp staging terms, guard
atoms and alternatives remain quoted data through normalization and costed selection, then
emit once.

### 2. Register-resident context versions, not boxed specialized opcodes

Task 381 already falsified isolated bitwise leaves because each leaf converted between boxed
F64-like `Value` state and I32 state. The Copy-and-Patch paper's performance comes from a
large finite library of implementation variants, while Deegen explicitly combines register
pinning, tag-register optimization, type-check removal, and strength reduction. The useful
unit here is therefore Task 385's `I32Context<Locations>` region: guard and unbox once, retain
values in fixed machine registers across the selected cover, and materialize only on a real
effect or side exit.

Task 157 should price entry conversion, exit materialization, spills, seams, copied bytes,
and shared-kernel reach. A larger stencil is retained only when rustc/LLVM demonstrably
removes those costs; reducing the number of stencil nodes is not itself a benefit.

### 3. Property-to-call recipes with shared code and separate fields

Property and call kernels dominate the object-oriented suites. CacheIR's useful physical
pattern is one canonical recipe `Guard* ; Pure* ; Result`, with per-site fields stored apart
from shareable code. Its documentation explicitly notes that baseline IC code can be shared
across different field values, while baking every field into optimized code may be a mistake.

For this VM, compose Tasks 153, 368, 146, and 379 as:

```text
GuardReceiverShape
; GuardPrototypeCondition
; LoadFixedMethodSlot
; GuardFunctionCode
; PushGuestFrame
; JumpGuestEntry
; ReturnContinuation
```

Choose `Kernel` for a shareable recipe plus field record and `StencilInstance` only when the
cost model proves that burning fields removes a critical dependency. Do not materialize the
method as an owned `Value`, and do not cross a Rust helper on the successful edge.

### 4. Watchpoint-licensed heap constant folding

JavaScriptCore's structures are hash-consed identities, and its property conditions plus
adaptive watchpoints can turn stable prototype properties and methods into constants. This
is stronger than adding wider PICs: a mutation-time invalidation lets ordinary execution
erase repeated prototype walks, method loads, and sometimes structure checks.

Task 368 already owns `Presence | Absence | Equivalence`. Prioritize `Equivalence` for stable
method targets and immutable built-ins because it composes directly with the hostless call
recipe. The mutation path pays publication/invalidation; immutable kernels and instances do
not mutate.

### 5. Compact traced heap, inline slots, and bump allocation

Word-sized values alone do not produce C-like memory traffic if every object access follows
an `Rc<RefCell<_>>` pointer to out-of-line storage. V8 separates named properties from
elements and uses shape-dependent layouts; JavaScriptCore uses a compact structure-bearing
header, inline property slots, fixed-size allocation blocks, and free lists. Its production
collector is nonmoving and generational.

The next coherent memory experiment is the existing Task 369 layout on top of Tasks 299 and
320: compact header, shape-described pointer map, bounded inline property slots, separate
array payload, inline bump allocation, and an outlined collection kernel. Task 321 may erase
or cluster barriers for fresh allocations. Measure object bytes, pointer loads, allocation
rate, and cache misses together; another local `Rc` clone removal is not this experiment.

### 6. Allocation sinking and partial escape materialization

JavaScriptCore documents object-allocation sinking, and its optimizer can rematerialize a
phantom allocation at an OSR exit. Apply Tasks 24, 33, and 176 to a quoted region: represent a
non-escaping allocation as scalar fields, forward property reads/writes, and materialize the
object only on the first path that truly escapes. This is especially relevant to temporary
numeric vector/color objects, but legality is defined by effect and escape facts, never by a
class or benchmark name.

### 7. Atom strings first; ropes and native RegExp after measurement

Intern property/identifier atoms so IC keys and shape transitions use stable compact IDs.
Keep numeric element keys out of the string table. For dynamic concatenation, a bounded rope
or slice view can defer copying, but it must flatten at a named depth to avoid degenerate
quadratic traversal. RegExp remains a separate compiled matcher-kernel problem. These are
Tasks 11, 230, 280, and 87; they follow the higher-reach numeric/property/call work.

### 8. Offline rustc/LLVM oracle plus minimum-cost cover

Copy-and-patch relies on a finite stencil library, not runtime instruction selection. Task
394 should enumerate typed semantic forms offline, let pinned rustc/LLVM expose which
whole-form variants actually remove loads/checks/spills, and retain relocation-closed Pareto
winners. Task 157 then solves the runtime-free minimum-cost cover using immutable recipes.
This is the scalable source of multi-level stencils; it replaces manually guessing opcode
pairs.

## Ideas deliberately not promoted

- Do not add another isolated arithmetic, bitwise, property, or call leaf. Local measurements
  already show that extra native coverage can lose when conversions and seams remain.
- Do not add trace hotness detection. Trace trees motivate typed path contexts, but their
  counter-driven selection violates this VM's first-execution policy. Static loop structure
  and first semantic IC observations are sufficient inputs.
- Do not optimize the failing interpreter path. The only total bottom case is the canonical
  semantic stencil/kernel, and every higher case must remain machine-code composition.
- Do not migrate the critical path to an e-graph or heavyweight optimizing IR before the
  bounded CFG/context-version experiment. Maglev and JavaScriptCore show that block-local SSA,
  known-value state, and limited versions already capture important wins at lower complexity.

## Execution order

1. Finish Task 393's guard-event census and Task 390's patch gate.
2. Run Task 396: exact failure attribution, then one bounded alternate native continuation.
3. Finish Task 385's register-resident I32 context and use Task 157 for physical selection.
4. Close the property/method/call/return continuum through Tasks 153, 368, 146, and 379.
5. Implement Task 369 with Tasks 299/320 rather than piecemeal `Rc` edits.
6. Add allocation sinking, atoms/ropes, and the RegExp matcher based on the refreshed native
   residual and allocation profiles.
7. Use Task 394 to automate further multi-level stencil discovery after the connector and
   effect vocabulary is stable.

Every experiment uses named policy constants, semantic differential tests, disassembly,
native-path counters, and the alternating exact V8v7 gate. External speedups motivate an
algorithm but are never projected onto this VM.

## Primary sources

- Deegen, first submitted 18 November 2024:
  <https://arxiv.org/abs/2411.11469>
- Copy-and-Patch Compilation:
  <https://arxiv.org/abs/2011.13127>
- Lazy basic-block versioning:
  <https://arxiv.org/abs/1411.0352>
- Interprocedural basic-block versioning:
  <https://arxiv.org/abs/1511.02956>
- Deoptless specialized continuations:
  <https://arxiv.org/abs/2203.02340>
- SpiderMonkey CacheIR:
  <https://firefox-source-docs.mozilla.org/js/cacheir.html>
- JavaScriptCore speculation, structures, watchpoints, OSR, and allocation sinking:
  <https://webkit.org/blog/10308/speculation-in-javascriptcore/>
- JavaScriptCore property conditions and adaptive watchpoints:
  <https://webkit.org/blog/6756/es6-feature-complete/>
- V8 Maglev's block-local SSA and known-value state:
  <https://v8.dev/blog/maglev>
- V8 fast properties and elements:
  <https://v8.dev/blog/fast-properties>
- V8 pointer-compression representation tradeoffs:
  <https://v8.dev/blog/pointer-compression>
- JavaScriptCore allocation and generational collector design:
  <https://webkit.org/blog/12967/understanding-gc-in-jsc-from-scratch/>
- Trace-based type specialization, used here only as evidence for path contexts:
  <https://doi.org/10.1145/1542476.1542528>

