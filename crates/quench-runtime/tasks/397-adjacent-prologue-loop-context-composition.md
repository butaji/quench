# 397 — Adjacent prologue/loop context composition

Status: complete

Compose an eligible straight-line numeric/property prologue and its uniquely entered loop
as one higher-level typed region, rather than emitting two independently guarded regions.
This is a general control-flow construction discovered by Task 396, not a Crypto-specific
fusion.

## Evidence

Crypto's `am3` family lowers the following shape:

```text
property/constant prologue : GammaEntry -> GammaLoop
numeric dense loop         : GammaLoop  -> GammaExit
```

The prologue copies object-valued fixed properties into loop locals and computes numeric
constants. When emitted independently, the loop repeats a guard immediately after the
prologue has already established its input context. Before Task 396's reaching-definition
fix, the seam also exposed false scratch-local guards because the generic fallback could
run beyond the prologue boundary while the native prologue could not.

## Canonical construction

Represent a candidate as quoted data:

```text
AdjacentRegion = Seq(Block<GammaEntry, GammaLoop>, Trace<GammaLoop, GammaExit>)
```

Admit it only when the CFG proves one prologue successor, one loop entry, no external edge
into the loop interior, compatible effect/ownership contexts, and one reconstructable exit
state. Normalize local/property/array facts across the boundary, run ordinary rewrite and
cover selection on the combined expression, then emit once. The construction must use the
same `Seq` category operation as any other stencil level; it is not a new machine-code
primitive.

The composed guard is evaluated once at `GammaEntry`. Prologue-established facts are
morphisms into `GammaLoop`, not redundant runtime predicates. Side exits materialize the
canonical frame state through existing typed exit adapters. Shared kernels and patched
instances remain interchangeable leaves.

## Acceptance

- No benchmark names, function names, source offsets, property spellings, or runtime heat.
- Differential tests cover skipped prologues, alternate predecessors, exceptions, shape and
  backing changes, string/function ownership, empty loops, and zero-iteration exits.
- Counters report removed guard entries, seams, copied bytes, and materializations.
- Disassembly proves one entry guard and a direct prologue-to-loop edge.
- Retain only if the complete alternating exact V8v7 gate improves with no component-floor
  violation.

This task depends on Tasks 01, 14, 17, 128, 144, 157, 164, 171, 330, 371, 384, and 396.

## Implementation

`quote_adjacent_loop` constructs the canonical
`Seq(Block<GammaEntry,GammaLoop>, Trace<GammaLoop,GammaExit>)` value before emission. It
admits a pair only after ordinary CFG checks prove bounds, a single allowed loop header,
no outside entry into the combined interior, and a unique exit. The existing categorical
`RegionNode::Seq` and `RegionNode::Trace` are the representation; there is no special
machine-code node and no runtime heat decision.

Property-result facts form an explicit lattice. `TriviallyCopyable` may refine to `Number`
or `DenseArray`; contradictory `Number`/`DenseArray` demands reject the region. A static
property that produces the loop's dense array is assigned a retained guarded view, and all
computed accesses inside the loop refer to that view by index. This removes the false
requirement that the newly written loop local already contain an external dense array at
entry.

The AArch64 linker considers only an immediately preceding eligible quoted block ending at
the loop header. It tries the combined quote and otherwise retains the independent loop.
This is a general CFG/type rule: it contains no source names, function names, byte offsets,
property strings, observed values, or benchmark identity.

Tests cover the normalized `Seq(Block,Trace)` shape, absence of the false local guard,
the dense property result, and shared dense-view indices. The complete release suite passes
152 tests. The rustc/LLVM stencil-cooker differential audit passes during the final release
build. A 20 ms Crypto diagnostic links eight adjacent regions; the important region becomes
one `[0,82)` native region and succeeds 155,678 times in that sample.

## Measurement

Composition alone was neutral in the three-pair 200 ms screen at
`reports/task397-adjacent-loop-composition/quick-ab/comparison.txt`: **-0.14%** aggregate.
It removed a semantic seam but did not remove repeated entry validation. Task 399 therefore
adds a lawful memo of the expensive composite guard result without changing this quoted
composition.

The final Task 397+399 binary is
`/tmp/deegen-task397-final-candidate`, SHA-256
`77b0f2b745e35e7f4b9d938cfe42b902022dc1bf77a8ed7830ff94a170241470`.
The required nine-pair exact comparison against the accepted Task 381 binary is at
`reports/task397-adjacent-loop-composition/exact-vs-accepted/comparison.md`. Aggregate
improves **2333.57 -> 2368.66 (+1.50%)**, with paired-bootstrap interval
**[+0.96%, +2.13%]**; the gate passes. Crypto improves **15.79%**. Richards regresses
**1.20%** with a negative interval, so subsequent work must recover it even though it stays
inside the configured component floor. The accepted checkpoint is 23.69% of the 10000 goal.
